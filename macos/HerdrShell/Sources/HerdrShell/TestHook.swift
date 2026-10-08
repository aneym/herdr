import AppKit
import CoreGraphics
import GhosttyKit

/// Scenario driver. Reads JSON lines from a FIFO and turns them into real key
/// events: a CGEvent built from a virtual keycode, converted to NSEvent (AppKit
/// derives characters from the keyboard layout) and dispatched through
/// NSApp.sendEvent, so menus, performKeyEquivalent and keyDown all run as for a
/// physical key. Events stay inside this process; nothing is posted system-wide.
final class TestHook {
    let path: String
    weak var controller: MainWindowController?
    var delivered: [String] = []
    /// "via":"pid" posts the CGEvent to this process through the window server
    /// (CGEvent.postToPid) instead of dispatching it in-process.
    var viaPid = false

    init(path: String, controller: MainWindowController) {
        self.path = path
        self.controller = controller
    }

    func start() {
        unlink(path)
        guard mkfifo(path, 0o600) == 0 else { log("mkfifo failed: \(path)"); return }
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
              let cmd = obj["cmd"] as? String else { log("hook: bad line \(line)"); return }
        switch cmd {
        case "type":
            viaPid = (obj["via"] as? String) == "pid"
            for ch in (obj["text"] as? String ?? "") { key(String(ch), mods: []) }
        case "key":
            viaPid = (obj["via"] as? String) == "pid"
            key(obj["key"] as? String ?? "", mods: obj["mods"] as? [String] ?? [])
        case "clipboard_image":
            // {"cmd":"clipboard_image","path":"<png>"}: later pastes read a private named
            // board holding only this image, so a check never touches the user's clipboard.
            guard let p = obj["path"] as? String,
                  let png = FileManager.default.contents(atPath: p) else {
                log("hook: clipboard_image refused"); return
            }
            let board = NSPasteboard(name: NSPasteboard.Name("herdr-shell-check-\(getpid())"))
            board.clearContents()
            board.setData(png, forType: .png)
            ClipboardImagePaste.board = board
        case "reclaim":
            controller?.reclaimPane(nil)
        case "hidden_policy":
            controller?.setHiddenPolicy(obj["value"] as? String ?? "")
        case "select":
            controller?.selectTab(obj["tab"] as? String ?? "")
        case "sidebar_fold":
            // {"cmd":"sidebar_fold","id":"tab:<id>|hidden|background","open":true}: what a chevron click does.
            if let id = obj["id"] as? String, let open = obj["open"] as? Bool { controller?.state.manualOpen[id] = open }
        case "goal":
            guard let c = controller else { return }
            c.state.spacesChrome.goalFilter = obj["value"] as? String
            c.state.saveSpacesChrome()
        case "spaces_click":
            guard let c = controller else { return }
            guard let id = obj["row"] as? String, let row = c.model.spacesRows(state: c.state).first(where: { $0.id == id }) else { return }
            let part = obj["part"] as? String ?? "body"
            if part == "focus" {
                let key = String(row.id.dropFirst(8))
                let split = key.lastIndex(of: ":")!
            let space = String(key[..<split]); let label = String(key[key.index(after: split)...])
            c.state.spacesChrome.focusedSection[space] = c.state.spacesChrome.focusedSection[space] == label ? nil : label
            } else if part == "pin" {
                // As the sidebar: a tab row's pin is a server fact, a space row's a local preference.
                if row.kind == .tab, let tab = row.tab { c.model.togglePin(tab) } else { c.state.spacesChrome.toggle("pin:" + String(row.id.dropFirst(6))) }
            }
            else if part == "plus", row.id == "pinned" {
                c.model.newPinnedTab(focused: c.state.selectedTab, done: c.selectWhenListed)
            } else if part == "plus" {
                let commands = c.commands
                let space = String(row.id.dropFirst(6))
                DispatchQueue.global(qos: .userInitiated).async {
                    if let made = commands.tabCreate(workspaceId: space, cwd: nil) { DispatchQueue.main.async { c.selectWhenListed(made.tabId) } }
                }
            } else if part == "link" {
                if let raw = row.link, let url = URL(string: raw), ["http", "https"].contains(url.scheme?.lowercased() ?? "") { NSWorkspace.shared.open(url) }
            } else if part == "chevron" || [.section, .group, .hidden].contains(row.kind) {
                if let key = row.toggleKey { c.state.spacesChrome.toggle(key, open: row.chevron == "open") }
            } else if let tab = row.tab { c.selectTab(tab) }
            c.state.saveSpacesChrome()
        case "state":
            writeState(obj["out"] as? String ?? "/dev/stderr")
        case "shot":
            // {"cmd":"shot","out":path,"scale":2}: a scale above the display's renders in-process at that scale.
            shot(obj["out"] as? String ?? "/tmp/shot.png", scale: obj["scale"] as? Double)
        case "mouse":
            mouse(obj)
        case "drag_divider":
            dragDivider(obj)
        case "drag_doc_handle":
            dragDocHandle(obj)
        case "pane-drag":
            paneDragEvent(obj)
        case "motion":
            guard let drag = controller?.paneDrag else { return }
            switch obj["op"] as? String {
            case "freeze": drag.freeze(ms: CGFloat((obj["ms"] as? NSNumber)?.doubleValue ?? 0))
            case "run": drag.run()
            case "reduce": drag.reduceOverride = obj["on"] as? Bool
            default: break
            }
        case "drag_pin":
            dragPin(obj)
        case "set_hidden":
            guard let c = controller else { return }
            let row = (obj["row"] as? String).flatMap { id in c.model.spacesRows(state: c.state).first { $0.id == id }?.tab }
            guard let tab = obj["tab"] as? String ?? obj["tab_id"] as? String ?? row, let hidden = obj["hidden"] as? Bool else { return }
            c.model.setAgentHidden(tab, hidden)
        case "set_role":
            guard let c = controller else { return }
            let row = (obj["row"] as? String).flatMap { id in c.model.spacesRows(state: c.state).first { $0.id == id }?.tab }
            guard let tab = obj["tab"] as? String ?? obj["tab_id"] as? String ?? row else { return }
            c.model.setAgentRole(tab, obj["role"] as? String == "agent")
        case "pin_move":
            // {"cmd":"pin_move","row":"pinned:<tab>","to":slot}: what a drop on that slot does.
            guard let c = controller, let id = obj["row"] as? String, let to = obj["to"] as? Int else { return }
            let rows = c.model.spacesRows(state: c.state)
            guard let section = PinDrag.section(of: id), let row = rows.first(where: { $0.id == id }), let tab = row.tab else { return }
            let machine = PinDrag.machine(of: tab)
            let ids = rows.filter { PinDrag.section(of: $0.id) == section && PinDrag.machine(of: $0.tab ?? "") == machine }.compactMap(\.tab)
            if let from = ids.firstIndex(of: tab) {
                PinDrag.shared.commit(model: c.model, ids: ids, from: from, to: to)
            }
        case "new_tab":
            controller?.newTab(nil)
        case "panel_shot":
            if let c = controller {
                let out = obj["out"] as? String ?? "/tmp/panel.png"
                let ok = MainActor.assumeIsolated { c.detailPanel.renderPNG(to: out, size: CGSize(width: DetailPanelController.width, height: 640)) }
                log("panel_shot: \(ok)")
            }
        case "detail":
            // {"cmd":"detail","action":"toggle|close|full","row":"<tab_id>" or "label":"<row label>"}: the
            // sidebar click path (toggle), the panel's own button (full) or Esc's action (close).
            guard let c = controller else { break }
            let rows = c.model.allRowsInOrder
            let id = (obj["row"] as? String) ?? rows.first { $0.label == obj["label"] as? String }?.id
            switch obj["action"] as? String ?? "toggle" {
            case "close": c.closeDetail()
            case "full": if let id { c.openFullTab(id) }
            default: if let id { c.toggleDetail(id) } else { log("hook: detail: no such row") }
            }
        case "approve":
            guard let c = controller, let tab = obj["tab"] as? String,
                  let quote = obj["quote"] as? String,
                  let lane = c.model.catalog.snapshot.lanes[tab] else { break }
            DispatchQueue.main.async {
                RemoteActions.approve(scopeURL: lane.scopeURL, title: lane.name, quote: quote) { _, _ in c.model.catalog.reload() }
            }
        case "unpark":
            guard let c = controller, let tab = obj["tab"] as? String else { break }
            ParkActions.run("unpark", tab: tab, promptOnFailure: false) { _, _ in c.model.catalog.reload() }
        case "park":
            // {"cmd":"park","tab":"<id>","note":"..."}: what the row menu's Park… does after its prompt.
            guard let c = controller, let tab = obj["tab"] as? String else { break }
            ParkActions.run("park", tab: tab, note: obj["note"] as? String, promptOnFailure: false) { _, _ in c.model.catalog.reload() }
        case "click":
            click(obj)
        case "split":
            controller?.split(obj["direction"] as? String ?? "right")
        case "action":
            // {"cmd":"action","name":"<keymap action>"}: the same fire a menu item runs.
            let name = obj["name"] as? String ?? ""
            guard let entry = Keymap.shared.entries.first(where: { $0.action == name && $0.pending == nil })
                    ?? Keymap.shared.entries.first(where: { $0.action == name }) else {
                log("hook: no action \(name)"); break
            }
            Keymap.shared.fire(entry)
        case "rename":
            // {"cmd":"rename","tab":"...","label":"..."}: tab.rename without the alert.
            if let c = controller, let tab = obj["tab"] as? String, let label = obj["label"] as? String {
                c.renameTab(tab, label)
            }
        case "scroll":
            scroll(obj)
        case "scroll_gesture":
            scrollGesture(obj)
        case "frame":
            let w = CGFloat(obj["w"] as? Double ?? Double(obj["w"] as? Int ?? 1440))
            let h = CGFloat(obj["h"] as? Double ?? Double(obj["h"] as? Int ?? 900))
            controller?.window.setContentSize(NSSize(width: w, height: h))
            controller?.pinOffscreen()
        case "docs":
            let open = obj["open"] as? Bool
            let width = (obj["width"] as? Double).map { CGFloat($0) } ?? (obj["width"] as? Int).map { CGFloat($0) }
            controller?.setDocs(open: open, width: width)
        case "factory":
            controller?.setFactory(open: obj["open"] as? Bool ?? true)
        case "pane_mode":
            if let id = obj["id"] as? String, let mode = obj["mode"] as? String {
                controller?.setPaneMode(id, mode)
            }
        case "updates":
            controller?.updates?.checkNow()
        case "appearance":
            // {"cmd":"appearance","mode":"system|light|dark"}: the live override.
            controller?.theme.override = AppearanceOverride(rawValue: obj["mode"] as? String ?? "") ?? .system
        case "glass":
            // {"cmd":"glass","surface":"sidebar|overlay","on":true}
            guard let t = controller?.theme else { break }
            var g = t.glass
            let on = obj["on"] as? Bool ?? false
            if (obj["surface"] as? String) == "sidebar" { g.sidebar = on }
            if (obj["surface"] as? String) == "overlay" { g.overlay = on }
            t.glass = g
        case "ime_sim":
            // {"cmd":"ime_sim","on":true}: replays the US dead key opt+e, e -> e-acute through the
            // NSTextInputClient calls the system input method makes (setMarkedText, then
            // insertText). Needed because a lab app is never the active app, and the system
            // only composes for the active app.
            guard let s = controller?.focusedSurface else { break }
            if obj["on"] as? Bool ?? true {
                let none = NSRange(location: NSNotFound, length: 0)
                s.interpretOverride = { [weak s] ev in
                    guard let s else { return }
                    if ev.keyCode == 14, ev.modifierFlags.contains(.option), !s.hasMarkedText() {
                        s.setMarkedText("\u{B4}", selectedRange: NSRange(location: 1, length: 0), replacementRange: none)
                    } else if ev.keyCode == 14, s.hasMarkedText() {
                        s.insertText("\u{E9}", replacementRange: none)
                    } else {
                        s.interpretKeyEvents([ev])
                    }
                }
            } else {
                s.interpretOverride = nil
            }
        case "open_url_sim":
            // Same path as GHOSTTY_ACTION_OPEN_URL, without a surface under the pointer.
            GhosttyRuntime.openLink(obj["url"] as? String ?? "", paneId: nil)
        case "activate":
            if agentRun {
                log("hook: activate ignored (--agent-run)")
            } else {
                NSApp.activate(ignoringOtherApps: true)
                controller?.window.makeKeyAndOrderFront(nil)
            }
        case "switcher":
            // {"cmd":"switcher","open":true|false,"query":"...","pick":N}
            // pick is 1-based, the same row ⌘N chooses.
            guard let c = controller else { break }
            if let q = obj["query"] as? String { c.quickSwitch.setQuery(q) }
            if let open = obj["open"] as? Bool {
                if open { c.quickSwitch.present(selectAll: false) } else { c.quickSwitch.dismiss() }
            }
            if let n = obj["pick"] as? Int { c.quickSwitch.pick(n) }
        default:
            log("hook: unknown cmd \(cmd)")
        }
    }

