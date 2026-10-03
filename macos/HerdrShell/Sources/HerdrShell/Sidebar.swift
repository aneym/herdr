import SwiftUI

enum SidebarMode: String { case areas, spaces }

final class SidebarState: ObservableObject {
    @Published var spacesChrome: SpacesChrome = {
        guard let data = SidebarState.store.data(forKey: "herdr.shell.spacesChrome") else { return SpacesChrome() }
        return (try? JSONDecoder().decode(SpacesChrome.self, from: data)) ?? SpacesChrome()
    }()
    func saveSpacesChrome() {
        if let data = try? JSONEncoder().encode(spacesChrome) { Self.store.set(data, forKey: "herdr.shell.spacesChrome"); sync() }
    }
    @Published var selectedTab: String?
    /// Fold state the user set by hand, by fold id (`tab:<id>`, `hidden`, `background`).
    /// A fold with no entry follows SidebarModel: closed, unless a row inside asks or failed.
    @Published var manualOpen: [String: Bool] = [:]
    @Published var focusedPane: String?
    /// Row whose details are open in the detail panel (P11).
    @Published var detailRow: String?
    /// Draw the rows without a ScrollView: only the in-process screenshot fallback sets it,
    /// because AppKit view caching does not draw a ScrollView's content.
    @Published var flat = false
    /// Where each row was last drawn (view-local, top-left origin), by line id. Read only by the
    /// test hook, which turns it into a real mouse click; not observed, so it never redraws.
    var rowFrames: [String: CGRect] = [:]

    /// Areas is the default once a lanes or areas file is present. Without one, the sidebar
    /// stays the spaces list (P10). A saved choice wins over that default.
    @Published var mode: SidebarMode
    @Published var chip: AreaChip
    @Published var areaOnly: String?
    @Published var foldedAreas: Set<String>
    @Published var focusExpanded: Bool
    /// 1-based index into the Focus ranking while ⌘] / ⌘[ is stepping. Nil otherwise.
    @Published var focusCursor: Int?
    @Published var docOpen: Bool
    @Published var docWidth: CGFloat
    /// The sidebar column. Hidden, the pane host takes that width. Persists like docs.
    @Published var sidebarVisible: Bool
    /// Factory view fills the pane host while this is true.
    @Published var factoryOpen = false
    /// Set by the test hook for the duration of a synthetic option-click. A real option
    /// click is read from the current event.
    var clickOption = false

    /// Dev keeps a suite per lab under the dev prefix. Prod uses the bundle domain.
    /// `UserDefaults.standard` in the real home would let two labs reopen each other.
    static let store: UserDefaults = Channel.store
    private static let prefix = "herdr.shell."

    init() {
        let d = Self.store
        if let raw = d.string(forKey: Self.prefix + "mode"), let m = SidebarMode(rawValue: raw) {
            mode = m
        } else {
            mode = .spaces
        }
        chip = AreaChip(rawValue: d.string(forKey: Self.prefix + "chip") ?? "") ?? .all
        areaOnly = d.string(forKey: Self.prefix + "areaOnly")
        foldedAreas = Set(d.stringArray(forKey: Self.prefix + "folded") ?? [])
        focusExpanded = (d.object(forKey: Self.prefix + "focusExpanded") as? Bool) ?? false
        selectedTab = d.string(forKey: Self.prefix + "selectedTab")
        docOpen = (d.object(forKey: Self.prefix + "docOpen") as? Bool) ?? false
        docWidth = CGFloat((d.object(forKey: Self.prefix + "docWidth") as? Double) ?? 420)
        sidebarVisible = (d.object(forKey: Self.prefix + "sidebarVisible") as? Bool) ?? true
        if docWidth < 320 { docWidth = 320 }
    }

    func setMode(_ m: SidebarMode) {
        mode = m
        focusCursor = nil
        write("mode", m.rawValue)
    }

    func setChip(_ c: AreaChip) {
        chip = c
        focusCursor = nil
        write("chip", c.rawValue)
    }

    func setAreaOnly(_ id: String?) {
        areaOnly = id
        if let id { write("areaOnly", id) } else { Self.store.removeObject(forKey: Self.prefix + "areaOnly"); sync() }
    }

