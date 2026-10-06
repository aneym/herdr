import Foundation

/// Rows the spaces tree draws once other machines' chats merge in; `--local` skips the machines.
struct Fixture: Decodable { let input: SpacesInput; let overlay: Overlay; let chrome: SpacesChrome; let now: Double; let machines: [MachineMerge.Machine] }
@main struct Dump {
    static func main() throws {
        let f = try JSONDecoder().decode(Fixture.self, from: Data(contentsOf: URL(fileURLWithPath: CommandLine.arguments[1])))
        let machines = CommandLine.arguments.contains("--local") ? [] : f.machines
        let rows = SpacesTree.build(MachineMerge.merge(f.input, machines: machines), overlay: f.overlay, chrome: f.chrome, now: f.now)
        for row in MachineMerge.badge(rows, machines: machines) { print(row.dump) }
    }
}
