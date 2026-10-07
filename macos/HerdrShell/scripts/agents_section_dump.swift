import Foundation

/// Rows the spaces tree draws once other machines' chats merge in; `--local` skips the machines.
/// `agentsDir` and `panes` ([pane, tab] pairs) attach agent cards as HerdrModel.spacesRows does.
struct Fixture: Decodable { let input: SpacesInput; let overlay: Overlay; let chrome: SpacesChrome; let now: Double; let machines: [MachineMerge.Machine]; var agentsDir: String? = nil; var panes: [[String]]? = nil }
@main struct Dump {
    static func main() throws {
        let f = try JSONDecoder().decode(Fixture.self, from: Data(contentsOf: URL(fileURLWithPath: CommandLine.arguments[1])))
        let machines = CommandLine.arguments.contains("--local") ? [] : f.machines
        let cards = AgentCards.load(dir: f.agentsDir ?? "")
        let local = AgentCards.attach(f.input, panes: (f.panes ?? []).map { ($0[0], $0[1]) }, cards: cards)
        let input = MachineMerge.merge(local, machines: machines)
        let rows = SpacesTree.build(input, overlay: f.overlay, chrome: f.chrome, now: f.now)
        let pins = SpacesTree.pinTabs(input.tabs, agents: true) + SpacesTree.pinTabs(input.tabs, agents: false)
        var seen = Set<String>()
        let numbered = (pins.map(\.id) + rows.filter { $0.kind == .tab }.compactMap(\.tab)).filter { seen.insert($0).inserted }
        print("numbered|" + numbered.joined(separator: ","))
        print("indices|" + input.tabs.compactMap { t in t.pinIndex.map { t.id + ":" + String($0) } }.joined(separator: ","))
        for row in MachineMerge.badge(rows, machines: machines) { print(row.dump) }
    }
}
