import Foundation

struct SpacesInput: Codable {
    struct Space: Codable { var id: String; var name: String; var pinned = false; var collapsed = false; var sortRank: UInt32 = 0; var parked = false
        init(id: String, name: String, pinned: Bool = false, collapsed: Bool = false, sortRank: UInt32 = 0, parked: Bool = false) { self.id = id; self.name = name; self.pinned = pinned; self.collapsed = collapsed; self.sortRank = sortRank; self.parked = parked }
        init(from decoder: Decoder) throws { let c = try decoder.container(keyedBy: Field.self); id = try c.decode(String.self, forKey: Field("id")); name = try c.decode(String.self, forKey: Field("name")); pinned = c.value("pinned", false); collapsed = c.value("collapsed", false); sortRank = c.value("sortRank", 0); parked = c.value("parked", false) }
    }
    struct Agent: Codable { var status: String; var parent: String? = nil; var explicitParent: String? = nil; var handsOn = false }
    struct Tab: Codable { var id: String; var space: String; var label: String; var agents: [Agent] = []; var focused = false; var status = "unknown"; var pinIndex: Int? = nil; var work: String? = nil; var role: String? = nil; var sortRank: UInt32 = 0; var request: String? = nil
        var agentName: String? = nil; var avatar: String? = nil; var hidden = false; var homeLocation: String? = nil
        var runsOn: String? = nil
        var home: HomeTag? = nil

