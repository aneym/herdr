import AppKit
import SwiftUI

/// ⌘K / ⌘G. Ranking is pure and separate from the panel so the test hook can
/// read the same order the rows are drawn in.
enum QuickSwitch {
    struct Row: Equatable {
        var id: String
        var label: String
        /// Herdr label when the drawn name came from the lane file.
        var aliases: [String]
        var workspace: String
        var area: String
        var agent: String
        var titles: [String]
        var host: String
        var badge: String
        var parked: Bool
        /// 2 blocked, 1 done, 0 otherwise. Parked rows still carry it, and sort after everyone else.
        var attention: Int
        /// Selection order in this app run. 0 means never selected.
        var recent: Int
        var glyph: ShellState
    }

    static func rank(query: String, rows: [Row]) -> [String] {
        let q = query.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        let scored: [(Row, Int)] = rows.compactMap { row in
            let s = q.isEmpty ? 1 : fields(row).map { score(q, $0) }.max() ?? 0
            return s > 0 ? (row, s) : nil
        }
        return scored.sorted { a, b in
            if a.0.parked != b.0.parked { return !a.0.parked }
            if !q.isEmpty, a.1 != b.1 { return a.1 > b.1 }
            if a.0.attention != b.0.attention { return a.0.attention > b.0.attention }
            if a.0.recent != b.0.recent { return a.0.recent > b.0.recent }
            if a.0.label != b.0.label { return a.0.label < b.0.label }
            return a.0.id < b.0.id
        }.map(\.0.id)
    }

    /// Prefix beats a word-start, and a word-start beats a loose subsequence.
    static func score(_ query: String, _ raw: String) -> Int {
        let text = raw.lowercased()
        guard let start = matchStart(query, text) else { return 0 }
        var s = 1 + max(0, 12 - start)
        if text.hasPrefix(query) { s += 80 }
        if text.contains(query) { s += 20 }
        let words = text.split { !$0.isLetter && !$0.isNumber }.map(String.init)
        if words.contains(where: { $0.hasPrefix(query) }) { s += 40 }
        return s
    }

    private static func fields(_ row: Row) -> [String] {
        [row.label, row.workspace, row.area, row.agent] + row.aliases + row.titles
    }

    /// Index of the first matched character, or nil when `query` is not a subsequence.
    private static func matchStart(_ query: String, _ text: String) -> Int? {
        guard !query.isEmpty, !text.isEmpty else { return nil }
        var i = text.startIndex
        var first: String.Index?
        for ch in query {
            guard let j = text[i...].firstIndex(of: ch) else { return nil }
            if first == nil { first = j }
            i = text.index(after: j)
        }
        guard let first else { return nil }
        return text.distance(from: text.startIndex, to: first)
    }

    static func makeRows(snapshot s: Snapshot, catalog: LaneSnapshot, tabs: [TabRow], recent: [String: Int]) -> [Row] {
        let space = Dictionary(s.tabs.map { ($0.tab_id, $0.workspace_id) }, uniquingKeysWith: { a, _ in a })
        let wsById = Dictionary(s.workspaces.map { ($0.workspace_id, $0) }, uniquingKeysWith: { a, _ in a })
        var titles: [String: [String]] = [:]
        func add(_ tab: String, _ values: [String?]) {
            for raw in values {
                let t = raw?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
                guard !t.isEmpty, titles[tab]?.contains(t) != true else { continue }
                titles[tab, default: []].append(t)
            }
        }
        for p in s.panes { add(p.tab_id, [p.terminal_title_stripped, p.terminal_title, p.title]) }
        for a in s.agents { add(a.tab_id, [a.terminal_title_stripped, a.terminal_title, a.title]) }
        let facts = SidebarModel.facts(s)
        return tabs.map { r in
            let wsID = space[r.id] ?? ""
            let ws = wsById[wsID]
            let named = ws?.label?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
            let workspace = named.isEmpty ? "space \(ws?.number ?? 0)" : named
            let lane = catalog.lanes[r.id]
            let areaID = catalog.areaId(tab: r.id, workspace: wsID, lane: lane)
            let label = catalog.displayName(tab: r.id, lane: lane, fallback: r.label)
            let failed = facts[r.id]?.failed ?? false
            let attention = r.status == "blocked" ? 2 : (r.status == "done" ? 1 : 0)
            return Row(id: r.id, label: label, aliases: label == r.label ? [] : [r.label],
                       workspace: workspace, area: catalog.areaName(areaID), agent: r.agent ?? "",
                       titles: titles[r.id] ?? [], host: r.host,
                       badge: SidebarModel.badge(stage: lane?.section, role: catalog.role(tab: r.id, lane: lane)),
                       parked: catalog.parked[r.id] != nil, attention: attention, recent: recent[r.id] ?? 0,
                       glyph: ShellState.from(status: r.status, failed: failed, hasAgent: r.agent != nil))
        }
    }
}

