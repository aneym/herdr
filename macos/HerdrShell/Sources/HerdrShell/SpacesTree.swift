import Foundation

struct SpacesInput: Codable {
    struct Space: Codable { var id: String; var name: String; var pinned = false; var collapsed = false; var sortRank: UInt32 = 0; var parked = false
        init(id: String, name: String, pinned: Bool = false, collapsed: Bool = false, sortRank: UInt32 = 0, parked: Bool = false) { self.id = id; self.name = name; self.pinned = pinned; self.collapsed = collapsed; self.sortRank = sortRank; self.parked = parked }
        init(from decoder: Decoder) throws { let c = try decoder.container(keyedBy: Field.self); id = try c.decode(String.self, forKey: Field("id")); name = try c.decode(String.self, forKey: Field("name")); pinned = c.value("pinned", false); collapsed = c.value("collapsed", false); sortRank = c.value("sortRank", 0); parked = c.value("parked", false) }
    }
    struct Agent: Codable { var status: String; var parent: String? = nil }
    /// `work` is herdr's one answer to "is this chat working" (server app/work_status.rs); nil from older servers.
    struct Tab: Codable { var id: String; var space: String; var label: String; var agents: [Agent] = []; var focused = false; var status = "unknown"; var pinIndex: Int? = nil; var work: String? = nil; var role: String? = nil; var sortRank: UInt32 = 0
        init(id: String, space: String, label: String, agents: [Agent] = [], focused: Bool = false, status: String = "unknown", pinIndex: Int? = nil, work: String? = nil, role: String? = nil, sortRank: UInt32 = 0) { self.id = id; self.space = space; self.label = label; self.agents = agents; self.focused = focused; self.status = status; self.pinIndex = pinIndex; self.work = work; self.role = role; self.sortRank = sortRank }
        init(from decoder: Decoder) throws {
            let c = try decoder.container(keyedBy: Field.self)
            id = try c.decode(String.self, forKey: Field("id")); space = try c.decode(String.self, forKey: Field("space")); label = try c.decode(String.self, forKey: Field("label"))
            agents = c.value("agents", []); focused = c.value("focused", false); status = c.value("status", "unknown"); pinIndex = c.optional("pinIndex"); work = c.optional("work"); role = c.optional("role"); sortRank = c.value("sortRank", 0)
        }
    }
    var spaces: [Space]; var tabs: [Tab]; var focusedTab: String?
}