        init(id: String, space: String, label: String, agents: [Agent] = [], focused: Bool = false, status: String = "unknown", pinIndex: Int? = nil, work: String? = nil, role: String? = nil, sortRank: UInt32 = 0, request: String? = nil, hidden: Bool = false, homeLocation: String? = nil, runsOn: String? = nil, home: HomeTag? = nil, agentName: String? = nil, avatar: String? = nil) { self.agentName = agentName; self.avatar = avatar; self.hidden = hidden; self.homeLocation = homeLocation; self.runsOn = runsOn; self.home = home; self.id = id; self.space = space; self.label = label; self.agents = agents; self.focused = focused; self.status = status; self.pinIndex = pinIndex; self.work = work; self.role = role; self.sortRank = sortRank; self.request = request }
        init(from decoder: Decoder) throws {
            let c = try decoder.container(keyedBy: Field.self)
            id = try c.decode(String.self, forKey: Field("id")); space = try c.decode(String.self, forKey: Field("space")); label = try c.decode(String.self, forKey: Field("label"))
            agents = c.value("agents", []); focused = c.value("focused", false); status = c.value("status", "unknown"); pinIndex = c.optional("pinIndex"); work = c.optional("work"); role = c.optional("role"); sortRank = c.value("sortRank", 0); request = c.optional("request"); hidden = c.value("hidden", false); homeLocation = c.optional("homeLocation"); runsOn = c.optional("runsOn"); home = c.optional("home"); agentName = c.optional("agentName"); avatar = c.optional("avatar")
        }
    }
    var spaces: [Space]; var tabs: [Tab]; var focusedTab: String?
    var automations: [AutomationSummary]? = nil
}

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
    struct Pulse: Codable, Equatable {
        var line = ""; var drifting = false
        init(from decoder: Decoder) throws { let c = try decoder.container(keyedBy: Field.self); line = c.value("line", ""); drifting = c.value("drifting", false) }
    }
    struct Tag: Codable {
        var kind = "unknown"; var home: HomeTag?; var mode = "active"; var name: String?; var parent: String?; var goal: String?; var goal_area: String?
        var scope_url: String?; var review_url: String?; var summary: String?; var pulse: Pulse?; var attention = "none"; var busy = false; var idle_reason: String?
        var runs: [Run] = []; var phase: String?; var started: Double?; var done = false; var badge: String?
        init() {}
        init(from decoder: Decoder) throws {
            let c = try decoder.container(keyedBy: Field.self)
            kind = c.value("kind", "unknown"); home = c.optional("home"); mode = c.value("mode", "active");
            if !["orchestrator", "lane", "workflow", "advisor"].contains(kind) { kind = "unknown" }
            if !["parked", "auto"].contains(mode) { mode = "active" }
             name = c.optional("name"); parent = c.optional("parent")
            goal = c.optional("goal"); goal_area = c.optional("goal_area"); scope_url = c.optional("scope_url"); review_url = c.optional("review_url")
            summary = c.optional("summary"); pulse = c.optional("pulse"); if pulse?.line.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty == true { pulse = nil }; attention = c.value("attention", "none"); busy = c.value("busy", false); idle_reason = c.optional("idle_reason")
            runs = c.value("runs", []); phase = c.optional("phase"); started = c.optional("started"); done = c.value("done", false); badge = c.optional("badge")
        }
    }
    struct Space: Codable {
        var attention = "none"; var summary: String?; var target_tab: String?
        init(from decoder: Decoder) throws { let c = try decoder.container(keyedBy: Field.self); attention = c.value("attention", "none"); summary = c.optional("summary"); target_tab = c.optional("target_tab") }
    }
    struct Host: Codable {
        var name = ""; var summary: String?; var attention = "none"; var url: String?; var detail: String?
        init(from decoder: Decoder) throws { let c = try decoder.container(keyedBy: Field.self); name = c.value("name", ""); summary = c.optional("summary"); attention = c.value("attention", "none"); url = c.optional("url"); detail = c.optional("detail") }
    }
    struct SpaceGroup: Codable, Equatable {
        var name = ""; var spaces: [String] = []
        init(name: String, spaces: [String]) { self.name = name; self.spaces = spaces }
        init(from decoder: Decoder) throws { let c = try decoder.container(keyedBy: Field.self); name = c.value("name", ""); spaces = c.value("spaces", []) }
    }
    var home_rev: Int? = nil
    var tabs: [String: Tag] = [:]; var spaces: [String: Space] = [:]; var usage: [Host] = []; var hosts: [Host] = []; var spaceGroups: [SpaceGroup] = []
    init() {}
    init(from decoder: Decoder) throws { let c = try decoder.container(keyedBy: Field.self); home_rev = c.optional("home_rev"); tabs = c.value("tabs", [:]); spaces = c.value("spaces", [:]); usage = c.value("usage", []); hosts = c.value("hosts", []); spaceGroups = c.value("space_groups", []) }
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
struct Field: CodingKey { var stringValue: String; var intValue: Int? { nil }; init(_ s: String) { stringValue = s }; init?(stringValue: String) { self.init(stringValue) }; init?(intValue: Int) { return nil } }
extension KeyedDecodingContainer where Key == Field {
    func value<T: Decodable>(_ name: String, _ fallback: T) -> T { (try? decode(T.self, forKey: Field(name))) ?? fallback }
    func optional<T: Decodable>(_ name: String) -> T? { try? decodeIfPresent(T.self, forKey: Field(name)) }
}

