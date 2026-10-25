import Foundation

enum SpacesTree {
    static let actionUnfoldsLane = false
    static func mark(_ tab: SpacesInput.Tab, _ t: Overlay.Tag, foldable: Bool = false) -> (glyph: String, tone: String, idle: Bool) {
        let priority = ["unknown": 0, "idle": 1, "done": 2, "working": 3, "blocked": 4]
        var status = tab.work ?? tab.agents.max { (priority[$0.status] ?? 0) < (priority[$1.status] ?? 0) }?.status ?? tab.status
        if tab.work == nil && t.busy && ["idle", "done"].contains(status) { status = "working" }
        if tab.work == nil && foldable && ["idle", "done", "unknown"].contains(status) { status = "working" }
        let idle = t.kind == "lane" && (tab.work != nil || !t.busy) && (t.summary ?? "").trimmingCharacters(in: .whitespaces).isEmpty && status == "idle"
        let glyph = t.kind == "workflow" ? (t.done ? (t.attention == "act" ? "✗" : "✓") : "◐") : idle ? "○" : statusGlyph(status)
        return (glyph, idle ? "mute" : status, idle)
    }
    static func statusGlyph(_ status: String) -> String { status == "blocked" || status == "done" ? "■" : "●" }
    static func runDone(_ run: Overlay.Run, of tab: SpacesInput.Tab) -> Bool {
        run.done || run.id.hasPrefix("agent:") && tab.work.map { !["working", "blocked"].contains($0) } == true
    }
    static let faceTints = 9
    static func face(name: String, avatar: String?) -> SpacesRow.Face {
        let key = name.trimmingCharacters(in: jsTrim)
        var hash: UInt32 = 7
        for scalar in key.lowercased().unicodeScalars { hash = hash &* 31 &+ scalar.value }
        let initial = key.unicodeScalars.first.map { String($0).uppercased() } ?? "?"
        return SpacesRow.Face(initial: initial, tint: Int(hash % UInt32(faceTints)), avatar: avatar)
    }
    static let jsTrim = CharacterSet(charactersIn: "\t\u{0B}\u{0C} \u{A0}\u{1680}\u{2000}\u{2001}\u{2002}\u{2003}\u{2004}\u{2005}\u{2006}\u{2007}\u{2008}\u{2009}\u{200A}\u{202F}\u{205F}\u{3000}\u{FEFF}\n\r\u{2028}\u{2029}")
    static func age(_ seconds: Double) -> String { let m = Int(max(0, seconds)) / 60; return m == 0 ? "<1m" : m < 60 ? "\(m)m" : "\(m / 60)h\(m % 60)m" }
    static func numbered(_ tabs: [SpacesInput.Tab], rows: [SpacesRow], rest: [String] = []) -> [String] {
        var seen = Set<String>()
        return (rows.filter { $0.kind == .tab }.compactMap(\.tab) + rest).filter { seen.insert($0).inserted }
    }
    static func closePinOrder(_ rows: [SpacesRow], selected: String?) -> [String] {
        let ids = rows.filter { $0.id.hasPrefix("home:") }.compactMap(\.tab)
        guard let selected, let index = ids.firstIndex(of: selected) else { return [] }
        return Array(ids.dropFirst(index + 1)) + Array(ids.prefix(index).reversed())
    }
    static func build(_ input: SpacesInput, overlay: Overlay, chrome: SpacesChrome, now: Double) -> [SpacesRow] {
        let choices = overlay.goalChoices
        let filter = chrome.goalFilter.flatMap { choices.contains($0) ? $0 : nil }
        var out = HomeRows.rows(input, overlay: overlay, chrome: chrome, now: now)
        var input = input
        let represented = HomeRows.represented(input, overlay: overlay)
        input.tabs.removeAll { represented.contains($0.id) }
        out.append(SpacesRow(id: "spaces", kind: .title, title: "Spaces"))
        out += AutomationRows.rows(input.automations ?? [])
        if !choices.isEmpty { out.append(SpacesRow(id: "goal", kind: .goal, title: "goal " + (filter?.replacingOccurrences(of: ":", with: " · ") ?? "All"), trailing: filter == nil ? "▾" : "✕")) }
        let hidden = input.spaces.filter { $0.collapsed || chrome.collapsedSpaces.contains($0.id) }
        let visible = input.spaces.filter { !$0.collapsed && !chrome.collapsedSpaces.contains($0.id) }
        func ranked(_ spaces: [SpacesInput.Space]) -> [SpacesInput.Space] {
            spaces.enumerated().sorted { a, b in a.element.sortRank == b.element.sortRank ? a.offset < b.offset : a.element.sortRank < b.element.sortRank }.map(\.element)
        }
        let ordered = ranked(visible.filter { $0.pinned || chrome.pinnedSpaces.contains($0.id) }) + ranked(visible.filter { !$0.pinned && !chrome.pinnedSpaces.contains($0.id) })
        func appendSpace(_ space: SpacesInput.Space, depth: Int) {
            let meta = overlay.spaces[space.id]
            let folded = space.parked && !chrome.expandedParkedSpaces.contains(space.id)
            out.append(SpacesRow(id: "space:" + space.id, kind: .space, depth: depth, chevron: folded ? "closed" : "open", title: space.name, trailing: meta?.summary ?? "", alert: meta?.attention ?? "none", tab: meta?.target_tab ?? input.tabs.first(where: { $0.space == space.id })?.id, toggleKey: (space.parked ? "parkedspace:" : "space:") + space.id))
            guard !folded else { return }
            let scope = SpaceScope(space, input: input, overlay: overlay, filter: filter, depth: depth)
            let tabs = scope.tabs, background = scope.background
            let orch = scope.orch, lanes = scope.lanes, workflows = scope.workflows, ordinary = scope.ordinary
            func tag(_ tab: SpacesInput.Tab) -> Overlay.Tag { scope.tag(tab) }
            func parent(_ tab: SpacesInput.Tab) -> String? { scope.parent(tab) }
            func root(_ tab: SpacesInput.Tab) -> String? { scope.root(tab) }
            func appendTab(_ tab: SpacesInput.Tab, _ level: Int, nest: Bool = true, inside: Bool = false) {
                let t = tag(tab)
                let rollup = scope.rollup(tab, nest: nest)
                let grouped = rollup.grouped, children = rollup.children, runs = rollup.runs
                let childTags = tabs.filter { candidate in
                    let ct = tag(candidate)
                    return ct.kind == "workflow" && (parent(candidate) == tab.id || grouped.contains { $0.id == parent(candidate) })
                }.map { tag($0) }
                let attentions = [t.attention] + childTags.map(\.attention) + runs.map(\.attention) + grouped.map { tag($0).attention }
                    + grouped.flatMap { tag($0).runs.map(\.attention) }
                let rank = ["none": 0, "warn": 1, "act": 2]
                let attention = attentions.max { (rank[$0] ?? 0) < (rank[$1] ?? 0) } ?? "none"
                let expandable = !children.isEmpty || runs.contains { !$0.done } || !grouped.isEmpty
                let focusedInside = (children + grouped + rollup.groupedWorkflows).contains { $0.id == input.focusedTab }
                let agents = rollup.agents, flows = rollup.flows, count = agents + flows
                let foldable = !inside && count > 0
                let open = inside || ((chrome.expandedTabs.contains(tab.id) || focusedInside || (SpacesTree.actionUnfoldsLane && attention == "act")) && !chrome.collapsedTabs.contains(tab.id))
                let state = SpacesTree.mark(tab, t, foldable: foldable)
                let idle = state.idle
                var name = t.kind != "workflow" && tab.label != tab.id ? tab.label : t.name ?? tab.label
                if t.kind == "workflow", name.hasPrefix("wf ") { name = String(name.dropFirst(3)) }
                if t.kind == "lane" {
                    if name.lowercased().hasPrefix("[scoping] ") { name = String(name.dropFirst(10)) }
                    if let range = name.range(of: " · ", options: .backwards), ["scoping", "implementing", "reviewing", "monitoring", "closed"].contains(String(name[range.upperBound...]).lowercased()) { name = String(name[..<range.lowerBound]) }
                }
                var trailing = t.badge ?? t.summary ?? (idle ? t.idle_reason ?? "idle" : "")
                if expandable {
                    let words = [agents > 0 ? "\(agents) agent" + (agents == 1 ? "" : "s") : nil,
                                 flows > 0 ? "\(flows) workflow" + (flows == 1 ? "" : "s") : nil].compactMap { $0 }
                    trailing = count > 0 ? words.joined(separator: " · ") : (idle ? t.idle_reason ?? "idle" : "")
                    if t.kind == "orchestrator", let summary = t.summary, !summary.isEmpty { trailing = count > 0 ? trailing + " · " + summary : summary }
                }
                let workflow = t.kind == "workflow"
                if workflow, !t.done, let phase = t.phase, !phase.isEmpty {
                    let progress = [phase.lowercased(), t.started.map { age(now - $0) }].compactMap { $0 }.joined(separator: " · ")
                    trailing = [trailing, progress].filter { !$0.isEmpty }.joined(separator: " · ")
                }
                out.append(SpacesRow(id: "tab:" + tab.id, kind: .tab, depth: level, chevron: foldable ? (open ? "open" : "closed") : "none", glyph: state.glyph, tone: state.tone, title: name, trailing: trailing, alert: attention, tab: tab.id, toggleKey: foldable ? "tab:" + tab.id : nil, dim: idle || t.done || t.kind == "advisor" || t.mode == "parked"))
                if open {
                    for child in grouped { appendTab(child, level + 1, inside: true) }
                    for child in children { appendTab(child, level + 1, nest: false) }
                    for run in runs where !run.done {
                        let progress = [run.phase?.lowercased(), run.started.map { age(now - $0) }].compactMap { $0 }.filter { !$0.isEmpty }.joined(separator: " · ")
                        out.append(SpacesRow(id: "run:" + run.id, kind: .run, depth: level + 1, glyph: "◐", tone: "working", title: run.name ?? run.id, trailing: [run.badge, progress.isEmpty ? nil : progress].compactMap { $0 }.joined(separator: " · "), alert: run.attention, tab: tab.id, dim: false))
                    }
                }
            }
            var hiddenCount = 0
            func section(_ label: String, _ members: [SpacesInput.Tab], count: String = "") {
                guard !members.isEmpty else { return }
                let key = space.id + ":" + label
                if let focus = chrome.focusedSection[space.id], focus != label && label != "ORCHESTRATOR" { hiddenCount += members.count; return }
                let closed = chrome.collapsedSections.contains(key) && chrome.focusedSection[space.id] == nil
                out.append(SpacesRow(id: "section:" + key, kind: .section, depth: depth + 1, chevron: closed ? "closed" : "open", title: label, trailing: closed ? String(members.count) : count, alert: closed && members.contains { tag($0).attention == "act" || $0.status == "blocked" } ? "act" : "none", toggleKey: "section:" + key))
                if !closed { for tab in members { appendTab(tab, depth + 1) } }
            }
            section("ORCHESTRATOR", orch)
            section("LANES", lanes.filter { tag($0).mode == "active" && root($0) == nil } + workflows.filter { parent($0) == nil } + ordinary)
            for (group, members) in [("services", lanes.filter { tag($0).mode == "auto" }), ("parked", lanes.filter { tag($0).mode == "parked" }), ("background", background)] {
                guard !members.isEmpty else { continue }
                if chrome.focusedSection[space.id] != nil { hiddenCount += members.count; continue }
                let key = space.id + ":" + group
                let open = chrome.expandedGroups.contains(key)
                let memberIds = Set(members.map(\.id))
                let groupLanes = lanes.filter { memberIds.contains($0.id) || root($0).map(memberIds.contains) ?? false }
                let groupIds = Set(groupLanes.map(\.id))
                let pool = groupLanes + members.filter { !groupIds.contains($0.id) }
                    + tabs.filter { tag($0).kind == "workflow" && (parent($0).map(groupIds.contains) ?? false) }
                let needs = pool.contains { member in
                    let t = tag(member)
                    return ([member.status] + member.agents.map(\.status)).contains("blocked") || t.attention == "act"
                        || t.runs.contains { $0.attention == "act" }
                }
                let attention = needs ? "act" : "none"
                out.append(SpacesRow(id: "group:" + key, kind: .group, depth: depth + 1, chevron: open ? "open" : "closed", title: group + " " + String(members.count), alert: attention, toggleKey: "group:" + key, dim: true))
                if open { for tab in members { appendTab(tab, depth + 2) } }
            }
            if chrome.focusedSection[space.id] != nil {
                out.append(SpacesRow(id: "all:" + space.id, kind: .group, depth: depth + 1, title: "show all", trailing: String(hiddenCount), toggleKey: "all:" + space.id, dim: true))
            }
        }
        let groups = overlay.spaceGroups.filter { !$0.name.trimmingCharacters(in: .whitespaces).isEmpty }
        var grouped = Overlay(); grouped.spaceGroups = groups
        var members = groups.map { _ in [SpacesInput.Space]() }
        var rest = [SpacesInput.Space]()
        for space in ordered {
            if let index = grouped.spaceGroup(id: space.id, label: space.name) { members[index].append(space) } else { rest.append(space) }
        }
        for (group, spaces) in zip(groups, members) where !spaces.isEmpty {
            out.append(SpacesRow(id: "spacegroup:" + group.name, kind: .title, title: group.name))
            func position(_ space: SpacesInput.Space) -> Int {
                group.spaces.firstIndex { $0 == space.id || $0.trimmingCharacters(in: .whitespaces).lowercased() == space.name.trimmingCharacters(in: .whitespaces).lowercased() } ?? Int.max
            }
            for space in spaces.enumerated().sorted(by: { a, b in position(a.element) == position(b.element) ? a.offset < b.offset : position(a.element) < position(b.element) }).map(\.element) { appendSpace(space, depth: 0) }
        }
        for space in rest { appendSpace(space, depth: 0) }
        if !hidden.isEmpty {
            out.append(SpacesRow(id: "hidden", kind: .hidden, chevron: chrome.hiddenExpanded ? "open" : "closed", title: "hidden \(hidden.count)", toggleKey: "hidden"))
            if chrome.hiddenExpanded { for space in hidden { appendSpace(space, depth: 1) } }
        }
        for (i, host) in overlay.usage.enumerated() { out.append(SpacesRow(id: "usage:\(i)", kind: .footerUsage, title: host.name, trailing: (host.summary ?? "").replacingOccurrences(of: " · ", with: " "), alert: host.attention, link: host.url)) }
        for (i, host) in overlay.hosts.enumerated() { out.append(SpacesRow(id: "host:\(i)", kind: .footerHost, title: host.name, trailing: (host.summary ?? "").replacingOccurrences(of: "load ", with: "").replacingOccurrences(of: " live", with: ""), alert: host.attention, hover: host.detail.map { host.name + ": " + $0 })) }
        return out
    }
}
