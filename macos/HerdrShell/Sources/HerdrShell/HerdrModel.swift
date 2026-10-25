import Combine
import Foundation

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
    let thoughts = ThoughtsModel()
    @Published private(set) var spacesOverlay = Overlay()
    private let overlayMonitor = FileChangeMonitor()
    @Published private(set) var snapshot: Snapshot?
    @Published private(set) var orchestrators: [TabRow] = []
    @Published private(set) var lanes: [TabRow] = []
    @Published private(set) var workflows: [TabRow] = []
    @Published private(set) var hosts: [(String, Int)] = []
    @Published private(set) var lastError: String?
    @Published private(set) var lastRefreshMs: Double = 0
    @Published private(set) var machines: [MachineState] = Machines.configs.map { MachineState(name: $0.name) }
    private var machineClients: [HerdrClient] = []
    private var afterMachineLoad: [(String, () -> Void)] = []

    var pollMs: Double { lastRefreshMs }

    private(set) var attentionTrail: [String] = []
    var latestAttentionTab: String? {
        attentionTrail.first { tab in
            guard let row = source(for: tab)?.tabs.first(where: { $0.tab_id == tab }) else { return true }
            return !(row.role == "agent" && (row.hidden ?? false))
        }
    }
    private var paneAttention: [String: String] = [:]
    private var attentionBaseline = false

    let herdrBin: String
    let env: [String: String]
    let client: HerdrClient
    let hostsModel: HostsModel
    let catalog = LaneCatalog()

    init(herdrBin: String, env: [String: String], hostsProvider: HostsProvider = HerdrOnlyHostsProvider()) {
        self.herdrBin = herdrBin
        self.env = env
        self.hostsModel = HostsModel(provider: hostsProvider)
        self.client = HerdrClient(socketPath: env["HERDR_SOCKET_PATH"] ?? "")
    }

    func start() {
        client.onSnapshot = { [weak self] applied in
            guard let self else { return }
            lastRefreshMs = applied.fetchMs
            apply(applied.snapshot)
            lastError = nil
            log(String(format: "p3: applied epoch=%.3f tabs=%@", Date().timeIntervalSince1970,
                       applied.snapshot.tabs.map { $0.label ?? "" }.joined(separator: "|")))
        }
        client.onStatus = { [weak self] message in
            if let message { self?.lastError = message } else if self?.lastError != nil { self?.lastError = nil }
        }
        client.start()
        startMachines()
        hostsModel.start()
        thoughts.start()
        thoughts.objectWillChange.receive(on: RunLoop.main).sink { [weak self] _ in
            self?.objectWillChange.send()
        }.store(in: &bag)
        catalog.start()
        let home = TerminalTheme.realHome()
        let raw = ProcessInfo.processInfo.environment["FACTORY_OVERLAY"]?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        let path = raw.isEmpty ? home + "/.agent-rails/herdr/overlay.json" : (raw as NSString).expandingTildeInPath
        var previous: Data?
        overlayMonitor.start(paths: { [path] }) { [weak self] in
            let overlay = (try? Data(contentsOf: URL(fileURLWithPath: path)))
                .flatMap { try? JSONDecoder().decode(Overlay.self, from: $0) } ?? Overlay()
            let encoder = JSONEncoder()
            encoder.outputFormatting = .sortedKeys
            let encoded = try? encoder.encode(overlay)
            guard encoded != previous else { return }
            previous = encoded
            DispatchQueue.main.async { [weak self] in self?.spacesOverlay = overlay }
        }
        catalog.objectWillChange.receive(on: RunLoop.main).sink { [weak self] _ in
            self?.objectWillChange.send()
        }.store(in: &bag)
    }

    private var bag = Set<AnyCancellable>()

    private var localEpoch = 0
    func snapshotEpoch(for id: String) -> Int {
        guard let name = Machines.split(id)?.machine else { return localEpoch }
        return machines.first { $0.name == name }?.epoch ?? 0
    }

    private func apply(_ s: Snapshot) {
        localEpoch += 1
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

    func source(for id: String) -> Snapshot? {
        guard let name = Machines.split(id)?.machine else { return snapshot }
        return machines.first { $0.name == name }?.snapshot
    }

    func hasTab(_ tabId: String) -> Bool {
        source(for: tabId)?.tabs.contains { $0.tab_id == tabId } == true
    }

    func whenMachineLoaded(_ id: String, _ action: @escaping () -> Void) {
        if machineLoaded(for: id) { action() } else { afterMachineLoad.append((id, action)) }
    }

    func machineLoaded(for id: String) -> Bool {
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
        guard !afterMachineLoad.isEmpty else { return }
        let ready = afterMachineLoad.filter { machineLoaded(for: $0.0) }
        afterMachineLoad.removeAll { machineLoaded(for: $0.0) }
        ready.forEach { $0.1() }
    }

    var allRowsInOrder: [TabRow] {
        func flat(_ r: [TabRow]) -> [TabRow] { r.flatMap { [$0] + $0.children } }
        return flat(orchestrators) + flat(lanes) + workflows
    }
}

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

    func token(_ key: String, of t: Snapshot.Tab) -> String? {
        for a in agentsByTab[t.tab_id] ?? [] {
            if let v = a.tokens?[key]?.trimmingCharacters(in: .whitespacesAndNewlines), !v.isEmpty { return v }
        }
        return nil
    }

    func taggedKind(of t: Snapshot.Tab) -> TabRow.Kind? {
        token("kind", of: t).flatMap { TabRow.Kind(rawValue: $0.lowercased()) }
    }

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
    func numberedTabIds(state: SidebarState) -> [String] {
        let tabs = ([snapshot].compactMap { $0 } + machines.compactMap(\.snapshot)).flatMap { $0.tabs }.map {
            SpacesInput.Tab(id: $0.tab_id, space: $0.workspace_id, label: $0.label ?? $0.tab_id, pinIndex: $0.pin_index, role: $0.role, hidden: $0.hidden ?? false, homeLocation: $0.home_location)
        }
        return SpacesTree.numbered(tabs, rows: spacesRows(state: state), rest: allRowsInOrder.map(\.id))
    }

    func isAgent(_ tab: String) -> Bool {
        source(for: tab)?.tabs.first { $0.tab_id == tab }?.role == "agent"
    }

    func setAgentRole(_ tab: String, _ on: Bool) {
        let commands = HerdrCommands(socketPath: env["HERDR_SOCKET_PATH"] ?? "")
        DispatchQueue.global(qos: .userInitiated).async { _ = commands.tabSetRole(tabId: tab, role: on ? "agent" : nil) }
    }

    func setAgentHidden(_ tab: String, _ hidden: Bool, done: ((Bool) -> Void)? = nil) {
        let commands = HerdrCommands(socketPath: env["HERDR_SOCKET_PATH"] ?? "")
        DispatchQueue.global(qos: .userInitiated).async {
            let ok = commands.tabSetHidden(tabId: tab, hidden: hidden)
            if let done { DispatchQueue.main.async { done(ok) } }
        }
    }

    var settingsInput: SpacesInput {
        guard let s = snapshot else { return SpacesInput(spaces: [], tabs: [], focusedTab: nil) }
        return SpacesInput(spaces: Self.spaces(s), tabs: s.tabs.map {
            SpacesInput.Tab(id: $0.tab_id, space: $0.workspace_id, label: $0.label ?? $0.tab_id, pinIndex: $0.pin_index,
                            role: $0.role, hidden: $0.hidden ?? false, homeLocation: $0.home_location, runsOn: $0.runs_on, agentName: $0.agent_card?.name, avatar: $0.agent_card?.avatar_url)
        }, focusedTab: nil)
    }

    func isPinned(_ tab: String) -> Bool {
        source(for: tab)?.tabs.first { $0.tab_id == tab }?.pin_index != nil
    }

    func reviewDoc(_ tab: String) -> TabDoc? {
        source(for: tab)?.tabs.first { $0.tab_id == tab }?.doc
    }

    func setPinnedAtEnd(_ tab: String, _ on: Bool) {
        let commands = HerdrCommands(socketPath: env["HERDR_SOCKET_PATH"] ?? "")
        DispatchQueue.global(qos: .userInitiated).async {
            if on { _ = commands.pinAtEnd(tabId: tab) } else if !commands.tabSetPinned(tabId: tab, pinned: false) { log("unpin failed \(tab)") }
        }
    }

    func togglePin(_ tab: String) {
        let pinned = isPinned(tab)
        let commands = HerdrCommands(socketPath: env["HERDR_SOCKET_PATH"] ?? "")
        DispatchQueue.global(qos: .userInitiated).async { _ = commands.tabSetPinned(tabId: tab, pinned: !pinned) }
    }

    func newPinnedTab(focused: String?, done: @escaping (String) -> Void) {
        if let focused, !machineLoaded(for: focused) {
            whenMachineLoaded(focused) { [weak self] in self?.newPinnedTab(focused: focused, done: done) }
            return
        }
        let workspace = focused.flatMap { id in source(for: id)?.tabs.first { $0.tab_id == id }?.workspace_id }
            ?? snapshot?.workspaces.first?.workspace_id
        guard let workspace else { return }
        let commands = HerdrCommands(socketPath: env["HERDR_SOCKET_PATH"] ?? "")
        DispatchQueue.global(qos: .userInitiated).async {
            guard let made = commands.tabCreate(workspaceId: workspace, cwd: nil) else { log("new pinned tab failed"); return }
            guard commands.pinAtEnd(tabId: made.tabId) else { return }
            DispatchQueue.main.async { done(made.tabId) }
        }
    }

    private static func spaces(_ s: Snapshot) -> [SpacesInput.Space] {
        s.workspaces.map {
            SpacesInput.Space(id: $0.workspace_id, name: $0.label ?? $0.workspace_id, pinned: $0.tokens?["pinned"] == "true", collapsed: $0.tokens?["hidden"] == "true", sortRank: $0.sort_rank ?? 0, parked: $0.parked ?? false)
        }
    }

    func displayedSpace(of tabId: String) -> SpacesInput.Space? {
        guard let s = snapshot else { return nil }
        let local = SpacesInput(spaces: Self.spaces(s), tabs: s.tabs.map { SpacesInput.Tab(id: $0.tab_id, space: $0.workspace_id, label: $0.label ?? $0.tab_id) }, focusedTab: nil)
        return MachineMerge.space(of: tabId, local: local, machines: MachineRows.inputs(machines, localProtocol: s.protocol))
    }

    var sidebarSpaces: [SpacesInput.Space] {
        guard let snapshot else { return [] }
        let local = SpacesInput(spaces: Self.spaces(snapshot), tabs: [], focusedTab: nil)
        return MachineMerge.merge(local, machines: MachineRows.inputs(machines, localProtocol: snapshot.protocol)).spaces
    }

    func spacesRows(state: SidebarState) -> [SpacesRow] {
        guard let s = snapshot else { return [SpacesRow(id: "agents", kind: .title, title: "agents")] }
        var input = SpacesInput(spaces: Self.spaces(s), tabs: s.tabs.map { tab in
            SpacesInput.Tab(id: tab.tab_id, space: tab.workspace_id, label: tab.label ?? tab.tab_id,
                agents: s.agents.filter { $0.tab_id == tab.tab_id }.map { agent in
                    let parentPane = agent.tokens?["parent_pane_id"] ?? agent.ownership?.current?.pane_id
                    let parentTab = s.agents.first { $0.pane_id == parentPane }?.tab_id
                    let group = agent.group
                    let explicitTab = group?.parent?.pane_id.flatMap { pane in s.agents.first { $0.pane_id == pane }?.tab_id }
                    return SpacesInput.Agent(status: agent.agent_status ?? "unknown", parent: parentTab, explicitParent: group?.placement == "under" ? explicitTab : nil, handsOn: group?.placement == "hands_on")
                },
                focused: tab.tab_id == state.selectedTab, status: tab.agent_status ?? "unknown", pinIndex: tab.pin_index, work: tab.work_status, role: tab.role, sortRank: tab.sort_rank ?? 0,
request: s.panes.filter { $0.tab_id == tab.tab_id }.compactMap { $0.tokens?["request"] }.first, hidden: tab.hidden ?? false, homeLocation: tab.home_location, runsOn: tab.runs_on, home: tab.home, agentName: tab.agent_card?.name, avatar: tab.agent_card?.avatar_url)
        }, focusedTab: state.selectedTab, automations: s.automations)
        var groupedOverlay = spacesOverlay
        for tab in s.tabs {
            if let home = tab.home {
                var tag = groupedOverlay.tabs[tab.tab_id] ?? Overlay.Tag()
                tag.home = home
                groupedOverlay.tabs[tab.tab_id] = tag
            }
        }
        if let groups = catalog.snapshot.spaceGroups { groupedOverlay.spaceGroups = groups }
        let remote = MachineRows.inputs(machines, localProtocol: snapshot?.protocol)
        var rows = SpacesTree.build(MachineMerge.merge(input, machines: remote), overlay: groupedOverlay,
                                    chrome: state.spacesChrome, now: Date().timeIntervalSince1970)
        return MachineRows.renameHosts(MachineMerge.badge(rows, machines: remote), machines: machines.map(\.name))
    }
}
