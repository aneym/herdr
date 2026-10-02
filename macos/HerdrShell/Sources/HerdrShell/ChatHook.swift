import AppKit
import CoreGraphics

/// Scenario driver for the `--demo chat` window (check_p19.py), the chat's counterpart
/// of TestHook. JSON lines on a FIFO become real key events: a CGEvent from a virtual
/// keycode, turned into an NSEvent addressed to this window and dispatched the way AppKit
/// dispatches a physical key, so the composer's keyDown runs. Nothing is posted
/// system-wide.
///   {"cmd":"type","text":"..."}            one key event per character
///   {"cmd":"key","key":"return","mods":["shift"|"cmd"]}
///   {"cmd":"state","out":"<path>"}         composer, sender and transcript state as JSON
///   {"cmd":"shot","out":"<path>"}          this window as a PNG
///   {"cmd":"click","target":"cancel|send_anyway"}  the draft warning's buttons
///   {"cmd":"click","target":"group","id":"<first tool id>"}  a tool run's line
///   {"cmd":"mode","mode":"focus|full"}     the header's Focus | Full switch
///   {"cmd":"load_earlier"}                 the "Load earlier messages" button
///   {"cmd":"race"}                         two 600-scalar sends on this pane, at once
/// Clicks call the button's own action: SwiftUI takes real mouse clicks only in the
/// active app, and a check must never take focus from what the person is typing into.
final class ChatHook {
    let path: String
    weak var window: NSWindow?
    let transcript: Transcript
    let sender: ChatSender
    let ui: ChatUI
    var delivered: [String] = []
    var racers: [ChatSender] = []

    init(path: String, window: NSWindow, transcript: Transcript, sender: ChatSender, ui: ChatUI) {
        self.path = path; self.window = window; self.transcript = transcript; self.sender = sender; self.ui = ui
    }

    func start() {
        unlink(path)
        guard mkfifo(path, 0o600) == 0 else { log("chat hook: mkfifo failed: \(path)"); return }
        Thread.detachNewThread { [self] in
            while true {
                guard let fh = FileHandle(forReadingAtPath: path) else { return }
                let data = fh.readDataToEndOfFile()
                fh.closeFile()
                for line in String(decoding: data, as: UTF8.self).split(separator: "\n") {
                    let sem = DispatchSemaphore(value: 0)
                    DispatchQueue.main.async { self.handle(String(line)); sem.signal() }
                    sem.wait()
                }
            }
        }
    }

    private func handle(_ line: String) {
        guard let obj = try? JSONSerialization.jsonObject(with: Data(line.utf8)) as? [String: Any],
              let cmd = obj["cmd"] as? String else { log("chat hook: bad line \(line)"); return }
        switch cmd {
        case "type": for ch in obj["text"] as? String ?? "" { key(String(ch), mods: []) }
        case "key": key(obj["key"] as? String ?? "", mods: obj["mods"] as? [String] ?? [])
        case "state": writeState(obj["out"] as? String ?? "/dev/stderr")
        case "shot": if let w = window { Self.capture(w, to: obj["out"] as? String ?? "/tmp/chat.png") }
        case "click": click(obj["target"] as? String ?? "", id: obj["id"] as? String)
        case "mode": ui.set(ChatUI.Mode(rawValue: obj["mode"] as? String ?? "") ?? .focus)
        case "load_earlier": transcript.loadEarlier()
        case "race": race()
        default: log("chat hook: unknown cmd \(cmd)")
        }
    }

