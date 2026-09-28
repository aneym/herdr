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
        case "reclaim":
            controller?.reclaimPane(nil)
        case "hidden_policy":
            controller?.setHiddenPolicy(obj["value"] as? String ?? "")
        case "select":
            controller?.selectTab(obj["tab"] as? String ?? "")
        case "sidebar_fold":
            // {"cmd":"sidebar_fold","id":"tab:<id>|hidden|background","open":true}: what a chevron click does.
            if let id = obj["id"] as? String, let open = obj["open"] as? Bool { controller?.state.manualOpen[id] = open }
        case "state":
            writeState(obj["out"] as? String ?? "/dev/stderr")
        case "shot":
            shot(obj["out"] as? String ?? "/tmp/shot.png")
        case "mouse":
            mouse(obj)
        case "drag_divider":
            dragDivider(obj)
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
        case "click":
            click(obj)
        case "split":
            controller?.split(obj["direction"] as? String ?? "right")
        case "scroll":
            scroll(obj)
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
        case "activate":
            NSApp.activate(ignoringOtherApps: true)
            controller?.window.makeKeyAndOrderFront(nil)
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
        let view: NSView, frame: CGRect
        switch obj["target"] as? String ?? "row" {
        case "open_full":
            view = c.detailPanel.view
            guard let f = c.detailPanel.model.targets["open_full"] else { log("hook: click: no open_full button"); return }
            frame = f
        default:
            view = c.sidebarHostView
            let label = obj["label"] as? String
            guard let line = c.sidebarLines.first(where: { $0.title == label && $0.tab != nil }),
                  let f = c.state.rowFrames[line.id] else { log("hook: click: no row \(label ?? "?")"); return }
            frame = f
        }
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
            guard let ev = NSEvent.mouseEvent(with: type, location: loc, modifierFlags: [],
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
        delivered.append("click \(obj["target"] ?? "row") \(obj["label"] ?? "") at window \(Int(loc.x)),\(Int(loc.y)) via NSApp.sendEvent")
        log("hook: click \(obj["target"] ?? "row") \(obj["label"] ?? "") frame=\(NSStringFromRect(frame)) window=\(NSStringFromPoint(loc)) key=\(c.window.isKeyWindow)")
    }

    /// Mouse events built as NSEvents addressed to the app's window and dispatched with
    /// window.sendEvent, so hit-testing, the responder chain and SurfaceView's
    /// mouse handlers run as for a physical mouse. {"cmd":"mouse","pane":id,
    /// "action":"down|up|drag|move","col":c,"row":r,"button":"left|right"}; col/row are
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
                                          context: nil, eventNumber: 0, clickCount: 1, pressure: type == .mouseMoved ? 0 : 1) else { return }
        // mouseMoved never reaches a view without acceptsMouseMovedEvents; hand it over directly.
        if type == .mouseMoved { s.mouseMoved(with: ev) } else { w.sendEvent(ev) }
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
                                              context: nil, eventNumber: 0, clickCount: 1, pressure: type == .leftMouseUp ? 0 : 1) else { return }
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
                // Not key (app not frontmost): emulate AppKit's order for a key event.
                if ev.type == .keyDown,
                   w.performKeyEquivalent(with: ev) || (NSApp.mainMenu?.performKeyEquivalent(with: ev) ?? false) {
                } else {
                    w.sendEvent(ev)
                }
            }
            if down { delivered.append("\(mods.joined(separator: "+"))\(mods.isEmpty ? "" : "+")\(name) via \(isKey ? "NSApp.sendEvent" : "window-emulated")") }
        }
    }

    /// Captures this app's own window, Metal layers included, through the window
    /// server. CGWindowListCreateImage is obsoleted in the macOS 15 SDK, so it is
    /// looked up at runtime; capturing one's own window needs no Screen Recording grant.
    private func shot(_ out: String) {
        // Ask every surface for a fresh frame first: a pane that is not being presented
        // (locked screen, occluded window) may otherwise hand back its last frame.
        for v in controller?.registry.byTerminal.values ?? [:].values {
            if let sf = v.surface { ghostty_surface_refresh(sf); ghostty_surface_draw(sf) }
        }
        RunLoop.current.run(until: Date().addingTimeInterval(0.4))
        var how = "window server"
        var img: CGImage?
        if let w = controller?.window,
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
            img = renderLayers()
            how = "layer render (window server capture was blank)"
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

    private func renderLayers() -> CGImage? {
        guard let view = controller?.window.contentView, let layer = view.layer,
              let cs = CGColorSpace(name: CGColorSpace.sRGB) else { return nil }
        let scale = controller?.window.backingScaleFactor ?? 2
        let w = Int(view.bounds.width * scale), h = Int(view.bounds.height * scale)
        guard let ctx = CGContext(data: nil, width: w, height: h, bitsPerComponent: 8, bytesPerRow: 0, space: cs,
                                  bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else { return nil }
        ctx.scaleBy(x: scale, y: scale)
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
            guard let rep = v.bitmapImageRepForCachingDisplay(in: v.bounds) else { continue }
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
                    "mouse_captured": s.mouseCaptured,
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
            "focused_pane": c.focusedSurface?.paneId ?? NSNull(),
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
            "sidebar_lines": c.sidebarLines.map { $0.dump },   // P10: the rows as drawn, in order
            "hosts": c.model.hostsModel.rows.map { r -> [String: Any] in
                ["host": r.host, "tabs": r.tabs, "slots_used": r.stats?.slotsUsed ?? NSNull(),
                 "slots_total": r.stats?.slotsTotal ?? NSNull(), "sessions": r.stats?.sessions ?? NSNull(),
                 "text": "\(r.host) \(r.detail)"]
            },
            "hosts_provider": c.model.hostsModel.provider.name,
            "theme": themeState(c),
            "detail": detailState(c),
            "poll_ms": c.model.pollMs,
            "surfaces": surfaces,
            "lifecycle": c.registry.lifecycleState(),
            "delivered": delivered,
        ]
        let data = try! JSONSerialization.data(withJSONObject: state, options: [.prettyPrinted, .sortedKeys])
        FileManager.default.createFile(atPath: out, contents: data)
    }
}
