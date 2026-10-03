import Combine
import Foundation

/// Live herdr state: the `session.snapshot` shape (what `herdr api snapshot` prints), read by HerdrClient.
struct Snapshot: Decodable {
    struct Workspace: Decodable {
        let workspace_id: String; let label: String?; let number: Int; let orchestrator_mode: Bool?
        // P10: the space list follows herdr's own focus and tokens (`pinned`, `hidden`).
        let focused: Bool?; let active_tab_id: String?; let tokens: [String: String]?
    }
    struct Tab: Decodable { let tab_id: String; let workspace_id: String; let label: String?; let number: Int; let agent_status: String?; let pane_count: Int? }
    struct Pane: Decodable {
        let pane_id: String; let tab_id: String; let terminal_id: String
        let agent_status: String?; let focused: Bool?
        let title: String?; let terminal_title: String?
    }
    struct Owner: Decodable { let pane_id: String? }
    struct Ownership: Decodable { let current: Owner? }
    struct Agent: Decodable {
        let pane_id: String; let tab_id: String; let agent: String?; let agent_status: String?
        let tokens: [String: String]?; let ownership: Ownership?
    }
    struct Rect: Decodable { let x: Double; let y: Double; let width: Double; let height: Double }
    struct LayoutPane: Decodable { let pane_id: String; let rect: Rect }
    struct Split: Decodable { let id: String; let direction: String; let ratio: Double; let rect: Rect }
    struct Layout: Decodable { let tab_id: String; let area: Rect; let panes: [LayoutPane]; let splits: [Split]? }

    let workspaces: [Workspace]
    let tabs: [Tab]
    let panes: [Pane]
    let agents: [Agent]
    let layouts: [Layout]
}

/// A sidebar row. Kind follows the mock: ORCHESTRATOR / LANES / WORKFLOWS, with
/// workflows owned by a lane or orchestrator folded under their owner.
struct TabRow: Identifiable, Equatable {
    enum Kind: String { case orchestrator, lane, workflow }
    let id: String          // tab_id
    let label: String
    let kind: Kind
    let status: String      // working | idle | blocked | unknown
    let host: String
    let agent: String?
    var children: [TabRow]
}

final class HerdrModel: ObservableObject {
    @Published private(set) var snapshot: Snapshot?
    @Published private(set) var orchestrators: [TabRow] = []
    @Published private(set) var lanes: [TabRow] = []
    @Published private(set) var workflows: [TabRow] = []
    @Published private(set) var hosts: [(String, Int)] = []
    @Published private(set) var lastError: String?
    @Published private(set) var lastRefreshMs: Double = 0

    /// Fetch time of the latest `session.snapshot`, for the status line.
    var pollMs: Double { lastRefreshMs }

    let herdrBin: String
    let env: [String: String]
    let client: HerdrClient
    /// Per-host counts for the sidebar's hosts row (P12); slots and sessions come from the provider.
    let hostsModel: HostsModel
    /// lanes.json and areas.json (P15).
    let catalog = LaneCatalog()

    init(herdrBin: String, env: [String: String], hostsProvider: HostsProvider = HerdrOnlyHostsProvider()) {
        self.herdrBin = herdrBin
        self.env = env
        self.hostsModel = HostsModel(provider: hostsProvider)
        self.client = HerdrClient(socketPath: env["HERDR_SOCKET_PATH"] ?? "")
    }

    /// Live state by subscription: no timer, no `herdr api snapshot` process.
    /// A snapshot is read over the API socket on connect and on each event batch.
    func start() {
        client.onSnapshot = { [weak self] applied in
            guard let self else { return }
            lastRefreshMs = applied.fetchMs
            apply(applied.snapshot)
            lastError = nil
            // Evidence for scripts/check_p3.py: when each snapshot reached the model.
            log(String(format: "p3: applied epoch=%.3f tabs=%@", Date().timeIntervalSince1970,
                       applied.snapshot.tabs.map { $0.label ?? "" }.joined(separator: "|")))
        }
        client.onStatus = { [weak self] message in
            if let message { self?.lastError = message } else if self?.lastError != nil { self?.lastError = nil }
        }
        client.start()
        hostsModel.start()
        catalog.start()
        catalog.objectWillChange.receive(on: RunLoop.main).sink { [weak self] _ in
            self?.objectWillChange.send()
        }.store(in: &bag)
    }

    private var bag = Set<AnyCancellable>()

