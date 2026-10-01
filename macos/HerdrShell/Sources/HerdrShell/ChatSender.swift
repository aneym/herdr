import Foundation
import Combine

struct ChatCLI {
    static func run(_ args: [String]) throws -> String {
        let p = Process(), output = Pipe()
        let env = ProcessInfo.processInfo.environment
        p.executableURL = URL(fileURLWithPath: "/usr/bin/env")
        p.arguments = [env["HERDR_BIN"] ?? "herdr"] + args
        p.standardOutput = output; p.standardError = FileHandle.nullDevice
        try p.run()
        let data = output.fileHandleForReading.readDataToEndOfFile()
        p.waitUntilExit()
        guard p.terminationStatus == 0 else { throw NSError(domain: "ChatCLI", code: Int(p.terminationStatus)) }
        return String(decoding: data, as: UTF8.self)
    }
    static func agent(_ pane: String) throws -> [String: Any] {
        let data = Data(try run(["agent", "get", pane]).utf8)
        guard let root = try JSONSerialization.jsonObject(with: data) as? [String: Any], let result = root["result"] as? [String: Any], let agent = result["agent"] as? [String: Any] else { throw NSError(domain: "ChatCLI", code: 1) }
        return agent
    }
    static func hasDraft(_ screen: String) -> Bool {
        let regex = try! NSRegularExpression(pattern: "\u{1b}\\[([0-9;:]*)m|\u{1b}\\[[0-?]*[ -/]*[@-~]")
        var visible = "", plain = "", faint = false, offset = screen.startIndex
        func append(_ s: String) {
            let text = s.replacingOccurrences(of: "\u{a0}", with: " ")
            plain += text
            visible += faint ? String(text.map { $0 == "\n" || $0 == "\r" ? $0 : " " }) : text
        }
        for match in regex.matches(in: screen, range: NSRange(screen.startIndex..., in: screen)) {
            guard let range = Range(match.range, in: screen) else { continue }
            append(String(screen[offset..<range.lowerBound]))
            if let sgr = Range(match.range(at: 1), in: screen) {
                let codes = screen[sgr].split(whereSeparator: { $0 == ";" || $0 == ":" }).map { Int($0) ?? 0 }
                var i = 0
                while i < codes.count {
                    let c = codes[i]
                    if [38, 48, 58].contains(c) { i += i + 1 < codes.count && codes[i + 1] == 2 ? 5 : i + 1 < codes.count && codes[i + 1] == 5 ? 3 : 2; continue }
                    if c == 2 { faint = true }; if c == 0 || c == 22 { faint = false }; i += 1
                }
                if codes.isEmpty { faint = false }
            }
            offset = range.upperBound
        }
        append(String(screen[offset...]))
        let lines = plain.components(separatedBy: .newlines), shown = visible.components(separatedBy: .newlines)
        guard let start = lines.lastIndex(where: { $0.trimmingCharacters(in: .whitespaces).hasPrefix("❯") }), let marker = lines[start].firstIndex(of: "❯") else { return false }
        let count = lines[start].distance(from: lines[start].startIndex, to: marker) + 1
        var input = String(shown[start].dropFirst(count))
        for i in (start + 1)..<lines.count {
            if lines[i].trimmingCharacters(in: .whitespaces).hasPrefix("─") { break }
            input += "\n" + shown[i]
        }
        return !input.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }
}

final class ChatSender: ObservableObject {
    @Published var status = ""
    @Published var warning = false
    @Published var pending = ""
    private let pane: String
    /// A read-only window (a `--transcript` render, or `--read-only` on a real pane)
    /// never runs `send-text`.
    let readOnly: Bool
    private let queue: DispatchQueue
    private var held: [(String, Bool)] = []
    private var timer: DispatchSourceTimer?
    /// Transcript ids present when the message went out; only a newer You item acknowledges it.
    private var known = Set<String>()
    init(pane: String, readOnly: Bool = false) {
        self.pane = pane
        self.readOnly = readOnly || pane.isEmpty
        queue = DispatchQueue(label: "herdr.chat.send.\(pane)", qos: .utility)
        let timer = DispatchSource.makeTimerSource(queue: queue)
        timer.schedule(deadline: .now() + 3, repeating: 3)
        timer.setEventHandler { [weak self] in self?.drain() }
        self.timer = timer; timer.resume()
    }
    deinit { timer?.cancel() }
    func send(_ text: String, anyway: Bool = false, known ids: [String] = []) {
        guard !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty, text.count <= 20_000 else { status = "Message must contain 1 to 20,000 characters"; return }
        guard !readOnly else { status = "Read-only: this window does not send"; return }
        pending = text; warning = false; known = Set(ids)
        queue.async { self.held.append((text, anyway)); self.drain() }
    }
    /// Drops the message and hands its text back so the composer can restore it.
    func cancel() -> String { let text = pending; pending = ""; warning = false; status = ""; return text }
    private func update(_ text: String, warning: Bool = false) { DispatchQueue.main.async { self.status = text; self.warning = warning } }
    private func drain() {
        guard let (text, anyway) = held.first else { return }
        do {
            let a = try ChatCLI.agent(pane)
            guard a["agent"] as? String == "claude", let session = (a["agent_session"] as? [String: Any])?["value"] as? String, !session.isEmpty else { held.removeFirst(); update("No Claude session is running in that pane"); return }
            if a["agent_status"] as? String == "blocked" { update("held: will send when the terminal stops asking"); return }
            let screen = try ChatCLI.run(["pane", "read", pane, "--source", "visible", "--lines", "12", "--format", "ansi"])
            if !anyway && ChatCLI.hasDraft(screen) { held.removeFirst(); update("There's unsent text in the terminal", warning: true); return }
            update("sending")
            let points = Array(text.unicodeScalars)
            for start in stride(from: 0, to: points.count, by: 300) {
                let chunk = String(String.UnicodeScalarView(points[start..<min(start + 300, points.count)]))
                _ = try ChatCLI.run(["pane", "send-text", "--human", pane, chunk])
            }
            _ = try ChatCLI.run(["pane", "send-text", "--human", pane, "\r"])
            held.removeFirst()
        } catch { held.removeFirst(); update("Send failed; delivery is uncertain") }
    }
    func acknowledge(_ items: [ChatItem]) {
        guard !pending.isEmpty, !warning else { return }
        let want = pending.trimmingCharacters(in: .whitespacesAndNewlines)
        if items.contains(where: { $0.kind == "user" && !known.contains($0.id) && $0.text.trimmingCharacters(in: .whitespacesAndNewlines) == want }) { pending = ""; status = "" }
    }
}