/// Owns the panel and the key sink. A child of the main window, not an NSPanel:
/// the app's own window capture then includes it.
final class QuickSwitchController: ObservableObject {
    weak var owner: MainWindowController?
    @Published var isOpen = false
    @Published var query = ""
    @Published var selectAll = false
    @Published var cursor = 0
    @Published private(set) var rows: [QuickSwitch.Row] = []
    private var recent: [String: Int] = [:]
    private var recentSeq = 0
    private weak var host: SwitcherHostingView?

    var results: [String] { QuickSwitch.rank(query: query, rows: rows) }
    var visible: [QuickSwitch.Row] {
        let ids = results.prefix(9)
        let by = Dictionary(rows.map { ($0.id, $0) }, uniquingKeysWith: { a, _ in a })
        return ids.compactMap { by[$0] }
    }

    init(owner: MainWindowController) { self.owner = owner }

    func install(in root: RootView, theme: ThemeStore) {
        let host = SwitcherHostingView(rootView: QuickSwitchPanel(model: self, theme: theme))
        host.isHidden = true
        self.host = host
        root.attachSwitcher(host)
    }

    func noteSelected(_ id: String) {
        recentSeq += 1
        recent[id] = recentSeq
    }

    func reload() {
        guard let o = owner, let s = o.model.snapshot else { rows = []; return }
        rows = QuickSwitch.makeRows(snapshot: s, catalog: o.model.catalog.snapshot,
                                    tabs: o.model.allRowsInOrder, recent: recent)
        clamp()
    }

    func setQuery(_ text: String) {
        query = text
        selectAll = false
        cursor = 0
        reload()
    }

    /// `selectAll` is ⌘G: the query is selected so the next character replaces it.
    func present(selectAll: Bool) {
        reload()
        let opening = !isOpen
        self.selectAll = selectAll
        if opening { cursor = 0 }
        clamp()
        isOpen = true
        host?.isHidden = false
        owner?.root.layoutSubtreeIfNeeded()
        if let host { owner?.window.makeFirstResponder(host) }
    }

    func dismiss() {
        guard isOpen else { return }
        isOpen = false
        selectAll = false
        host?.isHidden = true
        guard let o = owner else { return }
        o.focusPane(o.state.focusedPane ?? o.host.rects.first?.0.paneId)
    }

    /// 1-based, matching ⌘1–9. Selects that visible row and closes.
    func pick(_ n: Int) {
        let rows = visible
        guard n >= 1, n <= rows.count else { return }
        let id = rows[n - 1].id
        let reveal = owner?.state.mode == .areas
        isOpen = false
        selectAll = false
        host?.isHidden = true
        owner?.selectTab(id, revealDocs: reveal)
    }

    func commit() { pick(cursor + 1) }

    func move(_ d: Int) {
        let n = visible.count
        guard n > 0 else { return }
        cursor = min(n - 1, max(0, cursor + d))
    }

    /// Eats keys while the panel is up so they never reach the terminal.
    /// Command chords return false and stay with the menu. Esc returns false so
    /// the keymap's close chord runs and is logged with the other chords.
    func sink(_ event: NSEvent) -> Bool {
        guard isOpen, event.type == .keyDown else { return false }
        let mods = event.modifierFlags.intersection(Keymap.modMask)
        if mods.contains(.command) || event.keyCode == 53 { return false }
        if event.keyCode == 126 || (mods == .control && event.keyCode == 35) { move(-1); return true }
        if event.keyCode == 125 || (mods == .control && event.keyCode == 45) { move(1); return true }
        if event.keyCode == 36 { commit(); return true }
        if mods.contains(.control) { return true }
        if event.keyCode == 51 {
            if selectAll || query.isEmpty { query = "" } else { query.removeLast() }
            selectAll = false
            cursor = 0
            return true
        }
        guard let raw = event.characters else { return true }
        let text = raw.filter { ch in ch.unicodeScalars.allSatisfy { $0.value >= 32 && $0.value != 127 } }
        guard !text.isEmpty else { return true }
        if selectAll { query = "" }
        selectAll = false
        query.append(text)
        cursor = 0
        return true
    }