    private func apply(_ s: Snapshot) {
        snapshot = s
        let c = TabClassifier(s)
        var orch: [TabRow] = [], lane: [TabRow] = [], wf: [TabRow] = []
        var ownedBy: [String: [TabRow]] = [:]   // owner tab_id -> workflow rows
        for t in s.tabs.sorted(by: { ($0.workspace_id, $0.number) < ($1.workspace_id, $1.number) }) {
            let kind = c.kind(of: t)
            let r = c.row(t, kind)
            switch kind {
            case .workflow:
                if let ownerTab = c.ownerTab(of: t) { ownedBy[ownerTab, default: []].append(r) } else { wf.append(r) }
            case .orchestrator: orch.append(r)
            case .lane: lane.append(r)
            }
        }
        orch = orch.map { var r = $0; r.children = ownedBy[r.id] ?? []; return r }
        lane = lane.map { var r = $0; r.children = ownedBy[r.id] ?? []; return r }
        orchestrators = orch; lanes = lane; workflows = wf

        var h: [String: Int] = [:]
        for t in s.tabs { h[c.host(of: t), default: 0] += 1 }
        hosts = h.sorted { $0.key < $1.key }.map { ($0.key, $0.value) }
        hostsModel.update(tabCounts: hosts)
    }

    func layout(forTab tabId: String) -> Snapshot.Layout? {
        snapshot?.layouts.first { $0.tab_id == tabId }
    }

    func pane(_ paneId: String) -> Snapshot.Pane? {
        snapshot?.panes.first { $0.pane_id == paneId }
    }

    var allRowsInOrder: [TabRow] {
        func flat(_ r: [TabRow]) -> [TabRow] { r.flatMap { [$0] + $0.children } }
        return flat(orchestrators) + flat(lanes) + workflows
    }
}

/// Decides each tab's sidebar kind, host and owner (P4).
///
/// Tokens written by whoever starts the tab win: agent token `kind`
/// (orchestrator | lane | workflow) and `host`, plus `ownership` for folding.
/// A tab without a usable `kind` token falls back to the spike's label rules:
/// a `wf ` label or an ownership record means workflow, the first tab of a
/// workspace that has an agent is the orchestrator, everything else a lane.
struct TabClassifier {
    static let defaultHost = "Studio"

    private let snapshot: Snapshot
    private let agentsByTab: [String: [Snapshot.Agent]]
    private let tabByPane: [String: String]
    private let firstTab: [String: String]   // workspace_id -> tab_id with the lowest number

    init(_ s: Snapshot) {
        snapshot = s
        agentsByTab = Dictionary(grouping: s.agents, by: { $0.tab_id })
        tabByPane = Dictionary(s.panes.map { ($0.pane_id, $0.tab_id) }, uniquingKeysWith: { a, _ in a })
        firstTab = Dictionary(s.tabs.map { ($0.workspace_id, $0) }, uniquingKeysWith: { a, b in a.number <= b.number ? a : b })
            .mapValues { $0.tab_id }
    }

    /// First non-empty value of a token across the tab's agents, in snapshot order.
    func token(_ key: String, of t: Snapshot.Tab) -> String? {
        for a in agentsByTab[t.tab_id] ?? [] {
            if let v = a.tokens?[key]?.trimmingCharacters(in: .whitespacesAndNewlines), !v.isEmpty { return v }
        }
        return nil
    }

    /// The `kind` token, if present and one of the three known kinds.
    func taggedKind(of t: Snapshot.Tab) -> TabRow.Kind? {
        token("kind", of: t).flatMap { TabRow.Kind(rawValue: $0.lowercased()) }
    }

    /// Tab that owns this tab through an agent ownership record, if any and not itself.
    func ownerTab(of t: Snapshot.Tab) -> String? {
        for a in agentsByTab[t.tab_id] ?? [] {
            if let p = a.ownership?.current?.pane_id, let o = tabByPane[p], o != t.tab_id { return o }
        }
        return nil
    }

    private func hasOwnership(_ t: Snapshot.Tab) -> Bool {
        (agentsByTab[t.tab_id] ?? []).contains { $0.ownership?.current != nil }
    }

    func kind(of t: Snapshot.Tab) -> TabRow.Kind {
        if let k = taggedKind(of: t) { return k }
        if (t.label ?? "").hasPrefix("wf ") || hasOwnership(t) { return .workflow }
        if firstTab[t.workspace_id] == t.tab_id && !(agentsByTab[t.tab_id] ?? []).isEmpty { return .orchestrator }
        return .lane
    }

    func host(of t: Snapshot.Tab) -> String { token("host", of: t) ?? Self.defaultHost }

    func row(_ t: Snapshot.Tab, _ kind: TabRow.Kind) -> TabRow {
        let a = agentsByTab[t.tab_id]?.first
        return TabRow(id: t.tab_id, label: t.label ?? "tab \(t.number)", kind: kind,
                      status: a?.agent_status ?? t.agent_status ?? "unknown",
                      host: host(of: t), agent: a?.agent, children: [])
    }
}
