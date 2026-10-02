import Foundation
import Combine
import Darwin

struct ChatItem: Identifiable, Codable, Equatable {
    var id: String
    var kind: String
    var text: String
    var tool: String? = nil
    var status: String? = nil
    var input: String? = nil
    var result: String? = nil
    var queued: Bool = false
}

final class Transcript: ObservableObject {
    static let block = 2 * 1024 * 1024
    private static let bytesLock = NSLock()
    private static var readBytes: UInt64 = 0
    /// Bytes this process has read from transcript files. The check uses it to prove a tail read.
    static var bytesRead: UInt64 {
        bytesLock.lock(); defer { bytesLock.unlock() }
        return readBytes
    }
    static func noteRead(_ count: Int) {
        guard count > 0 else { return }
        bytesLock.lock(); readBytes += UInt64(count); bytesLock.unlock()
    }
    @Published private(set) var items: [ChatItem] = []
    @Published private(set) var state = "asleep"
    @Published private(set) var waiting = false
    @Published private(set) var earlier = false
    @Published private(set) var workingSince = Date()
    /// What the composer calls the agent: the pane's terminal title, else "Claude".
    @Published private(set) var name = "Claude"
    private let queue = DispatchQueue(label: "herdr.chat.transcript", qos: .utility)
    private var rows: [ChatItem] = []
    private var path = "", session = "", cwd = ""
    private var offset: UInt64 = 0, first: UInt64 = 0
    private var dev: UInt64 = 0, ino: UInt64 = 0, mtimeSec: Int64 = 0, mtimeNsec: Int64 = 0
    private var renderLimit = 400
    private var pending = Data()
    private var watcher: DispatchSourceFileSystemObject?
    private var timer: DispatchSourceTimer?
    private var tick = 0
    let pane: String?
    let dump: String?

