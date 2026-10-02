import AppKit
import Combine
import GhosttyKit
import SwiftUI

// SurfaceRegistry (retained surfaces and their attach lifecycle) lives in SurfaceRegistry.swift.

/// Lays out one tab's panes using herdr's own split geometry (layouts[].panes[].rect).
final class CapHostingView: NSHostingView<PaneCapBar> {
    override var acceptsFirstResponder: Bool { false }
}

final class FactoryHostingView: NSHostingView<FactoryPage> {
    override var acceptsFirstResponder: Bool { false }
}

final class PaneHostView: NSView {
    var rects: [(SurfaceView, Snapshot.Rect)] = []
    var area = Snapshot.Rect(x: 0, y: 0, width: 1, height: 1)
    /// Grab strips over the split dividers (P9), rebuilt from herdr's layout on every `show`.
    private var handles: [String: DividerHandleView] = [:]
    var onDividerDrag: ((PaneDivider, DividerHandleView.Phase, CGFloat, CGFloat) -> Void)?
    var dividerHandles: [DividerHandleView] { handles.values.sorted { $0.divider.splitId < $1.divider.splitId } }
    var caps: [String: PaneCapState] = [:]
    var capAction: ((String, String) -> Void)?
    var chatViews: [String: NSView] = [:]
    var tokens = ThemeStore.tokens(for: .dark, terminal: TerminalTheme(dark: "", light: "", darkColors: TerminalTheme.fallbackDark, lightColors: TerminalTheme.fallbackLight))
    private var capHosts: [String: NSHostingView<PaneCapBar>] = [:]
    var capFrames: [String: NSRect] { capHosts.mapValues { $0.frame } }

    override var isFlipped: Bool { true }

    override init(frame: NSRect) {
        super.init(frame: frame)
        wantsLayer = true
        // Terminal panes are always opaque: the terminal token, never glass.
        layer?.isOpaque = true
    }

    func setBackground(_ c: NSColor) { layer?.backgroundColor = c.cgColor }

    required init?(coder: NSCoder) { fatalError() }

    func show(_ items: [(SurfaceView, Snapshot.Rect)], area: Snapshot.Rect, dividers: [PaneDivider] = []) {
        let keep = Set(items.map { ObjectIdentifier($0.0) })
        for v in subviews where v is SurfaceView && !keep.contains(ObjectIdentifier(v)) { v.removeFromSuperview() }
        for (s, _) in items where s.superview !== self { addSubview(s) }
        syncHandles(dividers)
        rects = items
        self.area = area
        needsLayout = true
        layout()
    }

    override func layout() {
        super.layout()
        guard area.width > 0, area.height > 0 else { return }
        let sx = bounds.width / area.width, sy = bounds.height / area.height
        let live = Set(rects.map { $0.0.paneId })
        for (id, v) in capHosts where !live.contains(id) { v.removeFromSuperview(); capHosts[id] = nil }
        for (id, v) in chatViews where !live.contains(id) { v.removeFromSuperview(); chatViews[id] = nil }
        for (s, r) in rects {
            let gapL: CGFloat = r.x > area.x ? 1 : 0
            let gapT: CGFloat = r.y > area.y ? 1 : 0
            let full = NSRect(x: (r.x - area.x) * sx + gapL, y: (r.y - area.y) * sy + gapT,
                             width: r.width * sx - gapL, height: r.height * sy - gapT).integral
            let capH = min(36, full.height)
            let capFrame = NSRect(x: full.minX, y: full.minY, width: full.width, height: capH)
            let body = NSRect(x: full.minX, y: full.minY + capH, width: full.width, height: max(0, full.height - capH))
            let cap = ensureCap(s.paneId)
            cap.frame = capFrame
            let chatting = caps[s.paneId]?.chat == true
            s.isHidden = chatting
            s.frame = body
            if let chat = chatViews[s.paneId] {
                if chat.superview !== self { addSubview(chat) }
                chat.isHidden = !chatting
                chat.frame = body
            }
        }
        let reach: CGFloat = 4
        for h in handles.values {
            let d = h.divider
            let line = ((d.vertical ? d.pos - area.x : d.pos - area.y)) * (d.vertical ? sx : sy)
            let r = d.splitRect
            h.frame = d.vertical
                ? NSRect(x: line - reach, y: (r.y - area.y) * sy, width: 2 * reach + 1, height: r.height * sy)
                : NSRect(x: (r.x - area.x) * sx, y: line - reach, width: r.width * sx, height: 2 * reach + 1)
            h.window?.invalidateCursorRects(for: h)
        }
    }