/// Overlay writers evolve independently of the shell. Missing fields use the Rust defaults.
struct Overlay: Codable {
    struct Run: Codable {
        var id = ""; var name: String?; var phase: String?; var agents = 0; var started: Double?; var done = false; var attention = "none"; var badge: String?
        init() {}
        init(from decoder: Decoder) throws {
            let c = try decoder.container(keyedBy: Field.self)
            id = c.value("id", ""); name = c.optional("name"); phase = c.optional("phase"); agents = c.value("agents", 0)
            started = c.optional("started"); done = c.value("done", false); attention = c.value("attention", "none"); badge = c.optional("badge")
        }
    }
    struct Tag: Codable {
        var kind = "unknown"; var section: String?; var mode = "active"; var name: String?; var parent: String?; var goal: String?; var goal_area: String?
        var scope_url: String?; var review_url: String?; var summary: String?; var attention = "none"; var busy = false; var idle_reason: String?
        var runs: [Run] = []; var phase: String?; var started: Double?; var done = false; var badge: String?
        init() {}
        init(from decoder: Decoder) throws {
            let c = try decoder.container(keyedBy: Field.self)
            kind = c.value("kind", "unknown"); section = c.optional("section"); mode = c.value("mode", "active");
            if !["orchestrator", "lane", "workflow", "advisor"].contains(kind) { kind = "unknown" }
            if !["parked", "auto"].contains(mode) { mode = "active" }
            if ["inflight", "idle"].contains(section ?? "") { section = "implementing" }
            if ["waiting", "ready", "ready_for_review"].contains(section ?? "") { section = "reviewing" }
            if !["orchestrator", "scoping", "implementing", "reviewing", "monitoring", "closed"].contains(section ?? "") { section = nil }
             name = c.optional("name"); parent = c.optional("parent")
            goal = c.optional("goal"); goal_area = c.optional("goal_area"); scope_url = c.optional("scope_url"); review_url = c.optional("review_url")
            summary = c.optional("summary"); attention = c.value("attention", "none"); busy = c.value("busy", false); idle_reason = c.optional("idle_reason")
            runs = c.value("runs", []); phase = c.optional("phase"); started = c.optional("started"); done = c.value("done", false); badge = c.optional("badge")
        }
    }
    struct Space: Codable {
        var attention = "none"; var summary: String?; var target_tab: String?
        init(from decoder: Decoder) throws { let c = try decoder.container(keyedBy: Field.self); attention = c.value("attention", "none"); summary = c.optional("summary"); target_tab = c.optional("target_tab") }
    }
    struct Host: Codable {
        var name = ""; var summary: String?; var attention = "none"; var url: String?
        init(from decoder: Decoder) throws { let c = try decoder.container(keyedBy: Field.self); name = c.value("name", ""); summary = c.optional("summary"); attention = c.value("attention", "none"); url = c.optional("url") }
    }
    /// A named sidebar group of whole spaces (Rails, Open Factory). Members are workspace labels or ids.
    /// Rust: FactoryOverlay.space_groups, read from `space_groups` in areas.json.
    struct SpaceGroup: Codable, Equatable {
        var name = ""; var spaces: [String] = []
        init(name: String, spaces: [String]) { self.name = name; self.spaces = spaces }
        init(from decoder: Decoder) throws { let c = try decoder.container(keyedBy: Field.self); name = c.value("name", ""); spaces = c.value("spaces", []) }
    }
    var tabs: [String: Tag] = [:]; var spaces: [String: Space] = [:]; var usage: [Host] = []; var hosts: [Host] = []; var spaceGroups: [SpaceGroup] = []
    init() {}
    init(from decoder: Decoder) throws { let c = try decoder.container(keyedBy: Field.self); tabs = c.value("tabs", [:]); spaces = c.value("spaces", [:]); usage = c.value("usage", []); hosts = c.value("hosts", []); spaceGroups = c.value("space_groups", []) }
    /// As Rust FactoryOverlay::space_group: first group naming the space by id, or by label ignoring case.
    func spaceGroup(id: String, label: String) -> Int? {
        let label = label.trimmingCharacters(in: .whitespaces).lowercased()
        return spaceGroups.firstIndex { group in
            group.spaces.contains { member in
                let member = member.trimmingCharacters(in: .whitespaces)
                return member == id || (!label.isEmpty && member.lowercased() == label)
            }
        }
    }
    var goalChoices: [String] {
        ["recruiter", "closer", "rails"].flatMap { goal -> [String] in
            let tags = tabs.values.filter { $0.goal == goal }
            guard !tags.isEmpty else { return [] }
            return [goal] + Set(tags.compactMap(\.goal_area).filter { !$0.isEmpty }).sorted().map { goal + ":" + $0 }
        }
    }
}
private struct Field: CodingKey { var stringValue: String; var intValue: Int? { nil }; init(_ s: String) { stringValue = s }; init?(stringValue: String) { self.init(stringValue) }; init?(intValue: Int) { return nil } }
private extension KeyedDecodingContainer where Key == Field {
    func value<T: Decodable>(_ name: String, _ fallback: T) -> T { (try? decode(T.self, forKey: Field(name))) ?? fallback }
    func optional<T: Decodable>(_ name: String) -> T? { try? decodeIfPresent(T.self, forKey: Field(name)) }
}

