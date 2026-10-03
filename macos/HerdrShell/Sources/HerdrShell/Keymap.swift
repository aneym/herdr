import AppKit

/// The app's chord table, loaded from Resources/keymap.json.
///
/// Two lists, one rule: the terminal keeps every key the keymap does not claim
/// (agent-zero's key-ownership decision).
///  - `entries`: chords the app claims. Firing one runs an app action and sends
///    nothing to the pane.
///  - `terminal`: chords the app does not claim but translates into another key for
///    the pane (Alex's Ghostty sends `text:` bytes for these today), so they still reach it.
///
/// Chords match on the physical key (US ANSI virtual keycode) plus the exact set
/// of cmd/shift/opt/ctrl, so a chord means the same thing under any layout and the
/// dead-key state of the input source never changes what a chord is.
final class Keymap {
    struct Chord: Hashable {
        let keyCode: UInt16
        let mods: NSEvent.ModifierFlags

        func hash(into h: inout Hasher) { h.combine(keyCode); h.combine(mods.rawValue) }
    }

    struct Entry {
        let chord: String
        let action: String
        let title: String
        let menu: String
        /// Piece that will implement the action; nil when it works today.
        let pending: String?
        let parsed: Chord
        /// Claimed only while its `isActive` condition holds (added in code, not in keymap.json).
        var contextual = false
    }

    struct TerminalEntry {
        let chord: String
        /// The key the pane gets instead, as a chord (cmd+backspace -> ctrl+u).
        let send: Chord
        let parsed: Chord
    }

    static let modMask: NSEvent.ModifierFlags = [.command, .shift, .option, .control]

    static let shared = Keymap(path: args["keymap"] ?? shellResource("keymap.json"))

    private(set) var entries: [Entry] = []
    private(set) var terminal: [TerminalEntry] = []
    private var byChord: [Chord: Entry] = [:]
    private var terminalByChord: [Chord: TerminalEntry] = [:]

    /// Runs an app action by name; returns whether the action was handled. Set by
    /// the window controller.
    var dispatcher: ((String) -> Bool)?

    init(path: String) {
        guard let data = FileManager.default.contents(atPath: path),
              let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else {
            log("keymap: cannot read \(path); no app chords are claimed")
            return
        }
        for e in obj["entries"] as? [[String: Any]] ?? [] {
            guard let chord = e["chord"] as? String, let action = e["action"] as? String,
                  let parsed = Self.parse(chord) else {
                log("keymap: skipped entry \(e)")
                continue
            }
            let entry = Entry(chord: chord, action: action, title: e["title"] as? String ?? action,
                              menu: e["menu"] as? String ?? "Go", pending: e["pending"] as? String, parsed: parsed)
            if byChord[parsed] != nil { log("keymap: duplicate chord \(chord); first entry wins"); continue }
            byChord[parsed] = entry
            entries.append(entry)
        }
        for e in obj["terminal"] as? [[String: Any]] ?? [] {
            guard let chord = e["chord"] as? String, let sendName = e["send"] as? String,
                  let send = Self.parse(sendName), let parsed = Self.parse(chord), byChord[parsed] == nil else {
                log("keymap: skipped terminal entry \(e)")
                continue
            }
            let entry = TerminalEntry(chord: chord, send: send, parsed: parsed)
            terminalByChord[parsed] = entry
            terminal.append(entry)
        }
        log("keymap: \(entries.count) app chords, \(terminal.count) terminal chords from \(path)")
    }

    // MARK: lookup

    func entry(for event: NSEvent) -> Entry? {
        let chord = Chord(keyCode: event.keyCode, mods: event.modifierFlags.intersection(Self.modMask))
        if let hit = contextual.first(where: { $0.entry.parsed == chord && $0.isActive() }) { return hit.entry }
        return byChord[chord]
    }

    /// A chord the app claims only while a condition holds (Esc while the detail panel is open).
    /// Everything else about it is a normal entry: firing runs the action and sends nothing to the pane,
    /// and the key's release is swallowed too.
    private var contextual: [(entry: Entry, isActive: () -> Bool)] = []
    private var pendingReleases: Set<UInt16> = []

