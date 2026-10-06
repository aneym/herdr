import AppKit
import SwiftUI

// P11: the detail panel. Clicking the orchestrator or a lane row opens a fixed-width panel
// between the sidebar and the panes with that row's inbox and the workflows running under
// it. The panel never takes keyboard focus and never changes the selected tab, so the
// current pane stays live: typing still reaches it. Esc closes the panel (and only then
// is Esc the app's; with no panel open it goes to the pane).

struct DetailItem: Equatable {
    let text: String
    let source: String
}

struct DetailWorkflow: Equatable, Identifiable {
    let id: String        // tab_id
    let label: String
    let phase: String     // `phase` token, else the agent status
    let status: String
    let host: String
}

/// Workflows under one lane; `lane` is nil for the row's own (directly folded) workflows.
struct DetailGroup: Equatable {
    let lane: String?
    let workflows: [DetailWorkflow]
}

struct DetailContent: Equatable {
    let rowId: String
    let title: String
    let kind: TabRow.Kind
    let host: String
    let status: String
    let agent: String?
    let inbox: [DetailItem]
    let routed: [DetailItem]      // orchestrator only
    let groups: [DetailGroup]
    var workflowCount: Int { groups.reduce(0) { $0 + $1.workflows.count } }
}

/// Where the panel gets its content. The default reads herdr only (tokens and folded rows);
/// a factory-backed provider can replace it without touching the view.
protocol DetailProvider {
    func content(for rowId: String, in model: HerdrModel) -> DetailContent?
}

/// Herdr-only content.
///  - INBOX: items in the agent token `inbox_items` (`|` separated, each `text` or `text@source`),
///    plus one line for every workflow or lane under the row that is blocked (it wants you).
///    The token `inbox` stays the sidebar's numeric count (SidebarModel); the panel never reads it.
///  - ROUTED: the token `routed`, same format; orchestrator rows only.
///  - WORKFLOWS: the row's folded workflows; the orchestrator also lists the workflows of each lane in
///    its own space (never another space's) under the lane.
struct HerdrDetailProvider: DetailProvider {
    static func items(_ raw: String?, defaultSource: String) -> [DetailItem] {
        (raw ?? "").split(separator: "|").compactMap { part in
            let s = part.trimmingCharacters(in: .whitespacesAndNewlines)
            guard !s.isEmpty else { return nil }
            if let at = s.lastIndex(of: "@"), at != s.startIndex {
                let src = s[s.index(after: at)...].trimmingCharacters(in: .whitespaces)
                if !src.isEmpty, !src.contains(" ") { return DetailItem(text: String(s[..<at]).trimmingCharacters(in: .whitespaces), source: src) }
            }
            return DetailItem(text: s, source: defaultSource)
        }
    }

    func content(for rowId: String, in model: HerdrModel) -> DetailContent? {
        guard let snap = model.snapshot, let row = model.allRowsInOrder.first(where: { $0.id == rowId }),
              row.kind != .workflow, let tab = snap.tabs.first(where: { $0.tab_id == rowId }) else { return nil }
        let c = TabClassifier(snap)
        func wf(_ r: TabRow) -> DetailWorkflow {
            let t = snap.tabs.first { $0.tab_id == r.id }
            let phase = t.flatMap { c.token("phase", of: $0) } ?? r.status
            return DetailWorkflow(id: r.id, label: r.label, phase: phase, status: r.status, host: r.host)
        }
        var groups: [DetailGroup] = []
        if !row.children.isEmpty { groups.append(DetailGroup(lane: nil, workflows: row.children.map(wf))) }
        var asks: [DetailItem] = []
        for r in row.children where r.status == "blocked" { asks.append(DetailItem(text: "\(r.label) wants you", source: "blocked")) }
        if row.kind == .orchestrator {
            let space = tab.workspace_id
            for lane in model.lanes where snap.tabs.first(where: { $0.tab_id == lane.id })?.workspace_id == space {
                if lane.status == "blocked" { asks.append(DetailItem(text: "\(lane.label) wants you", source: "blocked")) }
                guard !lane.children.isEmpty else { continue }
                groups.append(DetailGroup(lane: lane.label, workflows: lane.children.map(wf)))
                for r in lane.children where r.status == "blocked" { asks.append(DetailItem(text: "\(r.label) wants you", source: "blocked")) }
            }
        }
        let inbox = Self.items(c.token("inbox_items", of: tab), defaultSource: "inbox") + asks
        let routed = row.kind == .orchestrator ? Self.items(c.token("routed", of: tab), defaultSource: "routed") : []
        return DetailContent(rowId: rowId, title: row.label, kind: row.kind, host: row.host, status: row.status,
                             agent: row.agent, inbox: inbox, routed: routed, groups: groups)
    }
}

