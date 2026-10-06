import Combine
import Foundation

/// Live herdr state: the `session.snapshot` shape (what `herdr api snapshot` prints), read by HerdrClient.
struct Snapshot: Decodable {
    struct Workspace: Decodable {
        let workspace_id: String; let label: String?; let number: Int; let orchestrator_mode: Bool?
        // P10: the space list follows herdr's own focus and tokens (`pinned`, `hidden`).
        let focused: Bool?; let active_tab_id: String?; let tokens: [String: String]?
    }
    /// `work_status` is herdr's one answer to "is this chat working" (server app/work_status.rs); older servers omit it.
    struct Tab: Decodable { let tab_id: String; let workspace_id: String; let label: String?; let number: Int; let agent_status: String?; let pane_count: Int?; let pin_index: Int?; var work_status: String? = nil; var role: String? = nil }
    struct Pane: Decodable {
        let pane_id: String; let tab_id: String; let terminal_id: String; let agent_status: String?; let focused: Bool?
        // Titles the quick switcher matches. Older snapshots omit them.
        let title: String?; let terminal_title: String?; let terminal_title_stripped: String?
    }
    struct Owner: Decodable { let pane_id: String? }
    struct Ownership: Decodable { let current: Owner? }
    struct Agent: Decodable {
        let pane_id: String; let tab_id: String; let agent: String?; let agent_status: String?; var work_status: String? = nil
        let tokens: [String: String]?; let ownership: Ownership?
        let title: String?; let terminal_title: String?; let terminal_title_stripped: String?
    }
    struct Rect: Decodable { let x: Double; let y: Double; let width: Double; let height: Double }
    struct LayoutPane: Decodable { let pane_id: String; let rect: Rect }
    struct Split: Decodable { let id: String; let direction: String; let ratio: Double; let rect: Rect }
    struct Layout: Decodable {
        let tab_id: String; let area: Rect; let panes: [LayoutPane]; let splits: [Split]?
        let zoomed: Bool?; let focused_pane_id: String?
    }

    let workspaces: [Workspace]
    let tabs: [Tab]
    let panes: [Pane]
    let agents: [Agent]
    let layouts: [Layout]
    let version: String?
    /// The server's wire protocol. Another machine whose protocol differs from the local
    /// server's cannot attach its terminals here, so its header says "needs update".
    let `protocol`: Int?
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
    @Published private(set) var spacesOverlay = Overlay()
    private var spacesOverlayMtime: Date?
    @Published private(set) var snapshot: Snapshot?
    @Published private(set) var orchestrators: [TabRow] = []
    @Published private(set) var lanes: [TabRow] = []
    @Published private(set) var workflows: [TabRow] = []
    @Published private(set) var hosts: [(String, Int)] = []
    @Published private(set) var lastError: String?
    @Published private(set) var lastRefreshMs: Double = 0
    /// Other machines, in config order. Never mixed into `snapshot`: local state stays local.
    @Published private(set) var machines: [MachineState] = Machines.configs.map { MachineState(name: $0.name) }
    private var machineClients: [HerdrClient] = []

    /// Fetch time of the latest `session.snapshot`, for the status line.
    var pollMs: Double { lastRefreshMs }