    private func ensureCap(_ paneId: String) -> NSView {
        if let v = capHosts[paneId] {
            if let state = caps[paneId] {
                v.rootView = PaneCapBar(state: state, tokens: tokens,
                                        onTerminal: { [weak self] in self?.capAction?(paneId, "terminal") },
                                        onChat: { [weak self] in self?.capAction?(paneId, "chat") },
                                        onFocus: { [weak self] in self?.capAction?(paneId, "focus") },
                                        onFull: { [weak self] in self?.capAction?(paneId, "full") })
            }
            return v
        }
        let state = caps[paneId] ?? PaneCapState(paneId: paneId, name: "Brief", agent: false, focused: false, chat: false, density: "focus", glyph: .asleep)
        let v = CapHostingView(rootView: PaneCapBar(state: state, tokens: tokens,
                                                   onTerminal: { [weak self] in self?.capAction?(paneId, "terminal") },
                                                   onChat: { [weak self] in self?.capAction?(paneId, "chat") },
                                                   onFocus: { [weak self] in self?.capAction?(paneId, "focus") },
                                                   onFull: { [weak self] in self?.capAction?(paneId, "full") }))
        capHosts[paneId] = v
        addSubview(v)
        return v
    }

    func placeChat(_ paneId: String, view: NSView?) {
        if chatViews[paneId] !== view {
            chatViews[paneId]?.removeFromSuperview()
            chatViews[paneId] = view
            if let view { addSubview(view) }
        }
        needsLayout = true
    }

    /// One handle per divider id, kept above the surfaces.
    private func syncHandles(_ dividers: [PaneDivider]) {
        let ids = Set(dividers.map(\.splitId))
        for (id, h) in handles where !ids.contains(id) { h.removeFromSuperview(); handles[id] = nil }
        for d in dividers {
            let h = handles[d.splitId] ?? {
                let n = DividerHandleView(divider: d)
                handles[d.splitId] = n
                return n
            }()
            h.divider = d
            h.lineColor = tokens.splitNS
            h.hoverColor = tokens.inkNS.withAlphaComponent(0.26)
            h.dragColor = tokens.accentNS.withAlphaComponent(0.75)
            h.needsDisplay = true
            h.onDrag = { [weak self] div, phase, delta in
                guard let self else { return }
                let scale = div.vertical ? bounds.width / area.width : bounds.height / area.height
                onDividerDrag?(div, phase, delta, CGFloat(div.extent) * scale)
            }
            addSubview(h, positioned: .above, relativeTo: nil)
        }
    }
}

final class MainWindowController: NSObject, NSWindowDelegate {
    let window: NSWindow
    let model: HerdrModel
    let state = SidebarState()
    let registry: SurfaceRegistry
    let host = PaneHostView(frame: .zero)
    let theme: ThemeStore
    // Set in init after super.init (they need `self` for the sidebar callbacks).
    private var sidebarContainer: SidebarContainer!
    var root: RootView!
    /// P11: details beside the sidebar (DetailPanel.swift).
    private(set) var detailPanel: DetailPanelController!
    /// P15/P16: docs on the right of the panes.
    private(set) var docPanel: DocPanelController!
    private var focusedPaneByTab: [String: String] = [:]
    private var lastLayoutKey = ""
    let commands: HerdrCommands
    let resizer: PaneResizeController
    /// The layout the pane host currently shows (from a snapshot or a resize reply).
    private(set) var shownLayout: Snapshot.Layout?
    private var pendingSelectTab: String?
    private var pendingFocusPane: String?
    private var didRestoreTabFocus = false
    private var didRestorePaneFocus = false

