import AppKit
import Foundation

/// Park and Resume run herdr-control's own tool (`herdr-lane park|unpark`), so the sidebar,
/// the TUI's "parked N" group and every other reader see one modes file change.
enum ParkActions {
    /// The helper updates modes on the server and returns them for the local sidebar.
    static func run(_ action: String, tab: String, note: String? = nil, done: @escaping (Bool, String) -> Void) {
        var args = [tab, "--by=\(RemoteActions.by)"]
        if let note, !note.isEmpty { args += ["--note=\(note)"] }
        RemoteActions.run(verb: action, args: args) { ok, message in
            if !ok {
                log("park: \(action) \(tab) -> failed \(message)")
                let alert = NSAlert()
                alert.messageText = action == "park" ? "Park failed" : "Resume failed"
                alert.informativeText = message
                alert.runModal()
            }
            done(ok, message)
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
