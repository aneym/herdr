import AppKit
import GhosttyKit

/// Where a pane's herdr attach stands. `running` is the normal state; every other
/// state means keys are not delivered to the pane.
enum AttachState: String {
    case running    // attach process alive
    case held       // another client holds the terminal (attach refused or taken over)
    case gone       // herdr no longer has this terminal
    case failed     // attach keeps exiting; manual retry
}

/// Per-terminal lifecycle record, exposed through the state dump.
struct PaneLifecycle {
    var state: AttachState = .running
    var holder: String?
    var respawns = 0
    var fastExits = 0
    var spawnedAt = Date()
    var lastExitAt: Date?
    /// Seconds from the last exit being seen to the replacement's first non-blank screen.
    var lastRecoverS: Double?
    var lastExitText = ""
}

/// Surfaces keyed by terminal_id, retained outside the view tree. Switching tabs
/// reparents them; nothing is recreated, so the herdr attach stays live.
///
/// Lifecycle (P7):
/// - The attach process exits (killed, crashed, herdr restarted): the surface is
///   replaced at once by a fresh attach to the same terminal. herdr redraws the whole
///   screen on attach, so the pane comes back as it was. Repeated fast exits back off
///   and end in `failed` with a Retry button.
/// - Another client already holds the terminal when this app attaches: this app takes
///   it over. Every client is the same user, and the one opening the pane is where they
///   are now; the holder is usually the other machine's Shell, often asleep with its
///   socket still open, and would otherwise hold the pane forever (Alex, 2026-10-10:
///   "swapping between machines seems to get it stuck").
/// - Another client takes the terminal from this app (`--takeover`): the surface stays,
///   shows who holds it, and delivers no keys. The user coming back takes it back: the
///   app becoming active or a click on the pane. Keys never do: a key would be lost
///   while the attach comes up, and the rest would reach the pane without it.
/// - Hidden tabs: `keep` (default) leaves every attach running; `detach` releases the
///   attach of any pane not on screen and reattaches when it is shown again.
final class SurfaceRegistry {
    enum HiddenPolicy: String {
        case keepAttached = "keep"
        case detach
    }

    private(set) var byTerminal: [String: SurfaceView] = [:]
    private(set) var lifecycle: [String: PaneLifecycle] = [:]
    let herdrBin: String
    let attachEnv: [String: String]
    var hiddenPolicy: HiddenPolicy
    /// Called on the main thread when the surface for a terminal is replaced in place.
    var onReplace: ((_ old: SurfaceView, _ new: SurfaceView) -> Void)?

    private var notices: [String: NoticeBar] = [:]
    /// Where each surface's attach command records its exit status and its stderr.
    private var exitFiles: [ObjectIdentifier: (status: String, stderr: String)] = [:]
    /// Retired views stay alive briefly: a libghostty callback for a closed surface may
    /// still be queued, and it carries the view pointer.
    private var retired: [SurfaceView] = []
    private var activeObserver: NSObjectProtocol?

    init(herdrBin: String, attachEnv: [String: String], hiddenPolicy: HiddenPolicy? = nil) {
        self.herdrBin = herdrBin
        self.attachEnv = attachEnv
        self.hiddenPolicy = hiddenPolicy
            ?? HiddenPolicy(rawValue: ProcessInfo.processInfo.environment["HERDR_SHELL_HIDDEN_TABS"] ?? "")
            ?? .keepAttached
        // The user is at this machine again: take back what another machine took meanwhile.
        activeObserver = NotificationCenter.default.addObserver(
            forName: NSApplication.didBecomeActiveNotification, object: nil, queue: .main
        ) { [weak self] _ in self?.reclaimAll() }
    }

    func surface(paneId: String, terminalId: String) -> SurfaceView {
        if let s = byTerminal[terminalId] { return s }
        return spawn(paneId: paneId, terminalId: terminalId, takeover: false)
    }

    // MARK: spawning

    private func makeView(paneId: String, terminalId: String, takeover: Bool) -> SurfaceView {
        // The attach's exit status and stderr go to files, not the screen: libghostty
        // reports 0 for every exit, and the screen can hold anything the pane's program
        // printed, or the terminal's replies to the client's color queries.
        let base = NSTemporaryDirectory() + "herdr-shell-exit-\(UUID().uuidString)"
        let files = (status: base, stderr: base + ".err")
        // libghostty runs the command as `exec -l <command>`, so the status write has to
        // live in a script of its own.
        // `stty -echo`: the client asks the terminal for its 256 palette colors at start.
        // An attach that is refused exits before the replies arrive, and the tty would
        // echo them onto the pane as `]4;N;rgb:...` text. The client restores this mode.
        // Another machine's terminal attaches through its forwarded sockets, by its raw id.
        let remote = Machines.attachTarget(terminalId)
        let script = "stty -echo 2>/dev/null; "
            + "'\(herdrBin)' terminal attach \(remote?.raw ?? terminalId) --no-escape" + (takeover ? " --takeover" : "")
            + " 2>'\(files.stderr)'; echo $? > '\(files.status)'"
        let cmd = "/bin/sh -c '" + script.replacingOccurrences(of: "'", with: "'\\''") + "'"
        let env = remote.map { attachEnv.merging($0.env) { _, new in new } } ?? attachEnv
        let s = SurfaceView(paneId: paneId, terminalId: terminalId, command: cmd, env: env, cwd: NSHomeDirectory())
        s.onExit = { [weak self] v in self?.handleExit(v) }
        exitFiles[ObjectIdentifier(s)] = files
        return s
    }

