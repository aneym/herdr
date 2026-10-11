import Foundation

/// Settings > Agents rows for a fixture `{"input": SpacesInput, "agentsDir": dir, "panes": [[pane, tab]], "pending": {tab: shown}}`;
/// agent cards load from agentsDir and attach as HerdrModel.settingsInput does.
/// Prints `row|tab|title|meta|shown|initial:tint` per row, then `settled|<tab,...>`.
struct Fixture: Decodable { let input: SpacesInput; var agentsDir: String? = nil; var panes: [[String]]? = nil; var pending: [String: Bool]? = nil }
@main struct Dump {
    static func main() throws {
        let f = try JSONDecoder().decode(Fixture.self, from: Data(contentsOf: URL(fileURLWithPath: CommandLine.arguments[1])))
        let pending = f.pending ?? [:]
        let input = AgentCards.attach(f.input, panes: (f.panes ?? []).map { ($0[0], $0[1]) }, cards: AgentCards.load(dir: f.agentsDir ?? ""))
        for row in SettingsAgents.rows(input, pending: pending) {
            print(["row", row.id, row.title, row.meta, String(row.shown), "\(row.face.initial):\(row.face.tint)"].joined(separator: "|"))
        }
        print("settled|" + SettingsAgents.settle(pending, input).keys.sorted().joined(separator: ","))
    }
}
