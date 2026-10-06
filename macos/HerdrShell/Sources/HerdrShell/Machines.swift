import Foundation

/// Other machines' herdr servers, shown beside the local one.
///
/// Each machine is a pair of sockets that `herdr-machine-tunnels` keeps forwarded
/// (`~/.config/herdr-machines/<name>/herdr.sock` and `herdr-client.sock`). The shell
/// never touches herdr's own endpoint catalog, so the local session draws exactly as
/// it does with no machines.
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
        configs = list.compactMap { m in
            guard let name = m["name"] as? String, !name.isEmpty, !name.contains("/"), !name.contains(":"),
                  m["enabled"] as? Bool != false else { return nil }
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

/// Sidebar rows for other machines, below the local spaces.
///
/// A machine header (open by default; its fold is remembered), then each workspace
/// with its agent tabs. Tabs with no agent fold into one quiet "shells N" row, so a
/// machine full of job shells reads as one line.
enum MachineRows {
    static func build(_ machines: [MachineState], chrome: SpacesChrome) -> [SpacesRow] {
        var out = [SpacesRow(id: "machines", kind: .title, title: "machines")]
        let priority = ["unknown": 0, "idle": 1, "done": 2, "working": 3, "blocked": 4]
        func glyph(_ status: String) -> String { status == "blocked" ? "■" : status == "idle" || status == "unknown" ? "○" : "●" }
        for m in machines {
            let open = !chrome.collapsedMachines.contains(m.name)
            let s = m.snapshot
            let agentsByTab = Dictionary(grouping: s?.agents ?? [], by: \.tab_id)
            func status(_ tab: Snapshot.Tab) -> String {
                (agentsByTab[tab.tab_id] ?? []).map { $0.agent_status ?? "unknown" }
                    .max { (priority[$0] ?? 0) < (priority[$1] ?? 0) } ?? tab.agent_status ?? "unknown"
            }
            let agentTabs = (s?.tabs ?? []).filter { agentsByTab[$0.tab_id] != nil }
            let top = agentTabs.map(status).max { (priority[$0] ?? 0) < (priority[$1] ?? 0) } ?? "unknown"
            let trailing: String
            if s == nil {
                trailing = "connecting"
            } else if m.problem != nil {
                trailing = "offline" + (m.downSince.map { " since " + Self.clock.string(from: $0) } ?? "")
            } else if open {
                trailing = s?.version.map { "herdr " + $0 } ?? ""
            } else {
                trailing = agentTabs.isEmpty ? "no agents" : "\(agentTabs.count) agent" + (agentTabs.count == 1 ? "" : "s")
            }
            out.append(SpacesRow(id: "machine:" + m.name, kind: .machine, chevron: open ? "open" : "closed",
                                 glyph: open || agentTabs.isEmpty ? "" : glyph(top), tone: top, title: m.name,
                                 trailing: trailing, toggleKey: "machine:" + m.name, dim: m.problem != nil))
            guard open, let s else { continue }
            for ws in s.workspaces.sorted(by: { $0.number < $1.number }) {
                let tabs = s.tabs.filter { $0.workspace_id == ws.workspace_id }.sorted { $0.number < $1.number }
                guard !tabs.isEmpty else { continue }
                out.append(SpacesRow(id: "msection:" + ws.workspace_id, kind: .section, depth: 1,
                                     title: (ws.label ?? ws.workspace_id).uppercased(), dim: m.problem != nil))
                for tab in tabs where agentsByTab[tab.tab_id] != nil {
                    let st = status(tab)
                    let quiet = st == "idle" || st == "unknown" || st == "done"
                    out.append(SpacesRow(id: "tab:" + tab.tab_id, kind: .tab, depth: 1, glyph: glyph(st), tone: st,
                                         title: tab.label ?? "tab \(tab.number)", trailing: st == "done" ? "done" : "",
                                         tab: tab.tab_id, dim: quiet || m.problem != nil))
                }
                let shells = tabs.filter { agentsByTab[$0.tab_id] == nil }
                guard !shells.isEmpty else { continue }
                let key = ws.workspace_id + ":shells"
                let shown = chrome.expandedGroups.contains(key)
                out.append(SpacesRow(id: "group:" + key, kind: .group, depth: 2, chevron: shown ? "open" : "closed",
                                     title: "shells \(shells.count)", toggleKey: "group:" + key, dim: true))
                if shown {
                    for tab in shells {
                        out.append(SpacesRow(id: "tab:" + tab.tab_id, kind: .tab, depth: 2, glyph: "○", tone: "mute",
                                             title: tab.label ?? "tab \(tab.number)", tab: tab.tab_id, dim: true))
                    }
                }
            }
        }
        return out
    }

    private static let clock: DateFormatter = {
        let f = DateFormatter()
        f.dateFormat = "HH:mm"
        return f
    }()
}
