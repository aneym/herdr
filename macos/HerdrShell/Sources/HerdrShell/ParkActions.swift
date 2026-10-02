import AppKit
import Foundation

/// Park and Resume run herdr-control's own tool (`herdr-lane park|unpark`), so the sidebar,
/// the TUI's "parked N" group and every other reader see one modes file change.
enum ParkActions {
    static var laneBin = ""
    static var kindBin: String?
    static var herdrBin = ""
    static let by = "herdr-shell"

    /// Runs off the main thread; `done` gets the tool's exit status and its last output line.
    static func run(_ action: String, tab: String, note: String? = nil, done: @escaping (Bool, String) -> Void) {
        var args = [action, tab, "--by", by]
        if let note, !note.isEmpty { args += ["--note", note] }
        DispatchQueue.global(qos: .userInitiated).async {
            let p = Process()
            // lane.js is `#!/usr/bin/env node`; a Finder launch has no Homebrew on PATH.
            p.executableURL = URL(fileURLWithPath: "/usr/bin/env")
            p.arguments = [laneBin] + args
            var env = ProcessInfo.processInfo.environment
            env["PATH"] = "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin" + (env["PATH"].map { ":" + $0 } ?? "")
            env["HERDR_BIN_PATH"] = herdrBin
            env["CONTROL_MODES"] = ShellPaths.modes
            if let kindBin { env["HERDR_KIND_BIN"] = kindBin }
            p.environment = env
            let out = Pipe()
            p.standardOutput = out
            p.standardError = out
            var ok = false, text = ""
            do {
                try p.run()
                let data = out.fileHandleForReading.readDataToEndOfFile()
                p.waitUntilExit()
                ok = p.terminationStatus == 0
                text = String(decoding: data, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)
            } catch {
                text = "\(error)"
            }
            let last = text.split(separator: "\n").last.map(String.init) ?? ""
            log("park: \(action) \(tab) -> \(ok ? "ok" : "failed") \(last)")
            DispatchQueue.main.async { done(ok, last) }
        }
    }

    /// The Park prompt: an optional note, saved with the park so the row says why.
    @MainActor
    static func askNote(name: String, window: NSWindow?) -> String? {
        let alert = NSAlert()
        alert.messageText = "Park \(name)?"
        alert.informativeText = "It leaves every filter but Parked and stops counting in Needs you. Resume puts it back."
        alert.addButton(withTitle: "Park")
        alert.addButton(withTitle: "Cancel")
        let field = NSTextField(frame: NSRect(x: 0, y: 0, width: 300, height: 24))
        field.placeholderString = "Note (optional): why, and when to come back"
        alert.accessoryView = field
        alert.window.initialFirstResponder = field
        guard alert.runModal() == .alertFirstButtonReturn else { return nil }
        return field.stringValue.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    /// "Oct 2" today and this year, "Oct 2 2025" otherwise; with the time when it is today.
    static func when(_ d: Date?, now: Date = Date()) -> String {
        guard let d else { return "" }
        let f = DateFormatter()
        f.locale = Locale(identifier: "en_US_POSIX")
        let cal = Calendar.current
        if cal.isDate(d, inSameDayAs: now) { f.dateFormat = "'today' h:mm a" }
        else if cal.component(.year, from: d) == cal.component(.year, from: now) { f.dateFormat = "MMM d" }
        else { f.dateFormat = "MMM d yyyy" }
        return f.string(from: d)
    }
}
