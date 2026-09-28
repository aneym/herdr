import Foundation

/// Blocking Unix-domain socket helpers for herdr's line-delimited JSON API.
enum HerdrSocket {
    static func connect(_ path: String) -> Int32? {
        let fd = Darwin.socket(AF_UNIX, SOCK_STREAM, 0)
        guard fd >= 0 else { return nil }
        var one: Int32 = 1
        setsockopt(fd, SOL_SOCKET, SO_NOSIGPIPE, &one, socklen_t(MemoryLayout<Int32>.size))
        var addr = sockaddr_un()
        addr.sun_family = sa_family_t(AF_UNIX)
        let bytes = Array(path.utf8)
        guard bytes.count < MemoryLayout.size(ofValue: addr.sun_path) else { close(fd); return nil }
        withUnsafeMutableBytes(of: &addr.sun_path) { raw in
            for (i, b) in bytes.enumerated() { raw[i] = b }
            raw[bytes.count] = 0
        }
        let len = socklen_t(MemoryLayout<sockaddr_un>.size)
        let rc = withUnsafePointer(to: &addr) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) { Darwin.connect(fd, $0, len) }
        }
        if rc != 0 { close(fd); return nil }
        return fd
    }

    @discardableResult
    static func writeAll(_ fd: Int32, _ data: Data) -> Bool {
        var off = 0
        return data.withUnsafeBytes { (p: UnsafeRawBufferPointer) -> Bool in
            while off < data.count {
                let n = write(fd, p.baseAddress! + off, data.count - off)
                if n <= 0 { return false }
                off += n
            }
            return true
        }
    }

    /// One request, one response line, on a fresh connection. `timeout` bounds the wait.
    static func request(_ path: String, _ json: String, timeout: TimeInterval = 5) -> Data? {
        guard let fd = connect(path) else { return nil }
        defer { close(fd) }
        guard writeAll(fd, Data((json + "\n").utf8)) else { return nil }
        var buf = Data()
        var chunk = [UInt8](repeating: 0, count: 65536)
        let deadline = Date().addingTimeInterval(timeout)
        while true {
            if let nl = buf.firstIndex(of: 0x0A) { return buf.prefix(upTo: nl) }
            let remaining = deadline.timeIntervalSinceNow
            if remaining <= 0 { return nil }
            var pfd = pollfd(fd: fd, events: Int16(POLLIN), revents: 0)
            let pr = poll(&pfd, 1, Int32(min(remaining, 1) * 1000))
            if pr < 0 { if errno == EINTR { continue }; return nil }
            if pr == 0 { continue }
            let n = read(fd, &chunk, chunk.count)
            if n <= 0 { return nil }
            buf.append(chunk, count: n)
        }
    }
}

/// One long-lived `events.subscribe` connection. Reads pushed lines on its own
/// thread; every chunk that carries events is reported once through `onBatch`,
/// so a burst of events costs one reconcile. Ends with `onClose` on EOF, on an
/// error response (`events_lost`, unknown pane) or on a bad ack.
final class EventStream {
    let path: String
    let subscriptions: [[String: Any]]
    var onAck: (() -> Void)?
    var onBatch: (() -> Void)?
    var onClose: ((String) -> Void)?

    private let lock = NSLock()
    private var fd: Int32 = -1
    private var stopped = false

    init(path: String, subscriptions: [[String: Any]]) {
        self.path = path
        self.subscriptions = subscriptions
    }

    func start() {
        Thread.detachNewThread { [self] in run() }
    }

    /// Intentional stop: `onClose` is not called.
    func stop() {
        lock.lock()
        stopped = true
        if fd >= 0 { shutdown(fd, SHUT_RDWR) }
        lock.unlock()
    }

    private var isStopped: Bool { lock.lock(); defer { lock.unlock() }; return stopped }

