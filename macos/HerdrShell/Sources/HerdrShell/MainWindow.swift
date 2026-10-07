import AppKit
import Combine
import GhosttyKit
import SwiftUI

// SurfaceRegistry (retained surfaces and their attach lifecycle) lives in SurfaceRegistry.swift.

/// Lays out one tab's panes using herdr's own split geometry (layouts[].panes[].rect).
/// Sits in the titlebar band like the docs header (DocPanelView): never a window drag region.
final class CapHostingView: NSHostingView<PaneCapBar> {
    override var acceptsFirstResponder: Bool { false }
    override var mouseDownCanMoveWindow: Bool { false }
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }
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
                                        onFull: { [weak self] in self?.capAction?(paneId, "full") },
                                        onPin: { [weak self] in self?.capAction?(paneId, "pin") },
                                        onRestart: { [weak self] in self?.capAction?(paneId, "restart") })
            }
            return v
        }
        let state = caps[paneId] ?? PaneCapState(paneId: paneId, name: "Brief", agent: false, focused: false, chat: false, density: "focus", glyph: .asleep)
        let v = CapHostingView(rootView: PaneCapBar(state: state, tokens: tokens,
                                                   onTerminal: { [weak self] in self?.capAction?(paneId, "terminal") },
                                                   onChat: { [weak self] in self?.capAction?(paneId, "chat") },
                                                   onFocus: { [weak self] in self?.capAction?(paneId, "focus") },
                                                   onFull: { [weak self] in self?.capAction?(paneId, "full") },
                                                   onPin: { [weak self] in self?.capAction?(paneId, "pin") },
                                                   onRestart: { [weak self] in self?.capAction?(paneId, "restart") }))
        // The cap sits under the transparent titlebar; with the titlebar's safe area its content
        // slid down into the body, where a chat view covered its lower half.
        v.safeAreaRegions = []
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
    private var pendingClose: (pane: String, tab: String?, order: [String], pins: [String])?
    /// Finished tabs the user has selected. They drop out of Next Needing You until they finish again.
    private var lookedAtFinished: Set<String> = []
    private var didRestoreTabFocus = false
    private var didRestorePaneFocus = false
    private(set) var quickSwitch: QuickSwitchController!
    /// Last snapshot the notifier has applied. The next one is compared against it.
    private var attentionOld = Notifier.Facts.empty
    /// `--agent-run` never activates, so the window stays non-key. A click through the
    /// test hook still means Alex is looking at the selected tab.
    var inProcessKey = false

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
        quickSwitch = QuickSwitchController(owner: self)
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
                                                          selectNew: { [weak self] in self?.selectWhenListed($0) },
                                                          onFactory: { [weak self] in self?.toggleFactory() },
                                                          onRename: { [weak self] tab in self?.promptRenameTab(tab) }))
        sidebarContainer = SidebarContainer(content: sidebar)
        root = RootView(sidebar: sidebarContainer, host: host)
        detailPanel = DetailPanelController(herdr: model, theme: theme) { [weak self] id in self?.openFullTab(id) }
        root.detail = detailPanel.view
        docPanel = DocPanelController()
        docPanel.attach(self)
        docPanel.onClose = { [weak self] in self?.setDocs(open: false) }
        docPanel.onWidth = { [weak self] w in self?.setDocs(width: w) }
        root.docs = docPanel.view
        syncDocsShown()
        applyDocs()
        root.sidebarVisible = state.sidebarVisible
        Keymap.shared.addContextual(chord: "escape", action: "close_detail") { [weak self] in
            guard let self, !self.quickSwitch.isOpen else { return false }
            return self.docPanel.hasFocus || self.detailClaimsEscape
        }
        Keymap.shared.addContextual(chord: "escape", action: "close_switcher") { [weak self] in
            self?.quickSwitch.isOpen == true
        }
        Keymap.shared.addContextual(chord: "cmd+l", action: "focus_doc_address") { [weak self] in
            self?.docPanel.hasFocus == true && self?.docPanel.showingWeb == true
        }
        window.contentView = root
        quickSwitch.install(in: root, theme: theme)
        applyTheme()
        registry.onReplace = { [weak self] old, new in self?.replaceSurface(old: old, new: new) }

        theme.objectWillChange.receive(on: RunLoop.main).sink { [weak self] _ in
            // objectWillChange fires before the value lands; read it on the next turn.
            DispatchQueue.main.async { self?.applyTheme() }
        }.store(in: &bag)
        model.$snapshot.receive(on: RunLoop.main).sink { [weak self] _ in self?.snapshotChanged() }.store(in: &bag)
        // $machines fires before the value lands; read it on the next turn.
        model.$machines.receive(on: RunLoop.main).sink { [weak self] _ in
            DispatchQueue.main.async { self?.machinesChanged() }
        }.store(in: &bag)
        model.catalog.objectWillChange.receive(on: RunLoop.main).sink { [weak self] _ in
            DispatchQueue.main.async {
                self?.refreshDocs()
                self?.noteAttention()
            }
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
        quickSwitch.reload()
        noteAttention()
        if reconcileClose() { return }
        // A panel for a row that no longer exists (or is now a workflow) has nothing to show.
        if let id = detailPanel.model.rowId, detailContent == nil, model.snapshot != nil,
           !model.allRowsInOrder.contains(where: { $0.id == id && $0.kind != .workflow }) {
            closeDetail()
        }
        if let t = pendingSelectTab, model.hasTab(t) {
            pendingSelectTab = nil
            selectTab(t)
            return
        }
        // Another machine's tab waits for that machine's first answer instead of being dropped.
        if let t = state.selectedTab, !model.machineLoaded(for: t) { refreshDocs(); return }
        if let t = state.selectedTab, model.hasTab(t) {
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

    /// A machine answered. Only a selected tab on that machine has panes to redraw; a
    /// selection that was waiting for it, or a new tab there waiting to be listed, goes through
    /// the normal snapshot path.
    func machinesChanged() {
        let remote = [state.selectedTab, pendingSelectTab].contains { $0.map(Machines.isRemote) == true }
        guard remote else { refreshDocs(); return }
        snapshotChanged()
    }

    /// Blocked, or done after working, on a tab Alex is not looking at.
    private func noteAttention() {
        let new = Notifier.capture(snapshot: model.snapshot, parked: Set(model.catalog.snapshot.parked.keys))
        Notifier.shared.observe(old: attentionOld, new: new, selected: state.selectedTab,
                                windowKey: window.isKeyWindow || inProcessKey)
        attentionOld = new
    }

    var forcedEmptyDocs = false
    /// The last click or scroll landed in the docs column. ⌘W then closes the column even
    /// after a doc tab click handed keyboard focus back to the pane.
    var docsLastClicked = false

    func selectTab(_ tabId: String, revealDocs: Bool = false) {
        if state.selectedTab != tabId { forcedEmptyDocs = false; docsLastClicked = false }
        quickSwitch.noteSelected(tabId)
        noteLookedAt(tabId)
        let stepping = state.focusCursor != nil && revealDocs
        revealSpace(of: tabId)
        state.selectedTab = tabId
        if !stepping { state.focusCursor = nil }
        state.saveSelected()
        if revealDocs, state.mode == .areas {
            focusHerdr(tabId)
        }
        lastLayoutKey = ""
        refreshHost()
        refreshDocs()
        focusPane(focusedPaneByTab[tabId] ?? host.rects.first?.0.paneId)
    }

    /// Quick switch, attention jumps and header clicks land on tabs the tree may hide.
    private func revealSpace(of tabId: String) {
        guard let space = model.displayedSpace(of: tabId),
              state.spacesChrome.collapsedSpaces.contains(space.id) || (space.parked && !state.spacesChrome.expandedParkedSpaces.contains(space.id)) else { return }
        if state.spacesChrome.reveal(space: space.id, parked: space.parked, selected: tabId, rows: model.spacesRows(state: state)) { state.saveSpacesChrome() }
    }

    private var seenDeskIds: [String: Set<String>]?

    func openOnDesk(_ url: URL, paneId: String?) {
        let tabId = paneId.flatMap { id in model.pane(id)?.tab_id } ?? state.selectedTab
        guard let tabId else { return }
        var params: [String: Any] = ["ref": url.isFileURL ? url.path : url.absoluteString, "opened_by": "user"]
        if let paneId { params["pane_id"] = paneId } else { params["tab_id"] = tabId }
        let cmds = commands
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            let result = cmds.deskReply("desk.open", params: params)
            DispatchQueue.main.async {
                guard let self else { return }
                switch result {
                case .unsupported: self.docPanel.addTransient(url, tabId: tabId)
                case .failed:
                    // A refused open (say a file the pane's server cannot see) still opens, in the browser.
                    Notifier.shared.recordOpened(url.absoluteString)
                    shellOpen(url)
                    return
                case .result: break
                }
                SidebarState.store.set(true, forKey: Self.docsShownKey(tabId))
                if self.state.selectedTab == tabId { self.setDocs(open: true) }
            }
        }
    }

    private func refreshDocs() {
        let snapshots = [model.snapshot].compactMap { $0 } + model.machines.compactMap(\.snapshot)
        if !snapshots.isEmpty {
            let desks = Dictionary(snapshots.flatMap(\.tabs).map { ($0.tab_id, $0.desk ?? .empty) },
                                   uniquingKeysWith: { _, latest in latest })
            let newTabs = landed(previous: seenDeskIds, current: desks,
                                 machineForTab: { Machines.split($0)?.machine ?? "" })
            seenDeskIds = desks.mapValues { Set($0.items.map(\.id)) }
            for tab in newTabs { SidebarState.store.set(true, forKey: Self.docsShownKey(tab)) }
            docPanel.show(model: model, tabId: state.selectedTab)
            if let selected = state.selectedTab, newTabs.contains(selected) {
                if let front = desks[selected]?.front { docPanel.activateDesk(front) }
                setDocs(open: true)
            }
        } else {
            docPanel.show(model: model, tabId: state.selectedTab)
        }
        syncDocsShown()
        applyDocs()
    }

    /// Docs are shown per tab and only once asked for (⌘\ or Toggle Docs). The key is new:
    /// the old window-wide `docOpen` kept RESUME and BRIEF open on every lane.
    private static func docsShownKey(_ tab: String) -> String { "herdr.shell.docsShown.\(tab)" }

    private func syncDocsShown() {
        state.docOpen = state.selectedTab.map { SidebarState.store.bool(forKey: Self.docsShownKey($0)) } ?? false
    }

    /// ⌘W follows focus: with the docs column focused or last clicked it closes what ✕
    /// closes, never the pane.
    var docsOwnClose: Bool { root.docsOpen && (docPanel.hasFocus || docsLastClicked) }

    func closeDocs() {
        docPanel.closeActive()
        docsLastClicked = root.docsOpen
    }

    /// Areas-mode row click, ⌘1..9 and Focus next/prev: select the tab. Docs stay as that tab left them.
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
        if let open {
            state.docOpen = open
            if let tab = state.selectedTab { SidebarState.store.set(open, forKey: Self.docsShownKey(tab)) }
            if !open { docsLastClicked = false }
        }
        if let width { state.docWidth = max(DocPanelController.minWidth, width) }
        state.saveDocs()
        applyDocs()
    }

    private func applyDocs() {
        let visible = state.docOpen && (docPanel.hasDocs || forcedEmptyDocs)
        root.docsOpen = visible
        root.docs?.isHidden = !visible
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
        // herdr reports the split rects even while a tab is zoomed; the shell draws the zoom itself.
        let zoomedPane = layout.zoomed == true ? layout.focused_pane_id : nil
        let shown: [Snapshot.LayoutPane] = zoomedPane.map { id in
            layout.panes.filter { $0.pane_id == id }.map { Snapshot.LayoutPane(pane_id: $0.pane_id, rect: layout.area) }
        } ?? layout.panes
        let key = tab + (zoomedPane.map { "zoom:\($0)|" } ?? "")
            + shown.map { "\($0.pane_id)@\($0.rect.x),\($0.rect.y),\($0.rect.width),\($0.rect.height)" }.joined(separator: "|")
            // A restarted remote server or a local live handoff keeps pane ids but hands out new terminals.
            + "|" + shown.compactMap { model.pane($0.pane_id)?.terminal_id }.joined(separator: ",")
        guard key != lastLayoutKey else { applyCaps(); return }
        lastLayoutKey = key
        // Capture actual keyboard ownership before terminal replacement removes the old view.
        let responder = window.firstResponder as? NSView
        let focusedPane = host.rects.first { surface, _ in
            responder === surface || (responder?.isDescendant(of: surface) ?? false)
        }?.0.paneId
        var items: [(SurfaceView, Snapshot.Rect)] = []
        for lp in shown {
            guard let p = model.pane(lp.pane_id) else { continue }
            let s = registry.surface(paneId: p.pane_id, terminalId: p.terminal_id)
            wire(s, tab: tab)
            items.append((s, lp.rect))
        }
        // Splits on another machine are not dragged from here: a resize in flight could outlive the selection.
        host.show(items, area: layout.area, dividers: zoomedPane == nil && !Machines.isRemote(tab) ? PaneDivider.from(layout) : [])
        applyPendingFocus()
        if let focusedPane, let replacement = items.first(where: { $0.0.paneId == focusedPane })?.0 {
            window.makeFirstResponder(replacement)
        }
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
    /// True while any pane's chat composer, on screen or in a hidden tab, holds unsent text.
    var hasUnsentDraft: Bool { chats.values.contains { !$0.ui.draft.isEmpty } }
    /// A sheet, an app-modal alert or the switcher is up; a dev reload waits for it to close.
    var busyWithModal: Bool { NSApp.modalWindow != nil || window.attachedSheet != nil || quickSwitch.isOpen }
    /// The pane a dev reload had focused comes back with its tab (DevReload.swift).
    func seedFocus(tab: String, pane: String) { focusedPaneByTab[tab] = pane }
    /// Each open chat as the view sees it: the agent state from herdr and the items read.
    var chatDump: [[String: Any]] {
        chats.map { id, c in ["id": id, "agent_state": c.transcript.state, "agent_name": c.transcript.name,
                              "waiting": c.transcript.waiting, "items": c.transcript.items.map(\.id)] }
    }

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
        if mode == "restart" {
            restartAgent(id)
            return
        }
        if mode == "pin" {
            // The pin belongs to the tab the cap's pane is in; the icon follows the next snapshot.
            guard let tab = state.selectedTab else { return }
            model.setPinnedAtEnd(tab, !model.isPinned(tab))
            return
        }
        if Machines.isRemote(id), mode != "terminal" { log("pane mode \(mode): local panes only"); return }
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

    private func restartAgent(_ id: String, force: Bool = false) {
        let commands = self.commands
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            let reply = commands.restartAgent(paneId: id, force: force)
            DispatchQueue.main.async {
                guard let self else { return }
                switch reply {
                case .success: break
                case .failure(let code, let message):
                    if PaneRestart.next(code: code, message: message, forced: force) == .confirm {
                        let alert = NSAlert()
                        alert.messageText = "Agent is working. Restart anyway?"
                        alert.informativeText = "It will resume the same chat."
                        alert.addButton(withTitle: "Restart")
                        alert.addButton(withTitle: "Cancel")
                        alert.beginSheetModal(for: self.window) { [weak self] response in
                            if response == .alertFirstButtonReturn { self?.restartAgent(id, force: true) }
                        }
                    } else {
                        let alert = NSAlert()
                        alert.messageText = "Could not restart agent"
                        alert.informativeText = PaneRestart.message(code: code, fallback: message)
                        alert.addButton(withTitle: "OK")
                        alert.beginSheetModal(for: self.window)
                    }
                }
            }
        }
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
        guard let tab = state.selectedTab, let snap = model.source(for: tab) else { return }
        var next: [String: PaneCapState] = [:]
        for p in snap.panes where p.tab_id == tab {
            let agents = snap.agents.filter { $0.pane_id == p.pane_id }
            let reported = !agents.isEmpty
            let agent = agents.first
            let named = (agent?.agent?.isEmpty == false ? agent?.agent : nil) ?? "Brief"
            let status = agent?.work_status ?? agent?.agent_status ?? p.agent_status ?? ""
            // Chat reads the local transcript and sends through the local server: local panes only.
            let chat = reported && !Machines.isRemote(p.pane_id) && Channel.mode(for: p.pane_id) == "chat"
            if chat { ensureChat(p.pane_id) }
            next[p.pane_id] = PaneCapState(
                paneId: p.pane_id, name: named.isEmpty ? "Brief" : named, agent: reported,
                focused: (state.focusedPane ?? snap.panes.first { $0.focused == true }?.pane_id) == p.pane_id,
                chat: chat, density: Channel.density(for: p.pane_id),
                glyph: ShellState.from(status: status, failed: false, hasAgent: reported))
        }
        // The tab's pin sits top right: on the cap of the top pane in the rightmost column.
        if let corner = host.rects.filter({ $0.1.y <= host.area.y })
            .max(by: { $0.1.x + $0.1.width < $1.1.x + $1.1.width })?.0.paneId, next[corner] != nil {
            next[corner]?.pinned = model.isPinned(tab)
        }
        host.caps = next
        host.needsLayout = true
        host.layoutSubtreeIfNeeded()
    }

    var factoryMachines: [MachineRow] { factoryModel?.snapshot.machines ?? [] }

    var focusedSurface: SurfaceView? { window.firstResponder as? SurfaceView }

    func focusPane(_ paneId: String?) {
        guard let paneId, let s = currentPanes.first(where: { $0.paneId == paneId }) else { return }
        state.focusedPane = paneId
        if let tab = state.selectedTab { focusedPaneByTab[tab] = paneId }
        // While the switcher is up the pane must not take keys.
        if !quickSwitch.isOpen { window.makeFirstResponder(s) }
        applyCaps()
    }

    // MARK: menu actions (app-level chords)

    @objc func nextPane(_ sender: Any?) { cyclePane(1) }
    @objc func prevPane(_ sender: Any?) { cyclePane(-1) }
    @objc func nextTab(_ sender: Any?) { cycleTab(1) }
    @objc func prevTab(_ sender: Any?) { cycleTab(-1) }

    @objc func gotoTab(_ sender: NSMenuItem) {
        let rows = model.numberedTabIds(state: state)
        if sender.tag >= 1, sender.tag <= rows.count { selectTab(rows[sender.tag - 1]) }
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
                selectWhenListed(r.tabId)
            }
        }
    }

    /// Selects a tab just created once a snapshot lists it.
    func selectWhenListed(_ tabId: String) {
        pendingSelectTab = tabId
        snapshotChanged()   // the tab.created event may already have landed
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

    /// Visible agent rows, in the order the sidebar draws them. Focus duplicates are the same tab.
    func visibleAgentTabs() -> [String] {
        var ids: [String] = []
        var seen = Set<String>()
        for line in sidebarLines {
            guard let tab = line.tab, !line.id.hasPrefix("focus:") else { continue }
            if seen.insert(tab).inserted { ids.append(tab) }
        }
        return ids
    }

    func attentionOrderIds() -> [String] {
        if let s = model.snapshot {
            let facts = SidebarModel.facts(s)
            lookedAtFinished = lookedAtFinished.filter { facts[$0]?.finished == true }
        }
        return SidebarModel.attentionOrder(snapshot: model.snapshot, orchestrators: model.orchestrators,
                                           lanes: model.lanes, workflows: model.workflows,
                                           catalog: model.catalog.snapshot, areas: state.mode == .areas,
                                           lookedAt: lookedAtFinished)
    }

    private func noteLookedAt(_ tabId: String) {
        guard let s = model.snapshot else { return }
        let facts = SidebarModel.facts(s)
        lookedAtFinished = lookedAtFinished.filter { facts[$0]?.finished == true }
        if facts[tabId]?.finished == true { lookedAtFinished.insert(tabId) }
    }

    /// After pane.close, the next snapshot says where focus goes: herdr's focused pane,
    /// else the tab's first pane, else the next sidebar row when the tab itself closed.
    private func reconcileClose() -> Bool {
        // A remote pane's close is settled by its machine's snapshot, not the local one.
        guard let pending = pendingClose, let snap = model.source(for: pending.pane) else { return false }
        guard !snap.panes.contains(where: { $0.pane_id == pending.pane }) else { return false }
        pendingClose = nil
        // Selection moved on while a remote close was in flight: leave it where the user put it.
        if Machines.isRemote(pending.pane), state.selectedTab != pending.tab { return false }
        if let tab = pending.tab, focusedPaneByTab[tab] == pending.pane { focusedPaneByTab[tab] = nil }
        if let tab = pending.tab, snap.tabs.contains(where: { $0.tab_id == tab }) {
            let layout = snap.layouts.first { $0.tab_id == tab }
            var focused = layout?.focused_pane_id
            if focused == pending.pane { focused = nil }
            if focused == nil || !snap.panes.contains(where: { $0.pane_id == focused }) {
                focused = snap.panes.first { $0.tab_id == tab && $0.focused == true && $0.pane_id != pending.pane }?.pane_id
                    ?? layout?.panes.first { $0.pane_id != pending.pane }?.pane_id
                    ?? snap.panes.first { $0.tab_id == tab && $0.pane_id != pending.pane }?.pane_id
            }
            if state.selectedTab != tab { selectTab(tab) }
            lastLayoutKey = ""
            refreshHost()
            if let focused { focusPane(focused) }
            refreshDocs()
            return true
        }
        if let next = pending.pins.first(where: { model.hasTab($0) }) {
            selectTab(next)
            return true
        }
        let ids = pending.order
        let i = pending.tab.flatMap { ids.firstIndex(of: $0) } ?? -1
        let after = i >= 0 ? Array(ids.dropFirst(i + 1)) : ids
        let before = i > 0 ? Array(ids.prefix(i)) : []
        if let next = (after + before).first(where: { id in model.hasTab(id) }) {
            selectTab(next)
        }
        return true
    }

    func closePane() {
        guard let pane = focusedSurface?.paneId ?? state.focusedPane else { log("close pane: no focused pane"); return }
        let tab = state.selectedTab
        let order = visibleAgentTabs()
        let pins = SpacesTree.closePinOrder(model.spacesRows(state: state), selected: tab)
        let cmds = commands
        DispatchQueue.global(qos: .userInitiated).async {
            guard cmds.paneClose(paneId: pane) else { return }
            DispatchQueue.main.async { [self] in
                pendingClose = (pane, tab, order, pins)
                lastLayoutKey = ""
                snapshotChanged()
            }
        }
    }

    func zoomPane() {
        guard let pane = focusedSurface?.paneId ?? state.focusedPane else { log("zoom: no focused pane"); return }
        let cmds = commands
        DispatchQueue.global(qos: .userInitiated).async {
            if cmds.paneZoom(paneId: pane) {
                DispatchQueue.main.async { [self] in
                    lastLayoutKey = ""
                    refreshHost()
                }
            }
        }
    }

    func nextAttention() {
        let order = attentionOrderIds()
        guard !order.isEmpty else { log("next_attention: none"); return }
        let next: String
        if let cur = state.selectedTab, let i = order.firstIndex(of: cur) {
            next = order[(i + 1) % order.count]
        } else {
            next = order[0]
        }
        log("next_attention: \(next)")
        selectTab(next)
    }

    func attentionJump() {
        if let tab = model.latestAttentionTab {
            log("attention_jump: \(tab)")
            selectTab(tab)
        } else {
            log("attention_jump: none, next_attention")
            nextAttention()
        }
    }

    func toggleSidebar() {
        state.sidebarVisible.toggle()
        state.saveSidebar()
        root.sidebarVisible = state.sidebarVisible
        root.layoutSubtreeIfNeeded()
    }

    func stepAgentList(_ d: Int) {
        let ids = visibleAgentTabs()
        guard !ids.isEmpty else { return }
        let i = ids.firstIndex(of: state.selectedTab ?? "") ?? (d > 0 ? -1 : 0)
        selectTab(ids[(i + d + ids.count) % ids.count])
    }

    func promptRenameTab(_ tabId: String?) {
        let tab = tabId ?? state.selectedTab
        guard let tab else { log("rename: no tab"); return }
        let label = model.snapshot?.tabs.first { $0.tab_id == tab }?.label
            ?? model.allRowsInOrder.first { $0.id == tab }?.label
            ?? ""
        let alert = NSAlert()
        alert.messageText = "Rename Tab"
        alert.addButton(withTitle: "OK")
        alert.addButton(withTitle: "Cancel")
        let field = NSTextField(frame: NSRect(x: 0, y: 0, width: 280, height: 24))
        field.stringValue = label
        alert.accessoryView = field
        alert.window.initialFirstResponder = field
        guard alert.runModal() == .alertFirstButtonReturn else { return }
        let text = field.stringValue.trimmingCharacters(in: .whitespacesAndNewlines)
        if text.isEmpty { return }
        renameTab(tab, text)
    }

    func renameTab(_ tab: String, _ label: String) {
        let text = label.trimmingCharacters(in: .whitespacesAndNewlines)
        if text.isEmpty { return }
        let cmds = commands
        DispatchQueue.global(qos: .userInitiated).async {
            _ = cmds.tabRename(tabId: tab, label: text)
        }
    }

    @objc func copy(_ sender: Any?) {
        guard let s = focusedSurface else { return forwardEdit(#selector(NSText.copy(_:)), sender) }
        if !s.copySelection() { NSSound.beep() }
    }
    @objc func paste(_ sender: Any?) {
        // With the switcher up, a paste is a query, never terminal input.
        if quickSwitch.isOpen {
            quickSwitch.setQuery(quickSwitch.query + (NSPasteboard.general.string(forType: .string) ?? ""))
            return
        }
        guard focusedSurface != nil else { return forwardEdit(#selector(NSText.paste(_:)), sender) }
        ClipboardImagePaste.userPaste = true
        defer { ClipboardImagePaste.userPaste = false }
        binding("paste_from_clipboard")
    }

    /// The Edit menu targets this controller, so without this a chat transcript, the chat
    /// composer or a text field never saw ⌘C or ⌘V. Hand the action to the first responder
    /// in this window that takes it (a selected transcript is an NSTextView under SwiftUI).
    private func forwardEdit(_ action: Selector, _ sender: Any?) {
        var responder = window.firstResponder
        while let r = responder, r !== self {
            if r.responds(to: action) { NSApp.sendAction(action, to: r, from: sender); return }
            responder = r.nextResponder
        }
    }

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

    override func sendEvent(_ event: NSEvent) {
        // The hook delivers keys with sendEvent only while the window is key.
        // Swallow them here so a focused pane never sees switcher typing.
        if event.type == .keyDown, let c = delegate as? MainWindowController, c.quickSwitch.sink(event) {
            return
        }
        if [.leftMouseDown, .rightMouseDown, .scrollWheel].contains(event.type),
           let c = delegate as? MainWindowController, let docs = c.root.docs {
            c.docsLastClicked = c.root.docsOpen && docs.bounds.contains(docs.convert(event.locationInWindow, from: nil))
        }
        super.sendEvent(event)
    }

    override func performKeyEquivalent(with event: NSEvent) -> Bool {
        // Not-key path (the lab): the hook calls this instead of sendEvent.
        if event.type == .keyDown, let c = delegate as? MainWindowController, c.quickSwitch.sink(event) {
            return true
        }
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
        static let sidebarWidth: CGFloat = ShellSpace.sidebarWidth
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
        private var switcher: NSView?
        var factoryOpen = false {
            didSet { factory?.isHidden = !factoryOpen; host.isHidden = factoryOpen; needsLayout = true }
        }
        var sidebarVisible = true {
            didSet { sidebar.isHidden = !sidebarVisible; needsLayout = true }
        }

    func attachSwitcher(_ view: NSView) {
        switcher?.removeFromSuperview()
        switcher = view
        addSubview(view, positioned: .above, relativeTo: nil)
        needsLayout = true
    }

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
        let w: CGFloat = sidebarVisible ? Self.sidebarWidth : 0
        sidebar.frame = NSRect(x: 0, y: 0, width: w, height: bounds.height)
        var x = sidebarVisible ? w + 1 : 0
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
        host.frame = NSRect(x: x, y: 0, width: max(0, bounds.width - x - docW), height: bounds.height)
        factory?.frame = host.frame
        if let switcher {
            switcher.frame = bounds
            addSubview(switcher, positioned: .above, relativeTo: nil)
        }
    }
}