    func addContextual(chord: String, action: String, isActive: @escaping () -> Bool) {
        guard let parsed = Self.parse(chord) else { log("keymap: bad contextual chord \(chord)"); return }
        let e = Entry(chord: chord, action: action, title: action, menu: "", pending: nil, parsed: parsed, contextual: true)
        contextual.append((e, isActive))
    }

    /// True once for the key-up that follows a fired contextual chord.
    func consumeRelease(_ event: NSEvent) -> Bool { pendingReleases.remove(event.keyCode) != nil }

    /// The key the pane gets for a chord the keymap translates, or nil.
    func terminalSend(for event: NSEvent) -> Chord? {
        terminalByChord[Chord(keyCode: event.keyCode, mods: event.modifierFlags.intersection(Self.modMask))]?.send
    }

    /// Fires an entry's action. Every firing is logged (the scenario check reads
    /// the log), including actions whose piece has not landed yet.
    @discardableResult
    func fire(_ entry: Entry) -> Bool {
        log("action \(entry.action) chord=\(entry.chord)" + (entry.pending.map { " pending=\($0)" } ?? ""))
        if entry.contextual { pendingReleases.insert(entry.parsed.keyCode) }
        if entry.pending != nil { return true }
        return dispatcher?(entry.action) ?? false
    }

    // MARK: chord names

    static let keyCodes: [String: UInt16] = [
        "a": 0, "s": 1, "d": 2, "f": 3, "h": 4, "g": 5, "z": 6, "x": 7, "c": 8, "v": 9, "b": 11, "q": 12,
        "w": 13, "e": 14, "r": 15, "y": 16, "t": 17, "1": 18, "2": 19, "3": 20, "4": 21, "6": 22, "5": 23,
        "=": 24, "9": 25, "7": 26, "-": 27, "8": 28, "0": 29, "]": 30, "o": 31, "u": 32, "[": 33, "i": 34,
        "p": 35, "return": 36, "l": 37, "j": 38, "'": 39, "k": 40, ";": 41, "\\": 42, ",": 43, "/": 44,
        "n": 45, "m": 46, ".": 47, "tab": 48, "space": 49, "`": 50, "backspace": 51, "escape": 53,
        "left": 123, "right": 124, "down": 125, "up": 126,
    ]

    static func parse(_ chord: String) -> Chord? {
        var mods: NSEvent.ModifierFlags = []
        var key: UInt16?
        for part in chord.lowercased().split(separator: "+", omittingEmptySubsequences: true) {
            switch part {
            case "cmd", "super": mods.insert(.command)
            case "shift": mods.insert(.shift)
            case "opt", "alt", "option": mods.insert(.option)
            case "ctrl", "control": mods.insert(.control)
            default:
                guard key == nil, let k = keyCodes[String(part)] else { return nil }
                key = k
            }
        }
        // A chord that ends in "+" (cmd++) is not expressible; the split above drops it.
        guard let key else { return nil }
        return Chord(keyCode: key, mods: mods)
    }

    /// Character a menu item needs for a chord, so the menu shows and (with no
    /// terminal focused) fires the same chord.
    static func menuKey(_ entry: Entry) -> String? {
        guard let name = keyCodes.first(where: { $0.value == entry.parsed.keyCode })?.key else { return nil }
        switch name {
        case "backspace": return "\u{8}"
        case "return": return "\r"
        case "tab": return "\t"
        case "escape": return "\u{1B}"
        case "space": return " "
        case "left": return String(UnicodeScalar(NSLeftArrowFunctionKey)!)
        case "right": return String(UnicodeScalar(NSRightArrowFunctionKey)!)
        case "up": return String(UnicodeScalar(NSUpArrowFunctionKey)!)
        case "down": return String(UnicodeScalar(NSDownArrowFunctionKey)!)
        default: return name
        }
    }
}

// MARK: actions and menus