    private func finish(_ reason: String) {
        lock.lock()
        let was = stopped
        stopped = true
        let f = fd
        fd = -1
        lock.unlock()
        if f >= 0 { close(f) }
        if !was { onClose?(reason) }
    }

    private func run() {
        guard let s = HerdrSocket.connect(path) else { finish("cannot connect to \(path)"); return }
        lock.lock()
        if stopped { lock.unlock(); close(s); return }
        fd = s
        lock.unlock()

        let req: [String: Any] = ["id": "shell:events", "method": "events.subscribe",
                                  "params": ["subscriptions": subscriptions]]
        guard let body = try? JSONSerialization.data(withJSONObject: req),
              HerdrSocket.writeAll(s, body + Data([0x0A])) else { finish("subscribe write failed"); return }

        var acked = false
        var pending = Data()
        var chunk = [UInt8](repeating: 0, count: 65536)
        while true {
            let n = read(s, &chunk, chunk.count)
            if n <= 0 { finish(isStopped ? "stopped" : "event stream closed"); return }
            pending.append(chunk, count: n)
            var events = 0
            while let nl = pending.firstIndex(of: 0x0A) {
                let line = pending.prefix(upTo: nl)
                pending.removeSubrange(pending.startIndex...nl)
                guard let obj = try? JSONSerialization.jsonObject(with: line) as? [String: Any] else { continue }
                if let err = obj["error"] as? [String: Any] {
                    finish("\(err["code"] as? String ?? "error"): \(err["message"] as? String ?? "")")
                    return
                }
                if !acked {
                    guard (obj["result"] as? [String: Any])?["type"] as? String == "subscription_started" else {
                        finish("unexpected subscribe response"); return
                    }
                    acked = true
                    onAck?()
                } else {
                    events += 1
                }
            }
            if events > 0 { onBatch?() }
        }
    }
}

/// Live herdr state without a timer. Subscribes to herdr's lifecycle events and
/// reconciles with one `session.snapshot` on connect and one per event batch;
/// events are only invalidation signals, the snapshot is the truth (herdr's
/// socket API docs, "Event subscriptions"). Refreshes are serialized and
/// coalesced: events that arrive while a snapshot is in flight cost one more
/// read after it. Reconnects with a short backoff on disconnect or `events_lost`.
///
/// Two streams: a fixed one for lifecycle events, and one with a
/// `pane.agent_status_changed` entry per pane (herdr requires a pane id there),
/// rebuilt whenever the pane set changes.
///
/// Gap: herdr emits no event for agent ownership changes, so a bare
/// `agent owner set/clear` shows at the next event or reconnect.
final class HerdrClient {
    struct Applied {
        let snapshot: Snapshot
        let fetchMs: Double
        /// Time since the first event this refresh answers; nil for connect/retry refreshes.
        let eventToApplyMs: Double?
    }

    static let lifecycleEvents = [
        "workspace.created", "workspace.updated", "workspace.metadata_updated", "workspace.renamed",
        "workspace.moved", "workspace.reordered", "workspace.closed", "workspace.focused",
        "tab.created", "tab.closed", "tab.focused", "tab.renamed", "tab.moved",
        "pane.created", "pane.updated", "pane.closed", "pane.focused", "pane.moved", "pane.exited",
        "pane.agent_detected", "layout.updated",
    ]
    static let backoff: TimeInterval = 0.5

    let socketPath: String
    /// Both callbacks run on the main queue.
    var onSnapshot: ((Applied) -> Void)?
    var onStatus: ((String?) -> Void)?

    private let q = DispatchQueue(label: "herdr.client")
    private var general: EventStream?
    private var statusStream: EventStream?
    private var statusPanes: Set<String> = []
    private var refreshing = false
    private var dirty = false
    private var stopped = false
    private var pendingEventAt: Date?
    private var requests = 0

    init(socketPath: String) { self.socketPath = socketPath }

