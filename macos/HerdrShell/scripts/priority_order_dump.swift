import Foundation

/// Pure ranking edge cases plus the real snapshot and persisted-chrome JSON boundaries.
/// No app launch or server; the Python harness owns expected output, not this driver.
@main struct PriorityDump {
    static func main() throws {
        let data = Data(#"{"workspaces":[{"workspace_id":"old","number":1},{"workspace_id":"new","number":2,"sort_rank":4294967295,"parked":true}],"tabs":[{"tab_id":"old:t1","workspace_id":"old","number":1},{"tab_id":"new:t1","workspace_id":"new","number":1,"sort_rank":7}],"panes":[],"agents":[],"layouts":[]}"#.utf8)
        let snapshot = try JSONDecoder().decode(Snapshot.self, from: data)
        print("snapshot|\(snapshot.workspaces[0].sort_rank ?? 0)|\(snapshot.workspaces[0].parked ?? false)|\(snapshot.tabs[0].sort_rank ?? 0)|\(snapshot.workspaces[1].sort_rank ?? 0)|\(snapshot.workspaces[1].parked ?? false)|\(snapshot.tabs[1].sort_rank ?? 0)")
        let oldInput = try JSONDecoder().decode(SpacesInput.self, from: Data(#"{"spaces":[{"id":"old","name":"Old"}],"tabs":[{"id":"old:t1","space":"old","label":"Old"}]}"#.utf8))
        print("input|\(oldInput.spaces[0].sortRank)|\(oldInput.spaces[0].parked)|\(oldInput.tabs[0].sortRank)")
        let oldChrome = try JSONDecoder().decode(SpacesChrome.self, from: Data(#"{"collapsedSpaces":["existing"],"pinnedSpaces":["b"]}"#.utf8))
        print("chrome|\(oldChrome.expandedParkedSpaces.isEmpty)|\(oldChrome.collapsedSpaces.contains("existing"))")
        let input = SpacesInput(spaces: [
            .init(id: "a", name: "A", sortRank: 4), .init(id: "b", name: "B", sortRank: 2),
            .init(id: "c", name: "C", sortRank: 2), .init(id: "d", name: "D", sortRank: 0),
            .init(id: "p", name: "Parked", sortRank: 8, parked: true)
        ], tabs: [
            .init(id: "latepin", space: "d", label: "Late pin", pinIndex: 0, sortRank: 4),
            .init(id: "plain1", space: "d", label: "Plain 1", sortRank: 1),
            .init(id: "pin1", space: "d", label: "Pin 1", pinIndex: 2, sortRank: 1),
            .init(id: "pin2", space: "d", label: "Pin 2", pinIndex: 1, sortRank: 1),
            .init(id: "plain2", space: "d", label: "Plain 2", sortRank: 1),
            .init(id: "parkedtab", space: "p", label: "Parked tab")
        ], focusedTab: nil)
        func dump(_ scenario: String, overlay: Overlay = Overlay(), chrome: SpacesChrome = SpacesChrome()) {
            for row in SpacesTree.build(input, overlay: overlay, chrome: chrome, now: 0) { print(scenario + "|" + row.dump) }
        }
        dump("rank")
        dump("partition", chrome: oldChrome)
        var overlay = Overlay(); overlay.spaceGroups = [.init(name: "Manual", spaces: ["a", "c", "d"])]
        dump("overlay", overlay: overlay)
        var chrome = oldChrome
        chrome.toggle("parkedspace:p", open: false)
        let persisted = try JSONEncoder().encode(chrome)
        chrome = try JSONDecoder().decode(SpacesChrome.self, from: persisted)
        dump("expanded", chrome: chrome)
        print("expandedChrome|\(chrome.expandedParkedSpaces.contains("p"))|\(chrome.collapsedSpaces == oldChrome.collapsedSpaces)")
        chrome.toggle("parkedspace:p", open: true)
        dump("refolded", chrome: chrome)
        print("refoldedChrome|\(chrome.expandedParkedSpaces.isEmpty)|\(chrome.collapsedSpaces == oldChrome.collapsedSpaces)")
    }
}
