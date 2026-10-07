import AppKit
import Carbon.HIToolbox
import GhosttyKit

/// One libghostty surface. Its process is `herdr terminal attach <terminal_id> --no-escape`,
/// so the pane renders through Ghostty's own renderer and every key is Ghostty-encoded.
///
/// Instances are retained by `SurfaceRegistry` outside the view tree (agent-zero's
/// retained-hosting decision) and reparented into pane slots; hidden, never recreated.
final class SurfaceView: NSView {
    let terminalId: String
    let paneId: String
    let clipboardSocketPath: String
    private(set) var surface: ghostty_surface_t?
    private(set) var focused = false
    var onFocus: ((SurfaceView) -> Void)?
    private(set) var exited = false
    let createdAt = Date()
    /// Seconds from surface creation to the first non-blank screen (measured).
    private(set) var firstTextAfter: Double?
    private var firstTextTimer: Timer?
    private var markedTextStore = NSMutableAttributedString()
    /// Test seam: stands in for the system input method when the app cannot be made the
    /// active app (the lab), so the client calls the system would make can be replayed.
    var interpretOverride: ((NSEvent) -> Void)?
    /// Key presses this surface has handed to Ghostty (releases not counted) and the last one's
    /// physical key and mods. The state dump reads them: the scenario proves a chord the keymap
    /// does not claim reached the pane, and that a claimed one did not.
    private(set) var keysSent = 0
    private(set) var lastKeySent = ""

    private func noteSent(keycode: UInt32, mods: UInt32, action: ghostty_input_action_e) {
        guard action != GHOSTTY_ACTION_RELEASE else { return }
        keysSent += 1
        lastKeySent = "keycode=\(keycode) mods=\(mods)"
    }
    /// Non-nil while inside keyDown: insertText collects committed text here.
    private var keyTextAccumulator: [String]?
    /// Timestamp of a command key we passed on, so doCommand can hand it back to keyDown.
    private var lastPerformKeyEvent: TimeInterval?
    private var leadSurrogate: UInt16?