    /// Number of `session.snapshot` requests made so far (test evidence that
    /// nothing polls while nothing changes).
    var snapshotRequests: Int { q.sync { requests } }

    func start() { q.async { self.connect() } }

    func stop() {
        q.async {
            self.stopped = true
            self.general?.stop(); self.general = nil
            self.statusStream?.stop(); self.statusStream = nil
        }
    }

    // MARK: connection (all on q)

    private func connect() {
        guard !stopped else { return }
        let stream = EventStream(path: socketPath, subscriptions: Self.lifecycleEvents.map { ["type": $0] })
        general = stream
        stream.onAck = { [weak self, weak stream] in
            self?.q.async {
                guard let self, let stream, self.general === stream else { return }
                self.requestRefresh()
            }
        }
        stream.onBatch = { [weak self, weak stream] in
            self?.q.async {
                guard let self, let stream, self.general === stream else { return }
                self.eventArrived()
            }
        }
        stream.onClose = { [weak self, weak stream] reason in
            self?.q.async {
                guard let self, let stream, self.general === stream, !self.stopped else { return }
                self.general = nil
                self.statusStream?.stop(); self.statusStream = nil
                self.statusPanes = []
                self.report("herdr events: \(reason)")
                self.q.asyncAfter(deadline: .now() + Self.backoff) { self.connect() }
            }
        }
        stream.start()
    }

    private func syncStatusStream(panes: Set<String>) {
        guard !stopped, general != nil, panes != statusPanes else { return }
        statusStream?.stop()
        statusStream = nil
        statusPanes = panes
        guard !panes.isEmpty else { return }
        let stream = EventStream(path: socketPath, subscriptions: panes.sorted().map {
            ["type": "pane.agent_status_changed", "pane_id": $0]
        })
        statusStream = stream
        // The ack closes the gap between the snapshot that named these panes and now.
        stream.onAck = { [weak self, weak stream] in
            self?.q.async {
                guard let self, let stream, self.statusStream === stream else { return }
                self.requestRefresh()
            }
        }
        stream.onBatch = { [weak self, weak stream] in
            self?.q.async {
                guard let self, let stream, self.statusStream === stream else { return }
                self.eventArrived()
            }
        }
        stream.onClose = { [weak self, weak stream] reason in
            self?.q.async {
                guard let self, let stream, self.statusStream === stream, !self.stopped else { return }
                // Typically a pane closed between snapshot and subscribe: reread, then resubscribe.
                self.statusStream = nil
                self.statusPanes = []
                self.q.asyncAfter(deadline: .now() + Self.backoff) { self.requestRefresh() }
            }
        }
        stream.start()
    }

    // MARK: refresh (all on q)

    private func eventArrived() {
        if pendingEventAt == nil { pendingEventAt = Date() }
        requestRefresh()
    }

    private func requestRefresh() {
        guard !stopped else { return }
        if refreshing { dirty = true; return }
        refreshing = true
        requests += 1
        let n = requests
        let eventAt = pendingEventAt
        pendingEventAt = nil
        let path = socketPath
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            let t0 = Date()
            let data = HerdrSocket.request(path, #"{"id":"shell:snapshot","method":"session.snapshot","params":{}}"#)
            let ms = Date().timeIntervalSince(t0) * 1000
            var snap: Snapshot?
            var failure = "herdr session.snapshot failed"
            if let data {
                struct Env: Decodable { struct R: Decodable { let snapshot: Snapshot }; let result: R }
                do { snap = try JSONDecoder().decode(Env.self, from: data).result.snapshot }
                catch { failure = "decode: \(error)" }
            }
            self?.q.async {
                guard let self else { return }
                self.refreshing = false
                guard !self.stopped else { return }
                if let snap {
                    let applied = Applied(snapshot: snap, fetchMs: ms,
                                          eventToApplyMs: eventAt.map { Date().timeIntervalSince($0) * 1000 })
                    log(String(format: "p3: snapshot #%d fetched in %.1f ms%@", n, ms,
                               applied.eventToApplyMs.map { String(format: ", event->apply %.1f ms", $0) } ?? ""))
                    DispatchQueue.main.async { self.onStatus?(nil); self.onSnapshot?(applied) }
                    self.syncStatusStream(panes: Set(snap.panes.map(\.pane_id)))
                } else {
                    self.report(failure)
                    if eventAt != nil { self.pendingEventAt = eventAt }
                    self.dirty = false
                    self.q.asyncAfter(deadline: .now() + Self.backoff) { self.requestRefresh() }
                    return
                }
                if self.dirty { self.dirty = false; self.requestRefresh() }
            }
        }
    }