    func setFolded(_ area: String, open: Bool) {
        if open { foldedAreas.remove(area) } else { foldedAreas.insert(area) }
        Self.store.set(foldedAreas.sorted(), forKey: Self.prefix + "folded")
        sync()
    }

    func setFocusExpanded(_ open: Bool) {
        focusExpanded = open
        Self.store.set(open, forKey: Self.prefix + "focusExpanded")
        sync()
    }

    func saveSelected() {
        if let selectedTab { write("selectedTab", selectedTab) }
        else { Self.store.removeObject(forKey: Self.prefix + "selectedTab"); sync() }
    }

    func saveDocs() {
        Self.store.set(docOpen, forKey: Self.prefix + "docOpen")
        Self.store.set(Double(docWidth), forKey: Self.prefix + "docWidth")
        sync()
    }

    func saveSidebar() {
        Self.store.set(sidebarVisible, forKey: Self.prefix + "sidebarVisible")
        sync()
    }

    private func write(_ key: String, _ value: String) {
        Self.store.set(value, forKey: Self.prefix + key)
        sync()
    }

    private func sync() { Self.store.synchronize() }

    var optionHeld: Bool {
        clickOption || NSApp.currentEvent?.modifierFlags.contains(.option) == true
    }
}

/// Frames of clickable things, keyed by name, reported up from the SwiftUI views (P11 check clicks).
struct ClickTargetKey: PreferenceKey {
    static var defaultValue: [String: CGRect] = [:]
    static func reduce(value: inout [String: CGRect], nextValue: () -> [String: CGRect]) {
        value.merge(nextValue(), uniquingKeysWith: { $1 })
    }
}

/// The button action a check invokes by target and id. SwiftUI only takes real mouse
/// clicks in the active app, so `--agent-run` calls these instead of posting a click.
final class ClickRegistry {
    static let shared = ClickRegistry()
    private var actions: [String: () -> Void] = [:]
    func set(_ key: String, _ action: @escaping () -> Void) { actions[key] = action }
    @discardableResult func call(_ key: String) -> Bool {
        guard let action = actions[key] else { return false }
        action()
        return true
    }
}

extension View {
    /// Reports this view's frame in the named "click" space under `id`.
    func clickTarget(_ id: String) -> some View {
        background(GeometryReader { g in
            Color.clear.preference(key: ClickTargetKey.self, value: [id: g.frame(in: .named("click"))])
        })
    }

    /// Registers `action` under `key` on every refresh, the same closure the control runs.
    func hookAction(_ key: String, _ action: @escaping () -> Void) -> some View {
        ClickRegistry.shared.set(key, action)
        return self
    }
}

struct SidebarView: View {
    @ObservedObject var model: HerdrModel
    @ObservedObject var state: SidebarState
    @ObservedObject var theme: ThemeStore
    /// Click on an orchestrator or lane row (P11): opens its details and leaves the current tab alone.
    /// nil means every row selects its tab.
    var openDetail: ((TabRow) -> Void)? = nil
    var select: (String) -> Void
    var onFactory: () -> Void = {}
    var onRename: (String) -> Void = { _ in }

    /// Tabs whose Resume is running, so a second click does nothing and the button says so.
    @State private var resuming: Set<String> = []
    @State private var approved: Set<String> = []
    @State private var hoveredSpaceRow: String?

