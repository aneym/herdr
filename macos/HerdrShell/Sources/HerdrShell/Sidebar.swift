import SwiftUI

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
}

/// Frames of clickable things, keyed by name, reported up from the SwiftUI views (P11 check clicks).
struct ClickTargetKey: PreferenceKey {
    static var defaultValue: [String: CGRect] = [:]
    static func reduce(value: inout [String: CGRect], nextValue: () -> [String: CGRect]) {
        value.merge(nextValue(), uniquingKeysWith: { $1 })
    }
}

extension View {
    /// Reports this view's frame in the named "click" space under `id`.
    func clickTarget(_ id: String) -> some View {
        background(GeometryReader { g in
            Color.clear.preference(key: ClickTargetKey.self, value: [id: g.frame(in: .named("click"))])
        })
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
        .font(.system(size: 12.5, design: .monospaced))
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
        SidebarModel.build(snapshot: model.snapshot, orchestrators: model.orchestrators, lanes: model.lanes,
                           workflows: model.workflows, selectedTab: state.selectedTab, manualOpen: state.manualOpen)
    }

    @ViewBuilder private func lineView(_ l: SidebarLine) -> some View {
        switch l.kind {
        case .header: header(l)
        case .note: note(l)
        default: rowView(l)
        }
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
            if !l.glyph.isEmpty { Text(l.glyph).foregroundStyle(l.kind == .space ? t.mute : tone(l.glyphTone)) }
            Text(l.title).foregroundStyle(titleColor(l)).lineLimit(1)
            Spacer(minLength: 4)
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
        .clickTarget(l.id)
    }

    private func titleColor(_ l: SidebarLine) -> Color {
        l.titleKind.map(kindColor) ?? t.ink
    }

    private func click(_ l: SidebarLine) {
        if let tab = l.tab {
            if l.kind != .workflow, let openDetail, let r = model.allRowsInOrder.first(where: { $0.id == tab }) {
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
        state.manualOpen[id] = !currentlyOpen
    }

    private func kindColor(_ k: TabRow.Kind) -> Color {
        switch k { case .orchestrator: return t.orch; case .lane: return t.lane; case .workflow: return t.wf }
    }
}
