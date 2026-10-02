import SwiftUI

enum SidebarMode: String { case areas, spaces }

final class SidebarState: ObservableObject {
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
            mode = ShellPaths.filesPresent ? .areas : .spaces
        }
        chip = AreaChip(rawValue: d.string(forKey: Self.prefix + "chip") ?? "") ?? .all
        areaOnly = d.string(forKey: Self.prefix + "areaOnly")
        foldedAreas = Set(d.stringArray(forKey: Self.prefix + "folded") ?? [])
        focusExpanded = (d.object(forKey: Self.prefix + "focusExpanded") as? Bool) ?? false
        selectedTab = d.string(forKey: Self.prefix + "selectedTab")
        docOpen = (d.object(forKey: Self.prefix + "docOpen") as? Bool) ?? false
        docWidth = CGFloat((d.object(forKey: Self.prefix + "docWidth") as? Double) ?? 420)
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
            Divider().overlay(t.line)
            detail
            Divider().overlay(t.line)
            hostsRow
        }
        .font(.system(size: 12.5, design: state.mode == .areas ? .default : .monospaced))
        .foregroundStyle(t.ink)
        // Glass on the sidebar puts a panel-colored scrim over the blur layer under the
        // view, so text keeps its contrast whatever the desktop behind is.
        .background(theme.glass.sidebar ? t.panel.opacity(ChromePalette.glassScrimAlpha) : t.panel)
        .coordinateSpace(name: "click")
        .onPreferenceChange(ClickTargetKey.self) { state.rowFrames = $0 }
    }

    private var rows: some View {
        VStack(alignment: .leading, spacing: 1) {
            ForEach(lines) { line in lineView(line) }
        }
        .padding(.horizontal, 8)
        .padding(.top, 10)
    }

    /// What the sidebar draws, from SidebarModel (the state dump reads the same lines).
    private var lines: [SidebarLine] {
        if state.mode == .spaces {
            return SidebarModel.build(snapshot: model.snapshot, orchestrators: model.orchestrators, lanes: model.lanes,
                                      workflows: model.workflows, selectedTab: state.selectedTab, manualOpen: state.manualOpen)
        }
        return SidebarModel.buildAreas(snapshot: model.snapshot, orchestrators: model.orchestrators, lanes: model.lanes,
                                       workflows: model.workflows, catalog: model.catalog.snapshot, chip: state.chip,
                                       areaOnly: state.areaOnly, folded: state.foldedAreas, focusExpanded: state.focusExpanded,
                                       focusCursor: state.focusCursor, selectedTab: state.selectedTab, manualOpen: state.manualOpen)
    }

    private var chrome: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 4) {
                modeButton("Areas", .areas)
                modeButton("Spaces", .spaces)
                Spacer(minLength: 0)
            }
            if state.mode == .areas {
                chipRow
                if let id = state.areaOnly {
                    Text("only: \(model.catalog.snapshot.areaName(id)) ✕")
                        .font(.system(size: 11))
                        .foregroundStyle(t.ink)
                        .padding(.horizontal, 8).padding(.vertical, 3)
                        .background(RoundedRectangle(cornerRadius: 5).fill(t.sel))
                        .onTapGesture { state.setAreaOnly(nil) }
                        .hookAction("only") { state.setAreaOnly(nil) }
                        .clickTarget("only")
                }
            }
        }
        .padding(.horizontal, 8)
        .padding(.top, 8)
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
        let chips: [(AreaChip, String)] = [
            (.all, "All"), (.needs, "Needs you"), (.scoping, "Scoping"),
            (.building, "Building"), (.review, "Review"), (.use, "Use"),
        ]
        return LazyVGrid(columns: [GridItem(.adaptive(minimum: 78), spacing: 4)], alignment: .leading, spacing: 4) {
            ForEach(chips, id: \.0.rawValue) { chip, title in
                Text(title)
                    .font(.system(size: 11, weight: state.chip == chip ? .semibold : .regular))
                    .foregroundStyle(state.chip == chip ? t.ink : t.mute)
                    .padding(.horizontal, 7).padding(.vertical, 3)
                    .frame(maxWidth: .infinity)
                    .background(RoundedRectangle(cornerRadius: 5).fill(state.chip == chip ? t.sel : t.ink.opacity(0.06)))
                    .contentShape(Rectangle())
                    .onTapGesture { state.setChip(chip) }
                    .hookAction("chip:\(chip.rawValue)") { state.setChip(chip) }
                    .clickTarget("chip:\(chip.rawValue)")
            }
        }
    }

    @ViewBuilder private func lineView(_ l: SidebarLine) -> some View {
        switch l.kind {
        case .header: header(l)
        case .note: note(l)
        case .area: areaHeader(l)
        default: rowView(l)
        }
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
            if !l.badge.isEmpty {
                Text(l.badge)
                    .font(.system(size: 10))
                    .foregroundStyle(t.mute)
                    .padding(.horizontal, 5).padding(.vertical, 1)
                    .background(RoundedRectangle(cornerRadius: 4).fill(t.ink.opacity(0.08)))
            }
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
        .background(RoundedRectangle(cornerRadius: 5).fill(l.selected ? t.sel : .clear))
        .overlay(RoundedRectangle(cornerRadius: 5).stroke(l.tab != nil && state.detailRow == l.tab ? t.mute : .clear, lineWidth: 1))
        .contentShape(Rectangle())
        .onTapGesture { click(l) }
        .hookAction(l.kind == .focus ? "focus" : "row:\(l.title)") { click(l) }
        .clickTarget(l.id)
    }

    private func titleColor(_ l: SidebarLine) -> Color {
        l.titleKind.map(kindColor) ?? t.ink
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

    private var detail: some View {
        let row = model.allRowsInOrder.first { $0.id == state.selectedTab }
        let panes = model.snapshot?.panes.filter { $0.tab_id == state.selectedTab } ?? []
        return VStack(alignment: .leading, spacing: 3) {
            Text("DETAIL").font(.system(size: 10.5, weight: .semibold)).tracking(0.8).foregroundStyle(t.mute)
            if let row {
                Text(row.label).foregroundStyle(kindColor(row.kind))
                Text("\(row.kind.rawValue) · \(row.agent ?? "shell") · \(row.status) · \(row.host)").foregroundStyle(t.mute)
                ForEach(panes, id: \.pane_id) { p in
                    Text("\(p.pane_id == state.focusedPane ? "›" : " ") \(p.pane_id)  \(p.terminal_id)")
                        .foregroundStyle(p.pane_id == state.focusedPane ? t.ink : t.mute)
                }
            } else {
                Text("no tab selected").foregroundStyle(t.mute)
            }
        }
        .font(.system(size: 11, design: .monospaced))
        .padding(10)
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private var hostsRow: some View {
        HostsRow(hosts: model.hostsModel, tokens: t,
                 status: model.lastError == nil ? String(format: "%.0f ms", model.pollMs) : "offline",
                 statusIsError: model.lastError != nil)
    }

    private func toggle(_ id: String, currentlyOpen: Bool) {
        if id == "focus" { state.setFocusExpanded(!currentlyOpen); return }
        if id.hasPrefix("area:") {
            state.setFolded(String(id.dropFirst("area:".count)), open: !currentlyOpen)
            return
        }
        state.manualOpen[id] = !currentlyOpen
    }

    private func kindColor(_ k: TabRow.Kind) -> Color {
        switch k { case .orchestrator: return t.orch; case .lane: return t.lane; case .workflow: return t.wf }
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