struct SpacesChrome: Codable {
    var collapsedSections: Set<String> = []; var expandedGroups: Set<String> = []; var expandedTabs: Set<String> = []; var collapsedTabs: Set<String> = []
    var pinnedSpaces: Set<String> = []; var collapsedSpaces: Set<String> = []; var expandedParkedSpaces: Set<String> = []; var hiddenExpanded = false; var goalFilter: String?; var focusedSection: [String: String] = [:]
    init() {}
    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: Field.self)
        collapsedSections = c.value("collapsedSections", []); expandedGroups = c.value("expandedGroups", []); expandedTabs = c.value("expandedTabs", []); collapsedTabs = c.value("collapsedTabs", [])
        pinnedSpaces = c.value("pinnedSpaces", []); collapsedSpaces = c.value("collapsedSpaces", []); expandedParkedSpaces = c.value("expandedParkedSpaces", []); hiddenExpanded = c.value("hiddenExpanded", false); goalFilter = c.optional("goalFilter"); focusedSection = c.value("focusedSection", [:])
        if let legacy: String = c.optional("focusedSection"), let split = legacy.lastIndex(of: ":") { focusedSection[String(legacy[..<split])] = String(legacy[legacy.index(after: split)...]) }
    }
    /// `open` is the row's rendered state; a tab row needs it because lanes fold by default
    /// yet focus can hold one open without an entry in either set.
    mutating func toggle(_ key: String, open: Bool? = nil) {
        if key.hasPrefix("all:") { focusedSection.removeValue(forKey: String(key.dropFirst(4))); return }
        if key == "hidden" { hiddenExpanded.toggle(); return }
        let parts = key.split(separator: ":", maxSplits: 1).map(String.init)
        guard parts.count == 2 else { return }
        func flip(_ set: inout Set<String>, _ value: String) { if !set.insert(value).inserted { set.remove(value) } }
        switch parts[0] {
        case "section": flip(&collapsedSections, parts[1])
        case "group": flip(&expandedGroups, parts[1])
        case "tab":
            if open ?? expandedTabs.contains(parts[1]) { expandedTabs.remove(parts[1]); collapsedTabs.insert(parts[1]) }
            else { collapsedTabs.remove(parts[1]); expandedTabs.insert(parts[1]) }
        case "space": flip(&collapsedSpaces, parts[1])
        case "parkedspace": flip(&expandedParkedSpaces, parts[1])
        case "pin": flip(&pinnedSpaces, parts[1])
        default: break
        }
    }
}

struct SpacesRow: Identifiable, Equatable {
    enum Kind: String { case title, goal, space, section, group, tab, run, hidden, footerUsage, footerHost }
    var id: String; var kind: Kind; var depth = 0; var chevron = "none"; var glyph = ""; var tone = "mute"; var title: String; var trailing = ""
    var alert = "none"; var link: String?; var tab: String?; var toggleKey: String?; var dim = false
    /// The machine running this chat when it is not this one (MachineMerge); nil on local rows.
    var badge: String?
    /// nil while that machine is healthy; "unreachable" or "needs update" otherwise.
    var badgeState: String?
    /// Semantic rather than width-dependent: native fonts do not truncate like a terminal grid.
    /// A badge adds one field, so local rows dump as they always have.
    var dump: String {
        let fields = [kind.rawValue, id, String(depth), chevron, glyph, tone, title, trailing, alert, link ?? "", tab ?? "", toggleKey ?? "", dim ? "dim" : ""]
        return (fields + (badge.map { ["@" + $0 + (badgeState.map { ":" + $0 } ?? "")] } ?? [])).joined(separator: "|")
    }
}