    private func clamp() {
        let n = min(9, results.count)
        if n == 0 { cursor = 0 }
        else if cursor >= n { cursor = n - 1 }
    }
}

/// The name contains HostingView so the in-process screenshot path draws it.
final class SwitcherHostingView: NSHostingView<QuickSwitchPanel> {
    override var acceptsFirstResponder: Bool { true }
}

struct QuickSwitchPanel: View {
    @ObservedObject var model: QuickSwitchController
    @ObservedObject var theme: ThemeStore

    var body: some View {
        let t = theme.tokens
        GeometryReader { geo in
            // Upper third, like Spotlight, but never pushed off a short window: the list scrolls.
            let top = min(96, max(12, geo.size.height * 0.14))
            let listMax = max(32, geo.size.height - top - 12 - 70)
            ZStack(alignment: .top) {
                t.ink.opacity(0.32)
                    .contentShape(Rectangle())
                    .onTapGesture { model.dismiss() }
                VStack(alignment: .leading, spacing: 2) {
                    field(t)
                    Rectangle().fill(t.mute.opacity(0.25)).frame(height: 1)
                        .padding(.horizontal, -12)
                        .padding(.bottom, 6)
                    let rows = model.visible
                    if rows.isEmpty {
                        Text("No matches")
                            .font(.system(size: 12.5))
                            .foregroundStyle(t.mute)
                            .padding(.horizontal, 8)
                            .padding(.bottom, 6)
                    } else {
                        ScrollViewReader { proxy in
                            ScrollView(.vertical) {
                                VStack(spacing: 0) {
                                    ForEach(Array(rows.enumerated()), id: \.element.id) { i, row in
                                        rowView(row, index: i, on: i == model.cursor, tokens: t)
                                            .id(i)
                                            .contentShape(Rectangle())
                                            .onTapGesture { model.pick(i + 1) }
                                    }
                                }
                            }
                            .scrollIndicators(.never)
                            .frame(height: min(CGFloat(rows.count) * 32, listMax))
                            .onChange(of: model.cursor) { _, c in proxy.scrollTo(c) }
                        }
                    }
                }
                .padding(12)
                .frame(width: 560)
                .background(OverlayBackground(theme: theme, corner: 10))
                .padding(.top, top)
            }
            .frame(width: geo.size.width, height: geo.size.height, alignment: .top)
        }
    }

    private func field(_ t: Tokens) -> some View {
        let shown = model.query.isEmpty ? "Jump to a lane, agent, or tab" : model.query
        // The field sits straight on the panel; a hairline below it, no inner box.
        return HStack(spacing: 8) {
            Image(systemName: "magnifyingglass")
                .font(.system(size: 14))
                .foregroundStyle(t.mute)
            Text(shown)
                .font(.system(size: 16))
                .foregroundStyle(model.query.isEmpty ? t.mute : t.ink)
                .lineLimit(1)
                .padding(.horizontal, 2)
                .background(RoundedRectangle(cornerRadius: 4).fill(model.selectAll && !model.query.isEmpty ? t.sel : Color.clear))
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.horizontal, 8)
        .padding(.top, 2)
        .padding(.bottom, 10)
    }

    private func rowView(_ row: QuickSwitch.Row, index: Int, on: Bool, tokens t: Tokens) -> some View {
        let state = row.parked ? "parked" : row.badge
        let meta = [row.workspace, state, row.host].filter { !$0.isEmpty }.joined(separator: " · ")
        return HStack(spacing: 8) {
            StateGlyph(state: row.glyph, tokens: t)
            Text(row.label)
                .font(.system(size: 13))
                .foregroundStyle(row.parked ? t.mute : t.ink)
                .lineLimit(1)
                .layoutPriority(1)
            Spacer(minLength: 8)
            Text(meta)
                .font(.system(size: 11))
                .foregroundStyle(t.mute)
                .lineLimit(1)
            Text("⌘\(index + 1)")
                .font(.system(size: 11).monospacedDigit())
                .foregroundStyle(t.mute.opacity(on ? 1 : 0.6))
                .frame(width: 22, alignment: .trailing)
        }
        .padding(.horizontal, 8)
        .frame(height: 32)
        .background(RoundedRectangle(cornerRadius: 6).fill(on ? t.sel : Color.clear))
    }
}