extension MainWindowController {
    /// Runs one keymap action. Returns false for a name nobody handles.
    func perform(action: String) -> Bool {
        switch action {
        case "next_pane":
            if state.mode == .areas { focusStep(1) } else { nextPane(nil) }
        case "prev_pane":
            if state.mode == .areas { focusStep(-1) } else { prevPane(nil) }
        case "toggle_area_mode":
            state.setMode(state.mode == .areas ? .spaces : .areas)
        case "toggle_docs":
            setDocs(open: !state.docOpen)
        case "filter_1": state.setChip(.all)
        case "filter_2": state.setChip(.needs)
        case "filter_3": state.setChip(.scoping)
        case "filter_4": state.setChip(.building)
        case "filter_5": state.setChip(.review)
        case "filter_6": state.setChip(.use)
        case "next_tab": nextTab(nil)
        case "prev_tab": prevTab(nil)
        case "focus_pane_left": focusNeighbor(dx: -1, dy: 0)
        case "focus_pane_right": focusNeighbor(dx: 1, dy: 0)
        case "focus_doc_address": docPanel.focusAddress()
        case "focus_pane_up": focusNeighbor(dx: 0, dy: -1)
        case "focus_pane_down": focusNeighbor(dx: 0, dy: 1)
        case "new_tab": newTab(nil)
        case "split_right": splitRight(nil)
        case "split_down": splitDown(nil)
        case "close_detail":
            if quickSwitch.isOpen { quickSwitch.dismiss(); break }
            if docPanel.hasFocus { docPanel.returnFocus(); break }
            closeDetail()
        case "close_switcher":
            quickSwitch.dismiss()
        case "search":
            quickSwitch.present(selectAll: false)
        case "goto":
            quickSwitch.present(selectAll: true)
        case "open_factory":
            toggleFactory()
        default:
            if action.hasPrefix("goto_tab_"), let n = Int(action.dropFirst("goto_tab_".count)) {
                if quickSwitch.isOpen { quickSwitch.pick(n); return true }
                let rows = model.allRowsInOrder
                if n >= 1, n <= rows.count { selectTab(rows[n - 1].id, revealDocs: state.mode == .areas) }
                return true
            }
            if action.hasPrefix("goto_space_"), let n = Int(action.dropFirst("goto_space_".count)) {
                gotoSpace(n)   // P10; no such space is a no-op
                return true
            }
            log("keymap: no handler for action \(action)")
            return false
        }
        return true
    }

    /// Menu item target: fires the entry so the log and the action match the
    /// chord path in SurfaceView.
    @objc func keymapAction(_ sender: NSMenuItem) {
        guard let name = sender.representedObject as? String,
              let entry = Keymap.shared.entries.first(where: { $0.action == name }) else { return }
        Keymap.shared.fire(entry)
    }

    /// Focus the pane next to the current one in herdr's own layout geometry: the
    /// nearest pane in that direction whose span overlaps the current pane's.
    private func focusNeighbor(dx: Int, dy: Int) {
        guard let cur = host.rects.first(where: { $0.0 === focusedSurface })?.1 else { return }
        var best: (SurfaceView, Double, Double)?
        for (s, r) in host.rects where s !== focusedSurface {
            let gap: Double, overlap: Double
            if dx != 0 {
                gap = dx < 0 ? cur.x - (r.x + r.width) : r.x - (cur.x + cur.width)
                overlap = min(cur.y + cur.height, r.y + r.height) - max(cur.y, r.y)
            } else {
                gap = dy < 0 ? cur.y - (r.y + r.height) : r.y - (cur.y + cur.height)
                overlap = min(cur.x + cur.width, r.x + r.width) - max(cur.x, r.x)
            }
            guard gap >= -0.5, overlap > 0 else { continue }
            if best == nil || gap < best!.1 || (gap == best!.1 && overlap > best!.2) { best = (s, gap, overlap) }
        }
        if let target = best?.0 { window.makeFirstResponder(target) }
    }
}

/// Menu items for every keymap entry, grouped by menu name, in file order. The
/// menu shows the chord and fires it when no terminal has focus.
func keymapMenus(target: MainWindowController) -> [(String, [NSMenuItem])] {
    var order: [String] = []
    var items: [String: [NSMenuItem]] = [:]
    for e in Keymap.shared.entries {
        guard let key = Keymap.menuKey(e) else { continue }
        let item = NSMenuItem(title: e.title, action: #selector(MainWindowController.keymapAction(_:)), keyEquivalent: key)
        item.keyEquivalentModifierMask = e.parsed.mods
        item.target = target
        item.representedObject = e.action
        if items[e.menu] == nil { order.append(e.menu) }
        items[e.menu, default: []].append(item)
    }
    return order.map { ($0, items[$0]!) }
}