    private func key(_ name: String, mods: [String]) {
        guard let (code, shift) = TestHook.codes[name], let w = window else { log("chat hook: no keycode for \(name)"); return }
        var flags: CGEventFlags = []
        if shift || mods.contains("shift") { flags.insert(.maskShift) }
        if mods.contains("cmd") { flags.insert(.maskCommand) }
        if mods.contains("ctrl") { flags.insert(.maskControl) }
        if mods.contains("opt") { flags.insert(.maskAlternate) }
        for down in [true, false] {
            guard let cg = CGEvent(keyboardEventSource: nil, virtualKey: code, keyDown: down) else { continue }
            cg.flags = flags
            guard let ev = NSEvent(cgEvent: cg),
                  let addressed = NSEvent.keyEvent(with: ev.type, location: .zero, modifierFlags: ev.modifierFlags,
                                                   timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: w.windowNumber,
                                                   context: nil, characters: ev.characters ?? "",
                                                   charactersIgnoringModifiers: ev.charactersIgnoringModifiers ?? "",
                                                   isARepeat: false, keyCode: ev.keyCode) else { continue }
            if w.isKeyWindow { NSApp.sendEvent(addressed) } else { w.sendEvent(addressed) }
            if down { delivered.append((mods + [name]).joined(separator: "+")) }
        }
    }

    /// Two senders, one pane. A process-wide queue keeps their chunks from mixing.
    private func race() {
        let other = ChatSender(pane: sender.pane, readOnly: false)
        racers.append(other)
        let a = String(repeating: "A", count: 600)
        let b = String(repeating: "B", count: 600)
        sender.send(a, anyway: true)
        other.send(b, anyway: true)
    }

    private func click(_ target: String, id: String?) {
        switch target {
        case "group":
            if let group = Self.groups(transcript.items).first(where: { $0.first?.id == id }) { ui.toggle(group) }
        case "cancel": _ = sender.cancel()
        case "send_anyway": sender.send(sender.pending, anyway: true, known: transcript.items.map(\.id))
        default: log("chat hook: unknown click target \(target)")
        }
    }

    /// Runs of consecutive tool items, as the view groups them.
    static func groups(_ items: [ChatItem]) -> [[ChatItem]] {
        var out: [[ChatItem]] = [], previousWasTool = false
        for item in items {
            let tool = item.kind == "tool"
            if tool && previousWasTool { out[out.count - 1].append(item) } else if tool { out.append([item]) }
            previousWasTool = tool
        }
        return out
    }

    private func writeState(_ out: String) {
        let composer = (window?.firstResponder as? NSTextView)
        let state: [String: Any] = [
            "composer_focused": composer != nil,
            "composer_text": composer?.string ?? "",
            "status": sender.status,
            "warning": sender.warning,
            "pending": sender.pending,
            "agent_state": transcript.state,
            "agent_name": transcript.name,
            "items": transcript.items.map { ["id": $0.id, "kind": $0.kind, "text": $0.text] },
            "delivered": delivered,
            "earlier": transcript.earlier,
            "window_on_screen": window?.isVisible ?? false,
            "app_active": NSApp.isActive,
        ].merging(Self.view(transcript, ui)) { a, _ in a }
        if let data = try? JSONSerialization.data(withJSONObject: state, options: [.sortedKeys]) {
            try? data.write(to: URL(fileURLWithPath: out), options: .atomic)
        }
    }

    /// What the view shows of the tool calls: Focus or Full, each run (open or folded),
    /// the rows drawn, and the live line. `--dump-chat` writes it beside the item dump.
    static func view(_ transcript: Transcript, _ ui: ChatUI) -> [String: Any] {
        [
            "mode": ui.mode.rawValue,
            "tool_groups": groups(transcript.items).map { g in
                ["first": g[0].id, "count": g.count, "tools": g.compactMap(\.tool), "failed": g.filter { $0.status == "error" }.count,
                 "open": ui.isOpen(g)] as [String: Any]
            },
            "rendered_tools": Array(ui.rendered).sorted(),
            "activity": transcript.state == "working" ? ChatUI.activity(transcript.items) : "",
            "transcript_bytes_read": Transcript.bytesRead,
        ]
    }

    /// This window's content as a PNG, drawn by AppKit (no Screen Recording grant needed).
    static func capture(_ window: NSWindow, to out: String) {
        guard let content = window.contentView, let bitmap = content.bitmapImageRepForCachingDisplay(in: content.bounds) else { return }
        content.cacheDisplay(in: content.bounds, to: bitmap)
        if let png = bitmap.representation(using: .png, properties: [:]) { try? png.write(to: URL(fileURLWithPath: out), options: .atomic) }
    }
}