    @discardableResult
    private func spawn(paneId: String, terminalId: String, takeover: Bool) -> SurfaceView {
        let s = makeView(paneId: paneId, terminalId: terminalId, takeover: takeover)
        byTerminal[terminalId] = s
        var lc = lifecycle[terminalId] ?? PaneLifecycle()
        lc.spawnedAt = Date()
        lc.state = .running
        lc.holder = nil
        lifecycle[terminalId] = lc
        return s
    }

    private func retire(_ view: SurfaceView) {
        view.onExit = nil
        view.onFocus = nil
        clearNotice(view.terminalId)
        view.removeFromSuperview()
        if let f = exitFiles.removeValue(forKey: ObjectIdentifier(view)) {
            try? FileManager.default.removeItem(atPath: f.status)
            try? FileManager.default.removeItem(atPath: f.stderr)
        }
        view.closeSurface()
        retired.append(view)
        DispatchQueue.main.asyncAfter(deadline: .now() + 5) { [weak self] in
            self?.retired.removeAll { $0 === view }
        }
    }

    private func replace(_ old: SurfaceView, takeover: Bool) {
        let new = spawn(paneId: old.paneId, terminalId: old.terminalId, takeover: takeover)
        lifecycle[old.terminalId]?.respawns += 1
        if let exitAt = lifecycle[old.terminalId]?.lastExitAt {
            watchRecovery(new, since: exitAt)
        }
        onReplace?(old, new)
        retire(old)
    }

