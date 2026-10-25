import Foundation

struct SpaceScope {
    let overlay: Overlay
    let leader: String?
    let tabs: [SpacesInput.Tab]; let background: [SpacesInput.Tab]
    let orch: [SpacesInput.Tab]; let lanes: [SpacesInput.Tab]; let workflows: [SpacesInput.Tab]; let ordinary: [SpacesInput.Tab]

    init(_ space: SpacesInput.Space, input: SpacesInput, overlay: Overlay, filter: String?, depth: Int, includeAgents: Bool = false) {
        self.overlay = overlay
        leader = input.tabs.first { $0.space == space.id && (includeAgents || $0.role != "agent") && overlay.tabs[$0.id]?.kind == "orchestrator" && overlay.tabs[$0.id]?.done != true }?.id
        let all = input.tabs.filter { $0.space == space.id }.enumerated().sorted { a, b in
            if a.element.sortRank != b.element.sortRank { return a.element.sortRank < b.element.sortRank }
            return a.offset < b.offset
        }.map(\.element)
        let tabs = all.filter { tab in
            guard let filter, let tag = overlay.tabs[tab.id] else { return filter == nil }
            let bits = filter.split(separator: ":", maxSplits: 1).map(String.init)
            return tag.kind == "orchestrator" || tag.mode == "auto" || (tag.goal == bits[0] && (bits.count == 1 || tag.goal_area == bits[1]))
        }
        func tag(_ tab: SpacesInput.Tab) -> Overlay.Tag { overlay.tabs[tab.id] ?? Overlay.Tag() }
        let foreground = tabs.filter { !tag($0).done && tag($0).kind != "advisor" }
        self.tabs = tabs
        background = tabs.filter { let t = tag($0); return t.kind == "advisor" || (t.done && t.kind != "workflow" && !(t.kind == "lane" && t.mode != "active")) }
        orch = foreground.filter { tag($0).kind == "orchestrator" }
        lanes = tabs.filter { tag($0).kind == "lane" && (!tag($0).done || tag($0).mode != "active") }
        workflows = foreground.filter { tag($0).kind == "workflow" }
        ordinary = foreground.filter { !["orchestrator", "lane", "workflow"].contains(tag($0).kind) }
    }

    func tag(_ tab: SpacesInput.Tab) -> Overlay.Tag { overlay.tabs[tab.id] ?? Overlay.Tag() }
    func parent(_ tab: SpacesInput.Tab) -> String? { let p = tag(tab).parent; return lanes.contains { $0.id == p } ? p : leader }
    func root(_ tab: SpacesInput.Tab) -> String? {
        guard tag(tab).kind == "lane", tag(tab).mode == "active" else { return nil }
        var seen = Set([tab.id]); var current = tab
        func edge(_ t: SpacesInput.Tab) -> Bool { tag(t).kind == "lane" && tag(t).mode == "active" }
        while edge(current), let parent = current.agents.first?.parent, let next = (lanes + orch).first(where: { $0.id == parent }) {
            guard seen.insert(parent).inserted else { return nil }
            current = next
        }
        return current.id == tab.id ? nil : current.id
    }

    struct Rollup {
        var grouped: [SpacesInput.Tab]; var children: [SpacesInput.Tab]; var groupedWorkflows: [SpacesInput.Tab]
        var runs: [Overlay.Run]; var agents: Int; var flows: Int
        var count: Int { agents + flows }
    }
    func rollup(_ tab: SpacesInput.Tab, nest: Bool) -> Rollup {
        let grouped = nest ? lanes.filter { root($0) == tab.id } : []
        let children = nest ? workflows.filter { parent($0) == tab.id } : []
        func settled(_ owner: SpacesInput.Tab) -> [Overlay.Run] {
            tag(owner).runs.map { run in var run = run; run.done = SpacesTree.runDone(run, of: owner); return run }
        }
        let runs = nest ? settled(tab) : []
        let groupedWorkflows = workflows.filter { w in grouped.contains { $0.id == parent(w) } }
        let live = (runs + grouped.flatMap { settled($0) }).filter { !$0.done }
        let agents = grouped.count + live.filter { $0.id.hasPrefix("agent:") }.count
        let flows = children.count + groupedWorkflows.count + live.filter { !$0.id.hasPrefix("agent:") }.count
        return Rollup(grouped: grouped, children: children, groupedWorkflows: groupedWorkflows, runs: runs, agents: agents, flows: flows)
    }
}
