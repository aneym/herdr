import Foundation

/// Pure ranking edge cases plus the real snapshot and persisted-chrome JSON boundaries.
/// No app launch or server; the Python harness owns expected output, not this driver.
@main struct PriorityDump {
    static func main() throws {
        // Pin-close ranking spans two blocks, skipped neighbors and the unpinned fallback.
        let closeInput = SpacesInput(spaces: [.init(id: "w", name: "W")], tabs: [
            .init(id: "p1", space: "w", label: "P1", pinIndex: 0),
            .init(id: "p2", space: "w", label: "P2", pinIndex: 1),
            .init(id: "p3", space: "w", label: "P3", pinIndex: 2),
            .init(id: "a1", space: "w", label: "A1", pinIndex: 3, role: "agent"),
            .init(id: "a2", space: "w", label: "A2", pinIndex: 4, role: "agent"),
            .init(id: "a3", space: "w", label: "A3", pinIndex: 5, role: "agent"),
            .init(id: "u1", space: "w", label: "U1"),
            .init(id: "u2", space: "w", label: "U2")
        ])
        let closeRows = SpacesTree.build(closeInput, overlay: Overlay(), chrome: SpacesChrome(), now: 0)
        for (selected, removed) in [("p2", ["p2"]), ("p3", ["p3"]), ("a2", ["a2"]),
                                    ("a3", ["a3"]), ("u1", ["u1"]),
                                    ("p2", ["p1", "p2", "p3"]), ("p2", ["p2", "p3"])] {
            let live = Set(closeInput.tabs.map(\.id).filter { !removed.contains($0) })
            let pins = SpacesTree.closePinOrder(closeRows, selected: selected)
            // MainWindow's existing sidebar fallback: below, then the original prefix.
            var seen = Set<String>()
            let order = closeRows.filter { $0.kind == .tab }.compactMap(\.tab).filter { seen.insert($0).inserted }
            let index = order.firstIndex(of: selected)!
            let fallback = Array(order.dropFirst(index + 1)) + Array(order.prefix(index))
            let next = (pins + fallback).first { live.contains($0) } ?? "-"
            print("close|\(selected)|\(removed.joined(separator: ","))|\(next)")
        }
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
        var allInput = input
        allInput.tabs.append(.init(id: "leadAll", space: "a", label: "Lead", pinIndex: 4, role: "agent"))
        var goalOverlay = Overlay()
        var goalTag = Overlay.Tag(); goalTag.goal = "rails"; goalTag.section = "orchestrator"
        goalOverlay.tabs["leadAll"] = goalTag
        for (name, value) in [("goalPresent", goalOverlay), ("goalAbsent", Overlay())] {
            let goalRows = SpacesTree.build(allInput, overlay: value, chrome: SpacesChrome(), now: 0)
            print("\(name)|\(goalRows.contains { $0.kind == .goal })|\(!allInput.spaces.isEmpty)")
        }
        var allChrome = SpacesChrome()
        allChrome.collapsedSections = ["d:SCOPING"]
        allChrome.expandedTabs = ["plain1"]
        for scenario in ["collapseAll", "expandAll"] {
            allChrome.toggleAllSpaces(allInput.spaces)
            allChrome = try JSONDecoder().decode(SpacesChrome.self, from: JSONEncoder().encode(allChrome))
            print("\(scenario)Chrome|\(allChrome.collapsedSpaces.count)|\(allChrome.expandedParkedSpaces.contains("p"))|\(allChrome.collapsedSections.contains("d:SCOPING"))|\(allChrome.expandedTabs.contains("plain1"))")
            for row in SpacesTree.build(allInput, overlay: Overlay(), chrome: allChrome, now: 0) { print(scenario + "|" + row.dump) }
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
        // Selecting a tab (quick switch, attention jump, header click) reveals its space.
        var folded = SpacesChrome(); folded.collapsedSpaces = ["d"]
        dump("folded", chrome: folded)
        let pinnedRows = SpacesTree.build(input, overlay: Overlay(), chrome: folded, now: 0)
        let pinRevealed = folded.reveal(space: "d", parked: false, selected: "pin1", rows: pinnedRows)
        let persistedFold = try JSONDecoder().decode(SpacesChrome.self, from: JSONEncoder().encode(folded))
        print("pinnedReveal|\(pinRevealed)|\(persistedFold.collapsedSpaces.contains("d"))")
        dump("revealPinned", chrome: persistedFold)
        var agentInput = input
        agentInput.tabs.append(.init(id: "lead", space: "d", label: "Lead", pinIndex: 3, role: "agent"))
        var agentFold = persistedFold
        let agentRows = SpacesTree.build(agentInput, overlay: Overlay(), chrome: agentFold, now: 0)
        let agentRevealed = agentFold.reveal(space: "d", parked: false, selected: "lead", rows: agentRows)
        let persistedAgentFold = try JSONDecoder().decode(SpacesChrome.self, from: JSONEncoder().encode(agentFold))
        print("agentReveal|\(agentRevealed)|\(persistedAgentFold.collapsedSpaces.contains("d"))")
        for row in SpacesTree.build(agentInput, overlay: Overlay(), chrome: persistedAgentFold, now: 0) { print("revealAgent|" + row.dump) }
        let unfolded = folded.reveal(space: "d", parked: false, selected: "plain1", rows: pinnedRows)
        dump("revealFolded", chrome: folded)
        var unparked = SpacesChrome()
        let opened = unparked.reveal(space: "p", parked: true)
        dump("revealParked", chrome: unparked)
        print("reveal|\(unfolded)|\(opened)|\(unparked.reveal(space: "p", parked: true))")
        // Another machine's chats: one homed in the local parked space by label, one in a parked space of its own.
        let box = MachineMerge.Machine(name: "box", health: nil, spaces: [
            .init(id: "box/w1", name: "parked", parked: false), .init(id: "box/w2", name: "Remote", parked: true)
        ], tabs: [
            .init(id: "box/w1:t1", space: "box/w1", label: "Homed", agents: [.init(status: "idle", parent: nil)]),
            .init(id: "box/w2:t1", space: "box/w2", label: "Away", agents: [.init(status: "idle", parent: nil)])
        ])
        let merged = MachineMerge.merge(input, machines: [box])
        for tab in ["box/w1:t1", "box/w2:t1", "missing"] {
            let space = MachineMerge.space(of: tab, local: input, machines: [box])
            var chrome = SpacesChrome()
            if let space { _ = chrome.reveal(space: space.id, parked: space.parked) }
            print("remote|\(tab)|\(space?.id ?? "-")|\(space?.parked ?? false)")
            for row in SpacesTree.build(merged, overlay: Overlay(), chrome: chrome, now: 0) { print("remote-" + tab + "|" + row.dump) }
        }
    }
}
