import Foundation

/// A standing agent's card, `<agents dir>/<name>/agent.json`, as agent-rails writes it. Rails reads the
/// same files for its Agents section, so both draw one face for one agent (pinned-agents-rails BRIEF,
/// ask 3). The Shell reads them from disk, beside lanes.json: herdr serves no agent name or picture.
struct AgentCard: Equatable {
    var name: String
    /// `avatar_url`, kept only when it is https.
    var avatar: String?
}

enum AgentCards {
    /// Cards by the pane they run in. A card with no name or no pane (`"none"` included) names no row.
    static func load(dir: String) -> [String: AgentCard] {
        var out: [String: AgentCard] = [:]
        for path in files(dir: dir) {
            guard let data = FileManager.default.contents(atPath: path),
                  let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
                  let name = str(obj["name"]), let pane = str(obj["pane"]), pane != "none" else { continue }
            let avatar = str(obj["avatar_url"]).flatMap { isPicture($0) ? $0 : nil }
            out[pane] = AgentCard(name: name, avatar: avatar)
        }
        return out
    }

    /// Every `<dir>/*/agent.json`, sorted so a pane two cards claim always resolves the same way.
    static func files(dir: String) -> [String] {
        guard !dir.isEmpty, let names = try? FileManager.default.contentsOfDirectory(atPath: dir) else { return [] }
        return names.sorted().map { (dir as NSString).appendingPathComponent($0 + "/agent.json") }
            .filter { FileManager.default.fileExists(atPath: $0) }
    }

    /// Gives each tab the name and picture of the card whose pane it holds; `panes` is (pane, tab).
    static func attach(_ input: SpacesInput, panes: [(String, String)], cards: [String: AgentCard]) -> SpacesInput {
        guard !cards.isEmpty else { return input }
        var byTab: [String: AgentCard] = [:]
        for (pane, tab) in panes { if byTab[tab] == nil, let card = cards[pane] { byTab[tab] = card } }
        var out = input
        out.tabs = input.tabs.map { tab in
            guard let card = byTab[tab.id] else { return tab }
            var tab = tab
            tab.agentName = card.name
            tab.avatar = card.avatar
            return tab
        }
        return out
    }

    /// Pictures load over https only, whatever the scheme's case.
    static func isPicture(_ url: String) -> Bool { URL(string: url)?.scheme?.lowercased() == "https" }

    private static func str(_ v: Any?) -> String? {
        guard let s = v as? String else { return nil }
        let t = s.trimmingCharacters(in: .whitespacesAndNewlines)
        return t.isEmpty ? nil : t
    }
}