    /// Records how long a replacement took to show a non-blank screen.
    private func watchRecovery(_ view: SurfaceView, since: Date) {
        Timer.scheduledTimer(withTimeInterval: 0.01, repeats: true) { [weak self, weak view] t in
            guard let self, let view, self.byTerminal[view.terminalId] === view else { t.invalidate(); return }
            if !view.visibleText().trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                self.lifecycle[view.terminalId]?.lastRecoverS = Date().timeIntervalSince(since)
                t.invalidate()
            } else if Date().timeIntervalSince(since) > 10 {
                t.invalidate()
            }
        }
    }

    // MARK: exit handling

    private func handleExit(_ view: SurfaceView) {
        guard byTerminal[view.terminalId] === view else { return }
        let exitAt = Date()
        // Let the attach client's last words (its stderr) reach the screen first.
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.08) { [weak self, weak view] in
            guard let self, let view, self.byTerminal[view.terminalId] === view else { return }
            self.classify(view, exitAt: exitAt)
        }
    }

    /// Lines the herdr client writes for itself: they start with `herdr:`.
    static func clientLines(_ stderr: String) -> [String] {
        stderr.lowercased().split(separator: "\n")
            .map { $0.trimmingCharacters(in: .whitespaces) }
            .filter { $0.hasPrefix("herdr:") }
    }

    /// The attach process's exit status and stderr, from the files its command wrote.
    /// A status above 128 means a signal ended it (kill, crash); nil means unknown.
    private func takeExit(_ view: SurfaceView) -> (status: Int?, stderr: String) {
        guard let f = exitFiles.removeValue(forKey: ObjectIdentifier(view)) else { return (nil, "") }
        defer {
            try? FileManager.default.removeItem(atPath: f.status)
            try? FileManager.default.removeItem(atPath: f.stderr)
        }
        let raw = (try? String(contentsOfFile: f.status, encoding: .utf8)) ?? ""
        return (Int(raw.trimmingCharacters(in: .whitespacesAndNewlines)),
                (try? String(contentsOfFile: f.stderr, encoding: .utf8)) ?? "")
    }

    private func classify(_ view: SurfaceView, exitAt: Date) {
        let id = view.terminalId
        let exit = takeExit(view)
        // A signal ended the attach: nobody said anything.
        let killed = (exit.status ?? 0) > 128
        let fresh = killed ? [] : SurfaceRegistry.clientLines(exit.stderr)
        let text = fresh.joined(separator: "\n")
        var lc = lifecycle[id] ?? PaneLifecycle()
        lc.lastExitAt = exitAt
        lc.lastExitText = fresh.last ?? ""
        let fast = exitAt.timeIntervalSince(lc.spawnedAt) < 3

        // Another client holds the pane (another machine, or our own attach the server has
        // not dropped yet): take it over. A takeover attach is never refused, so this runs
        // once per open; the fast-exit cap still stops a loop if that ever changes.
        if text.contains("already has an attached client"), !(fast && lc.fastExits >= 5) {
            lc.fastExits = fast ? lc.fastExits + 1 : 0
            lifecycle[id] = lc
            log("attach for \(id) refused: another client holds it; taking it over")
            replace(view, takeover: true)
            return
        }
        if text.contains("taken over") {
            lc.state = .held
            lifecycle[id] = lc
            showHeld(view)
            return
        }
        if text.contains("not found") && text.contains("terminal") {
            lc.state = .gone
            lifecycle[id] = lc
            showNotice(view, message: "This terminal no longer exists in herdr.", action: nil)
            return
        }
        // Killed or crashed: bring it back. A run of fast exits backs off, then stops.
        if fast { lc.fastExits += 1 } else { lc.fastExits = 0 }
        lifecycle[id] = lc
        if lc.fastExits >= 5 {
            lifecycle[id]?.state = .failed
            showNotice(view, message: "The attach to this pane keeps exiting.", action: ("Retry", { [weak self, weak view] in
                guard let self, let view else { return }
                self.lifecycle[id]?.fastExits = 0
                self.replace(view, takeover: false)
            }))
            return
        }
        let delay = lc.fastExits == 0 ? 0 : min(0.2 * pow(2, Double(lc.fastExits - 1)), 3)
        log("attach for \(id) exited; respawning in \(delay)s")
        DispatchQueue.main.asyncAfter(deadline: .now() + delay) { [weak self, weak view] in
            guard let self, let view, self.byTerminal[id] === view else { return }
            self.replace(view, takeover: false)
        }
    }

    // MARK: held panes

    private func showHeld(_ view: SurfaceView) {
        let id = view.terminalId
        let takeBack = { [weak self, weak view] in
            guard let self, let view else { return }
            self.reclaim(view.terminalId)
        }
        showNotice(view, message: "Another herdr client holds this pane. Click to take it back.",
                   action: ("Take back", takeBack))
        // A click on the held pane is the user here again: take it back.
        notices[id]?.onClick = takeBack
        // Keys go to the bar (which drops them), not to the ended surface.
        if let bar = notices[id], view.window?.firstResponder === view {
            view.window?.makeFirstResponder(bar)
        }
        DispatchQueue.global(qos: .utility).async { [weak self] in
            let who = SurfaceRegistry.findHolders(of: id)
            DispatchQueue.main.async {
                guard let self, self.byTerminal[id] != nil, self.lifecycle[id]?.state == .held else { return }
                self.lifecycle[id]?.holder = who
                self.notices[id]?.setMessage("Held by \(who). Click to take it back.")
            }
        }
    }

    /// Take a held (or failed) pane back: a fresh attach with `--takeover`, which shuts
    /// the other client's attach down.
    func reclaim(_ terminalId: String) {
        guard let view = byTerminal[terminalId], let lc = lifecycle[terminalId], lc.state != .running else { return }
        log("reclaiming \(terminalId) with --takeover")
        lifecycle[terminalId]?.fastExits = 0
        replace(view, takeover: true)
    }

    /// Take back every held pane and retry every failed one: the app became active, so
    /// the user is at this machine.
    func reclaimAll() {
        for (id, lc) in lifecycle where lc.state == .held || lc.state == .failed { reclaim(id) }
    }

    /// The terminal ids of every pane that is not running, for the menu action.
    var heldTerminals: [String] { lifecycle.filter { $0.value.state == .held }.map { $0.key } }

    /// Local processes attached to this terminal other than this app's own.
    static func findHolders(of terminalId: String) -> String {
        let p = Process()
        p.executableURL = URL(fileURLWithPath: "/bin/ps")
        p.arguments = ["-axo", "pid=,command="]
        let pipe = Pipe()
        p.standardOutput = pipe
        p.standardError = FileHandle.nullDevice
        guard (try? p.run()) != nil else { return "another herdr client" }
        let data = pipe.fileHandleForReading.readDataToEndOfFile()
        p.waitUntilExit()
        let needle = "terminal attach \(terminalId)"
        for line in String(decoding: data, as: UTF8.self).split(separator: "\n") {
            let t = line.trimmingCharacters(in: .whitespaces)
            guard let sp = t.firstIndex(of: " ") else { continue }
            let cmd = t[t.index(after: sp)...]
            guard let r = cmd.range(of: needle), !cmd.contains("herdr-shell-exit-") else { continue }
            let after = cmd[r.upperBound...]
            if after.isEmpty || after.first == " " {
                let pid = t[..<sp]
                let flag = cmd.contains("--takeover") ? " --takeover" : ""
                return "another herdr client (pid \(pid): herdr terminal attach\(flag))"
            }
        }
        return "another herdr client"
    }

    // MARK: notices

    private func showNotice(_ view: SurfaceView, message: String, action: (String, () -> Void)?) {
        clearNotice(view.terminalId)
        let bar = NoticeBar(message: message, action: action)
        bar.frame = NSRect(x: 0, y: view.bounds.height - NoticeBar.height, width: view.bounds.width, height: NoticeBar.height)
        bar.autoresizingMask = [.width, .minYMargin]
        view.addSubview(bar)
        notices[view.terminalId] = bar
    }

    private func clearNotice(_ terminalId: String) {
        notices[terminalId]?.removeFromSuperview()
        notices[terminalId] = nil
    }

    func noticeText(_ terminalId: String) -> String? { notices[terminalId]?.messageText }

    // MARK: hidden tabs

    /// Called after every layout change with the terminals now on screen.
    func setVisible(_ visible: Set<String>) {
        guard hiddenPolicy == .detach else { return }
        for (id, view) in byTerminal where !visible.contains(id) {
            log("detaching hidden pane \(id)")
            byTerminal[id] = nil
            lifecycle[id] = nil
            retire(view)
        }
    }

    /// State dump for the test hook: one entry per terminal plus the hidden-tab setting.
    func lifecycleState() -> [String: Any] {
        var panes: [String: Any] = [:]
        for (id, view) in byTerminal {
            let lc = lifecycle[id] ?? PaneLifecycle()
            panes[id] = [
                "pane": view.paneId, "state": lc.state.rawValue, "holder": lc.holder ?? NSNull(),
                "respawns": lc.respawns, "fast_exits": lc.fastExits,
                "last_exit": lc.lastExitText,
                "last_recover_s": lc.lastRecoverS ?? NSNull(),
                "notice": notices[id]?.messageText ?? NSNull(),
                "surface_exited": view.exited,
                "in_window": view.window != nil,
            ] as [String: Any]
        }
        return ["hidden_policy": hiddenPolicy.rawValue, "attached": attachedCount, "panes": panes]
    }

    /// Attach processes this app currently owns (running surfaces).
    var attachedCount: Int {
        byTerminal.values.filter { !$0.exited }.count
    }
}

