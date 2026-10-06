import Foundation

/// Other machines' herdr servers, whose chats join the local spaces tree (MachineMerge).
///
/// Each machine is a pair of sockets that `herdr-machine-tunnels` keeps forwarded
/// (`~/.config/herdr-machines/<name>/herdr.sock` and `herdr-client.sock`). The shell
/// never touches herdr's own endpoint catalog; local rows draw as they do with no machines,
/// and a remote chat adds only its own row and badge.
///
/// Remote ids carry the machine name: `ax42/w1:t2`, `ax42/term_65cc…`. Local ids never
/// contain `/`, so one string says which server owns a tab, pane or terminal.
enum Machines {
    struct Config: Equatable {
        let name: String
        let socket: String
        let clientSocket: String
    }

    /// Read once at startup, before main.swift clears HERDR_*.
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
            // One name, one server: ids carry only the name, so a duplicate would misroute commands.
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

    /// `ax42/w1:t2` -> ("ax42", "w1:t2"); a local id -> nil.
    static func split(_ id: String) -> (machine: String, raw: String)? {
        guard let slash = id.firstIndex(of: "/"), slash != id.startIndex else { return nil }
        return (String(id[..<slash]), String(id[id.index(after: slash)...]))
    }

    static func isRemote(_ id: String) -> Bool { split(id) != nil }

    static func config(for id: String) -> Config? {
        guard let name = split(id)?.machine else { return nil }
        return configs.first { $0.name == name }
    }

    /// What `herdr terminal attach` needs to reach a remote terminal: its raw id and sockets.
    static func attachTarget(_ terminalId: String) -> (raw: String, env: [String: String])? {
        guard let (_, raw) = split(terminalId), let c = config(for: terminalId) else { return nil }
        return (raw, ["HERDR_SOCKET_PATH": c.socket, "HERDR_CLIENT_SOCKET_PATH": c.clientSocket])
    }

    /// Keys whose string values are herdr ids, wherever they appear in a snapshot.
    static let idKeys: Set<String> = [
        "workspace_id", "tab_id", "pane_id", "terminal_id", "active_tab_id",
        "focused_workspace_id", "focused_tab_id", "focused_pane_id", "parent_pane_id",
    ]

    /// Puts the machine name on every id in a `session.snapshot` reply and tags each
    /// agent with `host`, so the rest of the shell can hold remote rows next to local ones.
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

/// One remote machine's live state, as the sidebar shows it.
struct MachineState: Identifiable, Equatable {
    var id: String { name }
    let name: String
    /// Namespaced snapshot; nil until the first one arrives.
    var snapshot: Snapshot?
    /// nil while connected; otherwise why not.
    var problem: String?
    var downSince: Date?

    static func == (a: MachineState, b: MachineState) -> Bool {
        a.name == b.name && a.problem == b.problem && a.downSince == b.downSince && a.epoch == b.epoch
    }
    /// Bumped on every applied snapshot, so equality notices new state without comparing it.
    var epoch = 0
}

/// Other machines as MachineMerge input: their spaces and tabs, ids already namespaced, and
/// each machine's health for its badge. No machine has rows of its own.
enum MachineRows {
    static func inputs(_ machines: [MachineState], localProtocol: Int? = nil) -> [MachineMerge.Machine] {
        machines.compactMap { m in
            guard let s = m.snapshot else { return nil }
            // Only a known protocol on both sides is a mismatch; an unknown one says nothing.
            let mismatch = localProtocol.flatMap { local in s.protocol.map { $0 != local } } ?? false
            let health = m.problem != nil ? "unreachable" : mismatch ? "needs update" : nil
            return MachineMerge.Machine(name: m.name, health: health, spaces: s.workspaces.map {
                SpacesInput.Space(id: $0.workspace_id, name: $0.label ?? $0.workspace_id,
                                  pinned: $0.tokens?["pinned"] == "true", collapsed: $0.tokens?["hidden"] == "true")
            }, tabs: s.tabs.map { tab in
                SpacesInput.Tab(id: tab.tab_id, space: tab.workspace_id, label: tab.label ?? "tab \(tab.number)",
                    agents: s.agents.filter { $0.tab_id == tab.tab_id }.map { agent in
                        let parentPane = agent.tokens?["parent_pane_id"] ?? agent.ownership?.current?.pane_id
                        return SpacesInput.Agent(status: agent.agent_status ?? "unknown",
                                                 parent: s.agents.first { $0.pane_id == parentPane }?.tab_id)
                    }, status: tab.agent_status ?? "unknown", pinIndex: tab.pin_index, work: tab.work_status, role: tab.role)
            })
        }
    }

    /// One name per machine: a host footer row that names a machine in another case
    /// ("PC" for "pc") takes the machine's name, so the two blocks agree. Machines are keyed by
    /// their exact name: a row that names one exactly keeps it, and a case-only match renames
    /// only when it is the one machine and the one row of that spelling, so `pc` and `PC` stay two.
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