    /// `state`/`name` are for a `--transcript` render with no pane (fixture screenshots);
    /// a live pane takes both from `herdr agent get`.
    init(pane: String?, file: String?, dump: String?, state: String? = nil, name: String? = nil) {
        self.pane = pane; self.dump = dump
        if let state { self.state = state }
        if let name, !name.isEmpty { self.name = name }
        queue.async { [weak self] in
            guard let self else { return }
            if let file { self.switchFile(file) } else { self.refreshSession() }
            let timer = DispatchSource.makeTimerSource(queue: self.queue)
            timer.schedule(deadline: .now() + 1, repeating: 1)
            timer.setEventHandler { [weak self] in
                guard let self else { return }
                self.tick += 1
                if self.pane != nil && self.tick % 2 == 0 { self.refreshSession() }
                self.read()
            }
            self.timer = timer; timer.resume()
        }
    }
    deinit { timer?.cancel(); watcher?.cancel() }
    static func transcriptPath(home: String, cwd: String, session: String) -> String {
        let encoded = cwd.unicodeScalars.map { scalar -> String in
            let v = scalar.value
            return (65...90).contains(v) || (97...122).contains(v) || (48...57).contains(v) ? String(scalar) : "-"
        }.joined()
        return home + "/projects/" + encoded + "/" + session + ".jsonl"
    }
    /// "0.8s", "42s", "1m 24s", "2h 05m".
    static func duration(_ ms: Double) -> String {
        let s = ms / 1000
        if s < 10 { return String(format: "%.1fs", s) }
        let whole = Int(s.rounded())
        if whole < 60 { return "\(whole)s" }
        if whole < 3600 { return "\(whole / 60)m \(whole % 60)s" }
        return String(format: "%dh %02dm", whole / 3600, (whole % 3600) / 60)
    }
    private func refreshSession() {
        guard let pane, let a = try? ChatCLI.agent(pane) else { return }
        let next = a["agent"] as? String == "claude" ? (a["agent_session"] as? [String: Any])?["value"] as? String ?? "" : ""
        let status = next.trimmingCharacters(in: .whitespaces).isEmpty ? "asleep" : (a["agent_status"] as? String ?? "idle")
        let title = (a["terminal_title_stripped"] as? String ?? "").trimmingCharacters(in: .whitespaces)
        DispatchQueue.main.async {
            if self.state != status && status == "working" { self.workingSince = Date() }
            self.state = status
            self.name = title.isEmpty ? "Claude" : title
        }
        cwd = a["cwd"] as? String ?? ""
        if next != session {
            session = next
            let home = ProcessInfo.processInfo.environment["CLAUDE_HOME"] ?? NSHomeDirectory() + "/.claude"
            switchFile(next.isEmpty ? "" : Self.transcriptPath(home: home, cwd: cwd, session: next))
        }
    }
    private func switchFile(_ file: String) {
        watcher?.cancel(); watcher = nil
        path = file; offset = 0; pending = Data(); rows = []; renderLimit = 400
        dev = 0; ino = 0; mtimeSec = 0; mtimeNsec = 0
        read(initial: true)
    }
    private func watch() {
        guard watcher == nil else { return }
        let fd = open(path, O_EVTONLY)
        guard fd >= 0 else { return }
        let source = DispatchSource.makeFileSystemObjectSource(fileDescriptor: fd, eventMask: [.write, .extend, .delete, .rename], queue: queue)
        source.setEventHandler { [weak self] in
            guard let self else { return }
            if self.watcher?.data.contains(.delete) == true || self.watcher?.data.contains(.rename) == true { self.watcher?.cancel(); self.watcher = nil }
            self.read()
        }
        source.setCancelHandler { close(fd) }
        watcher = source; source.resume()
    }
    private func fileStamp(_ handle: FileHandle) -> (dev: UInt64, ino: UInt64, sec: Int64, nsec: Int64) {
        var st = stat()
        guard fstat(handle.fileDescriptor, &st) == 0 else { return (0, 0, 0, 0) }
        return (UInt64(st.st_dev), st.st_ino, Int64(st.st_mtimespec.tv_sec), Int64(st.st_mtimespec.tv_nsec))
    }
    private func read(initial: Bool = false) {
        guard !path.isEmpty, let handle = FileHandle(forReadingAtPath: path) else {
            DispatchQueue.main.async { self.waiting = !self.path.isEmpty; self.items = [] }; return
        }
        defer { try? handle.close() }
        guard let size = try? handle.seekToEnd() else { return }
        let stamp = fileStamp(handle)
        let identityChanged = !initial && (stamp.dev != dev || stamp.ino != ino)
        let mtimeChanged = !initial && (stamp.sec != mtimeSec || stamp.nsec != mtimeNsec)
        // An append-only transcript only grows. Unchanged or smaller, with a new mtime or inode, is a rewrite of the tail.
        let stale = size <= offset && offset > 0 && (mtimeChanged || identityChanged)
        let reset = initial || size < offset || identityChanged || stale
        if reset {
            rows = []; pending = Data(); offset = size > UInt64(Self.block) ? size - UInt64(Self.block) : 0; first = offset
            dev = stamp.dev; ino = stamp.ino; mtimeSec = stamp.sec; mtimeNsec = stamp.nsec
        }
        if size > offset && size - offset > UInt64(Self.block) { offset = size - UInt64(Self.block); pending = Data() }
        guard size > offset || reset else { watch(); return }
        try? handle.seek(toOffset: offset)
        guard let bytes = try? handle.read(upToCount: Self.block) else { return }
        Self.noteRead(bytes.count)
        let dropPartial = pending.isEmpty && offset > 0 && (reset || size - offset >= UInt64(Self.block))
        offset += UInt64(bytes.count)
        var data = pending + bytes
        if dropPartial { if let newline = data.firstIndex(of: 10) { data.removeSubrange(...newline) } else { data = Data() } }
        if let last = data.lastIndex(of: 10) {
            let complete = data.prefix(upTo: last)
            for line in String(decoding: complete, as: UTF8.self).split(separator: "\n") { feed(String(line)) }
            pending = Data(data.suffix(from: data.index(after: last)))
        } else { pending = data }
        if pending.count > Self.block { pending = Data() }
        dev = stamp.dev; ino = stamp.ino; mtimeSec = stamp.sec; mtimeNsec = stamp.nsec
        publish(); watch()
    }
    func loadEarlier() {
        queue.async {
            guard self.first > 0, let handle = FileHandle(forReadingAtPath: self.path) else { return }
            defer { try? handle.close() }
            let end = self.first, start = end > UInt64(Self.block) ? end - UInt64(Self.block) : 0
            try? handle.seek(toOffset: start)
            guard var data = try? handle.read(upToCount: Int(end - start) + Self.block) else { return }
            Self.noteRead(data.count)
            // Include the boundary line, but stop at the first newline at/after the old boundary.
            let boundary = Int(end - start)
            if boundary < data.count, let last = data[boundary...].firstIndex(of: 10) { data = Data(data.prefix(through: last)) }
            if start > 0, let newline = data.firstIndex(of: 10) { data.removeSubrange(...newline) }
            let old = self.rows; self.rows = []
            for line in String(decoding: data, as: UTF8.self).split(separator: "\n") { self.feed(String(line)) }
            let ids = Set(self.rows.map(\.id)); self.rows += old.filter { !ids.contains($0.id) }
            let added = self.rows.count - old.count
            if added > 0 { self.renderLimit += added }
            self.first = start; self.publish()
        }
    }
    private func publish() {
        if rows.count > renderLimit { rows = Array(rows.suffix(renderLimit)) }
        let snapshot = rows, hasEarlier = first > 0
        DispatchQueue.main.async {
            self.items = snapshot; self.waiting = false; self.earlier = hasEarlier
            if let dump = self.dump, let data = try? JSONEncoder().encode(snapshot) { try? data.write(to: URL(fileURLWithPath: dump), options: .atomic) }
        }
    }
    private func unwrap(_ text: String) -> String {
        let pattern = #"^\s*<pasted_content id="([^"]+)">\n([\s\S]*)\n</pasted_content id="\1">\s*$"#
        guard let regex = try? NSRegularExpression(pattern: pattern), let match = regex.firstMatch(in: text, range: NSRange(text.startIndex..., in: text)), let range = Range(match.range(at: 2), in: text) else { return text }
        return String(text[range])
    }
    private func feed(_ line: String) {
        guard let data = line.data(using: .utf8), let r = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any], r["isSidechain"] as? Bool != true, r["isMeta"] as? Bool != true, r["isCompactSummary"] as? Bool != true else { return }
        let type = r["type"] as? String ?? "", id = r["uuid"] as? String ?? String(offset) + ":" + String(rows.count)
        let message = r["message"] as? [String: Any] ?? [:], attachment = r["attachment"] as? [String: Any] ?? [:]
        if type == "user", (r["origin"] as? [String: Any])?["kind"] as? String == "human", let text = message["content"] as? String {
            rows.append(ChatItem(id: id, kind: "user", text: unwrap(text))); return
        }
        let queued = type == "attachment" ? attachment : r
        if (queued["type"] as? String == "queued_command" || type == "queue-operation"), (queued["origin"] as? [String: Any])?["kind"] as? String == "human", let text = queued["prompt"] as? String ?? queued["content"] as? String {
            rows.append(ChatItem(id: id, kind: "user", text: unwrap(text), queued: true)); return
        }
        if type == "system", r["subtype"] as? String == "turn_duration" {
            rows.append(ChatItem(id: id, kind: "duration", text: Self.duration(r["durationMs"] as? Double ?? 0))); return
        }
        if let blocks = message["content"] as? [[String: Any]] {
            for (index, b) in blocks.enumerated() {
                if type == "assistant", b["type"] as? String == "text", let text = b["text"] as? String, !text.isEmpty { rows.append(ChatItem(id: "\(id):\(index)", kind: "assistant", text: text)) }
                if type == "assistant", b["type"] as? String == "tool_use", let toolID = b["id"] as? String, let name = b["name"] as? String {
                    let input = b["input"] as? [String: Any] ?? [:]
                    var summary = ""
                    if ["Read", "Edit", "Write"].contains(name) { summary = input["file_path"] as? String ?? ""; if !cwd.isEmpty && summary.hasPrefix(cwd + "/") { summary = String(summary.dropFirst(cwd.count + 1)) } }
                    else { summary = input["description"] as? String ?? input["command"] as? String ?? input.sorted(by: { $0.key < $1.key }).compactMap { $0.value as? String }.first ?? "" }
                    let json = (try? JSONSerialization.data(withJSONObject: input, options: [.prettyPrinted, .sortedKeys])) ?? Data()
                    var detail = String(decoding: json, as: UTF8.self)
                    if name == "Edit" || name == "Write" {
                        let old = input["old_string"] as? String ?? "", new = input["new_string"] as? String ?? input["content"] as? String ?? ""
                        detail = "--- old\n+++ new\n" + old.components(separatedBy: "\n").map { "-" + $0 }.joined(separator: "\n") + "\n" + new.components(separatedBy: "\n").map { "+" + $0 }.joined(separator: "\n")
                    }
                    rows.append(ChatItem(id: toolID, kind: "tool", text: String(summary.components(separatedBy: .newlines).first!.prefix(80)), tool: name, status: "running", input: detail))
                }
                if type == "user", b["type"] as? String == "tool_result", let toolID = b["tool_use_id"] as? String, let i = rows.firstIndex(where: { $0.id == toolID }) {
                    rows[i].status = b["is_error"] as? Bool == true ? "error" : "done"
                    let result = b["content"] as? String ?? (b["content"] as? [[String: Any]] ?? []).compactMap { $0["text"] as? String }.joined(separator: "\n")
                    rows[i].result = String(result.prefix(4096))
                }
            }
        }
        if type == "attachment", attachment["type"] as? String == "hook_additional_context" {
            let content = attachment["content"] as? String ?? (attachment["content"] as? [String] ?? []).joined(separator: "\n")
            let regex = try! NSRegularExpression(pattern: #"^\[lane bulletin\] \S+ from (\S+) \([^)]*\): (.*)$"#, options: .anchorsMatchLines)
            for (index, match) in regex.matches(in: content, range: NSRange(content.startIndex..., in: content)).enumerated() {
                if let p = Range(match.range(at: 1), in: content), let t = Range(match.range(at: 2), in: content) { rows.append(ChatItem(id: "\(id):\(index)", kind: "note", text: "from \(content[p]): \(content[t].prefix(200))")) }
            }
        }
    }
}