/// A plain opaque bar across the top of a pane: message and one button. No accent
/// stripe; emphasis is weight and a full hairline.
final class NoticeBar: NSView {
    static let height: CGFloat = 32
    private let label = NSTextField(labelWithString: "")
    private let button = NSButton(title: "", target: nil, action: nil)
    private var handler: (() -> Void)?

    var messageText: String { label.stringValue }

    init(message: String, action: (String, () -> Void)?) {
        super.init(frame: .zero)
        wantsLayer = true
        layer?.borderWidth = 1
        label.font = .systemFont(ofSize: ShellType.size12, weight: .medium)
        label.lineBreakMode = .byTruncatingMiddle
        label.stringValue = message
        addSubview(label)
        if let (title, h) = action {
            handler = h
            button.title = title
            button.bezelStyle = .rounded
            button.controlSize = .small
            button.target = self
            button.action = #selector(fire)
            addSubview(button)
        }
    }

    required init?(coder: NSCoder) { fatalError() }

    func setMessage(_ s: String) { label.stringValue = s; needsLayout = true }

    @objc private func fire() { handler?() }

    override func updateLayer() {
        layer?.backgroundColor = NSColor.windowBackgroundColor.cgColor
        layer?.borderColor = NSColor.separatorColor.cgColor
    }

    override var wantsUpdateLayer: Bool { true }

    override func layout() {
        super.layout()
        let pad: CGFloat = 10
        var right = bounds.width - pad
        if handler != nil {
            button.sizeToFit()
            let bw = button.frame.width
            button.frame = NSRect(x: right - bw, y: (bounds.height - button.frame.height) / 2, width: bw, height: button.frame.height)
            right -= bw + pad
        }
        let lh = label.intrinsicContentSize.height
        label.frame = NSRect(x: pad, y: (bounds.height - lh) / 2, width: max(0, right - pad), height: lh)
    }

    /// Called on a click on the bar; the click itself goes no further.
    var onClick: (() -> Void)?

    /// The bar takes clicks and typing itself; nothing reaches the terminal beneath it.
    /// Command chords still reach the app menu through performKeyEquivalent.
    override var acceptsFirstResponder: Bool { true }
    override func mouseDown(with event: NSEvent) { onClick?() }
    override func keyDown(with event: NSEvent) {}
    override func keyUp(with event: NSEvent) {}
}