    /// A real left click (mouse down, then up, through NSApp.sendEvent, so hit-testing and the
    /// SwiftUI tap gestures run as for a physical mouse) on a sidebar row or the detail panel's
    /// "open full tab" button. {"cmd":"click","target":"row","label":"<row title>"} or
    /// {"cmd":"click","target":"open_full"}. The point is the centre of the frame the view reported.
    /// SwiftUI ignores clicks while the app is inactive, so the caller sends {"cmd":"activate"} first.
    private func click(_ obj: [String: Any]) {
        guard let c = controller else { return }
        if agentRun {
            clickAction(obj, c)
            return
        }
        let view: NSView, frame: CGRect
        let target = obj["target"] as? String ?? "row"
        switch target {
        case "open_full":
            view = c.detailPanel.view
            guard let f = c.detailPanel.model.targets["open_full"] else { log("hook: click: no open_full button"); return }
            frame = f
        case "chip", "mode", "only":
            view = c.sidebarHostView
            let key: String
            if target == "only" { key = "only" }
            else { key = "\(target):\(obj["label"] as? String ?? "")" }
            guard let f = c.state.rowFrames[key] else { log("hook: click: no \(key)"); return }
            frame = f
        case "area":
            view = c.sidebarHostView
            let label = obj["label"] as? String
            guard let line = c.sidebarLines.first(where: { $0.kind == .area && $0.title == label }),
                  let f = c.state.rowFrames[line.id] else { log("hook: click: no area \(label ?? "?")"); return }
            frame = f
        case "pinned_plus":
            view = c.sidebarHostView
            guard let f = c.state.rowFrames["pinned+"] else { log("hook: click: no pinned +"); return }
            frame = f
        case "focus":
            view = c.sidebarHostView
            guard let f = c.state.rowFrames["focus"] else { log("hook: click: no focus row"); return }
            frame = f
        case "resume", "parked":
            // {"target":"resume","label":"<parked row title>"}: that row's Resume button.
            // {"target":"parked"}: the foot group's header.
            view = c.sidebarHostView
            var key = "parked"
            if target == "resume" {
                let label = obj["label"] as? String
                guard let line = c.sidebarLines.first(where: { $0.parked && $0.title == label }), let tab = line.tab else {
                    log("hook: click: no parked row \(label ?? "?")"); return
                }
                key = "resume:\(tab)"
            }
            guard let f = c.state.rowFrames[key] else { log("hook: click: no \(key)"); return }
            frame = f
        case "cap_pin":
            // The pane header's pin, centred at its fixed trailing slot (PaneCapBar.pin).
            guard let (id, _) = c.host.caps.first(where: { $0.value.pinned != nil }), let f = c.host.capFrames[id] else {
                log("hook: click: no cap pin"); return
            }
            let x = f.maxX - PaneCapBar.trailing - PaneCapBar.pinWidth - 8 - PaneCapBar.pinWidth / 2
            postClick(c, loc: c.host.convert(NSPoint(x: x, y: f.midY), to: nil), mods: clickMods(obj))
            return
        case "doc_tab":
            let name = obj["label"] as? String ?? ""
            guard let b = c.docPanel.tabButton(name) else { log("hook: click: no doc tab \(name)"); return }
            let loc = b.convert(NSPoint(x: b.bounds.midX, y: b.bounds.midY), to: nil)
            postClick(c, loc: loc, mods: clickMods(obj))
            return
        default:
            view = c.sidebarHostView
            let label = obj["label"] as? String
            guard let line = c.sidebarLines.first(where: { $0.title == label && $0.tab != nil }),
                  let f = c.state.rowFrames[line.id] else { log("hook: click: no row \(label ?? "?")"); return }
            frame = f
        }
        let mods = clickMods(obj)
        c.state.clickOption = mods.contains(.option)
        // The frame is in SwiftUI's space: top-left origin at the top-left of the view's safe area
        // (the transparent title bar insets it). Convert to view coordinates, then to the window.
        let safe = view.safeAreaRect
        let topInset = view.isFlipped ? safe.minY : view.bounds.height - safe.maxY
        let yFromTop = topInset + frame.midY
        let local = NSPoint(x: safe.minX + frame.midX, y: view.isFlipped ? yFromTop : view.bounds.height - yFromTop)
        let loc = view.convert(local, to: nil)
        // NSEvents addressed to the window and dispatched through NSApp.sendEvent, so the app's
        // event loop state (currentEvent, window routing, hit-testing, SwiftUI's tap gesture) runs as
        // for a physical mouse.
        var n = 0
        func post(_ type: NSEvent.EventType) {
            n += 1
            guard let ev = NSEvent.mouseEvent(with: type, location: loc, modifierFlags: mods,
                                              timestamp: ProcessInfo.processInfo.systemUptime,
                                              windowNumber: c.window.windowNumber, context: nil, eventNumber: n,
                                              clickCount: 1, pressure: type == .leftMouseUp ? 0 : 1) else { return }
            NSApp.sendEvent(ev)
        }
        post(.mouseMoved)
        // Down now, up after the run loop has turned, as a physical click has: SwiftUI resolves
        // the gesture from the down event on a later turn.
        post(.leftMouseDown)
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.08) { post(.leftMouseUp) }
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.4) { c.state.clickOption = false }
        delivered.append("click \(obj["target"] ?? "row") \(obj["label"] ?? "") at window \(Int(loc.x)),\(Int(loc.y)) via NSApp.sendEvent")
        log("hook: click \(obj["target"] ?? "row") \(obj["label"] ?? "") frame=\(NSStringFromRect(frame)) window=\(NSStringFromPoint(loc)) key=\(c.window.isKeyWindow)")
    }

    /// A sidebar click-space frame in window points from the top left, as a screenshot lays it out;
    /// the same conversion a hook click makes.
    private func windowFrame(_ frame: CGRect, in view: NSView, _ c: MainWindowController) -> [CGFloat] {
        let safe = view.safeAreaRect
        let topInset = view.isFlipped ? safe.minY : view.bounds.height - safe.maxY
        let yFromTop = topInset + frame.minY
        let local = NSPoint(x: safe.minX + frame.minX, y: view.isFlipped ? yFromTop : view.bounds.height - yFromTop)
        let loc = view.convert(local, to: nil)
        let height = c.window.contentView?.bounds.height ?? c.window.frame.height
        return [loc.x, height - loc.y, frame.width, frame.height]
    }

    /// Same action the control runs. No mouse event: SwiftUI drops clicks unless the app is active.
    private func clickAction(_ obj: [String: Any], _ c: MainWindowController) {
        let target = obj["target"] as? String ?? "row"
        let label = obj["label"] as? String ?? ""
        let key: String
        switch target {
        case "only", "focus", "open_full", "+", "✕": key = target
        case "add": key = "+"
        case "close": key = "✕"
        default: key = label.isEmpty ? target : "\(target):\(label)"
        }
        c.state.clickOption = clickMods(obj).contains(.option)
        let ok = ClickRegistry.shared.call(key)
        c.state.clickOption = false
        if ok {
            delivered.append("click \(target) \(label) via action")
            log("hook: click \(target) \(label) via action key=\(c.window.isKeyWindow)")
        } else {
            log("hook: click: no action \(key)")
        }
    }

    private func clickMods(_ obj: [String: Any]) -> NSEvent.ModifierFlags {
        var mods: NSEvent.ModifierFlags = []
        for m in obj["mods"] as? [String] ?? [] {
            switch m { case "shift": mods.insert(.shift); case "ctrl": mods.insert(.control)
                       case "opt": mods.insert(.option); case "cmd": mods.insert(.command); default: break }
        }
        return mods
    }

    private func postClick(_ c: MainWindowController, loc: NSPoint, mods: NSEvent.ModifierFlags) {
        var n = 0
        func post(_ type: NSEvent.EventType) {
            n += 1
            guard let ev = NSEvent.mouseEvent(with: type, location: loc, modifierFlags: mods,
                                              timestamp: ProcessInfo.processInfo.systemUptime,
                                              windowNumber: c.window.windowNumber, context: nil, eventNumber: n,
                                              clickCount: 1, pressure: type == .leftMouseUp ? 0 : 1) else { return }
            NSApp.sendEvent(ev)
        }
        post(.mouseMoved)
        post(.leftMouseDown)
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.08) { post(.leftMouseUp) }
    }

    /// Mouse events built as NSEvents addressed to the app's window and dispatched with
    /// window.sendEvent, so hit-testing, the responder chain and SurfaceView's
    /// mouse handlers run as for a physical mouse. {"cmd":"mouse","pane":id,
    /// "action":"down|up|drag|move","col":c,"row":r,"button":"left|right","clicks":n}; col/row are
    /// 0-based grid cells of that pane's surface (cell centre), or "x"/"y" in surface points.
    private func mouse(_ obj: [String: Any]) {
        guard let c = controller, let w = Optional(c.window),
              let s = c.currentPanes.first(where: { $0.paneId == obj["pane"] as? String }) else { log("hook: mouse: no pane"); return }
        let cell = s.cellPoints
        let x = (obj["x"] as? Double) ?? (s.paddingPoints.x + ((obj["col"] as? Double ?? 0) + 0.5) * cell.width)
        let y = (obj["y"] as? Double) ?? (s.paddingPoints.y + ((obj["row"] as? Double ?? 0) + 0.5) * cell.height)
        // Surface points (top-left origin) -> window coordinates (bottom-left origin).
        let inSurface = NSPoint(x: x, y: s.bounds.height - y)
        let loc = s.convert(inSurface, to: nil)
        let right = (obj["button"] as? String) == "right"
        let type: NSEvent.EventType
        switch obj["action"] as? String ?? "" {
        case "down": type = right ? .rightMouseDown : .leftMouseDown
        case "up": type = right ? .rightMouseUp : .leftMouseUp
        case "drag": type = right ? .rightMouseDragged : .leftMouseDragged
        default: type = .mouseMoved
        }
        var mods: NSEvent.ModifierFlags = []
        for m in obj["mods"] as? [String] ?? [] {
            switch m { case "shift": mods.insert(.shift); case "ctrl": mods.insert(.control)
                       case "opt": mods.insert(.option); case "cmd": mods.insert(.command); default: break }
        }
        guard let ev = NSEvent.mouseEvent(with: type, location: loc, modifierFlags: mods,
                                          timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: w.windowNumber,
                                          context: nil, eventNumber: 0, clickCount: obj["clicks"] as? Int ?? 1, pressure: type == .mouseMoved ? 0 : 1) else { return }
        // mouseMoved never reaches a view without acceptsMouseMovedEvents; hand it over directly.
        if type == .mouseMoved { s.mouseMoved(with: ev) } else { w.sendEvent(ev) }
        if agentRun { c.inProcessKey = true }
        delivered.append("mouse \(obj["action"] ?? "") on \(s.paneId) via window.sendEvent")
    }

    private var dragTimer: Timer?
    private(set) var dragRunning = false

    /// {"cmd":"drag_divider","index":0,"dx":120,"steps":8,"interval":0.05}: press on the centre of
    /// the index-th divider handle (sorted by split id), drag `dx` host points along its axis
    /// (right/down positive) in `steps` moves, release. NSEvents go through window.sendEvent, so
    /// hit-testing picks the handle as for a physical mouse. Returns at once; the drag runs on
    /// a timer and `drag_running` in the state says when it is done.
    private func dragDivider(_ obj: [String: Any]) {
        guard let c = controller else { return }
        let handles = c.host.dividerHandles
        let idx = obj["index"] as? Int ?? 0
        guard idx >= 0, idx < handles.count, !dragRunning else { log("hook: drag_divider: no handle \(idx)"); return }
        let h = handles[idx]
        let dx = CGFloat(obj["dx"] as? Double ?? Double(obj["dx"] as? Int ?? 0))
        let steps = max(1, obj["steps"] as? Int ?? 8)
        let interval = obj["interval"] as? Double ?? 0.05
        let start = h.convert(NSPoint(x: h.bounds.midX, y: h.bounds.midY), to: nil)
        // Host is flipped (y down), the window is not: a positive dy along a horizontal line is a negative window y.
        let axis = h.divider.vertical ? CGVector(dx: dx, dy: 0) : CGVector(dx: 0, dy: -dx)
        func post(_ type: NSEvent.EventType, _ p: NSPoint) {
            guard let ev = NSEvent.mouseEvent(with: type, location: p, modifierFlags: [],
                                              timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: c.window.windowNumber,
                                              context: nil, eventNumber: 0, clickCount: obj["clicks"] as? Int ?? 1, pressure: type == .leftMouseUp ? 0 : 1) else { return }
            c.window.sendEvent(ev)
        }
        dragRunning = true
        delivered.append("drag_divider \(h.divider.splitId) dx=\(dx) via window.sendEvent")
        post(.leftMouseDown, start)
        var i = 0
        dragTimer = Timer.scheduledTimer(withTimeInterval: interval, repeats: true) { [weak self] t in
            i += 1
            let f = CGFloat(min(i, steps)) / CGFloat(steps)
            let p = NSPoint(x: start.x + axis.dx * f, y: start.y + axis.dy * f)
            if i <= steps {
                post(.leftMouseDragged, p)
            } else {
                post(.leftMouseUp, p)
                t.invalidate()
                self?.dragRunning = false
            }
        }
    }

    /// {"cmd":"drag_doc_handle","dx":-80,"steps":8,"interval":0.05}: press on the docs column's
    /// width handle, drag `dx` points (right positive) and release, through window.sendEvent so
    /// a non-key window applies its first-mouse rule as for a physical click. `drag_running` in
    /// the state says when it is done.
    private func dragDocHandle(_ obj: [String: Any]) {
        // A hidden column has no laid-out handle; pressing its zero frame would hit the window corner.
        guard let c = controller, !dragRunning, !c.docPanel.view.isHidden, c.docPanel.view.window != nil,
              c.docPanel.widthHandle.bounds.height > 0 else { log("hook: drag_doc_handle: no docs column"); return }
        let h = c.docPanel.widthHandle
        let dx = CGFloat(obj["dx"] as? Double ?? Double(obj["dx"] as? Int ?? 0))
        let steps = max(1, obj["steps"] as? Int ?? 8)
        let interval = obj["interval"] as? Double ?? 0.05
        let start = h.convert(NSPoint(x: h.bounds.midX, y: h.bounds.midY), to: nil)
        func post(_ type: NSEvent.EventType, _ p: NSPoint) {
            guard let ev = NSEvent.mouseEvent(with: type, location: p, modifierFlags: [],
                                              timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: c.window.windowNumber,
                                              context: nil, eventNumber: 0, clickCount: 1, pressure: type == .leftMouseUp ? 0 : 1) else { return }
            c.window.sendEvent(ev)
        }
        dragRunning = true
        delivered.append("drag_doc_handle dx=\(dx) key=\(c.window.isKeyWindow) via window.sendEvent")
        post(.leftMouseDown, start)
        var i = 0
        // Common modes, so the release still comes while AppKit tracks the mouse.
        let timer = Timer(timeInterval: interval, repeats: true) { [weak self] t in
            i += 1
            let p = NSPoint(x: start.x + dx * CGFloat(min(i, steps)) / CGFloat(steps), y: start.y)
            if i <= steps {
                post(.leftMouseDragged, p)
            } else {
                post(.leftMouseUp, p)
                t.invalidate()
                self?.dragRunning = false
            }
        }
        RunLoop.main.add(timer, forMode: .common)
        dragTimer = timer
    }

    /// {"cmd":"drag_pin","row":"pinned:<tab>","dy":48,"steps":8,"interval":0.05,"esc":false}: press
    /// on the centre of a sidebar row, drag `dy` points (down positive) in `steps` moves and
    /// release; with "esc" Esc goes in before the release. Events go through NSApp.sendEvent as a
    /// physical mouse's do, so SwiftUI's own gestures tell a drag from a click. Returns at once;
    /// `drag_running` in the state says when it is done.
    private var paneDragPoint = NSPoint.zero
    private func paneDragEvent(_ obj: [String: Any]) {
        guard let c = controller else { return }
        func post(_ type: NSEvent.EventType, _ p: NSPoint) {
            if let e = NSEvent.mouseEvent(with: type, location: p, modifierFlags: [],
                timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: c.window.windowNumber,
                context: nil, eventNumber: 0, clickCount: 1, pressure: type == .leftMouseUp ? 0 : 1) { NSApp.sendEvent(e) }
        }
        switch obj["op"] as? String {
        case "begin":
            guard let pane = obj["pane"] as? String, let f = c.host.capFrames[pane] else { return }
            paneDragPoint = c.host.convert(NSPoint(x: f.midX, y: f.midY), to: nil)
            post(.mouseMoved, paneDragPoint); post(.leftMouseDown, paneDragPoint)
            paneDragPoint.x += CGFloat((obj["travel"] as? NSNumber)?.doubleValue ?? 8)
            post(.leftMouseDragged, paneDragPoint)
        case "move":
            let end: NSPoint
            if let row = obj["row"] as? String, let f = c.state.rowFrames[row] {
                let view = c.sidebarHostView, safe = view.safeAreaRect
                let inset = view.isFlipped ? safe.minY : view.bounds.height - safe.maxY
                let y = inset + f.midY
                end = view.convert(NSPoint(x: safe.minX + f.midX, y: view.isFlipped ? y : view.bounds.height - y), to: nil)
            } else {
                end = c.host.convert(NSPoint(x: (obj["x"] as? NSNumber)?.doubleValue ?? 0, y: (obj["y"] as? NSNumber)?.doubleValue ?? 0), to: nil)
            }
            let steps = max(1, obj["steps"] as? Int ?? 4), start = paneDragPoint
            for i in 1...steps {
                let t = CGFloat(i) / CGFloat(steps)
                post(.leftMouseDragged, NSPoint(x: start.x + (end.x - start.x) * t, y: start.y + (end.y - start.y) * t))
            }
            paneDragPoint = end
            if obj["drop"] as? Bool == true { post(.leftMouseUp, end) }
        case "drop": post(.leftMouseUp, paneDragPoint)
        case "fail-next-drop": c.paneDrag.failNextDrop = true
        case "hold-drops": c.paneDrag.holdDrops = obj["on"] as? Bool ?? true
        case "send-drop": c.paneDrag.sendHeldDrop()
        case "hold-replies": c.paneDrag.holdReplies = obj["on"] as? Bool ?? true
        case "send-reply": c.paneDrag.sendHeldReply()
        case "cancel":
            if obj["via"] as? String == "right" { post(.rightMouseDown, paneDragPoint); post(.rightMouseUp, paneDragPoint) }
            else if let e = NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: [],
                timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: c.window.windowNumber,
                context: nil, characters: "\u{1b}", charactersIgnoringModifiers: "\u{1b}", isARepeat: false, keyCode: 53) { NSApp.sendEvent(e) }
        default: break
        }
    }

    private func dragPin(_ obj: [String: Any]) {
        guard let c = controller, let id = obj["row"] as? String, let frame = c.state.rowFrames[id], !dragRunning else {
            log("hook: drag_pin: no row \(obj["row"] ?? "?")"); return
        }
        let view = c.sidebarHostView
        let safe = view.safeAreaRect
        let topInset = view.isFlipped ? safe.minY : view.bounds.height - safe.maxY
        // As the click hook: SwiftUI's frame is top-left origin inside the safe area.
        func point(_ dy: CGFloat) -> NSPoint {
            let yFromTop = topInset + frame.midY + dy
            let local = NSPoint(x: safe.minX + frame.midX, y: view.isFlipped ? yFromTop : view.bounds.height - yFromTop)
            return view.convert(local, to: nil)
        }
        let dy = CGFloat(obj["dy"] as? Double ?? Double(obj["dy"] as? Int ?? 0))
        let steps = max(1, obj["steps"] as? Int ?? 8)
        let interval = obj["interval"] as? Double ?? 0.05
        let esc = obj["esc"] as? Bool ?? false
        var n = 0
        func post(_ type: NSEvent.EventType, _ p: NSPoint) {
            n += 1
            guard let ev = NSEvent.mouseEvent(with: type, location: p, modifierFlags: [],
                                              timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: c.window.windowNumber,
                                              context: nil, eventNumber: n, clickCount: 1, pressure: type == .leftMouseUp ? 0 : 1) else { return }
            NSApp.sendEvent(ev)
        }
        dragRunning = true
        delivered.append("drag_pin \(id) dy=\(dy) esc=\(esc) via NSApp.sendEvent")
        post(.mouseMoved, point(0))
        post(.leftMouseDown, point(0))
        var i = 0
        dragTimer = Timer.scheduledTimer(withTimeInterval: interval, repeats: true) { [weak self] t in
            i += 1
            if i <= steps {
                post(.leftMouseDragged, point(dy * CGFloat(i) / CGFloat(steps)))
            } else if esc && i == steps + 1 {
                // Queued, not sent, so the app's key monitors see it as a typed Esc.
                if let key = NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: [],
                                              timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: c.window.windowNumber,
                                              context: nil, characters: "\u{1b}", charactersIgnoringModifiers: "\u{1b}",
                                              isARepeat: false, keyCode: 53) {
                    NSApp.postEvent(key, atStart: false)
                }
            } else {
                post(.leftMouseUp, point(dy))
                t.invalidate()
                self?.dragRunning = false
            }
        }
        // The typed Esc can leave the run loop in a tracking mode; a default-mode timer would
        // then never post the release and every later drag_pin is refused as still running.
        if let dragTimer { RunLoop.main.add(dragTimer, forMode: .common) }
    }

    /// {"cmd":"scroll","pane":id,"dy":lines}: a wheel event with line deltas
    /// (positive dy = wheel up, i.e. towards older output), built as a CGEvent
    /// and sent to the surface under the window's own event path.
    private func scroll(_ obj: [String: Any]) {
        guard let c = controller,
              let s = c.currentPanes.first(where: { $0.paneId == obj["pane"] as? String }),
              let cg = CGEvent(scrollWheelEvent2Source: nil, units: .line, wheelCount: 1,
                               wheel1: Int32(obj["dy"] as? Int ?? 0), wheel2: 0, wheel3: 0),
              let ev = NSEvent(cgEvent: cg) else { log("hook: scroll failed"); return }
        log("hook scroll: dy=\(ev.scrollingDeltaY) precise=\(ev.hasPreciseScrollingDeltas)")
        s.scrollWheel(with: ev)
        delivered.append("scroll \(obj["dy"] ?? 0) on \(s.paneId)")
    }

    /// {"cmd":"scroll_gesture","pane":id,"dy":px,"steps":n,"momentum":m,"decay":d,"interval":s,"out":path}:
    /// a trackpad swipe as AppKit delivers one: continuous pixel deltas with scroll phase
    /// began, changed and ended, then `m` momentum events that decay by `d` (default 0.92)
    /// each, one event per `interval` (default 1/120 s). On every tick, and for 0.4 s after,
    /// the surface's top visible row is sampled; `out` gets "ms<TAB>row" lines, a client-side
    /// frame log.
    private func scrollGesture(_ obj: [String: Any]) {
        guard let c = controller,
              let s = c.currentPanes.first(where: { $0.paneId == obj["pane"] as? String }),
              let out = obj["out"] as? String else { log("hook: scroll_gesture refused"); return }
        let dy = obj["dy"] as? Double ?? Double(obj["dy"] as? Int ?? 12)
        let steps = obj["steps"] as? Int ?? 30
        let momentum = obj["momentum"] as? Int ?? 40
        let decay = obj["decay"] as? Double ?? 0.92
        let interval = obj["interval"] as? Double ?? 1.0 / 120
        // (scroll phase, momentum phase, delta): CGScrollPhase began 1, changed 2, ended 4;
        // CGMomentumScrollPhase begin 1, continue 2, end 3.
        var events: [(Int64, Int64, Double)] = [(1, 0, dy)]
        events += Array(repeating: (2, 0, dy), count: max(0, steps - 2))
        events.append((4, 0, 0))
        for i in 0..<momentum {
            events.append((0, i == 0 ? 1 : (i == momentum - 1 ? 3 : 2), dy * pow(decay, Double(i + 1))))
        }
        // A swipe happens with the pointer over the pane, and Ghostty sends a wheel report at
        // the pointer; with no pointer position yet it reports nothing and nothing scrolls.
        let centre = s.convert(NSPoint(x: s.bounds.midX, y: s.bounds.midY), to: nil)
        if let move = NSEvent.mouseEvent(with: .mouseMoved, location: centre, modifierFlags: [],
                                         timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: c.window.windowNumber,
                                         context: nil, eventNumber: 0, clickCount: 0, pressure: 0) {
            s.mouseMoved(with: move)
        }
        var frames = ""
        var precise = 0
        let t0 = Date()
        let sample = { [weak s] in
            let top = s?.visibleText().split(separator: "\n", omittingEmptySubsequences: false).first ?? ""
            frames += String(format: "%.1f\t", Date().timeIntervalSince(t0) * 1000) + top + "\n"
        }
        var i = 0
        Timer.scheduledTimer(withTimeInterval: interval, repeats: true) { [weak s] t in
            if i < events.count, let s {
                let (phase, mom, d) = events[i]
                if let cg = CGEvent(scrollWheelEvent2Source: nil, units: .pixel, wheelCount: 1,
                                    wheel1: Int32(d.rounded()), wheel2: 0, wheel3: 0) {
                    cg.setIntegerValueField(.scrollWheelEventIsContinuous, value: 1)
                    cg.setIntegerValueField(.scrollWheelEventScrollPhase, value: phase)
                    cg.setIntegerValueField(.scrollWheelEventMomentumPhase, value: mom)
                    cg.setDoubleValueField(.scrollWheelEventPointDeltaAxis1, value: d)
                    if let ev = NSEvent(cgEvent: cg) {
                        if ev.hasPreciseScrollingDeltas { precise += 1 }
                        s.scrollWheel(with: ev)
                    }
                }
            }
            i += 1
            sample()
            if Double(i - events.count) * interval > 0.4 {
                t.invalidate()
                frames = "# events=\(events.count) precise=\(precise) interval_ms=\(interval * 1000)\n" + frames
                try? frames.write(toFile: out, atomically: true, encoding: .utf8)
            }
        }
        delivered.append("scroll_gesture \(events.count) events on \(s.paneId)")
    }

    // US ANSI virtual keycodes.
    static let codes: [String: (UInt16, Bool)] = {
        var m: [String: (UInt16, Bool)] = [:]
        let base: [(String, UInt16)] = [
            ("a", 0), ("s", 1), ("d", 2), ("f", 3), ("h", 4), ("g", 5), ("z", 6), ("x", 7), ("c", 8), ("v", 9),
            ("b", 11), ("q", 12), ("w", 13), ("e", 14), ("r", 15), ("y", 16), ("t", 17), ("1", 18), ("2", 19),
            ("3", 20), ("4", 21), ("6", 22), ("5", 23), ("=", 24), ("9", 25), ("7", 26), ("-", 27), ("8", 28),
            ("0", 29), ("]", 30), ("o", 31), ("u", 32), ("[", 33), ("i", 34), ("p", 35), ("l", 37), ("j", 38),
            ("'", 39), ("k", 40), (";", 41), ("\\", 42), (",", 43), ("/", 44), ("n", 45), ("m", 46), (".", 47),
            ("`", 50), (" ", 49),
        ]
        for (k, c) in base { m[k] = (c, false) }
        for (k, c) in base where k.first!.isLetter { m[k.uppercased()] = (c, true) }
        let shifted: [(String, String)] = [("_", "-"), ("+", "="), ("!", "1"), ("@", "2"), ("#", "3"), ("$", "4"),
                                           ("%", "5"), ("^", "6"), ("&", "7"), ("*", "8"), ("(", "9"), (")", "0"),
                                           (":", ";"), ("\"", "'"), ("<", ","), (">", "."), ("?", "/"), ("|", "\\"),
                                           ("{", "["), ("}", "]"), ("~", "`")]
        for (k, b) in shifted { m[k] = (m[b]!.0, true) }
        m["return"] = (36, false); m["tab"] = (48, false); m["space"] = (49, false)
        m["backspace"] = (51, false); m["escape"] = (53, false)
        m["left"] = (123, false); m["right"] = (124, false); m["down"] = (125, false); m["up"] = (126, false)
        return m
    }()

    func key(_ name: String, mods: [String]) {
        guard let (code, shift) = Self.codes[name] else { log("hook: no keycode for \(name)"); return }
        var flags: CGEventFlags = []
        if shift || mods.contains("shift") { flags.insert(.maskShift) }
        if mods.contains("ctrl") { flags.insert(.maskControl) }
        if mods.contains("opt") { flags.insert(.maskAlternate) }
        if mods.contains("cmd") { flags.insert(.maskCommand) }
        for down in [true, false] {
            guard let cg = CGEvent(keyboardEventSource: nil, virtualKey: code, keyDown: down) else { continue }
            cg.flags = flags
            if viaPid {
                if agentRun {
                    log("hook: refusing CGEvent via pid under --agent-run")
                    return
                }
                cg.postToPid(getpid())
                if down { delivered.append("\(mods.joined(separator: "+"))\(mods.isEmpty ? "" : "+")\(name) via postToPid") }
                continue
            }
            guard var ev = NSEvent(cgEvent: cg) else { continue }
            // Address the event to our window so AppKit routes it like a real key.
            if let w = controller?.window,
               let e2 = NSEvent.keyEvent(with: ev.type, location: .zero, modifierFlags: ev.modifierFlags,
                                         timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: w.windowNumber,
                                         context: nil, characters: ev.characters ?? "",
                                         charactersIgnoringModifiers: ev.charactersIgnoringModifiers ?? "",
                                         isARepeat: false, keyCode: ev.keyCode) {
                ev = e2
            }
            let isKey = controller?.window.isKeyWindow ?? false
            if isKey {
                NSApp.sendEvent(ev)
            } else if let w = controller?.window {
                // Not key: the app must not be activated. Claimed chords run through
                // performKeyEquivalent; everything else is delivered to the first responder.
                let viewClaimed = ev.type == .keyDown && claimKeyEquivalent(ev, window: w)
                // The menu matches a bare letter to an item whose equivalent is command+that letter
                // when the app is not active, which swallows the letter. Only command chords go there.
                let menuClaimed = !viewClaimed && ev.type == .keyDown && ev.modifierFlags.contains(.command)
                    && (NSApp.mainMenu?.performKeyEquivalent(with: ev) ?? false)
                if viewClaimed || menuClaimed {
                } else if let view = w.firstResponder as? NSView {
                    switch ev.type {
                    case .keyDown: view.keyDown(with: ev)
                    case .keyUp: view.keyUp(with: ev)
                    case .flagsChanged: view.flagsChanged(with: ev)
                    default: w.sendEvent(ev)
                    }
                } else {
                    w.sendEvent(ev)
                }
            }
            if down { delivered.append("\(mods.joined(separator: "+"))\(mods.isEmpty ? "" : "+")\(name) via \(isKey ? "NSApp.sendEvent" : "window-emulated")") }
        }
    }

    /// The window's own performKeyEquivalent returns false while the window is not key.
    /// Walk the first responder so a focused doc web view can claim ⌘L.
    private func claimKeyEquivalent(_ ev: NSEvent, window w: NSWindow) -> Bool {
        if w.performKeyEquivalent(with: ev) { return true }
        var responder = w.firstResponder
        var seen = Set<ObjectIdentifier>()
        while let r = responder, !(r is NSWindow) {
            let id = ObjectIdentifier(r)
            if !seen.insert(id).inserted { break }
            if r.performKeyEquivalent(with: ev) { return true }
            responder = r.nextResponder
        }
        return false
    }

    /// Captures this app's own window, Metal layers included, through the window
    /// server. CGWindowListCreateImage is obsoleted in the macOS 15 SDK, so it is
    /// looked up at runtime; capturing one's own window needs no Screen Recording grant.
    func shot(_ out: String, scale: Double? = nil) {
        // Ask every surface for a fresh frame first: a pane that is not being presented
        // (locked screen, occluded window) may otherwise hand back its last frame.
        for v in controller?.registry.byTerminal.values ?? [:].values {
            if let sf = v.surface { ghostty_surface_refresh(sf); ghostty_surface_draw(sf) }
        }
        RunLoop.current.run(until: Date().addingTimeInterval(0.4))
        var how = "window server"
        var img: CGImage?
        // A display at a lower scale than asked (the Cua Space is 1x) cannot give 2x pixels.
        let rerender = scale.map { CGFloat($0) > (controller?.window.backingScaleFactor ?? 2) } ?? false
        if !agentRun, !rerender, let w = controller?.window,
           let sym = dlsym(UnsafeMutableRawPointer(bitPattern: -2), "CGWindowListCreateImage") {
            typealias Fn = @convention(c) (CGRect, UInt32, UInt32, UInt32) -> Unmanaged<CGImage>?
            let f = unsafeBitCast(sym, to: Fn.self)
            // kCGWindowListOptionIncludingWindow = 1 << 3, kCGWindowImageBoundsIgnoreFraming = 1 << 0
            if let raw = f(.null, 1 << 3, UInt32(w.windowNumber), 1 << 0)?.takeRetainedValue() {
                // Tag pixels as sRGB so a token hex compares directly with what was drawn,
                // whatever the display profile is.
                let c = Self.toSRGB(raw)
                if !Self.isBlank(c) { img = c }
            }
        }
        if img == nil {
            // A locked screen or an occluded window makes the window server hand back
            // transparent pixels. Render the window's layer tree in-process instead: it
            // has every fill and the Ghostty surfaces' IOSurface contents, but no
            // window-level blur, so glass shows as its (transparent) layer only.
            img = renderLayers(scale: scale.map { CGFloat($0) })
            how = rerender ? "layer render at \(scale ?? 0)x" : agentRun ? "cacheDisplay" : "layer render (window server capture was blank)"
        }
        guard let img else { log("shot: no image"); return }
        let rep = NSBitmapImageRep(cgImage: img)
        try? rep.representation(using: .png, properties: [:])?.write(to: URL(fileURLWithPath: out))
        log("shot: \(img.width)x\(img.height) via \(how) -> \(out)")
    }

    /// True when the middle of the image is fully transparent.
    static func isBlank(_ img: CGImage) -> Bool {
        guard let cs = CGColorSpace(name: CGColorSpace.sRGB),
              let ctx = CGContext(data: nil, width: 1, height: 1, bitsPerComponent: 8, bytesPerRow: 4, space: cs,
                                  bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue),
              let px = ctx.data?.assumingMemoryBound(to: UInt8.self) else { return false }
        ctx.draw(img, in: CGRect(x: -img.width / 2, y: -img.height / 2, width: img.width, height: img.height))
        return px[3] == 0
    }

    private func renderLayers(scale asked: CGFloat? = nil) -> CGImage? {
        guard let view = controller?.window.contentView, let layer = view.layer,
              let cs = CGColorSpace(name: CGColorSpace.sRGB) else { return nil }
        let scale = asked ?? controller?.window.backingScaleFactor ?? 2
        let w = Int(view.bounds.width * scale), h = Int(view.bounds.height * scale)
        guard let ctx = CGContext(data: nil, width: w, height: h, bitsPerComponent: 8, bytesPerRow: 0, space: cs,
                                  bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else { return nil }
        ctx.scaleBy(x: scale, y: scale)
        // AppKit caches at the window's own scale; an asked-for scale needs a rep sized to it.
        func cachingRep(_ v: NSView) -> NSBitmapImageRep? {
            guard asked != nil else { return v.bitmapImageRepForCachingDisplay(in: v.bounds) }
            let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: Int(v.bounds.width * scale),
                                       pixelsHigh: Int(v.bounds.height * scale), bitsPerSample: 8, samplesPerPixel: 4,
                                       hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)
            rep?.size = v.bounds.size
            return rep
        }
        if let rep = cachingRep(view) {
            view.cacheDisplay(in: view.bounds, to: rep)
            if let cached = rep.cgImage {
                ctx.draw(cached, in: CGRect(x: 0, y: 0, width: view.bounds.width, height: view.bounds.height))
            }
        }
        layer.render(in: ctx)
        // SwiftUI content (sidebar, detail panel) does not come through CALayer.render;
        // draw those hosting views with AppKit's own view caching.
        var hosted: [NSView] = []
        func collect(_ v: NSView) {
            if String(describing: type(of: v)).contains("HostingView") { hosted.append(v); return }
            v.subviews.forEach(collect)
        }
        collect(view)
        controller?.state.flat = true
        defer { controller?.state.flat = false }
        RunLoop.current.run(until: Date().addingTimeInterval(0.3))
        for v in hosted where !v.isHidden && v.bounds.width > 0 {
            guard let rep = cachingRep(v) else { continue }
            v.cacheDisplay(in: v.bounds, to: rep)
            guard let img = rep.cgImage else { continue }
            let f = v.convert(v.bounds, to: view)
            ctx.draw(img, in: CGRect(x: f.minX, y: f.minY, width: f.width, height: f.height))
        }
        return ctx.makeImage()
    }

    private func themeState(_ c: MainWindowController) -> [String: Any] {
        let th = c.theme
        var d = ThemeStore.dump(terminal: th.terminal)
        d["override"] = th.override.rawValue
        d["system"] = th.system.rawValue
        d["effective"] = th.effective.rawValue
        d["glass"] = ["sidebar": th.glass.sidebar, "overlay": th.glass.overlay]
        d["sidebar_glass_view"] = c.sidebarGlassKind
        d["window_opaque"] = c.window.isOpaque
        d["window_visible"] = c.window.isVisible
        d["window_occlusion_visible"] = c.window.occlusionState.contains(.visible)
        d["window_screen"] = c.window.screen.map { "\(NSStringFromRect($0.frame)) scale \($0.backingScaleFactor)" } ?? "none"
        d["window_appearance"] = c.window.effectiveAppearance.name.rawValue
        d["sidebar_frame"] = NSStringFromRect(c.window.contentView?.subviews.first?.frame ?? .zero)
        d["host_frame"] = NSStringFromRect(c.host.frame)
        d["config_keybinds"] = ghosttyConfigText.components(separatedBy: "\n")
            .filter { TerminalTheme.key(of: $0) == "keybind" }.map { $0.trimmingCharacters(in: .whitespaces) }
        // Panes are opaque when libghostty's own resolved config says so.
        var opacity = 0.0
        let key = "background-opacity"
        if let cfg = GhosttyRuntime.shared?.config, ghostty_config_get(cfg, &opacity, key, UInt(key.utf8.count)) {
            d["terminal_background_opacity"] = opacity
        }
        d["surfaces_opaque"] = opacity == 1.0
        return d
    }

    private func detailState(_ c: MainWindowController) -> [String: Any] {
        let p = c.detailPanel.view
        var d: [String: Any] = ["open": c.detailPanel.model.isOpen,
                                "panel_frame": [p.frame.minX, p.frame.minY, p.frame.width, p.frame.height],
                                "panel_hidden": p.isHidden,
                                "sidebar_frame": NSStringFromRect(c.window.contentView?.subviews.first?.frame ?? .zero),
                                "panel_first_responder": c.window.firstResponder.map { r in
                                    (r as? NSView).map { $0 === p || $0.isDescendant(of: p) } ?? false } ?? false]
        if let x = c.detailContent {
            func wf(_ w: DetailWorkflow) -> [String: Any] {
                ["tab": w.id, "label": w.label, "phase": w.phase, "status": w.status, "host": w.host]
            }
            d["row"] = x.rowId
            d["title"] = x.title
            d["kind"] = x.kind.rawValue
            d["inbox"] = x.inbox.map { ["text": $0.text, "source": $0.source] }
            d["routed"] = x.routed.map { ["text": $0.text, "source": $0.source] }
            d["groups"] = x.groups.map { ["lane": $0.lane ?? NSNull(), "workflows": $0.workflows.map(wf)] as [String: Any] }
        }
        return d
    }

    /// Redraws into an sRGB bitmap (a real conversion, not a re-tag).
    static func toSRGB(_ src: CGImage) -> CGImage {
        guard let cs = CGColorSpace(name: CGColorSpace.sRGB),
              let ctx = CGContext(data: nil, width: src.width, height: src.height, bitsPerComponent: 8, bytesPerRow: 0,
                                  space: cs, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else { return src }
        ctx.draw(src, in: CGRect(x: 0, y: 0, width: src.width, height: src.height))
        return ctx.makeImage() ?? src
    }

    private func writeState(_ out: String) {
        guard let c = controller else { return }
        let surfaces = c.registry.byTerminal.values.sorted { $0.paneId < $1.paneId }.map { s -> [String: Any] in
            let g = s.gridSize
            let lines = s.visibleText().split(separator: "\n", omittingEmptySubsequences: false).map(String.init)
            return ["pane": s.paneId, "terminal": s.terminalId, "cols": g.cols, "rows": g.rows,
                    "first_responder": c.window.firstResponder === s, "exited": s.exited,
                    "mouse_captured": s.mouseCaptured, "mouse_sent": s.mouseSent,
                    "hovered_link": s.hoveredLink,
                    "keys_sent": s.keysSent, "last_key_sent": s.lastKeySent,
                    "frame": [s.frame.minX, s.frame.minY, s.frame.width, s.frame.height],
                    "in_host": s.superview === c.host,
                    "selection": s.selectedText() ?? NSNull(),
                    "cell_pt": [s.cellPoints.width, s.cellPoints.height],
                    "age_s": Date().timeIntervalSince(s.createdAt),
                    "first_text_after_s": s.firstTextAfter ?? NSNull(),
                    "visible_nonblank": lines.filter { !$0.trimmingCharacters(in: .whitespaces).isEmpty }]
        }
        func rows(_ r: [TabRow]) -> [[String: Any]] {
            r.map { ["tab": $0.id, "label": $0.label, "kind": $0.kind.rawValue, "status": $0.status, "host": $0.host,
                     "agent": $0.agent ?? NSNull(), "children": rows($0.children)] }
        }
        let state: [String: Any] = [
            "app_active": NSApp.isActive,
            "app_age_s": Date().timeIntervalSince(appStart),
            "window_key": c.window.isKeyWindow,
            "window_number": c.window.windowNumber,
            "window_frame": NSStringFromRect(c.window.frame),
            "host_frame": NSStringFromRect(c.host.frame),
            "selected_tab": c.state.selectedTab ?? NSNull(),
            "switcher_open": c.quickSwitch.isOpen,
            "switcher_results": c.quickSwitch.results,
            "sidebar_visible": c.state.sidebarVisible,
            "attention_order": c.attentionOrderIds(),
            "attention_latest": c.model.latestAttentionTab ?? NSNull(),
            "focused_pane": c.focusedSurface?.paneId ?? NSNull(),
            // Panes the host draws, with their frames in points (a zoomed tab draws one).
            "host_panes": c.host.rects.map { r -> [String: Any] in
                ["pane": r.0.paneId, "frame": [r.0.frame.minX, r.0.frame.minY, r.0.frame.width, r.0.frame.height]]
            },
            "paneDrag": c.paneDrag.dump(),
            "host_size": [c.host.bounds.width, c.host.bounds.height],
            "dividers": c.host.dividerHandles.map { h -> [String: Any] in
                ["split": h.divider.splitId, "vertical": h.divider.vertical, "ratio": h.divider.ratio,
                 "first_pane": h.divider.firstPane, "second_pane": h.divider.secondPane,
                 "frame": [h.frame.minX, h.frame.minY, h.frame.width, h.frame.height]]
            },
            "shown_layout": c.shownLayout.map { l -> [String: Any] in
                ["tab": l.tab_id, "area": [l.area.width, l.area.height],
                 "panes": l.panes.map { ["pane": $0.pane_id, "rect": [$0.rect.x, $0.rect.y, $0.rect.width, $0.rect.height]] }]
            } ?? NSNull(),
            "drag_running": dragRunning || c.resizer.isBusy,
            "resize_requests": c.resizer.requestsSent,
            "post_event_access": CGPreflightPostEventAccess(),
            "sidebar": ["orchestrator": rows(c.model.orchestrators), "lanes": rows(c.model.lanes),
                        "workflows": rows(c.model.workflows)],
            "spaces_rows": c.model.spacesRows(state: c.state).map { $0.dump },
            // Each drawn agent face in window points from the top left, the pictures fetched, and the dot colors.
            "face_frames": Dictionary(uniqueKeysWithValues: c.state.rowFrames.filter { $0.key.hasPrefix("face:") }
                .map { ($0.key, windowFrame($0.value, in: c.sidebarHostView, c)) }),
            "face_pictures": FacePictures.shared.images.keys.sorted(),
            "face_dots": ["working": ThemeStore.hex(c.theme.tokens.chrome.ok), "blocked": ThemeStore.hex(c.theme.tokens.chrome.accent),
                          "done": ThemeStore.hex(c.theme.tokens.chrome.warn)],
            // What ⌘1..9 select, in order (pins first).
            "numbered_tabs": Array(c.model.numberedTabIds(state: c.state).prefix(9)),
            "hidden_agents": ([c.model.snapshot].compactMap { $0 } + c.model.machines.compactMap(\.snapshot)).flatMap(\.tabs).filter { $0.role == "agent" && ($0.hidden ?? false) }.map(\.tab_id),
            "agent_tabs": c.model.numberedTabIds(state: c.state).filter { c.model.isAgent($0) },
            "pin_drag": ["dragged": PinDrag.shared.dragged as Any? ?? NSNull(), "target": PinDrag.shared.target as Any? ?? NSNull()] as [String: Any],
            // Each tab row's context menu items, as a right-click shows them.
            "spaces_menus": Dictionary(c.model.spacesRows(state: c.state).filter { $0.tab != nil }
                .map { ($0.id, RowMenu.items(for: $0, model: c.model).map(\.rawValue)) }, uniquingKeysWith: { a, _ in a }),
            "spaces_chrome": (try? JSONSerialization.jsonObject(with: JSONEncoder().encode(c.state.spacesChrome))) ?? [:],
            "docs_visible": c.root.docsOpen,
            "detail_open": c.detailPanel.model.isOpen,
            "sidebar_lines": c.sidebarLines.map { $0.dump },   // P10: the rows as drawn, in order
            "shell": [
                "mode": c.state.mode.rawValue,
                "chip": c.state.chip.rawValue,
                "parked_count": SidebarModel.parkedCount(snapshot: c.model.snapshot, orchestrators: c.model.orchestrators,
                                                         lanes: c.model.lanes, workflows: c.model.workflows,
                                                         catalog: c.model.catalog.snapshot, areaOnly: c.state.areaOnly),
                "area_only": c.state.areaOnly ?? NSNull(),
                "folded_areas": c.state.foldedAreas.sorted(),
                "focus_expanded": c.state.focusExpanded,
                "focus_cursor": c.state.focusCursor ?? NSNull(),
                "doc_open": c.state.docOpen,
                "doc_width": c.state.docWidth,
                "factory_open": c.state.factoryOpen,
                "running_commit": Channel.commit,
                "doc_frame_width": c.root.docs?.frame.width ?? 0,
                "selected_tab": c.state.selectedTab ?? NSNull(),
            ],
            "update_pill": c.updates?.shown ?? false,
            "pane_caps": c.host.caps.map { id, cap -> [String: Any] in
                let f = c.host.capFrames[id] ?? .zero
                return ["id": id, "name": cap.name, "agent": cap.agent, "chat": cap.chat,
                        "density": cap.density, "focused": cap.focused, "pinned": cap.pinned ?? NSNull(),
                        "frame": [f.minX, f.minY, f.width, f.height]]
            },
            "chats": c.chatDump,
            "machines": c.factoryMachines.map { ["name": $0.name, "usage": $0.usageLine, "usage_state": $0.usageState] },
            "hosts": c.model.hostsModel.rows.map { r -> [String: Any] in
                ["host": r.host, "tabs": r.tabs, "slots_used": r.stats?.slotsUsed ?? NSNull(),
                 "slots_total": r.stats?.slotsTotal ?? NSNull(), "sessions": r.stats?.sessions ?? NSNull(),
                 "text": "\(r.host) \(r.detail)"]
            },
            "hosts_provider": c.model.hostsModel.provider.name,
            "theme": themeState(c),
            "detail": detailState(c),
            "docs": c.docPanel.dump(),
            "desk": ["items": (c.state.selectedTab.flatMap { c.model.source(for: $0) }?.tabs.first { $0.tab_id == c.state.selectedTab }?.desk?.items ?? []).map { ["id": $0.id, "ref": $0.ref, "kind": $0.kind] },
                     "front": c.state.selectedTab.flatMap { c.model.source(for: $0) }?.tabs.first { $0.tab_id == c.state.selectedTab }?.desk?.front as Any? ?? NSNull(),
                     "active_item": c.docPanel.activeItem as Any? ?? NSNull()],
            "last_remote": RemoteActions.last,
            "opened_urls": Notifier.shared.openedURLs,
            "notifications": Notifier.shared.notifications.map { ["tab": $0.tab, "kind": $0.kind, "title": $0.title] },
            "dock_badge": Notifier.shared.dockBadge,
            "poll_ms": c.model.pollMs,
            "surfaces": surfaces,
            "lifecycle": c.registry.lifecycleState(),
            "delivered": delivered,
        ]
        let data = try! JSONSerialization.data(withJSONObject: state, options: [.prettyPrinted, .sortedKeys])
        FileManager.default.createFile(atPath: out, contents: data)
    }
}