enum SpacesTree {
    /// Whether a child that needs action unfolds its lane. Off, as Rust tree.rs ACTION_UNFOLDS_LANE;
    /// the folded header carries the child's "!" instead.
    static let actionUnfoldsLane = false
    /// A tab's state glyph and tone, as its row in the spaces tree draws it (Rust chat_status and
    /// chat_state_mark), so the pinned section and the space agree on who is working. `foldable`
    /// is a lane header with live children, which reads as working.
    /// herdr's `work` fact wins; only an older server falls back to this rollup.
    static func mark(_ tab: SpacesInput.Tab, _ t: Overlay.Tag, foldable: Bool = false) -> (glyph: String, tone: String, idle: Bool) {
        let priority = ["unknown": 0, "idle": 1, "done": 2, "working": 3, "blocked": 4]
        var status = tab.work ?? tab.agents.max { (priority[$0.status] ?? 0) < (priority[$1.status] ?? 0) }?.status ?? tab.status
        if tab.work == nil && t.busy && ["idle", "done"].contains(status) { status = "working" }
        // As Rust summarize_factory_parent: a header with live children shows as working, not idle.
        if tab.work == nil && foldable && ["idle", "done", "unknown"].contains(status) { status = "working" }
        let idle = t.kind == "lane" && (tab.work != nil || !t.busy) && (t.summary ?? "").trimmingCharacters(in: .whitespaces).isEmpty && status == "idle"
        let glyph = t.kind == "workflow" ? (t.done ? (t.attention == "act" ? "✗" : "✓") : "◐") : idle ? "○" : status == "blocked" ? "■" : "●"
        return (glyph, idle ? "mute" : status, idle)
    }
    /// As Rust run_done: an agent:<id> run is the chat's own Claude subagent or teammate, over once
    /// herdr reports the chat quiet, even while the overlay still lists it.
    static func runDone(_ run: Overlay.Run, of tab: SpacesInput.Tab) -> Bool {
        run.done || run.id.hasPrefix("agent:") && tab.work.map { !["working", "blocked"].contains($0) } == true
    }
    static func age(_ seconds: Double) -> String { let m = Int(max(0, seconds)) / 60; return m == 0 ? "<1m" : m < 60 ? "\(m)m" : "\(m / 60)h\(m % 60)m" }
    static func pinTabs(_ tabs: [SpacesInput.Tab], agents: Bool) -> [SpacesInput.Tab] {
        func machine(_ id: String) -> String { id.firstIndex(of: "/").map { String(id[..<$0]) } ?? "" }
        var machines = [""]
        for tab in tabs { let name = machine(tab.id); if !machines.contains(name) { machines.append(name) } }
        return machines.flatMap { name in
            tabs.filter { machine($0.id) == name && ($0.role == "agent") == agents && (agents || $0.pinIndex != nil) }
                .sorted { ($0.pinIndex ?? 0) < ($1.pinIndex ?? 0) }
        }
    }
    static func build(_ input: SpacesInput, overlay: Overlay, chrome: SpacesChrome, now: Double) -> [SpacesRow] {
        let choices = overlay.goalChoices
        let filter = chrome.goalFilter.flatMap { choices.contains($0) ? $0 : nil }
        func hint(start: Int, count: Int) -> String {
            guard count > 0, start <= 9 else { return "" }
            let end = min(start + count - 1, 9)
            return start == end ? "⌘\(start)" : "⌘\(start)..\(end)"
        }
        func pinRow(_ tab: SpacesInput.Tab, prefix: String) -> SpacesRow {
            let space = input.spaces.first { $0.id == tab.space }
            let header = space.map { space in
                let depth = space.collapsed || chrome.collapsedSpaces.contains(space.id) ? 1 : 0
                return SpaceScope(space, input: input, overlay: overlay, filter: filter, depth: depth, includeAgents: true).rollup(tab, nest: true).count > 0
            } ?? false
            let state = mark(tab, overlay.tabs[tab.id] ?? Overlay.Tag(), foldable: header)
            return SpacesRow(id: prefix + tab.id, kind: .tab, glyph: state.glyph, tone: state.tone,
                             title: tab.label, trailing: space?.name ?? tab.space, tab: tab.id)
        }
        let agents = pinTabs(input.tabs, agents: true)
        let pins = pinTabs(input.tabs, agents: false)
        // The AGENTS section heads the list itself; an "agents" title above it reads twice.
        var out = agents.isEmpty ? [SpacesRow(id: "agents", kind: .title, title: "agents")] : []
        if !agents.isEmpty {
            out.append(SpacesRow(id: "agentpins", kind: .section, title: "AGENTS", trailing: hint(start: 1, count: agents.count)))
            out += agents.map { pinRow($0, prefix: "agent:") }
        }
        if !pins.isEmpty {
            out.append(SpacesRow(id: "pinned", kind: .section, title: "PINNED", trailing: hint(start: agents.count + 1, count: pins.count)))
            out += pins.map { pinRow($0, prefix: "pinned:") }
        }
        if !choices.isEmpty { out.append(SpacesRow(id: "goal", kind: .goal, title: "goal " + (filter?.replacingOccurrences(of: ":", with: " · ") ?? "All"), trailing: filter == nil ? "▾" : "✕")) }
        let hidden = input.spaces.filter { $0.collapsed || chrome.collapsedSpaces.contains($0.id) }
        let visible = input.spaces.filter { !$0.collapsed && !chrome.collapsedSpaces.contains($0.id) }
        // Preserve the client pin partition, then rank stably within each partition.
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
            let tabs = scope.tabs, sectioned = scope.sectioned, background = scope.background
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
                // As Rust push_lane: grouped lanes' runs roll up too, so a folded header carries their "!".
                let attentions = [t.attention] + childTags.map(\.attention) + runs.map(\.attention) + grouped.map { tag($0).attention }
                    + grouped.flatMap { tag($0).runs.map(\.attention) }
                let rank = ["none": 0, "warn": 1, "act": 2]
                let attention = attentions.max { (rank[$0] ?? 0) < (rank[$1] ?? 0) } ?? "none"
                let expandable = !children.isEmpty || !runs.isEmpty || !grouped.isEmpty
                // As Rust factory_expanded (C45/C46): a lane opens on the user's expand or while the
                // focused tab sits inside; running work and a child that needs action leave it folded
                // (Alex, 2026-10-05: "default collapse everything workflows so we only see talking agent").
                let focusedInside = (children + grouped + rollup.groupedWorkflows).contains { $0.id == input.focusedTab }
                let agents = rollup.agents, flows = rollup.flows, count = agents + flows
                // As Rust: only live work makes a header foldable, and a grouped lane inside an open
                // parent always shows its own workflows and runs (it has no fold of its own).
                let foldable = !inside && count > 0
                let open = inside || ((chrome.expandedTabs.contains(tab.id) || focusedInside || (SpacesTree.actionUnfoldsLane && attention == "act")) && !chrome.collapsedTabs.contains(tab.id))
                let state = SpacesTree.mark(tab, t, foldable: foldable)
                let idle = state.idle
                var name = t.name ?? tab.label
                if t.kind == "workflow", name.hasPrefix("wf ") { name = String(name.dropFirst(3)) }
                if t.kind == "lane" {
                    if name.lowercased().hasPrefix("[scoping] ") { name = String(name.dropFirst(10)) }
                    if let range = name.range(of: " · ", options: .backwards), ["scoping", "implementing", "reviewing", "monitoring", "closed"].contains(String(name[range.upperBound...]).lowercased()) { name = String(name[..<range.lowerBound]) }
                }
                var trailing = t.badge ?? t.summary ?? (idle ? t.idle_reason ?? "idle" : "")
                var link: String?
                if t.section == "reviewing" { trailing = t.review_url == nil ? "no link" : "review ↗"; link = t.review_url }
                else if t.section == "scoping", let url = t.scope_url { trailing = "scope ↗"; link = url }
                if expandable {
                    let words = [agents > 0 ? "\(agents) agent" + (agents == 1 ? "" : "s") : nil,
                                 flows > 0 ? "\(flows) workflow" + (flows == 1 ? "" : "s") : nil].compactMap { $0 }
                    trailing = count > 0 ? words.joined(separator: " · ") : (idle ? t.idle_reason ?? "idle" : "")
                    // The terminal golden truncates the inbox suffix at 25 columns;
                    // native rows retain the complete orchestrator hint.
                    if t.kind == "orchestrator", let summary = t.summary, !summary.isEmpty { trailing = count > 0 ? trailing + " · " + summary : summary }
                    if t.section == "scoping", t.scope_url != nil { trailing = "scope ↗" }
                }
                let workflow = t.kind == "workflow"
                if workflow, !t.done, let phase = t.phase, !phase.isEmpty {
                    let progress = [phase.lowercased(), t.started.map { age(now - $0) }].compactMap { $0 }.joined(separator: " · ")
                    trailing = [trailing, progress].filter { !$0.isEmpty }.joined(separator: " · ")
                }
                out.append(SpacesRow(id: "tab:" + tab.id, kind: .tab, depth: level, chevron: foldable ? (open ? "open" : "closed") : "none", glyph: state.glyph, tone: state.tone, title: name, trailing: trailing, alert: attention, link: link, tab: tab.id, toggleKey: foldable ? "tab:" + tab.id : nil, dim: idle || t.done || t.kind == "advisor" || t.mode == "parked"))
                if open {
                    for child in grouped { appendTab(child, level + 1, inside: true) }
                    for child in children { appendTab(child, level + 1, nest: false) }
                    for run in runs {
                        let progress = run.done ? "" : [run.phase?.lowercased(), run.started.map { age(now - $0) }].compactMap { $0 }.filter { !$0.isEmpty }.joined(separator: " · ")
                        out.append(SpacesRow(id: "run:" + run.id, kind: .run, depth: level + 1, glyph: run.done ? (run.attention == "act" ? "✗" : "✓") : "◐", tone: run.done && run.attention == "act" ? "blocked" : run.done ? "mute" : "working", title: run.name ?? run.id, trailing: [run.badge, progress.isEmpty ? nil : progress].compactMap { $0 }.joined(separator: " · "), alert: run.attention, tab: tab.id, dim: run.done))
                    }
                }
            }
            var first = true
            var hiddenCount = 0
            func section(_ label: String, _ members: [SpacesInput.Tab], shortcut: String) {
                guard !members.isEmpty else { return }
                let key = space.id + ":" + label
                if let focus = chrome.focusedSection[space.id], focus != label && label != "ORCHESTRATOR" { hiddenCount += members.count; return }
                let closed = chrome.collapsedSections.contains(key) && chrome.focusedSection[space.id] == nil
                out.append(SpacesRow(id: "section:" + key, kind: .section, depth: depth + 1, chevron: sectioned ? (closed ? "closed" : "open") : "none", title: label, trailing: closed ? String(members.count) + (shortcut.contains("⌘") ? " " + shortcut : "") : shortcut, alert: closed && members.contains { tag($0).attention == "act" || $0.status == "blocked" } ? "act" : "none", toggleKey: sectioned ? "section:" + key : nil))
                if !closed { for tab in members { appendTab(tab, depth + 1) } }
            }
            section("ORCHESTRATOR", orch + lanes.filter { tag($0).section == "orchestrator" && tag($0).mode == "active" && root($0) == nil }, shortcut: "⌘0")
            if sectioned {
                for (value, label) in [("reviewing", "READY FOR REVIEW"), ("scoping", "SCOPING"), ("implementing", "IMPLEMENTING"), ("monitoring", "MONITORING")] {
                    let members = lanes.filter { tag($0).mode == "active" && root($0) == nil && (tag($0).section ?? "implementing") == value } + (value == "implementing" ? ordinary : [])
                    let hint = value == "reviewing" ? String(members.count) : first && pins.isEmpty && agents.isEmpty ? "⌘1..9" : ""
                    if !members.isEmpty && value != "reviewing" { first = false }
                    section(label, members, shortcut: hint)
                }
            } else {
                section("LANES", lanes.filter { tag($0).mode == "active" && root($0) == nil } + workflows.filter { parent($0) == nil } + ordinary, shortcut: pins.isEmpty && agents.isEmpty ? "⌘1..9" : "")
            }
            for (group, members) in [("services", lanes.filter { tag($0).mode == "auto" } + (sectioned ? workflows.filter { parent($0) == nil } : [])), ("parked", lanes.filter { tag($0).mode == "parked" }), ("closed", sectioned ? lanes.filter { tag($0).mode == "active" && tag($0).section == "closed" } : []), ("background", background)] {
                guard !members.isEmpty else { continue }
                if chrome.focusedSection[space.id] != nil { hiddenCount += members.count; continue }
                let key = space.id + ":" + group
                let open = chrome.expandedGroups.contains(key)
                // As Rust group_state: a bool set only by Act or Blocked, over the members, their runs and
                // every workflow whose parent is a member (tree.rs ~990-1015).
                let memberIds = Set(members.map(\.id))
                let groupLanes = lanes.filter { memberIds.contains($0.id) || root($0).map(memberIds.contains) ?? false }
                let groupIds = Set(groupLanes.map(\.id))
                // Completed workflows count too (Rust all_workflows).
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
        // As Rust group_spaces: named groups in order, manual order inside each, ungrouped spaces after.
        let groups = overlay.spaceGroups.filter { !$0.name.trimmingCharacters(in: .whitespaces).isEmpty }
        var grouped = Overlay(); grouped.spaceGroups = groups
        var members = groups.map { _ in [SpacesInput.Space]() }
        var rest = [SpacesInput.Space]()
        for space in ordered {
            if let index = grouped.spaceGroup(id: space.id, label: space.name) { members[index].append(space) } else { rest.append(space) }
        }
        for (group, spaces) in zip(groups, members) where !spaces.isEmpty {
            out.append(SpacesRow(id: "spacegroup:" + group.name, kind: .title, title: group.name))
            // Explicit client member order wins over server rank; unlisted members retain rank order.
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
        for (i, host) in overlay.hosts.enumerated() { out.append(SpacesRow(id: "host:\(i)", kind: .footerHost, title: host.name, trailing: (host.summary ?? "").replacingOccurrences(of: "load ", with: "").replacingOccurrences(of: " live", with: ""), alert: host.attention)) }
        return out
    }
}

/// One space's tab roles and each header's live children, shared by the space's rows and the
/// pinned section so a pinned lane rolls up exactly what its header in the space does.
private struct SpaceScope {
    let overlay: Overlay
    let leader: String?
    let tabs: [SpacesInput.Tab]; let sectioned: Bool; let background: [SpacesInput.Tab]
    let orch: [SpacesInput.Tab]; let lanes: [SpacesInput.Tab]; let workflows: [SpacesInput.Tab]; let ordinary: [SpacesInput.Tab]

