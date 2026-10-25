import Foundation

enum Machines {
    struct Config: Equatable {
        let name: String
        let socket: String
        let clientSocket: String
    }

    private(set) static var configs: [Config] = []

    static func configure(env: [String: String]) {
        let root = ((env["HERDR_MACHINES_DIR"] ?? "~/.config/herdr-machines") as NSString).expandingTildeInPath
        let path = env["HERDR_SHELL_MACHINES"] ?? (root + "/tunnels.json")
        guard let data = FileManager.default.contents(atPath: path),
              let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let list = obj["machines"] as? [[String: Any]] else { return }
        var seen = Set<String>()
        configs = list.compactMap { m in
            guard let name = m["name"] as? String, !name.isEmpty, !name.contains("/"), !name.contains(":"),
                  m["enabled"] as? Bool != false else { return nil }
            guard seen.insert(name).inserted else { log("machines: duplicate \(name) ignored"); return nil }
            let dir = ((m["dir"] as? String) ?? root + "/" + name) as NSString
            let socket = (m["socket"] as? String).map { ($0 as NSString).expandingTildeInPath }
                ?? dir.expandingTildeInPath + "/herdr.sock"
            let client = (m["client_socket"] as? String).map { ($0 as NSString).expandingTildeInPath }
                ?? (socket as NSString).deletingLastPathComponent + "/herdr-client.sock"
            return Config(name: name, socket: socket, clientSocket: client)
        }
        if !configs.isEmpty { log("machines: " + configs.map(\.name).joined(separator: ", ")) }
    }

    static func prefix(_ name: String) -> String { name + "/" }

    static func split(_ id: String) -> (machine: String, raw: String)? {
        guard let slash = id.firstIndex(of: "/"), slash != id.startIndex else { return nil }
        return (String(id[..<slash]), String(id[id.index(after: slash)...]))
    }

    static func isRemote(_ id: String) -> Bool { split(id) != nil }

    static func config(for id: String) -> Config? {
        guard let name = split(id)?.machine else { return nil }
        return configs.first { $0.name == name }
    }

    static func attachTarget(_ terminalId: String) -> (raw: String, env: [String: String])? {
        guard let (_, raw) = split(terminalId), let c = config(for: terminalId) else { return nil }
        return (raw, ["HERDR_SOCKET_PATH": c.socket, "HERDR_CLIENT_SOCKET_PATH": c.clientSocket])
    }

    static let idKeys: Set<String> = [
        "workspace_id", "tab_id", "pane_id", "terminal_id", "active_tab_id",
        "focused_workspace_id", "focused_tab_id", "focused_pane_id", "parent_pane_id",
    ]

    static func namespace(_ data: Data, machine: String) -> Data {
        guard let obj = try? JSONSerialization.jsonObject(with: data) else { return data }
        let p = prefix(machine)
        func walk(_ v: Any, key: String?) -> Any {
            if let s = v as? String, let key, idKeys.contains(key), !s.isEmpty { return p + s }
            if let a = v as? [Any] { return a.map { walk($0, key: key == "agents" ? "agent" : nil) } }
            if var d = v as? [String: Any] {
                for (k, x) in d { d[k] = walk(x, key: k) }
                if key == "agent" {
                    var tokens = d["tokens"] as? [String: Any] ?? [:]
                    if tokens["host"] == nil { tokens["host"] = machine }
                    d["tokens"] = tokens
                }
                return d
            }
            return v
        }
        let out = walk(obj, key: nil)
        return (try? JSONSerialization.data(withJSONObject: out)) ?? data
    }
}

struct MachineState: Identifiable, Equatable {
    var id: String { name }
    let name: String
    var snapshot: Snapshot?
    var problem: String?
    var downSince: Date?

    static func == (a: MachineState, b: MachineState) -> Bool {
        a.name == b.name && a.problem == b.problem && a.downSince == b.downSince && a.epoch == b.epoch
    }
    var epoch = 0
}

enum MachineRows {
    static func inputs(_ machines: [MachineState], localProtocol: Int? = nil) -> [MachineMerge.Machine] {
        machines.compactMap { m in
            guard let s = m.snapshot else { return nil }
            let mismatch = localProtocol.flatMap { local in s.protocol.map { $0 != local } } ?? false
            let health = m.problem != nil ? "unreachable" : mismatch ? "needs update" : nil
            return MachineMerge.Machine(name: m.name, health: health, spaces: s.workspaces.map {
                SpacesInput.Space(id: $0.workspace_id, name: $0.label ?? $0.workspace_id,
                                  pinned: $0.tokens?["pinned"] == "true", collapsed: $0.tokens?["hidden"] == "true", sortRank: $0.sort_rank ?? 0, parked: $0.parked ?? false)
            }, tabs: s.tabs.map { tab in
                SpacesInput.Tab(id: tab.tab_id, space: tab.workspace_id, label: tab.label ?? "tab \(tab.number)",
                    agents: s.agents.filter { $0.tab_id == tab.tab_id }.map { agent in
                        let parentPane = agent.tokens?["parent_pane_id"] ?? agent.ownership?.current?.pane_id
                        let group = agent.group
                        let explicitTab = group?.parent?.pane_id.flatMap { pane in s.agents.first { $0.pane_id == pane }?.tab_id }
                        return SpacesInput.Agent(status: agent.agent_status ?? "unknown",
                                                 parent: s.agents.first { $0.pane_id == parentPane }?.tab_id,
                                                 explicitParent: group?.placement == "under" ? explicitTab : nil,
                                                 handsOn: group?.placement == "hands_on")
                    }, status: tab.agent_status ?? "unknown", pinIndex: tab.pin_index, work: tab.work_status, role: tab.role, sortRank: tab.sort_rank ?? 0,
request: s.panes.filter { $0.tab_id == tab.tab_id }.compactMap { $0.tokens?["request"] }.first, hidden: tab.hidden ?? false, homeLocation: tab.home_location, runsOn: tab.runs_on, home: tab.home, agentName: tab.agent_card?.name, avatar: tab.agent_card?.avatar_url)
            })
        }
    }

    static func renameHosts(_ rows: [SpacesRow], machines: [String]) -> [SpacesRow] {
        func folds(_ a: String, _ b: String) -> Bool { a.caseInsensitiveCompare(b) == .orderedSame }
        let hosts = rows.filter { $0.kind == .footerHost }.map(\.title)
        return rows.map { row in
            guard row.kind == .footerHost, !machines.contains(row.title) else { return row }
            let candidates = machines.filter { folds($0, row.title) }
            guard candidates.count == 1, let name = candidates.first,
                  !hosts.contains(name), hosts.filter({ folds($0, name) }).count == 1 else { return row }
            var r = row
            r.title = name
            return r
        }
    }
}