    init(model: HerdrModel, registry: SurfaceRegistry, theme: ThemeStore) {
        self.model = model
        self.registry = registry
        self.theme = theme
        commands = HerdrCommands(socketPath: model.env["HERDR_SOCKET_PATH"] ?? "")
        resizer = PaneResizeController(client: commands)
        window = ShellWindow(contentRect: NSRect(x: 120, y: 120, width: 1400, height: 820),
                             styleMask: [.titled, .closable, .miniaturizable, .resizable, .fullSizeContentView],
                             backing: .buffered, defer: false)
        super.init()
        window.title = Channel.name
        if Channel.kind == .dev { window.subtitle = "DEV" }
        window.titlebarAppearsTransparent = true
        window.delegate = self
        window.isReleasedWhenClosed = false

        let sidebar = NSHostingView(rootView: SidebarView(model: model, state: state, theme: theme,
                                                          openDetail: { [weak self] row in self?.toggleDetail(row.id) },
                                                          select: { [weak self] tab in
                                                              guard let self else { return }
                                                              if self.state.mode == .areas { self.selectAreaTab(tab) } else { self.selectTab(tab) }
                                                          },
                                                          onFactory: { [weak self] in self?.toggleFactory() }))
        sidebarContainer = SidebarContainer(content: sidebar)
        root = RootView(sidebar: sidebarContainer, host: host)
        detailPanel = DetailPanelController(herdr: model, theme: theme) { [weak self] id in self?.openFullTab(id) }
        root.detail = detailPanel.view
        docPanel = DocPanelController()
        docPanel.attach(self)
        docPanel.onClose = { [weak self] in self?.setDocs(open: false) }
        docPanel.onWidth = { [weak self] w in self?.setDocs(width: w) }
        root.docs = docPanel.view
        applyDocs()
        Keymap.shared.addContextual(chord: "escape", action: "close_detail") { [weak self] in
            guard let self else { return false }
            return self.docPanel.hasFocus || self.detailClaimsEscape
        }
        Keymap.shared.addContextual(chord: "cmd+l", action: "focus_doc_address") { [weak self] in
            self?.docPanel.hasFocus == true && self?.docPanel.showingWeb == true
        }
        window.contentView = root
        applyTheme()
        registry.onReplace = { [weak self] old, new in self?.replaceSurface(old: old, new: new) }

        theme.objectWillChange.receive(on: RunLoop.main).sink { [weak self] _ in
            // objectWillChange fires before the value lands; read it on the next turn.
            DispatchQueue.main.async { self?.applyTheme() }
        }.store(in: &bag)
        model.$snapshot.receive(on: RunLoop.main).sink { [weak self] _ in self?.snapshotChanged() }.store(in: &bag)
        model.catalog.objectWillChange.receive(on: RunLoop.main).sink { [weak self] _ in
            DispatchQueue.main.async { self?.refreshDocs() }
        }.store(in: &bag)

        host.onDividerDrag = { [weak self] d, phase, delta, extentPx in
            self?.resizer.handle(d, phase, deltaPx: delta, extentPx: extentPx)
        }
        host.capAction = { [weak self] id, action in self?.setPaneMode(id, action) }
        resizer.currentRatio = { [weak self] id in self?.shownLayout?.splits?.first { $0.id == id }?.ratio }
        resizer.onLayout = { [weak self] layout in self?.refreshHost(using: layout) }
        // Snapshots are held back during a drag; catch up with the latest one afterwards.
        resizer.onIdle = { [weak self] in
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.2) { self?.refreshHost() }
        }
        updates = UpdateController(controller: self)
    }

    var updates: UpdateController?

    @objc func checkForUpdates(_ sender: Any?) { updates?.checkNow() }

    /// Persist the UI the shell can put back after it is replaced. The panes stay up.
    func saveForRelaunch() {
        state.saveSelected()
        state.saveDocs()
        if let row = state.detailRow { Channel.store.set(row, forKey: Channel.detailRowKey) }
        let f = window.frame
        Channel.store.set("\(f.origin.x),\(f.origin.y),\(f.size.width),\(f.size.height)", forKey: Channel.frameKey)
        Channel.store.synchronize()
        let obj: [String: Any] = [
            "selectedTab": state.selectedTab ?? "",
            "selectedRow": state.detailRow ?? "",
            "sidebarMode": state.mode.rawValue,
            "docTabs": docPanel.tabTitles,
            "docActive": docPanel.active ?? "",
            "paneModes": Channel.paneModes(),
            "windowFrame": [f.origin.x, f.origin.y, f.size.width, f.size.height],
        ]
        try? FileManager.default.createDirectory(at: Channel.appSupport, withIntermediateDirectories: true)
        if let data = try? JSONSerialization.data(withJSONObject: obj, options: [.sortedKeys]) {
            try? data.write(to: Channel.sessionFile)
        }
    }

    private func restoreSavedUI() {
        var raw = Channel.store.string(forKey: Channel.frameKey)
        if raw == nil,
           let data = try? Data(contentsOf: Channel.sessionFile),
           let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any] {
            if let arr = obj["windowFrame"] as? [Any] {
                let nums = arr.compactMap { ($0 as? NSNumber)?.doubleValue }
                if nums.count == 4 { raw = "\(nums[0]),\(nums[1]),\(nums[2]),\(nums[3])" }
            }
            if let row = obj["selectedRow"] as? String, !row.isEmpty {
                Channel.store.set(row, forKey: Channel.detailRowKey)
            }
            if let modes = obj["paneModes"] as? [String: String] { Channel.setPaneModes(modes) }
        }
        if let raw {
            let p = raw.split(separator: ",").compactMap { Double($0) }
            if p.count == 4, p[2] >= 400, p[3] >= 300 {
                window.setFrame(NSRect(x: p[0], y: p[1], width: p[2], height: p[3]), display: false)
            }
        }
        if let row = Channel.store.string(forKey: Channel.detailRowKey), !row.isEmpty {
            state.detailRow = row
            detailPanel.model.open(row)
        }
    }

    private var bag = Set<AnyCancellable>()

    /// Apply the effective mode everywhere at once: window appearance (native controls),
    /// chrome fills, the terminal-side backdrop, the Ghostty color scheme, and glass.
    func applyTheme() {
        let t = theme.tokens
        window.appearance = theme.nsAppearance
        let glass = theme.glass.any
        // A blurred sidebar needs a window that lets the desktop through; every
        // other surface paints its own opaque fill.
        window.isOpaque = !glass
        window.backgroundColor = glass ? .clear : t.windowBgNS
        root.setBackground(t.windowBgNS)
        host.setBackground(t.terminalBgNS)
        sidebarContainer.apply(glass: theme.glass.sidebar, panel: NSColor(hex: t.chrome.panel))
        docPanel?.apply(panel: NSColor(hex: t.chrome.panel), ink: NSColor(hex: t.chrome.ink))
        host.tokens = t
        host.needsLayout = true
        GhosttyRuntime.shared?.setColorScheme(theme.effective, theme: theme.terminal, surfaces: Array(registry.byTerminal.values))
    }

    var sidebarGlassKind: String { sidebarContainer.effectKind }
    /// The sidebar's SwiftUI host view (the test hook clicks inside it).
    var sidebarHostView: NSView { sidebarContainer.content }
    func show() {
        restoreSavedUI()
        if agentRun {
            (window as? ShellWindow)?.staysInactive = true
            window.isMovable = false
            var f = window.frame
            f.origin.x = -20000
            window.setFrame(f, display: false)
            window.orderBack(nil)
        } else {
            window.makeKeyAndOrderFront(nil)
        }
    }

    /// `--agent-run` keeps the window fully offscreen. AppKit would otherwise clamp it onto a display.
    func pinOffscreen() {
        guard agentRun, window.frame.origin.x != -20000 else { return }
        var f = window.frame
        f.origin.x = -20000
        window.setFrame(f, display: false)
    }

    func snapshotChanged() {
        // A panel for a row that no longer exists (or is now a workflow) has nothing to show.
        if let id = detailPanel.model.rowId, detailContent == nil, model.snapshot != nil,
           !model.allRowsInOrder.contains(where: { $0.id == id && $0.kind != .workflow }) {
            closeDetail()
        }
        if let t = pendingSelectTab, model.snapshot?.tabs.contains(where: { $0.tab_id == t }) == true {
            pendingSelectTab = nil
            selectTab(t)
            return
        }
        if let t = state.selectedTab, model.snapshot?.tabs.contains(where: { $0.tab_id == t }) == true {
            refreshHost()
            if !didRestoreTabFocus {
                didRestoreTabFocus = true
                focusHerdr(t)
            }
            if !didRestorePaneFocus {
                let pane = focusedPaneByTab[t] ?? host.rects.first?.0.paneId
                if let pane, currentPanes.contains(where: { $0.paneId == pane }) {
                    focusPane(pane)
                    didRestorePaneFocus = true
                }
            }
            refreshDocs()
            return
        }
        if state.selectedTab != nil, model.snapshot != nil {
            state.selectedTab = nil
            state.saveSelected()
        }
        if state.selectedTab == nil, let first = model.lanes.first(where: { ($0.label) == "shell spike" }) ?? model.lanes.first {
            selectTab(first.id)
            return
        }
        refreshDocs()
        refreshHost()
    }

    func selectTab(_ tabId: String, revealDocs: Bool = false) {
        let stepping = state.focusCursor != nil && revealDocs
        state.selectedTab = tabId
        if !stepping { state.focusCursor = nil }
        state.saveSelected()
        if revealDocs, state.mode == .areas {
            setDocs(open: true)
            focusHerdr(tabId)
        }
        lastLayoutKey = ""
        refreshHost()
        refreshDocs()
        focusPane(focusedPaneByTab[tabId] ?? host.rects.first?.0.paneId)
    }

    private func refreshDocs() {
        docPanel.show(model: model, tabId: state.selectedTab)
    }

    /// Areas-mode row click, ⌘1..9 and Focus next/prev: select the tab and open its docs.
    func selectAreaTab(_ tabId: String) {
        selectTab(tabId, revealDocs: true)
    }

    func focusStep(_ delta: Int) {
        let ids = SidebarModel.focusTabs(snapshot: model.snapshot, orchestrators: model.orchestrators,
                                         lanes: model.lanes, workflows: model.workflows, catalog: model.catalog.snapshot)
        guard !ids.isEmpty else { return }
        let next: Int
        if let cur = state.focusCursor {
            next = cur - 1 + delta
        } else if let i = ids.firstIndex(of: state.selectedTab ?? "") {
            next = i + delta
        } else {
            next = delta > 0 ? 0 : ids.count - 1
        }
        let i = (next % ids.count + ids.count) % ids.count
        state.focusCursor = i + 1
        selectTab(ids[i], revealDocs: true)
    }

    func setDocs(open: Bool? = nil, width: CGFloat? = nil) {
        if let open { state.docOpen = open }
        if let width { state.docWidth = max(DocPanelController.minWidth, width) }
        state.saveDocs()
        applyDocs()
    }

    private func applyDocs() {
        root.docsOpen = state.docOpen
        root.docs?.isHidden = !state.docOpen
        root.docsWidth = state.docWidth
        root.needsLayout = true
        root.layoutSubtreeIfNeeded()
    }

    private func focusHerdr(_ tabId: String) {
        let cmds = commands
        DispatchQueue.global(qos: .userInitiated).async { cmds.tabFocus(tabId: tabId) }
    }

    /// Rebuild the pane host only when the tab's geometry actually changed.
    /// `forced` is a layout herdr just answered a resize with; a snapshot is skipped while a drag is live.
    func refreshHost(using forced: Snapshot.Layout? = nil) {
        if forced == nil, resizer.isBusy { return }
        guard let tab = state.selectedTab, let layout = forced ?? model.layout(forTab: tab), layout.tab_id == tab else { return }
        shownLayout = layout
        let key = tab + layout.panes.map { "\($0.pane_id)@\($0.rect.x),\($0.rect.y),\($0.rect.width),\($0.rect.height)" }.joined(separator: "|")
        guard key != lastLayoutKey else { applyCaps(); return }
        lastLayoutKey = key
        var items: [(SurfaceView, Snapshot.Rect)] = []
        for lp in layout.panes {
            guard let p = model.pane(lp.pane_id) else { continue }
            let s = registry.surface(paneId: p.pane_id, terminalId: p.terminal_id)
            wire(s, tab: tab)
            items.append((s, lp.rect))
        }
        host.show(items, area: layout.area, dividers: PaneDivider.from(layout))
        applyPendingFocus()
        applyVisibility()
        applyCaps()
    }

    private func wire(_ s: SurfaceView, tab: String) {
        s.setColorScheme(theme.effective)
        s.onFocus = { [weak self] v in
            self?.focusedPaneByTab[tab] = v.paneId
            self?.state.focusedPane = v.paneId
            self?.applyCaps()
        }
    }

    /// Hidden-tab policy: tell the registry which terminals are on screen.
    func applyVisibility() {
        registry.setVisible(Set(host.rects.map { $0.0.terminalId }))
    }

    /// The registry replaced a pane's surface (attach respawned or taken back):
    /// put the new one in the old one's slot and keep keyboard focus where it was.
    func replaceSurface(old: SurfaceView, new: SurfaceView) {
        let fr = window.firstResponder as? NSView
        let wasFocused = fr === old || (fr?.isDescendant(of: old) ?? false)
        guard let i = host.rects.firstIndex(where: { $0.0 === old }) else { return }
        if let tab = state.selectedTab { wire(new, tab: tab) }
        new.frame = old.frame
        host.rects[i].0 = new
        host.addSubview(new)
        old.removeFromSuperview()
        host.needsLayout = true
        host.layout()
        if wasFocused { window.makeFirstResponder(new) }
    }

    func setHiddenPolicy(_ value: String) {
        guard let p = SurfaceRegistry.HiddenPolicy(rawValue: value) else { return }
        registry.hiddenPolicy = p
        applyVisibility()
    }

    /// Menu and hook entry: take back every pane another client holds.
    @objc func reclaimPane(_ sender: Any?) {
        let held = registry.heldTerminals
        let target = (focusedSurface?.terminalId).flatMap { held.contains($0) ? $0 : nil }
            ?? (window.firstResponder as? NSView).flatMap { v in currentPanes.first { v.isDescendant(of: $0) }?.terminalId }
            ?? held.first
        if let target { registry.reclaim(target) }
    }

    var currentPanes: [SurfaceView] { host.rects.map { $0.0 } }

    private var factoryModel: FactoryModel?
    private var chats: [String: PaneChat] = [:]

    func toggleFactory() { setFactory(open: !state.factoryOpen) }

    func setFactory(open: Bool) {
        state.factoryOpen = open
        if open {
            if factoryModel == nil {
                let model = FactoryModel()
                factoryModel = model
                model.start()
                let page = FactoryHostingView(rootView: FactoryPage(model: model, theme: theme))
                root.factory = page
                root.addSubview(page)
                if let s = focusedSurface { window.makeFirstResponder(s) }
            }
        }
        root.factoryOpen = open
        root.layoutSubtreeIfNeeded()
    }

    func setPaneMode(_ id: String, _ mode: String) {
        if mode == "focus" || mode == "full" {
            Channel.setMode("chat", for: id)
            Channel.setDensity(mode, for: id)
            ensureChat(id)
            chats[id]?.ui.set(mode == "full" ? .full : .focus)
        } else if mode == "chat" {
            Channel.setMode("chat", for: id)
            ensureChat(id)
        } else {
            Channel.setMode("terminal", for: id)
        }
        applyCaps()
    }

    private func ensureChat(_ id: String) {
        if chats[id] != nil {
            host.placeChat(id, view: chats[id]?.view)
            return
        }
        let transcript = Transcript(pane: id, file: nil, dump: nil, state: nil, name: nil)
        let sender = ChatSender(pane: id, readOnly: flags.contains("--read-only"))
        let ui = ChatUI(pinned: Channel.density(for: id) == "full" ? .full : .focus)
        let view = NSHostingView(rootView: ChatView(transcript: transcript, theme: theme, sender: sender, ui: ui,
                                                    codeFamily: ChatFont.family(config: ghosttyConfigText)))
        chats[id] = PaneChat(transcript: transcript, sender: sender, ui: ui, view: view)
        host.placeChat(id, view: view)
    }

    func applyCaps() {
        guard let tab = state.selectedTab, let snap = model.snapshot else { return }
        var next: [String: PaneCapState] = [:]
        for p in snap.panes where p.tab_id == tab {
            let agents = snap.agents.filter { $0.pane_id == p.pane_id }
            let reported = !agents.isEmpty
            let agent = agents.first
            let named = (agent?.agent?.isEmpty == false ? agent?.agent : nil) ?? "Brief"
            let status = agent?.agent_status ?? p.agent_status ?? ""
            let chat = reported && Channel.mode(for: p.pane_id) == "chat"
            if chat { ensureChat(p.pane_id) }
            next[p.pane_id] = PaneCapState(
                paneId: p.pane_id, name: named.isEmpty ? "Brief" : named, agent: reported,
                focused: (state.focusedPane ?? snap.panes.first { $0.focused == true }?.pane_id) == p.pane_id,
                chat: chat, density: Channel.density(for: p.pane_id),
                glyph: ShellState.from(status: status, failed: false, hasAgent: reported))
        }
        host.caps = next
        host.needsLayout = true
        host.layoutSubtreeIfNeeded()
        root.titleReserve = (updates?.shown == true) ? 28 : 0
    }

    var factoryMachines: [MachineRow] { factoryModel?.snapshot.machines ?? [] }

    var focusedSurface: SurfaceView? { window.firstResponder as? SurfaceView }

    func focusPane(_ paneId: String?) {
        guard let paneId, let s = currentPanes.first(where: { $0.paneId == paneId }) else { return }
        state.focusedPane = paneId
        if let tab = state.selectedTab { focusedPaneByTab[tab] = paneId }
        window.makeFirstResponder(s)
        applyCaps()
    }

    // MARK: menu actions (app-level chords)

    @objc func nextPane(_ sender: Any?) { cyclePane(1) }
    @objc func prevPane(_ sender: Any?) { cyclePane(-1) }
    @objc func nextTab(_ sender: Any?) { cycleTab(1) }
    @objc func prevTab(_ sender: Any?) { cycleTab(-1) }

    @objc func gotoTab(_ sender: NSMenuItem) {
        let rows = model.allRowsInOrder
        if sender.tag - 1 < rows.count { selectTab(rows[sender.tag - 1].id) }
    }

    // MARK: herdr commands (new tab, split)

    private func applyPendingFocus() {
        if let p = pendingFocusPane, currentPanes.contains(where: { $0.paneId == p }) {
            pendingFocusPane = nil
            focusPane(p)
        }
    }

    @objc func newTab(_ sender: Any?) {
        guard let tab = state.selectedTab,
              let ws = model.snapshot?.tabs.first(where: { $0.tab_id == tab })?.workspace_id else { return }
        let cmds = commands
        DispatchQueue.global(qos: .userInitiated).async {
            let r = cmds.tabCreate(workspaceId: ws, cwd: nil)
            DispatchQueue.main.async { [self] in
                guard let r else { log("new tab failed"); return }
                pendingSelectTab = r.tabId
                snapshotChanged()   // the tab.created event may already have landed
            }
        }
    }

    @objc func splitRight(_ sender: Any?) { split("right") }
    @objc func splitDown(_ sender: Any?) { split("down") }

    func split(_ direction: String) {
        guard let target = focusedSurface?.paneId ?? state.focusedPane else { return }
        let cmds = commands
        DispatchQueue.global(qos: .userInitiated).async {
            let r = cmds.paneSplit(targetPaneId: target, direction: direction)
            DispatchQueue.main.async { [self] in
                guard let r else { log("split failed"); return }
                pendingFocusPane = r.paneId
                lastLayoutKey = ""
                refreshHost()   // no-op until the snapshot that contains the new pane arrives
            }
        }
    }

    @objc func copy(_ sender: Any?) { binding("copy_to_clipboard") }
    @objc func paste(_ sender: Any?) { binding("paste_from_clipboard") }

    private func binding(_ action: String) {
        guard let s = focusedSurface?.surface else { return }
        _ = ghostty_surface_binding_action(s, action, UInt(action.utf8.count))
    }

    private func cyclePane(_ d: Int) {
        let panes = currentPanes
        guard !panes.isEmpty else { return }
        let i = panes.firstIndex { $0 === focusedSurface } ?? 0
        window.makeFirstResponder(panes[(i + d + panes.count) % panes.count])
    }

    private func cycleTab(_ d: Int) {
        let rows = model.allRowsInOrder
        guard !rows.isEmpty else { return }
        let i = rows.firstIndex { $0.id == state.selectedTab } ?? 0
        selectTab(rows[(i + d + rows.count) % rows.count].id)
    }

    func windowDidBecomeKey(_ notification: Notification) {
        ghostty_app_set_focus(GhosttyRuntime.shared.app, true)
    }

    func windowDidResignKey(_ notification: Notification) {
        ghostty_app_set_focus(GhosttyRuntime.shared.app, false)
    }
}