    /// Tokens for the effective mode; the view re-renders when the store changes.
    private var t: Tokens { theme.sidebarTokens }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            chrome
            if state.flat {
                rows
                Spacer(minLength: 0)
            } else {
                ScrollView { rows }
            }
            if state.mode == .spaces { spacesFooter } else { factoryFooter }
        }
        .font(.system(size: 12.5))
        .foregroundStyle(t.ink)
        // Glass on the sidebar puts a panel-colored scrim over the blur layer under the
        // view, so text keeps its contrast whatever the desktop behind is.
        .background(theme.glass.sidebar ? t.panel.opacity(ChromePalette.glassScrimAlpha) : t.panel)
        .coordinateSpace(name: "click")
        .onReceive(NotificationCenter.default.publisher(for: .remoteActionFinished)) { _ in
            model.catalog.reload()
            approved = Set(model.catalog.snapshot.lanes.values.filter {
                $0.scopeURL.map { RemoteActions.approvedScopes.contains($0) } ?? false
            }.map(\.tab))
        }
        .onPreferenceChange(ClickTargetKey.self) { state.rowFrames = $0 }
    }

    private var rows: some View {
        VStack(alignment: .leading, spacing: 1) {
            if state.mode == .spaces {
                ForEach(model.spacesRows(state: state).filter { $0.kind != .footerUsage && $0.kind != .footerHost }) { spacesRow($0) }
            } else { ForEach(lines) { line in lineView(line) } }
        }
        .padding(.horizontal, 8)
        .padding(.top, 10)
    }

    /// What the sidebar draws, from SidebarModel (the state dump reads the same lines).
    private var lines: [SidebarLine] {
        if state.mode == .spaces {
            return SidebarModel.build(snapshot: model.snapshot, orchestrators: model.orchestrators, lanes: model.lanes,
                                      workflows: model.workflows, selectedTab: state.selectedTab, manualOpen: state.manualOpen,
                                      parked: Set(model.catalog.snapshot.parked.keys))
        }
        return SidebarModel.buildAreas(snapshot: model.snapshot, orchestrators: model.orchestrators, lanes: model.lanes,
                                       workflows: model.workflows, catalog: model.catalog.snapshot, chip: state.chip,
                                       areaOnly: state.areaOnly, folded: state.foldedAreas, focusExpanded: state.focusExpanded,
                                       focusCursor: state.focusCursor, selectedTab: state.selectedTab, manualOpen: state.manualOpen)
    }


    private var spacesFooter: some View {
        VStack(alignment: .leading, spacing: 0) {
            footerGroup(.footerUsage)
            footerGroup(.footerHost)
        }.padding(.horizontal, 8).padding(.bottom, 8)
    }

    private func footerGroup(_ kind: SpacesRow.Kind) -> some View {
        let rows = model.spacesRows(state: state).filter { $0.kind == kind }
        return ViewThatFits(in: .horizontal) {
            HStack(spacing: 8) {
                ForEach(rows) { row in
                    HStack(spacing: 4) {
                        Text(row.title).foregroundStyle(t.mute)
                        Text(row.trailing).foregroundStyle(row.alert == "act" ? t.bad : row.alert == "warn" ? t.warn : t.mute)
                    }.fixedSize().contentShape(Rectangle())
                        .onTapGesture { if kind == .footerUsage { spacesClick(row, part: "link") } }
                }
            }.frame(height: 23)
            VStack(alignment: .leading, spacing: 0) { ForEach(rows) { spacesRow($0) } }
        }
    }

    private var spacesFirstSpaceId: String? { model.spacesRows(state: state).first { $0.kind == .space }?.id }

    /// Status color, as Ghostty: working green, blocked red, done peach, idle and unknown mute.
    private func spacesTone(_ tone: String) -> Color {
        switch tone {
        case "working": return t.ok
        case "blocked": return t.bad
        case "done": return t.warn
        default: return t.mute
        }
    }

    /// Leading inset: sections sit under the space name, rows under the section label.
    private func spacesIndent(_ row: SpacesRow) -> CGFloat {
        switch row.kind {
        case .tab, .run: return CGFloat(row.depth) * 12 + 12
        case .section, .group: return CGFloat(row.depth) * 12
        default: return 0
        }
    }

    private func spacesRow(_ row: SpacesRow) -> some View {
        HStack(spacing: 5) {
            if row.chevron != "none", row.kind != .space, row.kind != .hidden {
                Image(systemName: row.chevron == "open" ? "chevron.down" : "chevron.right").font(.system(size: 8, weight: .semibold)).foregroundStyle(t.mute)
                    .frame(width: 9).onTapGesture { spacesClick(row, part: "chevron") }
            } else if row.kind == .section {
                Color.clear.frame(width: 9, height: 1)
            }
            if !row.glyph.isEmpty {
                Text(row.glyph).font(.system(size: 10)).foregroundStyle(spacesTone(row.tone)).frame(width: 12)
            }
            if row.kind == .goal {
                Text("goal").foregroundStyle(t.mute)
                Menu(state.spacesChrome.goalFilter ?? "All") {
                    Button("All") { state.spacesChrome.goalFilter = nil; state.saveSpacesChrome() }
                    ForEach(model.spacesOverlay.goalChoices, id: \.self) { goal in
                        Button(goal) { state.spacesChrome.goalFilter = goal; state.saveSpacesChrome() }
                    }
                }.menuStyle(.borderlessButton).fixedSize()
            } else {
                Text(row.title)
                    .font(.system(size: row.kind == .section ? 10.5 : 12.5,
                                  weight: row.kind == .space || row.kind == .title ? .semibold
                                      : (row.kind == .tab && !row.dim ? .medium : .regular)))
                    .tracking(row.kind == .section ? 0.4 : 0)
                    .foregroundStyle(row.kind == .section || row.kind == .group || row.kind == .hidden || row.dim ? t.mute : t.ink)
                    .lineLimit(1).truncationMode(.tail)
            }
            Spacer(minLength: 4)
            if !row.trailing.isEmpty, row.kind != .goal {
                Text(row.trailing).font(.system(size: 10.5)).foregroundStyle(row.link == nil ? t.mute : t.accent)
                    .lineLimit(1).fixedSize()
                    .onTapGesture { spacesClick(row, part: row.link == nil ? "body" : "link") }
            }
            if row.alert != "none" { Text("!").fontWeight(.bold).foregroundStyle(row.alert == "act" ? t.bad : t.warn) }
            if hoveredSpaceRow == row.id, let tab = row.tab, row.kind == .tab, model.spacesOverlay.tabs[tab]?.mode == "parked" {
                Button("Resume") { resume(tab) }.buttonStyle(.plain).foregroundStyle(t.accent)
            }
            if row.kind == .section, row.toggleKey != nil {
                Text(state.spacesChrome.focusedSection == String(row.id.dropFirst(8)) ? "✕" : "◎")
                    .font(.system(size: 10)).foregroundStyle(t.mute).onTapGesture { spacesClick(row, part: "focus") }
            }
            if row.kind == .space {
                let pinned = state.spacesChrome.pinnedSpaces.contains(String(row.id.dropFirst(6)))
                Text("⚲").foregroundStyle(pinned ? t.accent : t.mute).onTapGesture { spacesClick(row, part: "pin") }
                Text("+").foregroundStyle(t.mute).onTapGesture { spacesClick(row, part: "plus") }
            }
            if row.kind == .space || row.kind == .hidden, row.chevron != "none" {
                Image(systemName: row.chevron == "open" ? "chevron.down" : "chevron.right").font(.system(size: 8, weight: .semibold)).foregroundStyle(t.mute)
                    .frame(width: 9).onTapGesture { spacesClick(row, part: "chevron") }
            }
        }
        .padding(.top, row.kind == .space && row.id != spacesFirstSpaceId ? 10 : 0)
        .frame(height: 23).padding(.leading, spacesIndent(row)).padding(.horizontal, 4)
        .background(RoundedRectangle(cornerRadius: 4).fill(row.tab == state.selectedTab && row.kind == .tab ? t.sel : .clear))
        .contentShape(Rectangle()).onTapGesture { spacesClick(row, part: "body") }
        .onHover { hoveredSpaceRow = $0 ? row.id : nil }
        .contextMenu {
            if let tab = row.tab {
                Button("Rename…") { onRename(tab) }
                Button("Show info") { if let r = model.allRowsInOrder.first(where: { $0.id == tab }) { openDetail?(r) } }
                if row.id.contains(":parked") || model.spacesOverlay.tabs[tab]?.mode == "parked" { Button("Resume") { resume(tab) } }
                else { Button("Park…") { ParkActions.run("park", tab: tab, note: nil) { _, _ in model.catalog.reload() } } }
                if RemoteActions.slug(model.catalog.snapshot.lanes[tab]?.scopeURL) != nil { Button("Approve scope…") { approve(tab) } }
            }
        }.clickTarget(row.id)
    }

    private func spacesClick(_ row: SpacesRow, part: String) {
        if part == "plus" {
            let id = String(row.id.dropFirst(6))
            let commands = HerdrCommands(socketPath: model.env["HERDR_SOCKET_PATH"] ?? "")
            DispatchQueue.global(qos: .userInitiated).async {
                if let made = commands.tabCreate(workspaceId: id, cwd: nil) { DispatchQueue.main.async { select(made.tabId) } }
            }
        } else if part == "link" {
            if let raw = row.link, let url = URL(string: raw), ["http", "https"].contains(url.scheme?.lowercased() ?? "") { NSWorkspace.shared.open(url) }
        } else if part == "pin" {
            state.spacesChrome.toggle("pin:" + String(row.id.dropFirst(6))); state.saveSpacesChrome()
        } else if part == "focus" {
            let key = String(row.id.dropFirst(8))
            state.spacesChrome.focusedSection = state.spacesChrome.focusedSection == key ? nil : key; state.saveSpacesChrome()
        } else if part == "chevron" || [.section, .group, .hidden, .space].contains(row.kind) {
            if let key = row.toggleKey { state.spacesChrome.toggle(key); state.saveSpacesChrome() }
        } else if row.kind == .footerUsage {
            spacesClick(row, part: "link")
        } else if let tab = row.tab { select(tab) }
    }

    private var chrome: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack(spacing: 4) {
                Spacer(minLength: 0)
                modeButton("Areas", .areas)
                modeButton("Spaces", .spaces)
            }
            .padding(.leading, 52)
            .frame(height: 36)
            if state.mode == .areas {
                chipRow
                if let id = state.areaOnly {
                    HStack(spacing: 4) {
                        Text("Only").foregroundStyle(t.mute)
                        Text(model.catalog.snapshot.areaName(id)).foregroundStyle(t.ink)
                        Text("✕").foregroundStyle(t.mute)
                    }
                    .font(.system(size: 11.5))
                    .padding(.horizontal, 8).padding(.vertical, 3)
                    .contentShape(Rectangle())
                    .onTapGesture { state.setAreaOnly(nil) }
                    .hookAction("only") { state.setAreaOnly(nil) }
                    .clickTarget("only")
                }
            }
        }
        .padding(.horizontal, 8)
    }

    private func modeButton(_ title: String, _ mode: SidebarMode) -> some View {
        Text(title)
            .font(.system(size: 12, weight: state.mode == mode ? .semibold : .regular))
            .padding(.horizontal, 8).padding(.vertical, 3)
            .background(RoundedRectangle(cornerRadius: 5).fill(state.mode == mode ? t.sel : Color.clear))
            .contentShape(Rectangle())
            .onTapGesture { state.setMode(mode) }
            .hookAction("mode:\(mode.rawValue)") { state.setMode(mode) }
            .clickTarget("mode:\(mode.rawValue)")
    }

    private var chipRow: some View {
        let parkedCount = SidebarModel.parkedCount(snapshot: model.snapshot, orchestrators: model.orchestrators,
                                                  lanes: model.lanes, workflows: model.workflows,
                                                  catalog: model.catalog.snapshot, areaOnly: state.areaOnly)
        let chips: [(AreaChip, String)] = [
            (.all, "All"), (.scoping, "Scope"), (.building, "Build"), (.review, "Review"), (.use, "Use"),
            (.parked, parkedCount > 0 ? "Parked \(parkedCount)" : "Parked"),
        ]
        // Six chips share the sidebar width: one line each, tighter gaps, never a wrapped "Parked N".
        return HStack(spacing: 4) {
            ForEach(chips, id: \.0.rawValue) { chip, title in
                Text(title)
                    .lineLimit(1)
                    .fixedSize()
                    .font(.system(size: 12, weight: state.chip == chip ? .medium : .regular))
                    .foregroundStyle(state.chip == chip ? t.ink : t.mute)
                    .padding(.horizontal, 4).padding(.vertical, 2)
                    .background(RoundedRectangle(cornerRadius: 6).fill(state.chip == chip ? t.sel : Color.clear))
                    .contentShape(Rectangle())
                    .onTapGesture { state.setChip(chip) }
                    .hookAction("chip:\(chip.rawValue)") { state.setChip(chip) }
                    .clickTarget("chip:\(chip.rawValue)")
            }
            Spacer(minLength: 0)
        }
    }

    @ViewBuilder private func lineView(_ l: SidebarLine) -> some View {
        switch l.kind {
        case .header:
            if state.mode == .areas { Color.clear.frame(height: 6) } else { header(l) }
        case .note: note(l)
        case .area: areaHeader(l)
        case .focus: focusHeader(l)
        case .parked: parkedHeader(l)
        default:
            if l.parked { parkedRow(l) } else { rowView(l) }
        }
    }

    private func focusHeader(_ l: SidebarLine) -> some View {
        HStack(spacing: 6) {
            StateGlyph(state: .needs, tokens: t)
            Text("Focus").font(.system(size: 12, weight: .semibold)).foregroundStyle(t.ink)
            Text(l.trailing).font(.system(size: 11, weight: .semibold)).foregroundStyle(t.warn)
            Spacer(minLength: 4)
            if let open = l.chevron {
                Text(open ? "▾" : "▸").foregroundStyle(t.faint)
            }
        }
        .padding(.horizontal, 6)
        .frame(height: 28)
        .background(RoundedRectangle(cornerRadius: 6).fill(state.chip == .needs ? t.sel : Color.clear))
        .contentShape(Rectangle())
        .onTapGesture {
            state.setChip(.needs)
            click(l)
        }
        .hookAction("focus") { click(l) }
        .hookAction("chip:needs") { state.setChip(.needs) }
        .clickTarget("focus")
        .clickTarget("chip:needs")
    }

    /// The foot group: a hairline above, then "Parked N", shut until opened.
    private func parkedHeader(_ l: SidebarLine) -> some View {
        VStack(spacing: 0) {
            Rectangle().fill(t.line).frame(height: 1).padding(.top, 10).padding(.bottom, 6)
            HStack(spacing: 6) {
                Text((l.chevron ?? false) ? "▾" : "▸").foregroundStyle(t.mute)
                Text(l.title).foregroundStyle(t.mute)
                Spacer(minLength: 4)
                Text(l.trailing).foregroundStyle(t.mute)
            }
            .padding(.horizontal, 6)
            .padding(.vertical, 4)
            .contentShape(Rectangle())
            .onTapGesture { if let id = l.toggle { toggle(id, currentlyOpen: l.chevron ?? false) } }
            .clickTarget(l.id)
        }
    }

    /// A parked row: name, then when and why on a second line; Resume on the right.
    private func parkedRow(_ l: SidebarLine) -> some View {
        HStack(alignment: .center, spacing: 6) {
            if !l.glyph.isEmpty { Text(l.glyph).foregroundStyle(t.mute) }
            VStack(alignment: .leading, spacing: 1) {
                Text(l.title).foregroundStyle(t.ink).lineLimit(1)
                if let note = l.parkNote, !note.isEmpty {
                    Text(note).font(.system(size: 11)).foregroundStyle(t.mute).lineLimit(2).truncationMode(.tail)
                        .help(note)
                }
            }
            Spacer(minLength: 4)
            if let tab = l.tab {
                Text(resuming.contains(tab) ? "Resuming…" : "Resume")
                    .font(.system(size: 11, weight: .medium))
                    .foregroundStyle(t.ink)
                    .padding(.horizontal, 8).padding(.vertical, 2)
                    .background(RoundedRectangle(cornerRadius: 5).stroke(t.line, lineWidth: 1))
                    .contentShape(Rectangle())
                    .onTapGesture { resume(tab) }
                    .clickTarget("resume:\(tab)")
            }
        }
        .padding(.leading, CGFloat(6 + l.depth * 16))
        .padding(.trailing, 6)
        .padding(.vertical, 4)
        .background(RoundedRectangle(cornerRadius: 5).fill(l.selected ? t.sel : .clear))
        .contentShape(Rectangle())
        .onTapGesture { click(l) }
        .contextMenu {
            if let tab = l.tab {
                Button("Rename…") { onRename(tab) }
                Button("Resume") { resume(tab) }
                if RemoteActions.slug(model.catalog.snapshot.lanes[tab]?.scopeURL) != nil {
                    Button("Approve scope…") { approve(tab) }
                }
            }
        }
        .clickTarget(l.id)
    }

    private func resume(_ tab: String) {
        guard !resuming.contains(tab) else { return }
        resuming.insert(tab)
        ParkActions.run("unpark", tab: tab) { _, _ in
            resuming.remove(tab)
            model.catalog.reload()
        }
    }

    private func approve(_ tab: String) {
        guard let lane = model.catalog.snapshot.lanes[tab] else { return }
        RemoteActions.approve(scopeURL: lane.scopeURL, title: lane.name) { _, _ in }
    }

    private func park(_ l: SidebarLine) {
        guard let tab = l.tab, let note = ParkActions.askNote(name: l.title, window: NSApp.keyWindow) else { return }
        ParkActions.run("park", tab: tab, note: note.isEmpty ? nil : note) { _, _ in model.catalog.reload() }
    }

    private func areaHeader(_ l: SidebarLine) -> some View {
        HStack(spacing: 6) {
            if let open = l.chevron {
                Text(open ? "▾" : "▸").foregroundStyle(t.mute)
            }
            Circle().fill(Color(shellHex: l.color ?? "#999999")).frame(width: 8, height: 8)
            Text(l.title).lineLimit(1)
            Spacer(minLength: 4)
            Text(l.trailing).foregroundStyle(t.mute)
        }
        .padding(.horizontal, 6)
        .padding(.vertical, 4)
        .contentShape(Rectangle())
        .onTapGesture { areaClick(l) }
        .hookAction("area:\(l.title)") { areaClick(l) }
        .clickTarget(l.id)
    }

    private func header(_ l: SidebarLine) -> some View {
        HStack {
            Text(l.title).font(.system(size: 10.5, weight: .semibold)).tracking(0.8)
            Spacer()
            Text(l.trailing).font(.system(size: 10.5))
        }
        .foregroundStyle(t.mute)
        .padding(.horizontal, 6)
        .padding(.top, 12)
        .padding(.bottom, 4)
    }

    /// The mock's one-line summary under a collapsed group.
    private func note(_ l: SidebarLine) -> some View {
        Text(l.title).lineLimit(1).truncationMode(.tail)
            .font(.system(size: 11))
            .foregroundStyle(t.mute)
            .padding(.leading, CGFloat(6 + l.depth * 16 + 12))
            .padding(.trailing, 6)
            .padding(.bottom, 2)
    }

    private func tone(_ tone: SidebarLine.Tone) -> Color {
        switch tone { case .normal: return t.ink; case .ok: return t.ok; case .warn: return t.warn; case .mute: return t.mute }
    }

    private func rowView(_ l: SidebarLine) -> some View {
        HStack(spacing: 6) {
            if let open = l.chevron {
                Text(open ? "▾" : "▸").foregroundStyle(t.mute)
                    .contentShape(Rectangle())
                    .onTapGesture { if let id = l.toggle { toggle(id, currentlyOpen: open) } }
            } else if l.depth == 0 {
                Text(" ").foregroundStyle(.clear)
            }
            if !l.glyph.isEmpty {
                Text(l.glyph).foregroundStyle(glyphColor(l))
            }
            Text(l.title).foregroundStyle(titleColor(l)).lineLimit(1)
            Spacer(minLength: 4)
            if !l.badge.isEmpty, state.chip == .all || !stageImplied(l) {
                Text(stageWord(l.badge))
                    .font(.system(size: 11))
                    .foregroundStyle(l.badge == "Review" || l.stage == "reviewing" ? t.ink : t.mute)
            }
            if let tab = l.tab, approved.contains(tab) { Text("Approved").foregroundStyle(t.ok) }
            if !l.trailing.isEmpty { Text(l.trailing).foregroundStyle(tone(l.trailingTone)) }
            if let host = l.host {
                Text(host)
                    .font(.system(size: 10, weight: .semibold))
                    .padding(.horizontal, 5).padding(.vertical, 1)
                    .foregroundStyle(host == TabClassifier.defaultHost ? t.mute : t.warn)
                    .background(RoundedRectangle(cornerRadius: 4).stroke(t.line, lineWidth: 1))
            }
        }
        .opacity(l.dim ? 0.72 : 1)
        .padding(.leading, CGFloat(6 + l.depth * 16))
        .padding(.trailing, 6)
        .padding(.vertical, 4)
        .background(RoundedRectangle(cornerRadius: 6).fill(l.selected ? t.sel : .clear))
        .contentShape(Rectangle())
        .onTapGesture { click(l) }
        .hookAction(l.kind == .focus ? "focus" : "row:\(l.title)") { click(l) }
        .contextMenu {
            if let tab = l.tab { Button("Rename…") { onRename(tab) } }
            if state.mode == .areas, l.tab != nil, l.kind == .orchestrator || l.kind == .lane {
                Button("Park…") { park(l) }
            }
            if let tab = l.tab, RemoteActions.slug(model.catalog.snapshot.lanes[tab]?.scopeURL) != nil {
                Button("Approve scope…") { approve(tab) }
            }
        }
        .clickTarget(l.id)
    }

    private func titleColor(_ l: SidebarLine) -> Color {
        l.dim || l.stage == "closed" ? t.faint : t.ink
    }

    private func glyphColor(_ l: SidebarLine) -> Color {
        if l.selected, let hex = l.color { return Color(shellHex: hex) }
        if l.kind == .space { return t.mute }
        return tone(l.glyphTone)
    }

    private func areaClick(_ l: SidebarLine) {
        guard let area = l.area else { return }
        if state.optionHeld { state.setAreaOnly(area) }
        else { state.setFolded(area, open: !(l.chevron ?? true)) }
    }

    private func click(_ l: SidebarLine) {
        if l.kind == .focus {
            state.setFocusExpanded(!(l.chevron ?? false))
            return
        }
        if l.kind == .area { areaClick(l); return }
        if let tab = l.tab {
            if state.mode == .areas || l.kind == .workflow {
                select(tab)
            } else if let openDetail, let r = model.allRowsInOrder.first(where: { $0.id == tab }) {
                openDetail(r)
            } else {
                select(tab)
            }
        } else if let ws = l.space, let s = model.snapshot, let tab = SidebarModel.targetTab(s, space: ws) {
            select(tab)
        } else if let id = l.toggle, let open = l.chevron {
            toggle(id, currentlyOpen: open)
        }
    }

    private func stageImplied(_ l: SidebarLine) -> Bool {
        switch state.chip {
        case .scoping: return l.stage == "scoping"
        case .building: return l.stage == "implementing" || l.role == "desk" || l.role == "job"
        case .review: return l.stage == "reviewing"
        case .use: return l.role == "desk" || l.role == "job"
        default: return false
        }
    }

    private func stageWord(_ badge: String) -> String {
        switch badge {
        case "Scoping": return "Scope"
        case "Building": return "Build"
        case "Ready for review": return "Review"
        case "Monitoring": return "Live"
        case "In use": return "desk"
        default: return badge
        }
    }

    private var factoryFooter: some View {
        HStack(spacing: 8) {
            Text("Factory")
                .font(.system(size: 12, weight: .medium))
                .foregroundStyle(t.ink)
            Spacer(minLength: 4)
            ForEach(model.hostsModel.rows.prefix(4)) { h in
                Text(h.host)
                    .font(.system(size: 11))
                    .foregroundStyle(t.mute)
                    .lineLimit(1)
            }
            if Channel.kind == .dev {
                Text("DEV").font(.system(size: 9, weight: .semibold)).foregroundStyle(t.warn)
            }
        }
        .padding(.horizontal, 10)
        .frame(height: 36)
        .frame(maxWidth: .infinity)
        .background(state.factoryOpen ? t.sel : Color.clear)
        .contentShape(Rectangle())
        .onTapGesture { onFactory() }
        .hookAction("factory") { onFactory() }
        .clickTarget("factory")
    }

    private func toggle(_ id: String, currentlyOpen: Bool) {
        if id == "focus" { state.setFocusExpanded(!currentlyOpen); return }
        if id.hasPrefix("area:") {
            state.setFolded(String(id.dropFirst("area:".count)), open: !currentlyOpen)
            return
        }
        state.manualOpen[id] = !currentlyOpen
    }

}

extension Color {
    init(shellHex: String) {
        var s = shellHex.trimmingCharacters(in: .whitespaces)
        if s.hasPrefix("#") { s.removeFirst() }
        var v: UInt64 = 0
        Scanner(string: s).scanHexInt64(&v)
        self.init(red: Double((v >> 16) & 0xFF) / 255, green: Double((v >> 8) & 0xFF) / 255, blue: Double(v & 0xFF) / 255)
    }
}