    /// Tabs whose pane changed into blocked or done since the app started, newest first.
    /// The first snapshot is the baseline: a tab that was already blocked is not a jump.
    private(set) var attentionTrail: [String] = []
    var latestAttentionTab: String? { attentionTrail.first }
    private var paneAttention: [String: String] = [:]
    private var attentionBaseline = false

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
        startMachines()
        hostsModel.start()
        catalog.start()
        Timer.publish(every: 1, on: .main, in: .common).autoconnect().sink { [weak self] _ in self?.reloadSpacesOverlay() }.store(in: &bag)
        catalog.objectWillChange.receive(on: RunLoop.main).sink { [weak self] _ in
            self?.objectWillChange.send()
        }.store(in: &bag)
    }

    private var bag = Set<AnyCancellable>()

    private func reloadSpacesOverlay() {
        let path = FactorySources.resolved().overlay
        let stamp = (try? FileManager.default.attributesOfItem(atPath: path)[.modificationDate]) as? Date
        if stamp != spacesOverlayMtime {
            spacesOverlayMtime = stamp
            spacesOverlay = (try? Data(contentsOf: URL(fileURLWithPath: path))).flatMap { try? JSONDecoder().decode(Overlay.self, from: $0) } ?? Overlay()
        }
    }

    private func apply(_ s: Snapshot) {
        reloadSpacesOverlay()
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
        trackAttention(s)
    }

    /// A pane moving into blocked or done while we are running. Kept to the last 20.
    private func trackAttention(_ s: Snapshot) {
        let agentsByPane = Dictionary(grouping: s.agents, by: { $0.pane_id })
        var next: [String: String] = [:]
        var hits: [String] = []
        for p in s.panes {
            let agents = agentsByPane[p.pane_id] ?? []
            let status = (agents.first?.agent_status ?? p.agent_status ?? "unknown").lowercased()
            var phase = ""
            for a in agents {
                if let v = a.tokens?["phase"]?.trimmingCharacters(in: .whitespacesAndNewlines).lowercased(), !v.isEmpty {
                    phase = v
                    break
                }
            }
            let cls: String
            if status == "blocked" { cls = "blocked" }
            else if status == "done" || phase == "done" || phase == "finished" { cls = "done" }
            else { cls = "other" }
            next[p.pane_id] = cls
            if attentionBaseline, let prev = paneAttention[p.pane_id], prev != cls, (cls == "blocked" || cls == "done") {
                hits.append(p.tab_id)
                log("attention: \(p.tab_id) pane=\(p.pane_id) \(prev)->\(cls)")
            }
        }
        paneAttention = next
        if !attentionBaseline {
            attentionBaseline = true
            return
        }
        for tab in hits { attentionTrail.insert(tab, at: 0) }
        if attentionTrail.count > 20 { attentionTrail.removeLast(attentionTrail.count - 20) }
    }

    func layout(forTab tabId: String) -> Snapshot.Layout? {
        source(for: tabId)?.layouts.first { $0.tab_id == tabId }
    }

    func pane(_ paneId: String) -> Snapshot.Pane? {
        source(for: paneId)?.panes.first { $0.pane_id == paneId }
    }

    /// The snapshot that owns an id: the local one, or the machine named in it.
    func source(for id: String) -> Snapshot? {
        guard let name = Machines.split(id)?.machine else { return snapshot }
        return machines.first { $0.name == name }?.snapshot
    }

    func hasTab(_ tabId: String) -> Bool {
        source(for: tabId)?.tabs.contains { $0.tab_id == tabId } == true
    }

    /// True once the machine that owns a remote id has answered at least once.
    func machineLoaded(for id: String) -> Bool {
        // A machine no longer configured never answers; treat it as loaded so the selection falls back.
        guard let name = Machines.split(id)?.machine, let m = machines.first(where: { $0.name == name }) else { return true }
        return m.snapshot != nil
    }

    private func startMachines() {
        for c in Machines.configs {
            let client = HerdrClient(socketPath: c.socket, machine: c.name)
            client.onSnapshot = { [weak self] applied in
                self?.updateMachine(c.name) { m in
                    m.snapshot = applied.snapshot; m.problem = nil; m.downSince = nil; m.epoch += 1
                }
            }
            client.onStatus = { [weak self] message in
                guard let message else { return }
                self?.updateMachine(c.name) { m in
                    if m.problem == nil { m.downSince = Date() }
                    m.problem = message
                }
            }
            client.start()
            machineClients.append(client)
        }
    }

    private func updateMachine(_ name: String, _ change: (inout MachineState) -> Void) {
        guard let i = machines.firstIndex(where: { $0.name == name }) else { return }
        var m = machines[i]
        change(&m)
        if m != machines[i] { machines[i] = m }
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
                      status: t.work_status ?? a?.agent_status ?? t.agent_status ?? "unknown",
                      host: host(of: t), agent: a?.agent, children: [])
    }
}


extension HerdrModel {
    /// The shared pin order owns the numbered slots on every surface.
    func numberedTabIds(state: SidebarState) -> [String] {
        let tabs = ([snapshot].compactMap { $0 } + machines.compactMap(\.snapshot)).flatMap { $0.tabs }.map {
            SpacesInput.Tab(id: $0.tab_id, space: $0.workspace_id, label: $0.label ?? $0.tab_id, pinIndex: $0.pin_index, role: $0.role)
        }
        let pins = [true, false].flatMap { agents in
            PinDrag.shared.ordered(SpacesTree.pinTabs(tabs, agents: agents).map(\.id), section: agents ? "agents" : "pinned")
        }
        var seen = Set<String>()
        let displayed = spacesRows(state: state).filter { $0.kind == .tab }.compactMap(\.tab)
        return (pins + displayed + allRowsInOrder.map(\.id)).filter { seen.insert($0).inserted }
    }