    private func report(_ message: String) {
        DispatchQueue.main.async { self.onStatus?(message) }
    }
}

/// Commands that change herdr's layout, sent over the API socket (P9). Blocking; call
/// off the main thread. The app never applies a change itself: it rebuilds its panes
/// from the layout herdr answers with (or from the next snapshot).
struct HerdrCommands {
    let socketPath: String

    struct CreatedPane { let paneId: String; let terminalId: String; let tabId: String }
    struct ResizeOutcome { let changed: Bool; let layout: Snapshot.Layout }

    private func call(_ method: String, _ params: [String: Any]) -> Data? {
        let body: [String: Any] = ["id": "shell:\(method)", "method": method, "params": params]
        guard let data = try? JSONSerialization.data(withJSONObject: body),
              let json = String(data: data, encoding: .utf8),
              let reply = HerdrSocket.request(socketPath, json) else { return nil }
        return reply
    }

    /// `pane.resize`. `direction` is left|right|up|down; `amount` is a share of the split
    /// (herdr takes the absolute value and caps it at 0.5).
    func paneResize(paneId: String, direction: String, amount: Double) -> ResizeOutcome? {
        struct Env: Decodable {
            struct R: Decodable {
                struct Z: Decodable { let changed: Bool; let layout: Snapshot.Layout }
                let resize: Z
            }
            let result: R
        }
        guard let data = call("pane.resize", ["pane_id": paneId, "direction": direction, "amount": amount]),
              let env = try? JSONDecoder().decode(Env.self, from: data) else { return nil }
        return ResizeOutcome(changed: env.result.resize.changed, layout: env.result.resize.layout)
    }

    /// `pane.split` of `targetPaneId`; direction is right|down. Returns the new pane.
    func paneSplit(targetPaneId: String, direction: String) -> CreatedPane? {
        struct Env: Decodable {
            struct R: Decodable {
                struct P: Decodable { let pane_id: String; let terminal_id: String; let tab_id: String }
                let pane: P
            }
            let result: R
        }
        guard let data = call("pane.split", ["target_pane_id": targetPaneId, "direction": direction, "focus": false]),
              let env = try? JSONDecoder().decode(Env.self, from: data) else { return nil }
        let p = env.result.pane
        return CreatedPane(paneId: p.pane_id, terminalId: p.terminal_id, tabId: p.tab_id)
    }

    /// `tab.create` in a workspace. Returns the new tab id and its root pane.
    func tabCreate(workspaceId: String, cwd: String?) -> CreatedPane? {
        struct Env: Decodable {
            struct R: Decodable {
                struct T: Decodable { let tab_id: String }
                struct P: Decodable { let pane_id: String; let terminal_id: String }
                let tab: T
                let root_pane: P
            }
            let result: R
        }
        var params: [String: Any] = ["workspace_id": workspaceId, "focus": false]
        if let cwd { params["cwd"] = cwd }
        guard let data = call("tab.create", params),
              let env = try? JSONDecoder().decode(Env.self, from: data) else { return nil }
        return CreatedPane(paneId: env.result.root_pane.pane_id, terminalId: env.result.root_pane.terminal_id, tabId: env.result.tab.tab_id)
    }
}