/// `--agent-run` keeps the frame offscreen. The window is ordered back and never made key,
/// so it does not become the front app; it can still take key events sent in-process.
final class ShellWindow: NSWindow {
    var staysInactive = false
    override func constrainFrameRect(_ frameRect: NSRect, to screen: NSScreen?) -> NSRect {
        staysInactive ? frameRect : super.constrainFrameRect(frameRect, to: screen)
    }

    override func performKeyEquivalent(with event: NSEvent) -> Bool {
        if event.type == .keyDown, let entry = Keymap.shared.entry(for: event), entry.contextual {
            return Keymap.shared.fire(entry)
        }
        return super.performKeyEquivalent(with: event)
    }
}

    /// Fixed-width sidebar and a pane host filling the rest. (An NSSplitView let the
    /// SwiftUI sidebar claim half the window.) Docs sit to the right of the panes.
    final class RootView: NSView {
        let sidebar: NSView, host: NSView
        static let sidebarWidth: CGFloat = 300
        /// P11 detail panel between the sidebar and the pane host; takes no space while closed.
        var detail: NSView? {
            didSet {
                oldValue?.removeFromSuperview()
                if let detail { addSubview(detail, positioned: .above, relativeTo: sidebar) }
            }
        }
        var detailOpen = false {
            didSet { detail?.isHidden = !detailOpen; needsLayout = true }
        }
        var docs: NSView? {
            didSet {
                oldValue?.removeFromSuperview()
                if let docs { addSubview(docs) }
            }
        }
        var docsOpen = false {
            didSet { docs?.isHidden = !docsOpen; needsLayout = true }
        }
        var docsWidth: CGFloat = DocPanelController.defaultWidth
        var factory: NSView?
        var factoryOpen = false {
            didSet { factory?.isHidden = !factoryOpen; host.isHidden = factoryOpen; needsLayout = true }
        }
        /// Extra top inset while the update pill is showing, so it never covers a pane cap.
        var titleReserve: CGFloat = 0 { didSet { needsLayout = true } }

    func setBackground(_ c: NSColor) { layer?.backgroundColor = c.cgColor }

    init(sidebar: NSView, host: NSView) {
        self.sidebar = sidebar
        self.host = host
        super.init(frame: .zero)
        wantsLayer = true
        addSubview(sidebar)
        addSubview(host)
    }

    required init?(coder: NSCoder) { fatalError() }

    override func layout() {
        super.layout()
        let w = Self.sidebarWidth
        sidebar.frame = NSRect(x: 0, y: 0, width: w, height: bounds.height)
        var x = w + 1
        if let detail, detailOpen {
            detail.frame = NSRect(x: x, y: 0, width: DetailPanelController.width, height: bounds.height)
            x += DetailPanelController.width + 1
        }
        var docW: CGFloat = 0
        if let docs, docsOpen {
            docW = max(DocPanelController.minWidth, docsWidth)
            docs.frame = NSRect(x: bounds.width - docW, y: 0, width: docW, height: bounds.height)
            docW += 1
        }
        let top = titleReserve
        host.frame = NSRect(x: x, y: 0, width: max(0, bounds.width - x - docW), height: max(0, bounds.height - top))
        factory?.frame = host.frame
    }
}
