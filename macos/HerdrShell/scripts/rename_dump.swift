import Foundation

/// The title the spaces tree draws for each tab of a live `session.snapshot`, built over an
/// overlay file by SpacesTree.build as HerdrModel.spacesRows does, every lane unfolded.
///   rename_dump <snapshot.json> <overlay.json>
@main struct RenameDump {
    static func main() throws {
        let args = CommandLine.arguments
        let s = try JSONDecoder().decode(Snapshot.self, from: Data(contentsOf: URL(fileURLWithPath: args[1])))
        let overlay = (try? JSONDecoder().decode(Overlay.self, from: Data(contentsOf: URL(fileURLWithPath: args[2])))) ?? Overlay()
        let input = SpacesInput(spaces: s.workspaces.map { SpacesInput.Space(id: $0.workspace_id, name: $0.label ?? $0.workspace_id) },
                                tabs: s.tabs.map { SpacesInput.Tab(id: $0.tab_id, space: $0.workspace_id, label: $0.label ?? $0.tab_id, pinIndex: $0.pin_index, role: $0.role) },
                                focusedTab: nil)
        var chrome = SpacesChrome()
        chrome.expandedTabs = Set(s.tabs.map(\.tab_id))
        for row in SpacesTree.build(input, overlay: overlay, chrome: chrome, now: Date().timeIntervalSince1970) where row.kind == .tab {
            print("spaces|\(row.tab ?? "")|\(row.title)")
        }
    }
}