/// Which row's details are open. The view and the state dump both read it.
final class DetailPanelModel: ObservableObject {
    @Published private(set) var rowId: String?
    let provider: DetailProvider
    /// Where the panel's clickable things were last drawn (view-local, top-left); read by the test hook.
    var targets: [String: CGRect] = [:]

    init(provider: DetailProvider = HerdrDetailProvider()) { self.provider = provider }

    var isOpen: Bool { rowId != nil }

    /// A second click on the open row closes it; a click on another row switches to that row.
    func toggle(_ id: String) { rowId = rowId == id ? nil : id }
    func open(_ id: String) { rowId = id }
    func close() { rowId = nil }
}

struct DetailPanelView: View {
    @ObservedObject var model: HerdrModel
    @ObservedObject var panel: DetailPanelModel
    @ObservedObject var theme: ThemeStore
    var openFull: (String) -> Void
    /// false only for ImageRenderer snapshots, which cannot draw a ScrollView.
    var scrolls = true

    private var t: Tokens { theme.tokens }

    var body: some View {
        Group {
            if let id = panel.rowId, let c = panel.provider.content(for: id, in: model) {
                let stack = VStack(alignment: .leading, spacing: 14) { body(c) }.padding(12)
                if scrolls { ScrollView { stack } } else { stack.frame(maxHeight: .infinity, alignment: .topLeading) }
            } else {
                Text("row is gone").foregroundStyle(t.mute).padding(12).frame(maxWidth: .infinity, alignment: .topLeading)
            }
        }
        .font(.system(size: 12, design: .monospaced))
        .foregroundStyle(t.ink)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .background(t.panel)
        .coordinateSpace(name: "click")
        .onPreferenceChange(ClickTargetKey.self) { panel.targets = $0 }
    }

    @ViewBuilder private func body(_ c: DetailContent) -> some View {
        HStack {
            Text(c.title).font(.system(size: 13, weight: .semibold)).foregroundStyle(kindColor(c.kind)).lineLimit(1)
            Spacer(minLength: 4)
            Text("esc").foregroundStyle(t.mute)
        }
        Text("\(c.kind.rawValue) · \(c.agent ?? "shell") · \(c.status) · \(c.host)").foregroundStyle(t.mute).font(.system(size: 11))
        section("INBOX", count: c.inbox.count) {
            if c.inbox.isEmpty { Text("nothing waiting").foregroundStyle(t.mute) }
            ForEach(Array(c.inbox.enumerated()), id: \.offset) { _, i in
                line(i.text, trailing: i.source, trailingColor: i.source == "blocked" ? t.warn : t.mute)
            }
        }
        if c.kind == .orchestrator, !c.routed.isEmpty {
            section("ROUTED", count: c.routed.count) {
                ForEach(Array(c.routed.enumerated()), id: \.offset) { _, i in line("→ " + i.text, trailing: i.source) }
            }
        }
        section(c.kind == .orchestrator ? "WORKFLOWS UNDER IT" : "WORKFLOWS", count: c.workflowCount) {
            if c.groups.isEmpty { Text("none running").foregroundStyle(t.mute) }
            ForEach(Array(c.groups.enumerated()), id: \.offset) { _, g in
                if let lane = g.lane {
                    Text(lane).foregroundStyle(t.lane).font(.system(size: 11, weight: .semibold)).padding(.top, 2)
                }
                ForEach(g.workflows) { w in workflowRow(w) }
            }
        }
        Button { openFull(c.rowId) } label: {
            Text("open full \(c.kind.rawValue) tab")
            .padding(.horizontal, 8).padding(.vertical, 4)
            .overlay(RoundedRectangle(cornerRadius: 5).stroke(t.line, lineWidth: 1))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .hookAction("open_full") { openFull(c.rowId) }
        .clickTarget("open_full")
    }

    private func section<Content: View>(_ title: String, count: Int, @ViewBuilder _ content: () -> Content) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Divider().overlay(t.line)
            HStack {
                Text(title).font(.system(size: 10.5, weight: .semibold)).tracking(0.8)
                Spacer()
                Text("\(count)")
            }
            .foregroundStyle(t.mute).padding(.top, 4)
            content()
        }
    }

    private func line(_ text: String, trailing: String, trailingColor: Color? = nil) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 6) {
            Text(text).lineLimit(2)
            Spacer(minLength: 4)
            Text(trailing).foregroundStyle(trailingColor ?? t.mute).font(.system(size: 11))
        }
    }

    private func workflowRow(_ w: DetailWorkflow) -> some View {
        HStack(spacing: 6) {
            Text(dot(w.status)).foregroundStyle(w.status == "working" ? t.ok : (w.status == "blocked" ? t.warn : t.mute))
            Text(w.label).foregroundStyle(t.wf).lineLimit(1)
            Spacer(minLength: 4)
            Text(w.phase).foregroundStyle(w.status == "blocked" ? t.warn : t.mute)
            Text(w.host)
                .font(.system(size: 10, weight: .semibold))
                .padding(.horizontal, 5).padding(.vertical, 1)
                .foregroundStyle(w.host == TabClassifier.defaultHost ? t.mute : t.warn)
                .overlay(RoundedRectangle(cornerRadius: 4).stroke(t.line, lineWidth: 1))
        }
    }

    private func dot(_ s: String) -> String {
        switch s { case "working": return "●"; case "blocked": return "◐"; case "idle": return "○"; default: return "·" }
    }

    private func kindColor(_ k: TabRow.Kind) -> Color {
        switch k { case .orchestrator: return t.orch; case .lane: return t.lane; case .workflow: return t.wf }
    }
}