    init(paneId: String, terminalId: String, command: String, env: [String: String], cwd: String) {
        self.paneId = paneId
        self.clipboardSocketPath = env["HERDR_SOCKET_PATH"] ?? ""
        self.terminalId = terminalId
        super.init(frame: NSRect(x: 0, y: 0, width: 800, height: 600))

        var cfg = ghostty_surface_config_new()
        cfg.userdata = Unmanaged.passUnretained(self).toOpaque()
        cfg.platform_tag = GHOSTTY_PLATFORM_MACOS
        cfg.platform = ghostty_platform_u(macos: ghostty_platform_macos_s(
            nsview: Unmanaged.passUnretained(self).toOpaque()))
        cfg.scale_factor = Double(NSScreen.main?.backingScaleFactor ?? 2)
        cfg.font_size = 0
        cfg.wait_after_command = false
        cfg.context = GHOSTTY_SURFACE_CONTEXT_SPLIT

        let keys = Array(env.keys)
        let cKeys = keys.map { strdup($0) }
        let cVals = keys.map { strdup(env[$0]!) }
        defer { (cKeys + cVals).forEach { free($0) } }
        var envVars = (0..<keys.count).map { ghostty_env_var_s(key: cKeys[$0], value: cVals[$0]) }

        surface = cwd.withCString { cwdp in
            command.withCString { cmdp in
                envVars.withUnsafeMutableBufferPointer { buf -> ghostty_surface_t? in
                    cfg.working_directory = cwdp
                    cfg.command = cmdp
                    cfg.env_vars = buf.baseAddress
                    cfg.env_var_count = keys.count
                    return ghostty_surface_new(GhosttyRuntime.shared.app, &cfg)
                }
            }
        }
        if surface == nil { log("ghostty_surface_new failed for \(terminalId)") }
        // Give the PTY a real grid at once. `herdr terminal attach` exits with
        // "terminal reported a zero-sized grid" if it starts before a resize.
        if let surface {
            let scale = cfg.scale_factor
            ghostty_surface_set_content_scale(surface, scale, scale)
            ghostty_surface_set_size(surface, UInt32(frame.width * scale), UInt32(frame.height * scale))
        }
        firstTextTimer = Timer.scheduledTimer(withTimeInterval: 0.02, repeats: true) { [weak self] t in
            guard let self else { t.invalidate(); return }
            if !self.visibleText().trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                self.firstTextAfter = Date().timeIntervalSince(self.createdAt)
                t.invalidate()
            }
        }
        updateTrackingAreas()
    }

    required init?(coder: NSCoder) { fatalError() }

    deinit { if let surface { ghostty_surface_free(surface) } }

    func setColorScheme(_ mode: Mode) {
        guard let surface else { return }
        ghostty_surface_set_color_scheme(surface, mode == .dark ? GHOSTTY_COLOR_SCHEME_DARK : GHOSTTY_COLOR_SCHEME_LIGHT)
    }

    /// Ends this surface's attach now (hidden-tab detach, replacement): freeing the
    /// libghostty surface closes its pty, which ends the `herdr terminal attach` client.
    /// The view itself can be kept alive a little longer for queued callbacks.
    func closeSurface() {
        guard let s = surface else { return }
        surface = nil
        exited = true
        onExit = nil
        ghostty_surface_free(s)
    }

    /// Set by `SurfaceRegistry`: called once when this surface's attach process ends.
    var onExit: ((SurfaceView) -> Void)?

    func processExited() {
        guard !exited else { return }
        exited = true
        log("surface for \(paneId) exited")
        onExit?(self)
    }

    // MARK: sizing

    override func setFrameSize(_ newSize: NSSize) {
        super.setFrameSize(newSize)
        pushSize()
        layoutLinkPreview()
    }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        viewDidChangeBackingProperties()
    }

    override func viewDidChangeBackingProperties() {
        super.viewDidChangeBackingProperties()
        guard let surface, let window else { return }
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        layer?.contentsScale = window.backingScaleFactor
        CATransaction.commit()
        let fb = convertToBacking(bounds)
        if bounds.width > 0 {
            ghostty_surface_set_content_scale(surface, fb.width / bounds.width, fb.height / bounds.height)
        }
        if let screen = window.screen,
           let id = screen.deviceDescription[NSDeviceDescriptionKey("NSScreenNumber")] as? UInt32 {
            ghostty_surface_set_display_id(surface, id)
        }
        pushSize()
    }

    private func pushSize() {
        guard let surface, bounds.width > 0, bounds.height > 0 else { return }
        let fb = convertToBacking(bounds.size)
        ghostty_surface_set_size(surface, UInt32(fb.width), UInt32(fb.height))
    }

    var gridSize: (cols: Int, rows: Int) {
        guard let surface else { return (0, 0) }
        let s = ghostty_surface_size(surface)
        return (Int(s.columns), Int(s.rows))
    }

    // MARK: focus

    // A pane whose attach has ended (held by another client, gone) takes no keys.
    override var acceptsFirstResponder: Bool { !exited }

    override func becomeFirstResponder() -> Bool {
        let ok = super.becomeFirstResponder()
        if ok { setFocused(true) }
        return ok
    }

    override func resignFirstResponder() -> Bool {
        let ok = super.resignFirstResponder()
        if ok { setFocused(false) }
        return ok
    }

    private func setFocused(_ f: Bool) {
        guard focused != f else { return }
        focused = f
        if let surface { ghostty_surface_set_focus(surface, f) }
        if f { onFocus?(self) }
    }

    // MARK: keyboard

    /// The app's own chords come from the keymap and are claimed here, before
    /// AppKit's menu and before the text input system sees them: the action runs
    /// and nothing reaches the pane. Anything the keymap does not claim goes to
    /// Ghostty, so ⌘C/⌘V (Edit menu), ctrl chords and every other key stay the
    /// terminal's.
    override func performKeyEquivalent(with event: NSEvent) -> Bool {
        guard event.type == .keyDown, focused else { return false }
        if let entry = Keymap.shared.entry(for: event) {
            return Keymap.shared.fire(entry)
        }
        if event.modifierFlags.contains(.command) {
            // Not ours: let the menu (Edit, Quit) try, and remember the event so that
            // doCommand can hand it back to keyDown if the input system swallows it.
            if event.timestamp > 0 { lastPerformKeyEvent = event.timestamp }
            return false
        }
        // Control chords: AppKit would otherwise route some (ctrl+return, ctrl+/)
        // to the responder chain instead of keyDown. Send all of them to Ghostty.
        if event.modifierFlags.contains(.control) {
            keyDown(with: event)
            return true
        }
        return false
    }

    override func keyDown(with event: NSEvent) {
        guard let surface else {
            interpretKeyEvents([event])
            return
        }

        // A claimed chord that AppKit did not offer to performKeyEquivalent (option
        // or shift chords) is claimed here: the action runs, the pane sees nothing.
        if let entry = Keymap.shared.entry(for: event) {
            Keymap.shared.fire(entry)
            return
        }

        // Chords the keymap turns into another key (⌘⌫ -> ctrl+u): they skip the input
        // system, which would only turn them into doCommand selectors. Sent as a key
        // event, not text, so the pane sees a keypress and not a paste.
        if markedTextStore.length == 0, let send = Keymap.shared.terminalSend(for: event) {
            sendChord(send, action: event.isARepeat ? GHOSTTY_ACTION_REPEAT : GHOSTTY_ACTION_PRESS)
            sendChord(send, action: GHOSTTY_ACTION_RELEASE)
            return
        }

        // Ghostty's mods for text translation (option-as-alt and friends). Hidden
        // modifier bits matter for dead keys, so only flip the four we know.
        let translationModsGhostty = eventModifierFlags(ghostty_surface_key_translation_mods(surface, ghosttyMods(event.modifierFlags)))
        var translationMods = event.modifierFlags
        for flag in [NSEvent.ModifierFlags.shift, .control, .option, .command] {
            if translationModsGhostty.contains(flag) { translationMods.insert(flag) } else { translationMods.remove(flag) }
        }
        // Reuse the original event whenever the mods are equal: Korean input needs it.
        let translationEvent: NSEvent
        if translationMods == event.modifierFlags {
            translationEvent = event
        } else {
            translationEvent = NSEvent.keyEvent(
                with: event.type, location: event.locationInWindow, modifierFlags: translationMods,
                timestamp: event.timestamp, windowNumber: event.windowNumber, context: nil,
                characters: event.characters(byApplyingModifiers: translationMods) ?? "",
                charactersIgnoringModifiers: event.charactersIgnoringModifiers ?? "",
                isARepeat: event.isARepeat, keyCode: event.keyCode) ?? event
        }

        let action = event.isARepeat ? GHOSTTY_ACTION_REPEAT : GHOSTTY_ACTION_PRESS

        // Non-nil marks "inside keyDown": insertText accumulates instead of sending.
        keyTextAccumulator = []
        defer { keyTextAccumulator = nil }

        let markedTextBefore = markedTextStore.length > 0
        // An input source switch made by this key must not reach the terminal.
        let keyboardIdBefore: String? = markedTextBefore ? nil : currentInputSourceId()
        lastPerformKeyEvent = nil

        if let interpretOverride { interpretOverride(event) } else { interpretKeyEvents([translationEvent]) }

        if !markedTextBefore && keyboardIdBefore != currentInputSourceId() { return }

        syncPreedit(clearIfNeeded: markedTextBefore)

        // Composing: preedit is showing, or this key just ended one (Japanese: begin
        // composing, press backspace: that only cancels, it must not delete text).
        let composing = markedTextStore.length > 0 || markedTextBefore

        if markedTextBefore, let list = keyTextAccumulator, !list.isEmpty {
            // The input method committed part of the preedit. Send that text alone,
            // then replay only the keys that should still act on the terminal.
            for text in list where !Self.suppressComposingControl(text, composing: composing) {
                committedText(action, text)
            }
            if shouldReplayCommittedPreeditKey(translationEvent) {
                _ = sendKey(action, event: event, translationEvent: translationEvent, text: nil, composing: false)
            }
            return
        }

        if let list = keyTextAccumulator, !list.isEmpty {
            // Text the input system composed (dead key + letter, IME commit).
            for text in list where !Self.suppressComposingControl(text, composing: composing) {
                _ = sendKey(action, event: event, translationEvent: translationEvent, text: text)
            }
        } else {
            // A control character arriving mid-composition belongs to the IME.
            if Self.suppressComposingControl(event.characters, composing: composing) { return }
            _ = sendKey(action, event: event, translationEvent: translationEvent,
                        text: ghosttyText(translationEvent), composing: composing)
        }
    }

    override func keyUp(with event: NSEvent) {
        // The press of a claimed chord never reached the terminal; neither does its release.
        if Keymap.shared.entry(for: event) != nil || Keymap.shared.terminalSend(for: event) != nil { return }
        if Keymap.shared.consumeRelease(event) { return }
        _ = sendKey(GHOSTTY_ACTION_RELEASE, event: event, translationEvent: nil, text: nil)
    }

    override func flagsChanged(with event: NSEvent) {
        let bit: UInt32
        switch event.keyCode {
        case 0x39: bit = GHOSTTY_MODS_CAPS.rawValue
        case 0x38, 0x3C: bit = GHOSTTY_MODS_SHIFT.rawValue
        case 0x3B, 0x3E: bit = GHOSTTY_MODS_CTRL.rawValue
        case 0x3A, 0x3D: bit = GHOSTTY_MODS_ALT.rawValue
        case 0x37, 0x36: bit = GHOSTTY_MODS_SUPER.rawValue
        default: return
        }
        if bit == GHOSTTY_MODS_SUPER.rawValue, let window {
            // Pressing or releasing Cmd over a resting pointer starts or ends link hover.
            let p = convert(window.mouseLocationOutsideOfEventStream, from: nil)
            if bounds.contains(p) {
                updateLinkHover(at: NSPoint(x: p.x, y: bounds.height - p.y), flags: event.modifierFlags)
            } else {
                links.hover(nil)
            }
        }
        // Modifier changes are not input while a preedit is showing.
        if hasMarkedText() { return }
        let mods = ghosttyMods(event.modifierFlags)
        _ = sendKey(mods.rawValue & bit != 0 ? GHOSTTY_ACTION_PRESS : GHOSTTY_ACTION_RELEASE,
                    event: event, translationEvent: nil, text: nil)
    }

    /// A key event built from a keymap chord (no NSEvent): physical key plus mods.
    private func sendChord(_ chord: Keymap.Chord, action: ghostty_input_action_e) {
        guard let surface else { return }
        var ev = ghostty_input_key_s()
        ev.action = action
        ev.keycode = UInt32(chord.keyCode)
        var mods: UInt32 = 0
        if chord.mods.contains(.shift) { mods |= GHOSTTY_MODS_SHIFT.rawValue }
        if chord.mods.contains(.control) { mods |= GHOSTTY_MODS_CTRL.rawValue }
        if chord.mods.contains(.option) { mods |= GHOSTTY_MODS_ALT.rawValue }
        if chord.mods.contains(.command) { mods |= GHOSTTY_MODS_SUPER.rawValue }
        ev.mods = ghostty_input_mods_e(mods)
        noteSent(keycode: ev.keycode, mods: mods, action: action)
        ev.consumed_mods = GHOSTTY_MODS_NONE
        ev.composing = false
        ev.text = nil
        ev.unshifted_codepoint = Keymap.keyCodes.first(where: { $0.value == chord.keyCode })
            .flatMap { $0.key.unicodeScalars.count == 1 ? $0.key.unicodeScalars.first?.value : nil } ?? 0
        _ = ghostty_surface_key(surface, ev)
    }

    @discardableResult
    func sendKey(_ action: ghostty_input_action_e, event: NSEvent, translationEvent: NSEvent?,
                 text: String?, composing: Bool = false) -> Bool {
        guard let surface else { return false }
        var ev = ghostty_input_key_s()
        ev.action = action
        ev.keycode = UInt32(event.keyCode)
        ev.mods = ghosttyMods(event.modifierFlags)
        noteSent(keycode: ev.keycode, mods: ev.mods.rawValue, action: action)
        // Control and command never contribute to text; assume the rest did.
        ev.consumed_mods = ghosttyMods((translationEvent?.modifierFlags ?? event.modifierFlags).subtracting([.control, .command]))
        ev.composing = composing
        ev.unshifted_codepoint = 0
        if event.type == .keyDown || event.type == .keyUp,
           let chars = event.characters(byApplyingModifiers: []),
           let cp = chars.unicodeScalars.first {
            ev.unshifted_codepoint = cp.value
        }
        if let text, let first = text.unicodeScalars.first, first.value >= 0x20 {
            return text.withCString { p in
                ev.text = p
                return ghostty_surface_key(surface, ev)
            }
        }
        ev.text = nil
        return ghostty_surface_key(surface, ev)
    }

    /// Committed IME or dictation text goes down as a key event with text, never
    /// as a paste, so programs treat it as typed input.
    private func committedText(_ action: ghostty_input_action_e, _ text: String) {
        guard let surface else { return }
        var ev = ghostty_input_key_s()
        ev.action = action
        ev.keycode = 0
        ev.mods = GHOSTTY_MODS_NONE
        ev.consumed_mods = GHOSTTY_MODS_NONE
        ev.composing = false
        ev.unshifted_codepoint = 0
        text.withCString { p in
            ev.text = p
            _ = ghostty_surface_key(surface, ev)
        }
    }

    /// Arrow keys that finished a Korean or Japanese commit still move the caret.
    private func shouldReplayCommittedPreeditKey(_ event: NSEvent) -> Bool {
        switch event.keyCode {
        case 125, 124, 126: return true
        case 123: return !event.modifierFlags.isDisjoint(with: [.shift, .control, .option, .command])
        default: return false
        }
    }

    /// True for a single C0 control character that arrives while the IME composes:
    /// it belongs to the IME and must not reach the terminal.
    static func suppressComposingControl(_ text: String?, composing: Bool) -> Bool {
        guard composing, let text else { return false }
        let scalars = text.unicodeScalars
        guard let s = scalars.first, scalars.index(after: scalars.startIndex) == scalars.endIndex else { return false }
        return s.value < 0x20
    }

    private func eventModifierFlags(_ mods: ghostty_input_mods_e) -> NSEvent.ModifierFlags {
        var f = NSEvent.ModifierFlags()
        if mods.rawValue & GHOSTTY_MODS_SHIFT.rawValue != 0 { f.insert(.shift) }
        if mods.rawValue & GHOSTTY_MODS_CTRL.rawValue != 0 { f.insert(.control) }
        if mods.rawValue & GHOSTTY_MODS_ALT.rawValue != 0 { f.insert(.option) }
        if mods.rawValue & GHOSTTY_MODS_SUPER.rawValue != 0 { f.insert(.command) }
        return f
    }

    private func currentInputSourceId() -> String? {
        guard let src = TISCopyCurrentKeyboardInputSource()?.takeRetainedValue(),
              let p = TISGetInputSourceProperty(src, kTISPropertyInputSourceID) else { return nil }
        return Unmanaged<CFString>.fromOpaque(p).takeUnretainedValue() as String
    }

    /// Same rule as Ghostty's NSEvent.ghosttyCharacters: control characters are
    /// encoded by Ghostty from the keycode, and PUA function-key glyphs are dropped.
    private func ghosttyText(_ event: NSEvent) -> String? {
        guard let chars = event.characters else { return nil }
        if chars.count == 1, let s = chars.unicodeScalars.first {
            if s.value < 0x20 {
                return event.characters(byApplyingModifiers: event.modifierFlags.subtracting(.control))
            }
            if s.value >= 0xF700 && s.value <= 0xF8FF { return nil }
        }
        return chars
    }

    /// Tell libghostty what the IME is composing so it draws the preedit at the cursor.
    private func syncPreedit(clearIfNeeded: Bool = true) {
        guard let surface else { return }
        if markedTextStore.length > 0 {
            let str = markedTextStore.string
            let len = str.utf8CString.count
            if len > 0 {
                str.withCString { ghostty_surface_preedit(surface, $0, UInt(len - 1)) }
            }
        } else if clearIfNeeded {
            ghostty_surface_preedit(surface, nil, 0)
        }
    }

    /// Cell size in view points, for the IME candidate window.
    private var cellSize: NSSize {
        guard let surface else { return .zero }
        let s = ghostty_surface_size(surface)
        return convertFromBacking(NSSize(width: Double(s.cell_width_px), height: Double(s.cell_height_px)))
    }

    /// Needs to exist so a command chord the input system turns into a selector
    /// (⌘. -> cancel:) does not beep, and so a command key we did not claim is
    /// handed back to keyDown for Ghostty to encode.
    override func doCommand(by selector: Selector) {
        if let last = lastPerformKeyEvent, let current = NSApp.currentEvent, last == current.timestamp {
            lastPerformKeyEvent = nil
            NSApp.sendEvent(current)
        }
    }

    // MARK: mouse

    private(set) var hoveredLink = ""
    /// libghostty's own hover (OSC 8, one-row URLs); the server's answer wins over it.
    private var ghosttyLink = ""
    private var cursorBeforeLink: NSCursor?
    /// Set while a Cmd-click that opened a server-resolved link is down: its drag and
    /// release belong to the link, not to the pane.
    private var linkClickDown = false
    /// Buffer a claimed gesture until its independent click resolution finishes.
    /// On a miss, replay the original press/drag/release through the native path.
    private final class LinkGesture {
        var events: [NSEvent]
        init(_ event: NSEvent) { events = [event] }
    }
    private var pendingLinkGesture: LinkGesture?
    private lazy var links: TerminalLinks = {
        let links = TerminalLinks(paneId: paneId, socketPath: clipboardSocketPath)
        links.readSpan = { [unowned self] r in self.readCells((r.start_col, r.row), (r.end_col, r.row)) }
        links.onChange = { [unowned self] _ in self.showLink() }
        return links
    }()
    private lazy var linkPreview: LinkPreview = {
        let label = LinkPreview(labelWithString: "")
        label.font = .systemFont(ofSize: 11)
        label.textColor = .secondaryLabelColor
        label.backgroundColor = .windowBackgroundColor
        label.drawsBackground = true
        label.lineBreakMode = .byTruncatingMiddle
        label.isHidden = true
        addSubview(label)
        return label
    }()

    /// libghostty's MOUSE_OVER_LINK.
    func setHoveredLink(_ url: String) {
        ghosttyLink = url
        showLink()
    }

    private func showLink() {
        let url = links.url.isEmpty ? ghosttyLink : links.url
        guard hoveredLink != url else { return }
        if hoveredLink.isEmpty, !url.isEmpty { cursorBeforeLink = NSCursor.current }
        hoveredLink = url
        linkPreview.stringValue = url
        linkPreview.isHidden = url.isEmpty
        layoutLinkPreview()
        if url.isEmpty {
            cursorBeforeLink?.set()
            cursorBeforeLink = nil
        } else {
            NSCursor.pointingHand.set()
        }
        window?.invalidateCursorRects(for: self)
    }

    override func resetCursorRects() {
        super.resetCursorRects()
        if !hoveredLink.isEmpty { addCursorRect(bounds, cursor: .pointingHand) }
    }

    private func layoutLinkPreview() {
        guard !hoveredLink.isEmpty else { return }
        let width = min(max(0, bounds.width - 12), linkPreview.intrinsicContentSize.width + 8)
        linkPreview.frame = NSRect(x: 6, y: 4, width: width, height: 18)
    }

    override func updateTrackingAreas() {
        trackingAreas.forEach { removeTrackingArea($0) }
        addTrackingArea(NSTrackingArea(
            rect: bounds,
            options: [.mouseEnteredAndExited, .mouseMoved, .inVisibleRect, .activeAlways],
            owner: self, userInfo: nil))
    }

    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }

    /// Position in the surface's own point space, top-left origin (Ghostty's convention).
    private func surfacePoint(_ event: NSEvent) -> NSPoint {
        let p = convert(event.locationInWindow, from: nil)
        return NSPoint(x: p.x, y: bounds.height - p.y)
    }

    private func sendPos(_ event: NSEvent) {
        guard let surface else { return }
        let p = surfacePoint(event)
        ghostty_surface_mouse_pos(surface, p.x, p.y, ghosttyMouseMods(event.modifierFlags))
    }

    /// The viewport cell at a surface point, nil outside the grid.
    private func viewportCell(_ p: NSPoint) -> Cell? {
        let g = gridSize, c = cellPoints
        guard g.cols > 0, g.rows > 0, c.width > 0, c.height > 0 else { return nil }
        let x = (p.x - paddingPoints.x) / c.width, y = (p.y - paddingPoints.y) / c.height
        guard x >= 0, y >= 0, Int(x) < g.cols, Int(y) < g.rows else { return nil }
        return (Int(x), Int(y))
    }

    /// Cmd held over the grid with no button down asks the server for the link there.
    private func updateLinkHover(at p: NSPoint, flags: NSEvent.ModifierFlags) {
        guard flags.contains(.command), NSEvent.pressedMouseButtons == 0 else {
            links.hover(nil)
            return
        }
        links.hover(viewportCell(p))
    }

    /// Ghostty decides what a mouse event means: selection when the program has
    /// not asked for the mouse, mouse reports (through herdr's attach client, to
    /// the pane) when it has. Shift forces selection, as in Ghostty itself.
    private func sendButton(_ state: ghostty_input_mouse_state_e, _ button: ghostty_input_mouse_button_e,
                            _ event: NSEvent) -> Bool {
        guard let surface else { return false }
        sendPos(event)
        return ghostty_surface_mouse_button(surface, state, button, ghosttyMouseMods(event.modifierFlags))
    }

    override func mouseDown(with event: NSEvent) {
        // Hand first responder to the surface on every click (agent-zero's
        // terminals never took keys because this only happened once).
        window?.makeFirstResponder(self)
        if event.modifierFlags.contains(.command), let c = viewportCell(surfacePoint(event)) {
            let gesture = LinkGesture(event)
            pendingLinkGesture = gesture
            links.activate(at: c, shift: event.modifierFlags.contains(.shift)) { [weak self] opened in
                guard let self else { return }
                let events = gesture.events
                let isCurrent = self.pendingLinkGesture === gesture
                if isCurrent { self.pendingLinkGesture = nil }
                if opened {
                    if isCurrent { self.linkClickDown = !events.contains(where: { $0.type == .leftMouseUp }) }
                } else {
                    for event in events {
                        switch event.type {
                        case .leftMouseDown: self.mouseDownNative(event)
                        case .leftMouseUp:
                            self.noteRelease()
                            _ = self.sendButton(GHOSTTY_MOUSE_RELEASE, GHOSTTY_MOUSE_LEFT, event)
                        case .leftMouseDragged:
                            if self.appDragStart != nil, let c = self.cell(event) { self.appDragEnd = c }
                            self.sendPos(event)
                        default: break
                        }
                    }
                }
            }
            return
        }
        mouseDownNative(event)
    }

    private func mouseDownNative(_ event: NSEvent) {
        notePress(event)
        _ = sendButton(GHOSTTY_MOUSE_PRESS, GHOSTTY_MOUSE_LEFT, event)
    }

    override func mouseUp(with event: NSEvent) {
        if let gesture = pendingLinkGesture { gesture.events.append(event); return }
        if linkClickDown { linkClickDown = false; return }
        noteRelease()
        _ = sendButton(GHOSTTY_MOUSE_RELEASE, GHOSTTY_MOUSE_LEFT, event)
    }

    override func rightMouseDown(with event: NSEvent) {
        window?.makeFirstResponder(self)
        // A shadow selection or an unconsumed click takes AppKit's context menu path.
        if appSelection != nil || !sendButton(GHOSTTY_MOUSE_PRESS, GHOSTTY_MOUSE_RIGHT, event) { super.rightMouseDown(with: event) }
    }

    override func rightMouseUp(with event: NSEvent) {
        if appSelection != nil || !sendButton(GHOSTTY_MOUSE_RELEASE, GHOSTTY_MOUSE_RIGHT, event) { super.rightMouseUp(with: event) }
    }

    override func otherMouseDown(with event: NSEvent) {
        guard event.buttonNumber == 2 else { super.otherMouseDown(with: event); return }
        window?.makeFirstResponder(self)
        _ = sendButton(GHOSTTY_MOUSE_PRESS, GHOSTTY_MOUSE_MIDDLE, event)
    }

    override func otherMouseUp(with event: NSEvent) {
        guard event.buttonNumber == 2 else { super.otherMouseUp(with: event); return }
        _ = sendButton(GHOSTTY_MOUSE_RELEASE, GHOSTTY_MOUSE_MIDDLE, event)
    }

    override func mouseMoved(with event: NSEvent) {
        updateLinkHover(at: surfacePoint(event), flags: event.modifierFlags)
        sendPos(event)
    }
    override func mouseDragged(with event: NSEvent) {
        if let gesture = pendingLinkGesture { gesture.events.append(event); return }
        if linkClickDown { return }
        if appDragStart != nil, let c = cell(event) { appDragEnd = c }
        sendPos(event)
    }
    override func rightMouseDragged(with event: NSEvent) { sendPos(event) }
    override func otherMouseDragged(with event: NSEvent) { sendPos(event) }
    override func mouseEntered(with event: NSEvent) {
        updateLinkHover(at: surfacePoint(event), flags: event.modifierFlags)
        sendPos(event)
    }

    override func mouseExited(with event: NSEvent) {
        links.clear()
        setHoveredLink("")
        // Leave the position alone while a button is held: a drag selection keeps
        // extending outside the view. Otherwise park it outside so hover state clears.
        guard let surface, NSEvent.pressedMouseButtons == 0 else { return }
        ghostty_surface_mouse_pos(surface, -1, -1, ghosttyMouseMods(event.modifierFlags))
    }

    /// Packed like Ghostty's macOS app: bit 0 precise (trackpad), bits 1...3 momentum phase.
    override func scrollWheel(with event: NSEvent) {
        guard let surface else { return }
        var x = event.scrollingDeltaX
        var y = event.scrollingDeltaY
        let precise = event.hasPreciseScrollingDeltas
        if precise { x *= 2; y *= 2 }
        let momentum: ghostty_input_mouse_momentum_e
        switch event.momentumPhase {
        case .began: momentum = GHOSTTY_MOUSE_MOMENTUM_BEGAN
        case .stationary: momentum = GHOSTTY_MOUSE_MOMENTUM_STATIONARY
        case .changed: momentum = GHOSTTY_MOUSE_MOMENTUM_CHANGED
        case .ended: momentum = GHOSTTY_MOUSE_MOMENTUM_ENDED
        case .cancelled: momentum = GHOSTTY_MOUSE_MOMENTUM_CANCELLED
        case .mayBegin: momentum = GHOSTTY_MOUSE_MOMENTUM_MAY_BEGIN
        default: momentum = GHOSTTY_MOUSE_MOMENTUM_NONE
        }
        let mods = (precise ? 1 : 0) | Int32(momentum.rawValue) << 1
        ghostty_surface_mouse_scroll(surface, x, y, mods)
    }

    // MARK: copy

    /// A drag that a program owning the mouse receives (Claude Code, codex) selects
    /// nothing in Ghostty, so ⌘C had nothing to copy. As in the TUI, keep a shadow of
    /// it: the text under the dragged cells, read at release. A double click keeps the
    /// word, a triple click the line. Any left press clears it, so it is always newer
    /// than Ghostty's own selection.
    private var appDragStart: Cell?
    private var appDragEnd: Cell?
    private(set) var appSelection: String?
    /// The shadow's cells; the pasteboard's change count when it was made; and, once the
    /// program has had time to answer the release, whether it copied the selection itself.
    private var appRange: (Cell, Cell)?
    private var appBoardCount = 0
    private var appProgramCopied: Bool?
    private var appShadowId = 0
    typealias Cell = (col: Int, row: Int)

    /// ⌘C in a pane: the shadow of a drag the program received, else Ghostty's own
    /// selection (a shift-drag, or any drag where the program does not own the mouse).
    func copySelection() -> Bool {
        guard let surface else { return false }
        if let text = appSelection {
            // The program copied this selection itself (Claude's copy on select, OSC 52, which
            // herdr's attach client puts on the pasteboard with pbcopy).
            if appProgramCopied ?? (NSPasteboard.general.changeCount != appBoardCount) { return true }
            appProgramCopied = false
            let board = NSPasteboard.general
            board.clearContents()
            board.setString(text, forType: .string)
            if let range = appRange { refineCopy(text, range, changeCount: board.changeCount) }
            return true
        }
        let action = "copy_to_clipboard"
        return ghostty_surface_binding_action(surface, action, UInt(action.utf8.count))
    }

    /// The attach redraws rows with cursor positioning, so the grid read above splits a
    /// soft-wrapped line and keeps blanks a redraw wrote. The server reads the same cells
    /// from the pane's wrap-aware screen; its text replaces the copy unless the pasteboard
    /// changed meanwhile. An older server or a moved viewport keeps the grid's text.
    private func refineCopy(_ text: String, _ range: (Cell, Cell), changeCount: Int) {
        let commands = HerdrCommands(socketPath: clipboardSocketPath), paneId = paneId
        DispatchQueue.global(qos: .userInitiated).async {
            guard let exact = commands.paneViewportText(paneId: paneId, from: range.0, to: range.1),
                  exact != text, !exact.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { return }
            DispatchQueue.main.async {
                let board = NSPasteboard.general
                guard board.changeCount == changeCount else { return }
                board.clearContents()
                board.setString(exact, forType: .string)
            }
        }
    }

    private func setShadow(_ a: Cell, _ b: Cell) {
        let (s, e) = (a.row, a.col) <= (b.row, b.col) ? (a, b) : (b, a)
        appSelection = nonEmpty(readCells(s, e))
        appRange = appSelection == nil ? nil : (s, e)
        appBoardCount = NSPasteboard.general.changeCount
        appProgramCopied = nil
        appShadowId += 1
        // A program copies on the release or not at all; a later pasteboard change (another
        // app, or this ⌘C) does not mean it did.
        let id = appShadowId
        DispatchQueue.main.asyncAfter(deadline: .now() + 1.5) { [weak self] in
            guard let self, self.appShadowId == id, self.appProgramCopied == nil else { return }
            self.appProgramCopied = NSPasteboard.general.changeCount != self.appBoardCount
        }
    }

    private func notePress(_ event: NSEvent) {
        appSelection = nil
        appRange = nil
        appDragStart = nil
        // Shift, and ⌘ through ghosttyMouseMods, make Ghostty select instead of reporting.
        guard mouseCaptured, event.modifierFlags.isDisjoint(with: [.shift, .command]),
              let c = cell(event) else { return }
        switch event.clickCount {
        case 2: if let w = wordRange(at: c) { setShadow(w.0, w.1) }
        case 3: setShadow((0, c.row), (gridSize.cols - 1, c.row))
        default: appDragStart = c; appDragEnd = c
        }
    }

    private func noteRelease() {
        defer { appDragStart = nil }
        guard let a = appDragStart, let b = appDragEnd, a != b else { return }
        setShadow(a, b)
    }

    private func nonEmpty(_ s: String) -> String? {
        s.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? nil : s
    }

    /// The viewport cell under the pointer, clamped to the grid.
    private func cell(_ event: NSEvent) -> Cell? {
        let g = gridSize, c = cellPoints
        guard g.cols > 0, g.rows > 0, c.width > 0, c.height > 0 else { return nil }
        let p = surfacePoint(event)
        let col = Int(((p.x - paddingPoints.x) / c.width).rounded(.down))
        let row = Int(((p.y - paddingPoints.y) / c.height).rounded(.down))
        return (min(max(col, 0), g.cols - 1), min(max(row, 0), g.rows - 1))
    }

    /// Text from one viewport cell to another in reading order, as a stream selection,
    /// without the blank cells that pad each row (Ghostty's own copy drops them too).
    private func readCells(_ a: Cell, _ b: Cell) -> String {
        guard let surface else { return "" }
        let (s, e) = (a.row, a.col) <= (b.row, b.col) ? (a, b) : (b, a)
        let sel = ghostty_selection_s(
            top_left: ghostty_point_s(tag: GHOSTTY_POINT_VIEWPORT, coord: GHOSTTY_POINT_COORD_EXACT,
                                      x: UInt32(s.col), y: UInt32(s.row)),
            bottom_right: ghostty_point_s(tag: GHOSTTY_POINT_VIEWPORT, coord: GHOSTTY_POINT_COORD_EXACT,
                                          x: UInt32(e.col), y: UInt32(e.row)),
            rectangle: false)
        var text = ghostty_text_s()
        guard ghostty_surface_read_text(surface, sel, &text) else { return "" }
        defer { ghostty_surface_free_text(surface, &text) }
        guard let p = text.text else { return "" }
        return String(cString: p).split(separator: "\n", omittingEmptySubsequences: false)
            .map { $0.replacingOccurrences(of: "\\s+$", with: "", options: .regularExpression) }
            .joined(separator: "\n")
    }

    /// The run of non-blank cells around a cell on its row.
    private func wordRange(at c: Cell) -> (Cell, Cell)? {
        let inWord = { (col: Int) in
            !self.readCells((col, c.row), (col, c.row)).trimmingCharacters(in: .whitespaces).isEmpty
        }
        guard inWord(c.col) else { return nil }
        var lo = c.col, hi = c.col
        while lo > 0, inWord(lo - 1) { lo -= 1 }
        while hi < gridSize.cols - 1, inWord(hi + 1) { hi += 1 }
        return ((lo, c.row), (hi, c.row))
    }

    /// Right click the program does not take: Copy and Paste, through the Edit menu's actions.
    override func menu(for event: NSEvent) -> NSMenu? {
        let menu = NSMenu()
        menu.addItem(withTitle: "Copy", action: #selector(MainWindowController.copy(_:)), keyEquivalent: "")
        menu.addItem(withTitle: "Paste", action: #selector(MainWindowController.paste(_:)), keyEquivalent: "")
        return menu
    }

    /// Selected text as Ghostty sees it, nil when nothing is selected (test hook).
    func selectedText() -> String? {
        guard let surface, ghostty_surface_has_selection(surface) else { return nil }
        var text = ghostty_text_s()
        guard ghostty_surface_read_selection(surface, &text) else { return nil }
        defer { ghostty_surface_free_text(surface, &text) }
        guard let p = text.text else { return nil }
        return String(cString: p)
    }

    /// One grid cell in points, and the padding before the grid (window-padding-x/y).
    var cellPoints: NSSize {
        guard let surface, bounds.width > 0 else { return .zero }
        let sz = ghostty_surface_size(surface)
        let scale = CGFloat(sz.width_px) / bounds.width
        return NSSize(width: CGFloat(sz.cell_width_px) / scale, height: CGFloat(sz.cell_height_px) / scale)
    }
    var paddingPoints: NSPoint { NSPoint(x: 8, y: 6) }

    var mouseCaptured: Bool { surface.map { ghostty_surface_mouse_captured($0) } ?? false }

    // MARK: test hook

    /// Visible text of this surface, read from Ghostty's own screen state.
    func visibleText() -> String {
        guard let surface else { return "" }
        let sel = ghostty_selection_s(
            top_left: ghostty_point_s(tag: GHOSTTY_POINT_VIEWPORT, coord: GHOSTTY_POINT_COORD_TOP_LEFT, x: 0, y: 0),
            bottom_right: ghostty_point_s(tag: GHOSTTY_POINT_VIEWPORT, coord: GHOSTTY_POINT_COORD_BOTTOM_RIGHT, x: 0, y: 0),
            rectangle: false)
        var text = ghostty_text_s()
        guard ghostty_surface_read_text(surface, sel, &text) else { return "" }
        defer { ghostty_surface_free_text(surface, &text) }
        guard let p = text.text else { return "" }
        return String(cString: p)
    }
}

func ghosttyMods(_ flags: NSEvent.ModifierFlags) -> ghostty_input_mods_e {
    var m: UInt32 = GHOSTTY_MODS_NONE.rawValue
    if flags.contains(.shift) { m |= GHOSTTY_MODS_SHIFT.rawValue }
    if flags.contains(.control) { m |= GHOSTTY_MODS_CTRL.rawValue }
    if flags.contains(.option) { m |= GHOSTTY_MODS_ALT.rawValue }
    if flags.contains(.command) { m |= GHOSTTY_MODS_SUPER.rawValue }
    if flags.contains(.capsLock) { m |= GHOSTTY_MODS_CAPS.rawValue }
    return ghostty_input_mods_e(m)
}

/// Command is the macOS link click. While a program owns the mouse, Ghostty
/// only rechecks links for the shift capture-override, so a ⌘-hover has to
/// carry that bit too or the URL never becomes a link under the cursor.
/// Keyboard paths stay on `ghosttyMods`: an unclaimed ⌘ chord must not arrive as super+shift.
func ghosttyMouseMods(_ flags: NSEvent.ModifierFlags) -> ghostty_input_mods_e {
    var m = ghosttyMods(flags).rawValue
    if flags.contains(.command) { m |= GHOSTTY_MODS_SHIFT.rawValue }
    return ghostty_input_mods_e(m)
}

// MARK: NSTextInputClient

/// Marked text, dead keys and IME, ported from Ghostty's own SurfaceView_AppKit.swift
/// (MIT). keyDown feeds events to the text input system; committed text comes back
/// through insertText and marked text through setMarkedText.
extension SurfaceView: NSTextInputClient {
    func hasMarkedText() -> Bool { markedTextStore.length > 0 }

    func markedRange() -> NSRange {
        guard markedTextStore.length > 0 else { return NSRange(location: NSNotFound, length: 0) }
        return NSRange(location: 0, length: markedTextStore.length)
    }

    func selectedRange() -> NSRange {
        guard let surface else { return NSRange() }
        var text = ghostty_text_s()
        guard ghostty_surface_read_selection(surface, &text) else { return NSRange() }
        defer { ghostty_surface_free_text(surface, &text) }
        return NSRange(location: Int(text.offset_start), length: Int(text.offset_len))
    }

    func setMarkedText(_ string: Any, selectedRange: NSRange, replacementRange: NSRange) {
        switch string {
        case let v as NSAttributedString: markedTextStore = NSMutableAttributedString(attributedString: v)
        case let v as String: markedTextStore = NSMutableAttributedString(string: v)
        default: log("unknown marked text: \(string)")
        }
        // Outside a keyDown (an input source change while composing) show it at once.
        if keyTextAccumulator == nil { syncPreedit() }
    }

    func unmarkText() {
        if markedTextStore.length > 0 {
            markedTextStore.mutableString.setString("")
            syncPreedit()
        }
    }

    func validAttributesForMarkedText() -> [NSAttributedString.Key] { [] }

    func attributedSubstring(forProposedRange range: NSRange, actualRange: NSRangePointer?) -> NSAttributedString? {
        guard let surface, range.length > 0 else { return nil }
        var text = ghostty_text_s()
        guard ghostty_surface_read_selection(surface, &text) else { return nil }
        defer { ghostty_surface_free_text(surface, &text) }
        guard let p = text.text else { return nil }
        return NSAttributedString(string: String(cString: p))
    }

    func characterIndex(for point: NSPoint) -> Int { 0 }

    func firstRect(forCharacterRange range: NSRange, actualRange: NSRangePointer?) -> NSRect {
        guard let surface else { return NSRect(x: frame.origin.x, y: frame.origin.y, width: 0, height: 0) }
        // Ghostty says where the IME window belongs (top-left origin).
        var x = 0.0, y = 0.0, width = Double(cellSize.width), height = Double(cellSize.height)
        ghostty_surface_ime_point(surface, &x, &y, &width, &height)
        if range.length == 0, width > 0 {
            // A zero width lets the dictation indicator start at the caret.
            width = 0
            x += Double(cellSize.width) * Double(range.location + range.length)
        }
        let viewRect = NSRect(x: x, y: frame.size.height - y, width: width, height: max(height, Double(cellSize.height)))
        let winRect = convert(viewRect, to: nil)
        guard let window else { return winRect }
        return window.convertToScreen(winRect)
    }

    func insertText(_ string: Any, replacementRange: NSRange) {
        // keyDown sets the accumulator before the input method calls back. An in-process
        // key (the check hook) has no NSApp.currentEvent, and still has to commit.
        guard keyTextAccumulator != nil || NSApp.currentEvent != nil else { return }
        var chars = ""
        if let v = string as? String {
            chars = v
        } else if let v = string as? NSAttributedString {
            chars = v.string
        } else if let v = string as? NSString {
            if v.length == 1, UTF16.isLeadSurrogate(v.character(at: 0)) {
                leadSurrogate = v.character(at: 0)
            } else if v.length == 1, UTF16.isTrailSurrogate(v.character(at: 0)) {
                // A trail with no lead is dropped, as Terminal.app does.
                if let lead = leadSurrogate { chars = String(decoding: [lead, v.character(at: 0)], as: UTF16.self) }
                leadSurrogate = nil
            } else {
                chars = v as String
                leadSurrogate = nil
            }
        } else {
            return
        }
        // insertText means the preedit is over.
        unmarkText()
        // Inside keyDown the text is collected and sent with the key event.
        if var acc = keyTextAccumulator {
            acc.append(chars)
            keyTextAccumulator = acc
            return
        }
        if !chars.isEmpty { committedText(GHOSTTY_ACTION_PRESS, chars) }
    }
}

/// The preview must never intercept terminal clicks or drag selection.
private final class LinkPreview: NSTextField {
    override func hitTest(_ point: NSPoint) -> NSView? { nil }
}