    func isAgent(_ tab: String) -> Bool {
        source(for: tab)?.tabs.first { $0.tab_id == tab }?.role == "agent"
    }

    func setAgentRole(_ tab: String, _ on: Bool) {
        let commands = HerdrCommands(socketPath: env["HERDR_SOCKET_PATH"] ?? "")
        DispatchQueue.global(qos: .userInitiated).async { _ = commands.tabSetRole(tabId: tab, role: on ? "agent" : nil) }
    }

    func isPinned(_ tab: String) -> Bool {
        source(for: tab)?.tabs.first { $0.tab_id == tab }?.pin_index != nil
    }

    /// Pins or unpins on the server that owns the tab: HerdrCommands sends a remote id to its machine.
    func togglePin(_ tab: String) {
        let pinned = isPinned(tab)
        let commands = HerdrCommands(socketPath: env["HERDR_SOCKET_PATH"] ?? "")
        DispatchQueue.global(qos: .userInitiated).async { _ = commands.tabSetPinned(tabId: tab, pinned: !pinned) }
    }

    /// A new chat pinned at the end of PINNED, in the focused chat's space (a local space when
    /// nothing is focused), then `done` with its id once it is pinned. Alex, 2026-10-06: "i need
    /// a button next to pinned to make a new tab that's pinned please".
    func newPinnedTab(focused: String?, done: @escaping (String) -> Void) {
        let workspace = focused.flatMap { id in source(for: id)?.tabs.first { $0.tab_id == id }?.workspace_id }
            ?? snapshot?.workspaces.first?.workspace_id
        guard let workspace else { return }
        let commands = HerdrCommands(socketPath: env["HERDR_SOCKET_PATH"] ?? "")
        DispatchQueue.global(qos: .userInitiated).async {
            guard let made = commands.tabCreate(workspaceId: workspace, cwd: nil) else { log("new pinned tab failed"); return }
            guard commands.tabSetPinned(tabId: made.tabId, pinned: true) else { log("new pinned tab: pin failed \(made.tabId)"); return }
            // A pin lands by priority, so one dragged to the end can still sit after it. The end is
            // read from the owning server after the pin, not from a snapshot that may be behind.
            if let pins = commands.pinCount(near: made.tabId), pins > 0, !commands.tabPinMove(tabId: made.tabId, pinIndex: pins - 1) {
                log("new pinned tab: pin_move failed \(made.tabId)")
            }
            DispatchQueue.main.async { done(made.tabId) }
        }
    }

    func spacesRows(state: SidebarState) -> [SpacesRow] {
        guard let s = snapshot else { return [SpacesRow(id: "agents", kind: .title, title: "agents")] }
        let input = SpacesInput(spaces: s.workspaces.map {
            SpacesInput.Space(id: $0.workspace_id, name: $0.label ?? $0.workspace_id, pinned: $0.tokens?["pinned"] == "true", collapsed: $0.tokens?["hidden"] == "true")
        }, tabs: s.tabs.map { tab in
            SpacesInput.Tab(id: tab.tab_id, space: tab.workspace_id, label: tab.label ?? tab.tab_id,
                agents: s.agents.filter { $0.tab_id == tab.tab_id }.map { agent in
                    let parentPane = agent.tokens?["parent_pane_id"] ?? agent.ownership?.current?.pane_id
                    let parentTab = s.agents.first { $0.pane_id == parentPane }?.tab_id
                    return SpacesInput.Agent(status: agent.agent_status ?? "unknown", parent: parentTab)
                },
                focused: tab.tab_id == state.selectedTab, status: tab.agent_status ?? "unknown", pinIndex: tab.pin_index, work: tab.work_status, role: tab.role)
        }, focusedTab: state.selectedTab)
        // areas.json owns the space groups whenever it exists (an empty list clears them), as the Rust
        // server merges them; without it the overlay's own groups stand.
        var groupedOverlay = spacesOverlay
        if let groups = catalog.snapshot.spaceGroups { groupedOverlay.spaceGroups = groups }
        // Other machines' chats join their spaces; each carries its machine's badge (MachineMerge).
        let remote = MachineRows.inputs(machines, localProtocol: snapshot?.protocol)
        var rows = SpacesTree.build(MachineMerge.merge(input, machines: remote), overlay: groupedOverlay,
                                    chrome: state.spacesChrome, now: Date().timeIntervalSince1970)
        rows = PinDrag.shared.reorder(rows)
        return MachineRows.renameHosts(MachineMerge.badge(rows, machines: remote), machines: machines.map(\.name))
    }
}
