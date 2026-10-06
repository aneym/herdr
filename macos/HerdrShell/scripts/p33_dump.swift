import Foundation

struct Fixture: Decodable { let input: SpacesInput; let overlay: Overlay; var chrome: SpacesChrome; let now: Double }
@main struct Dump {
    static func main() throws {
        var fixture = try JSONDecoder().decode(Fixture.self, from: Data(contentsOf: URL(fileURLWithPath: CommandLine.arguments[1])))
        for key in CommandLine.arguments.dropFirst(2) { fixture.chrome.toggle(key) }
        for row in SpacesTree.build(fixture.input, overlay: fixture.overlay, chrome: fixture.chrome, now: fixture.now) { print(row.dump) }
    }
}