    init(_ space: SpacesInput.Space, input: SpacesInput, overlay: Overlay, filter: String?, depth: Int, includeAgents: Bool = false) {
        self.overlay = overlay
        // Priority affects display only, not the original orchestrator used for implicit parents.
        leader = input.tabs.first { $0.space == space.id && overlay.tabs[$0.id]?.kind == "orchestrator" && overlay.tabs[$0.id]?.done != true }?.id
        let all = input.tabs.filter { $0.space == space.id }.enumerated().sorted { a, b in
            if a.element.sortRank != b.element.sortRank { return a.element.sortRank < b.element.sortRank }
            return a.offset < b.offset
        }.map(\.element)
        let sectioned = all.contains { overlay.tabs[$0.id]?.section != nil }
        let tabs = all.filter { tab in
            guard includeAgents || tab.role != "agent" else { return false }
            guard sectioned, depth == 0, let filter else { return true }
            guard let tag = overlay.tabs[tab.id] else { return false }
            let bits = filter.split(separator: ":", maxSplits: 1).map(String.init)
            return tag.kind == "orchestrator" || tag.mode == "auto" || (tag.goal == bits[0] && (bits.count == 1 || tag.goal_area == bits[1]))
        }
        func tag(_ tab: SpacesInput.Tab) -> Overlay.Tag { overlay.tabs[tab.id] ?? Overlay.Tag() }
        let foreground = tabs.filter { !tag($0).done && tag($0).kind != "advisor" }
        self.tabs = tabs; self.sectioned = sectioned
        background = tabs.filter { let t = tag($0); return t.kind == "advisor" || (t.done && t.kind != "workflow" && !(t.kind == "lane" && t.mode != "active")) }
        orch = foreground.filter { tag($0).kind == "orchestrator" }
        lanes = tabs.filter { tag($0).kind == "lane" && (!tag($0).done || tag($0).mode != "active") }
        workflows = foreground.filter { tag($0).kind == "workflow" }
        ordinary = foreground.filter { !["orchestrator", "lane", "workflow"].contains(tag($0).kind) }
    }