/// The panel's AppKit side: a fixed-width host for the SwiftUI view. It is never first responder.
final class DetailPanelController {
    static let width: CGFloat = 340

    let model: DetailPanelModel
    let view: NSView

    private let herdr: HerdrModel, theme: ThemeStore

    /// PNG of the panel drawn straight from SwiftUI (independent of window capture), for check evidence.
    @MainActor func renderPNG(to path: String, size: CGSize) -> Bool {
        let v = DetailPanelView(model: herdr, panel: model, theme: theme, openFull: { _ in }, scrolls: false)
            .frame(width: size.width, height: size.height)
            .environment(\.colorScheme, theme.effective == .dark ? .dark : .light)
        let r = ImageRenderer(content: v)
        r.scale = 2
        guard let img = r.cgImage else { return false }
        let rep = NSBitmapImageRep(cgImage: img)
        return (try? rep.representation(using: .png, properties: [:])?.write(to: URL(fileURLWithPath: path))) != nil
    }

    init(herdr: HerdrModel, theme: ThemeStore, provider: DetailProvider = HerdrDetailProvider(),
         openFull: @escaping (String) -> Void) {
        self.herdr = herdr
        self.theme = theme
        model = DetailPanelModel(provider: provider)
        view = NoFocusHostingView(rootView: DetailPanelView(model: herdr, panel: model, theme: theme, openFull: openFull))
        view.isHidden = true
    }
}

/// Clicks inside the panel must not move keyboard focus off the pane.
final class NoFocusHostingView<Content: View>: NSHostingView<Content> {
    override var acceptsFirstResponder: Bool { false }
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }
    override var mouseDownCanMoveWindow: Bool { false }
}

// MARK: window controller glue

extension MainWindowController {
    var detailContent: DetailContent? {
        detailPanel.model.rowId.flatMap { detailPanel.model.provider.content(for: $0, in: model) }
    }

    /// Sidebar click on an orchestrator or lane row. The selected tab and the focused pane stay as they are.
    func toggleDetail(_ rowId: String) {
        setDetail(detailPanel.model.rowId == rowId ? nil : rowId)
    }

    func closeDetail() { setDetail(nil) }

    /// Esc is the app's only while a panel is open and no input method is composing.
    var detailClaimsEscape: Bool { detailPanel.model.isOpen && focusedSurface?.hasMarkedText() != true }

    private func setDetail(_ rowId: String?) {
        let keep = window.firstResponder
        if let rowId { detailPanel.model.open(rowId) } else { detailPanel.model.close() }
        state.detailRow = detailPanel.model.rowId
        root.detailOpen = detailPanel.model.isOpen
        root.needsLayout = true
        root.layoutSubtreeIfNeeded()
        // The panel never takes focus; make sure the pane that had it still does.
        if let keep, window.firstResponder !== keep { window.makeFirstResponder(keep) }
    }

    /// "open full tab": show the row's own tab in the pane area and close the panel.
    func openFullTab(_ rowId: String) {
        closeDetail()
        selectTab(rowId)
    }
}