struct SpacesChrome: Codable {
    var collapsedSections: Set<String> = []; var expandedGroups: Set<String> = []; var expandedTabs: Set<String> = []; var collapsedTabs: Set<String> = []
    var pinnedSpaces: Set<String> = []; var collapsedSpaces: Set<String> = []; var expandedParkedSpaces: Set<String> = []; var hiddenExpanded = false; var hiddenAgentsExpanded = false; var goalFilter: String?; var focusedSection: [String: String] = [:]
    init() {}
    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: Field.self)
        collapsedSections = c.value("collapsedSections", []); expandedGroups = c.value("expandedGroups", []); expandedTabs = c.value("expandedTabs", []); collapsedTabs = c.value("collapsedTabs", [])
        pinnedSpaces = c.value("pinnedSpaces", []); collapsedSpaces = c.value("collapsedSpaces", []); expandedParkedSpaces = c.value("expandedParkedSpaces", []); hiddenExpanded = c.value("hiddenExpanded", false); hiddenAgentsExpanded = c.value("hiddenAgentsExpanded", false); goalFilter = c.optional("goalFilter"); focusedSection = c.value("focusedSection", [:])
        if let legacy: String = c.optional("focusedSection"), let split = legacy.lastIndex(of: ":") { focusedSection[String(legacy[..<split])] = String(legacy[legacy.index(after: split)...]) }
    }
    func anySpaceExpanded(_ spaces: [SpacesInput.Space]) -> Bool {
        spaces.contains { !$0.collapsed && !collapsedSpaces.contains($0.id) && (!$0.parked || expandedParkedSpaces.contains($0.id)) }
    }
    mutating func toggleAllSpaces(_ spaces: [SpacesInput.Space]) {
        if anySpaceExpanded(spaces) {
            collapsedSpaces.formUnion(spaces.map(\.id))
            expandedParkedSpaces.removeAll()
        } else {
            collapsedSpaces.subtract(spaces.map(\.id))
            expandedParkedSpaces.formUnion(spaces.filter(\.parked).map(\.id))
        }
    }
    mutating func reveal(space: String, parked: Bool, selected: String? = nil, rows: [SpacesRow] = [], hiddenAgent: Bool = false) -> Bool {
        if hiddenAgent || selected.map({ selected in rows.contains { $0.id == "hiddenagent:" + selected } }) == true {
            let changed = !hiddenAgentsExpanded; hiddenAgentsExpanded = true; return changed
        }
        if let selected, rows.contains(where: { $0.id == "pinned:" + selected || $0.id == "agent:" + selected }) { return false }
        let unfolded = collapsedSpaces.remove(space) != nil
        let unparked = parked && expandedParkedSpaces.insert(space).inserted
        return unfolded || unparked
    }
    mutating func toggle(_ key: String, open: Bool? = nil) {
        if key.hasPrefix("all:") { focusedSection.removeValue(forKey: String(key.dropFirst(4))); return }
        if key == "hiddenagents" { hiddenAgentsExpanded.toggle(); return }
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
    enum Kind: String { case title, goal, space, section, group, tab, run, automation, hidden, footerUsage, footerHost, divider }
    var id: String; var kind: Kind; var depth = 0; var chevron = "none"; var glyph = ""; var tone = "mute"; var title: String; var trailing = ""
    var alert = "none"; var link: String?; var tab: String?; var toggleKey: String?; var dim = false
    var seats = 0
    var homeState: String?
    var report: String?
    var badge: String?
    var badgeState: String?
    var request: String?
    var home: String?
    var runsOn: String?
    var place: String? { runsOn.flatMap { $0 == "box" || badge == nil ? $0 : nil } }
    var setsHidden: Bool? { id.hasPrefix("agent:") ? true : id.hasPrefix("hiddenagent:") ? false : nil }
    var face: Face?
    var hover: String?
    struct Face: Equatable { var initial: String; var tint: Int; var avatar: String? }
    var faceDot: String? {
        guard face != nil else { return nil }
        if request != nil { return "accent" }
        switch tone {
        case "working": return "working"
        case "blocked": return "accent"
        case "done": return "ok"
        default: return nil
        }
    }
    var trailingRequest: String? { face == nil ? request : nil }
    var dump: String {
        let fields = [kind.rawValue, id, String(depth), chevron, glyph, tone, title, trailing, alert, link ?? "", tab ?? "", toggleKey ?? "", dim ? "dim" : ""]
        var faceField: [String] = []
        if let face {
            let picture: String = face.avatar.map { ":" + $0 } ?? ""
            faceField = ["face:" + face.initial + ":" + String(face.tint) + picture]
        }
        var dotField: [String] = []
        if id == "hiddenagents", alert == "act" { dotField = ["dot:accent"] } else if let faceDot { dotField = ["dot:" + faceDot] } else if let trailingRequest { dotField = ["request:" + trailingRequest] }
        let badgeField: [String] = badge.map { ["@" + $0 + (badgeState.map { ":" + $0 } ?? "")] } ?? []
        var output = fields
        output.append(contentsOf: faceField)
        output.append(contentsOf: dotField)
        output.append(contentsOf: badgeField)
        if let place { output.append("on:" + place) }
        if let home { output.append("home:" + home) }
        return output.joined(separator: "|")
    }
}