    func tag(_ tab: SpacesInput.Tab) -> Overlay.Tag { overlay.tabs[tab.id] ?? Overlay.Tag() }
    func parent(_ tab: SpacesInput.Tab) -> String? { let p = tag(tab).parent; return lanes.contains { $0.id == p } ? p : leader }
    func root(_ tab: SpacesInput.Tab) -> String? {
        guard tag(tab).kind == "lane", tag(tab).mode == "active", tag(tab).section != "scoping" else { return nil }
        var seen = Set([tab.id]); var current = tab
        // Edges exist only from active, non-scoping lanes (tree.rs direct_parent), so a parked lane ends the walk.
        func edge(_ t: SpacesInput.Tab) -> Bool { tag(t).kind == "lane" && tag(t).mode == "active" && tag(t).section != "scoping" }
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
    /// A header's grouped lanes, child workflows and live runs; `nest` false is a row with none.
    func rollup(_ tab: SpacesInput.Tab, nest: Bool) -> Rollup {
        let grouped = nest ? lanes.filter { root($0) == tab.id } : []
        let children = nest ? workflows.filter { parent($0) == tab.id } : []
        // A run reads done once SpacesTree.runDone says so, here and in the run rows drawn from `runs`.
        func settled(_ owner: SpacesInput.Tab) -> [Overlay.Run] {
            tag(owner).runs.map { run in var run = run; run.done = SpacesTree.runDone(run, of: owner); return run }
        }
        let runs = nest ? settled(tab) : []
        let groupedWorkflows = workflows.filter { w in grouped.contains { $0.id == parent(w) } }
        // agent:<id> runs and grouped lanes are agents; workflow tabs and other runs are workflows.
        let live = (runs + grouped.flatMap { settled($0) }).filter { !$0.done }
        let agents = grouped.count + live.filter { $0.id.hasPrefix("agent:") }.count
        let flows = children.count + groupedWorkflows.count + live.filter { !$0.id.hasPrefix("agent:") }.count
        return Rollup(grouped: grouped, children: children, groupedWorkflows: groupedWorkflows, runs: runs, agents: agents, flows: flows)
    }
}
