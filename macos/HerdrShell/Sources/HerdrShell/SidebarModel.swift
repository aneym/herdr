import AppKit
import Foundation

/// P10: the sidebar as data. `SidebarModel.build` turns herdr's snapshot and the
/// classified rows into the flat list of lines the sidebar draws, so the view has
/// nothing to decide and the state dump (scripts/check_p10.py) asserts exactly the
/// rows, order and fold state on screen. The shape follows the mock,
/// ~/.claude/pretty-docs/factory-devenv-mock-2026-09-28.html:
///
///     SPACES                 ⌘⇧1..9     one line per space; the current one open
///     ▾ ◆ agent-rails        factory    (`plain` when it has no orchestrator)
///     ORCHESTRATOR           inbox 3    the current space's rows sit right under its row
///     ● rails orchestrator   [Studio]
///     LANES                  7 open
///     ▾ ● recruiter          2 wf       workflows fold into their lane as a count
///         ● wf recruiter-2320  [PC]
///     ▸ background           3          finished and advisor tabs, collapsed
///       step back · toyo research · recruiting email ✓
///     ▸ ◆ agent-lb           1          other spaces follow in herdr's order: a count, `●` working, `● n` when n rows want you
///     ▸ hidden               3          hidden spaces, collapsed, last
///
/// A row comes forward only when it fails or asks: a workflow that is blocked or
/// failed opens its lane on its own, and an advisor that asks is a lane again.
struct SidebarLine: Identifiable, Equatable {
    enum Kind: String { case header, space, hidden, orchestrator, lane, workflow, background, note, area, focus, parked }
    enum Tone: String { case normal, ok, warn, mute }

    var id: String
    var kind: Kind
    var depth = 0
    /// nil: no chevron; true: open (▾); false: closed (▸).
    var chevron: Bool?
    var glyph = ""
    var glyphTone: Tone = .normal
    var title: String
    var titleKind: TabRow.Kind?      // colors the title like the mock (orchestrator, lane, workflow)
    var trailing = ""
    var trailingTone: Tone = .mute
    var host: String?
    var tab: String?                 // tab to select on click
    var space: String?               // space whose target tab a click selects
    var toggle: String?              // fold id a chevron click flips
    var selected = false
    var dim = false
    /// Areas mode (P15). Nil on a spaces-mode line.
    var area: String?
    var role: String?
    var stage: String?
    var badge = ""
    var color: String?
    /// Parked rows (P26): the park note and date line under the title, and a Resume button.
    var parked = false
    var parkNote: String?

    /// The row as text, the way the mock draws it. Fold state is the chevron.
    var text: String {
        var left = [String]()
        if let c = chevron { left.append(c ? "▾" : "▸") }
        if !glyph.isEmpty { left.append(glyph) }
        left.append(title)
        var s = String(repeating: "  ", count: depth) + left.joined(separator: " ")
        if !trailing.isEmpty { s += "  " + trailing }
        if !badge.isEmpty { s += "  " + badge }
        if let h = host { s += "  [\(h)]" }
        return s
    }

    var dump: [String: Any] {
        ["id": id, "kind": kind.rawValue, "depth": depth, "text": text, "title": title,
         "chevron": chevron.map { $0 as Any } ?? NSNull(), "glyph": glyph, "trailing": trailing,
         "host": host ?? NSNull(), "tab": tab ?? NSNull(), "space": space ?? NSNull(),
         "area": area ?? NSNull(), "role": role ?? NSNull(), "stage": stage ?? NSNull(),
         "selected": selected, "dim": dim, "parked": parked, "park_note": parkNote ?? NSNull()]
    }
}

enum SidebarModel {
    /// The chord in Resources/keymap.json (Alex's Ghostty chords), not the mock's ⌘⇧1..9,
    /// which macOS keeps for screenshots.
    static let spaceChord = "⌘⇧1..9"

    /// Facts about one tab that the row classifier does not carry.
    struct Facts {
        var attention = false     // asks (blocked) or failed: the row comes forward
        var failed = false
        var finished = false
        var advisor = false
        var devLoop = false
        var inbox: Int?
    }

    static func facts(_ s: Snapshot) -> [String: Facts] {
        let byTab = Dictionary(grouping: s.agents, by: { $0.tab_id })
        var out: [String: Facts] = [:]
        for t in s.tabs {
            let agents = byTab[t.tab_id] ?? []
            func token(_ k: String) -> String? {
                for a in agents {
                    if let v = a.tokens?[k]?.trimmingCharacters(in: .whitespacesAndNewlines), !v.isEmpty { return v }
                }
                return nil
            }
            var f = Facts()
            let status = agents.first?.agent_status ?? t.agent_status ?? "unknown"
            let state = token("state")?.lowercased() ?? ""
            f.failed = ["failed", "error", "quarantined", "stalled"].contains(state)
            let ask = token("attention")?.lowercased() ?? ""
            f.attention = status == "blocked" || f.failed || !(ask.isEmpty || ["none", "false", "0", "no"].contains(ask))
            f.finished = status == "done" || ["done", "finished"].contains(token("phase")?.lowercased() ?? "")
            f.advisor = token("kind")?.lowercased() == "advisor"
            f.devLoop = token("dev_loop") != nil
            f.inbox = token("inbox").flatMap { Int($0) }
            out[t.tab_id] = f
        }
        return out
    }

    static func flag(_ v: String?) -> Bool {
        guard let v = v?.lowercased() else { return false }
        return ["1", "true", "yes", "on"].contains(v)
    }

    /// Spaces the list shows, in herdr's order, and the hidden ones.
    static func spaces(_ s: Snapshot) -> (visible: [Snapshot.Workspace], hidden: [Snapshot.Workspace]) {
        let all = s.workspaces.sorted { $0.number < $1.number }
        return (all.filter { !flag($0.tokens?["hidden"]) }, all.filter { flag($0.tokens?["hidden"]) })
    }

    /// The space the window is in: the selected tab's, else herdr's focused one, else the first.
    static func currentSpace(_ s: Snapshot, selectedTab: String?) -> String? {
        if let t = selectedTab, let ws = s.tabs.first(where: { $0.tab_id == t })?.workspace_id { return ws }
        return (s.workspaces.first { $0.focused == true } ?? s.workspaces.sorted { $0.number < $1.number }.first)?.workspace_id
    }

    /// The tab a space click or ⌘⇧n selects: herdr's active tab there, else its first tab.
    static func targetTab(_ s: Snapshot, space: String) -> String? {
        if let a = s.workspaces.first(where: { $0.workspace_id == space })?.active_tab_id,
           s.tabs.contains(where: { $0.tab_id == a }) { return a }
        return s.tabs.filter { $0.workspace_id == space }.min { $0.number < $1.number }?.tab_id
    }

    static func build(snapshot s: Snapshot?, orchestrators: [TabRow], lanes: [TabRow], workflows: [TabRow],
                      selectedTab: String?, manualOpen: [String: Bool], parked: Set<String> = []) -> [SidebarLine] {
        guard let s else { return [] }
        var facts = Self.facts(s)
        // A parked tab never asks for you: it does not open its lane or count on its space.
        for t in parked { facts[t]?.attention = false }
        let space = currentSpace(s, selectedTab: selectedTab)
        let tabSpace = Dictionary(s.tabs.map { ($0.tab_id, $0.workspace_id) }, uniquingKeysWith: { a, _ in a })
        func inSpace(_ r: TabRow) -> Bool { tabSpace[r.id] == space }
        func f(_ r: TabRow) -> Facts { facts[r.id] ?? Facts() }
        func open(_ id: String, auto: Bool) -> Bool { manualOpen[id] ?? auto }
        func done(_ r: TabRow) -> Bool { r.status == "done" || f(r).finished }

        let orch = orchestrators.filter(inSpace)
        var foreground: [TabRow] = [], background: [TabRow] = []
        for l in lanes.filter(inSpace) {
            let quiet = (f(l).advisor && !f(l).attention) || (done(l) && l.children.allSatisfy(done))
            if quiet { background.append(l) } else { foreground.append(l) }
        }
        var loose: [TabRow] = []
        for w in workflows.filter(inSpace) {
            if done(w) { background.append(w) } else { loose.append(w) }
        }

        // The list is spaces in herdr's order with the current space's rows directly under its own
        // row (mock frames "agent-rails" and "homebase"), then `hidden`. `head` runs through the
        // current space's row, `tail` is the remaining spaces and `hidden`; `out` (below) is the
        // current space's content and sits between them.
        var head: [SidebarLine] = [], tail: [SidebarLine] = []
        head.append(SidebarLine(id: "hdr:spaces", kind: .header, title: "SPACES", trailing: spaceChord))
        let sp = spaces(s)
        var passedCurrent = false
        for w in sp.visible {
            let l = spaceLine(w, current: w.workspace_id == space, snapshot: s, facts: facts, hasOrchestrator: !orch.isEmpty)
            if passedCurrent { tail.append(l) } else { head.append(l) }
            if w.workspace_id == space { passedCurrent = true }
        }
        if !sp.hidden.isEmpty {
            let o = open("hidden", auto: false)
            tail.append(SidebarLine(id: "hidden", kind: .hidden, chevron: o, title: "hidden",
                                    trailing: "\(sp.hidden.count)", toggle: "hidden", dim: true))
            if o {
                for w in sp.hidden {
                    var l = spaceLine(w, current: w.workspace_id == space, snapshot: s, facts: facts, hasOrchestrator: false)
                    l.depth = 1; l.chevron = nil
                    tail.append(l)
                }
            }
        }

        var out: [SidebarLine] = []

        // MARK: orchestrator, lanes, loose workflows
        func group(_ r: TabRow, depth: Int, dim: Bool = false) {
            let attn = r.children.contains { f($0).attention }
            let o = r.children.isEmpty ? false : open("tab:\(r.id)", auto: attn)
            var l = tabLine(r, facts: f(r), depth: depth, selected: r.id == selectedTab)
            l.dim = l.dim || dim
            if !r.children.isEmpty {
                l.chevron = o
                l.toggle = "tab:\(r.id)"
                l.trailing = "\(r.children.count) wf"
                l.trailingTone = .mute
            }
            out.append(l)
            if o {
                for c in r.children {
                    out.append(tabLine(c, facts: f(c), depth: depth + 1, selected: c.id == selectedTab))
                }
            }
        }

        if !orch.isEmpty {
            let inbox = f(orch[0]).inbox
            out.append(SidebarLine(id: "hdr:orchestrator", kind: .header, title: "ORCHESTRATOR",
                                   trailing: inbox.map { "inbox \($0)" } ?? "⌘1"))
            for r in orch { group(r, depth: 0) }
        }
        if !foreground.isEmpty {
            out.append(SidebarLine(id: "hdr:lanes", kind: .header, title: "LANES", trailing: "\(foreground.count) open"))
            for r in foreground { group(r, depth: 0) }
        }
        if !loose.isEmpty {
            out.append(SidebarLine(id: "hdr:workflows", kind: .header, title: "WORKFLOWS", trailing: "\(loose.count)"))
            for r in loose { out.append(tabLine(r, facts: f(r), depth: 0, selected: r.id == selectedTab)) }
        }

        // MARK: background: finished and advisor tabs, collapsed until opened
        if !background.isEmpty {
            let o = open("background", auto: false)
            out.append(SidebarLine(id: "background", kind: .background, chevron: o, title: "background",
                                   trailing: "\(background.count)", toggle: "background", dim: true))
            if o {
                for r in background {
                    var l = tabLine(r, facts: f(r), depth: 1, selected: r.id == selectedTab)
                    l.dim = true
                    out.append(l)
                }
            } else {
                let names = background.map { $0.label + (done($0) ? " ✓" : "") }.joined(separator: " · ")
                out.append(SidebarLine(id: "background:names", kind: .note, depth: 1, title: names, dim: true))
            }
        }
        return head + out + tail
    }

    private static func spaceLine(_ w: Snapshot.Workspace, current: Bool, snapshot s: Snapshot,
                                  facts: [String: Facts], hasOrchestrator: Bool) -> SidebarLine {
        let tabs = s.tabs.filter { $0.workspace_id == w.workspace_id }
        let wants = tabs.filter { facts[$0.tab_id]?.attention == true }.count
        let working = tabs.contains { t in
            (s.agents.first { $0.tab_id == t.tab_id }?.agent_status ?? t.agent_status) == "working"
        }
        var l = SidebarLine(id: "space:\(w.workspace_id)", kind: .space, chevron: current,
                            glyph: flag(w.tokens?["pinned"]) ? "◆" : "◇",
                            title: w.label ?? "space \(w.number)", space: w.workspace_id, selected: current)
        if current {
            l.trailing = hasOrchestrator ? "factory" : "plain"
        } else if wants > 0 {
            l.trailing = "● \(wants)"; l.trailingTone = .warn
        } else if working {
            l.trailing = "●"; l.trailingTone = .ok
        } else {
            l.trailing = "\(tabs.count)"
        }
        return l
    }

    private static func dot(_ status: String, failed: Bool) -> (String, SidebarLine.Tone) {
        if failed { return ("✕", .warn) }
        switch status {
        case "working": return ("●", .ok)
        case "blocked": return ("◐", .warn)
        case "idle": return ("○", .mute)
        case "done": return ("✓", .mute)
        default: return ("·", .mute)
        }
    }

    private static func tabLine(_ r: TabRow, facts f: Facts, depth: Int, selected: Bool) -> SidebarLine {
        let d = dot(f.finished && r.status != "done" ? "done" : r.status, failed: f.failed)
        let kind: SidebarLine.Kind
        switch r.kind { case .orchestrator: kind = .orchestrator; case .lane: kind = .lane; case .workflow: kind = .workflow }
        var l = SidebarLine(id: "tab:\(r.id)", kind: kind, depth: depth, glyph: d.0, glyphTone: d.1,
                            title: r.label + (f.devLoop ? " ⟳" : ""), titleKind: r.kind, tab: r.id, selected: selected)
        // Host badges: on the orchestrator and on every workflow, as in the mock; a lane only
        // when it runs off the default host.
        if r.kind != .lane || r.host != TabClassifier.defaultHost { l.host = r.host }
        if r.kind == .lane, r.children.isEmpty, r.status == "idle", !f.finished {
            l.trailing = "idle"
            l.dim = true
        }
        return l
    }
}

// MARK: areas (P15)

enum AreaChip: String, CaseIterable {
    case all, needs, scoping, building, review, use, parked
}

extension SidebarModel {
    /// One tab the areas list can show. Children stay folded under their owner.
    struct AreaItem {
        var row: TabRow
        var area: String
        var role: String
        var stage: String?
        var name: String
        var color: String
        var number: Int
        var failed: Bool
        var devLoop: Bool
        var park: ParkRecord?
        var children: [AreaItem]
    }

    static func badge(stage: String?, role: String) -> String {
        if role == "desk" || role == "job" { return "In use" }
        switch stage {
        case "scoping": return "Scoping"
        case "implementing": return "Building"
        case "reviewing": return "Ready for review"
        case "monitoring": return "Monitoring"
        default: return ""
        }
    }

    /// Needs you: reviewing, scoping, or blocked. Closed rows are not in the queue.
    /// Rank: blocked, then reviewing, then scoping; ties by area order, then tab number.
    static func focusTabs(snapshot s: Snapshot?, orchestrators: [TabRow], lanes: [TabRow], workflows: [TabRow],
                          catalog: LaneSnapshot) -> [String] {
        guard let s else { return [] }
        let items = areaItems(snapshot: s, orchestrators: orchestrators, lanes: lanes, workflows: workflows, catalog: catalog)
        var flat: [AreaItem] = []
        func walk(_ i: AreaItem) {
            if i.park == nil { flat.append(i) }
            i.children.forEach(walk)
        }
        items.forEach(walk)
        let order = catalog.orderedAreaIds(Set(flat.map(\.area)))
        func bucket(_ i: AreaItem) -> Int? {
            if i.stage == "closed" { return nil }
            if i.row.status == "blocked" { return 0 }
            if i.stage == "reviewing" { return 1 }
            if i.stage == "scoping" { return 2 }
            return nil
        }
        return flat.compactMap { i -> (AreaItem, Int)? in
            guard let b = bucket(i) else { return nil }
            return (i, b)
        }.sorted { a, b in
            if a.1 != b.1 { return a.1 < b.1 }
            let ia = order.firstIndex(of: a.0.area) ?? 0
            let ib = order.firstIndex(of: b.0.area) ?? 0
            if ia != ib { return ia < ib }
            if a.0.number != b.0.number { return a.0.number < b.0.number }
            return a.0.row.id < b.0.row.id
        }.map { $0.0.row.id }
    }

    static func passes(_ i: AreaItem, chip: AreaChip) -> Bool {
        let closed = i.stage == "closed"
        if chip == .parked { return i.park != nil }
        if i.park != nil { return false }
        switch chip {
        case .parked: return false
        case .all: return true
        case .needs:
            if closed { return false }
            return i.stage == "reviewing" || i.stage == "scoping" || i.row.status == "blocked"
        case .scoping: return i.stage == "scoping"
        case .building: return i.stage == "implementing"
        case .review: return i.stage == "reviewing"
        case .use: return !closed && (i.role == "desk" || i.role == "job")
        }
    }

    static func buildAreas(snapshot s: Snapshot?, orchestrators: [TabRow], lanes: [TabRow], workflows: [TabRow],
                           catalog: LaneSnapshot, chip: AreaChip, areaOnly: String?, folded: Set<String>,
                           focusExpanded: Bool, focusCursor: Int?, selectedTab: String?,
                           manualOpen: [String: Bool]) -> [SidebarLine] {
        guard let s else { return [] }
        let facts = Self.facts(s)
        let items = areaItems(snapshot: s, orchestrators: orchestrators, lanes: lanes, workflows: workflows, catalog: catalog)
        // The Parked chip draws its own flat list below, not area groups.
        let shown = chip == .parked ? [] : liveItems(items).filter { passes($0, chip: chip) && (areaOnly == nil || $0.area == areaOnly) }
        let focus = focusTabs(snapshot: s, orchestrators: orchestrators, lanes: lanes, workflows: workflows, catalog: catalog)
        var out: [SidebarLine] = []

        var focusLine = SidebarLine(id: "focus", kind: .focus, chevron: focusExpanded, title: "Focus",
                                    trailing: "\(focus.count)", toggle: "focus")
        if let c = focusCursor, c >= 1, c <= focus.count {
            focusLine.trailing = "\(c) of \(focus.count)"
        }
        out.append(focusLine)
        let byId = Dictionary(items.flatMap { flatItems($0) }.map { ($0.row.id, $0) }, uniquingKeysWith: { a, _ in a })
        if !focusExpanded, let id = focus.first, let i = byId[id] {
            var line = itemLine(i, facts: facts[id] ?? Facts(), depth: 1, selected: id == selectedTab, idPrefix: "focus:")
            line.chevron = nil
            line.toggle = nil
            line.trailing = "next · \(catalog.areaName(i.area))"
            out.append(line)
        }
        if focusExpanded {
            for id in focus {
                guard let i = byId[id] else { continue }
                var line = itemLine(i, facts: facts[id] ?? Facts(), depth: 1, selected: id == selectedTab, idPrefix: "focus:")
                line.chevron = nil
                line.toggle = nil
                out.append(line)
            }
        }

        let order = catalog.orderedAreaIds(Set(shown.map(\.area)))
        for area in order {
            let rows = shown.filter { $0.area == area }
            if rows.isEmpty { continue }
            let open = !folded.contains(area)
            out.append(SidebarLine(id: "area:\(area)", kind: .area, chevron: open, title: catalog.areaName(area),
                                   trailing: "\(rows.count)", toggle: "area:\(area)", area: area,
                                   color: catalog.areaColor(area)))
            if !open { continue }
            func group(_ title: String, _ rows: [AreaItem]) {
                let ordered = orderGroup(title, rows)
                guard !ordered.isEmpty else { return }
                out.append(SidebarLine(id: "sub:\(area):\(title)", kind: .header, title: title, area: area))
                for i in ordered {
                    out.append(contentsOf: groupLines(i, facts: facts, depth: 1, selectedTab: selectedTab, manualOpen: manualOpen))
                }
            }
            group("ORCHESTRATOR", rows.filter { $0.role == "top" || $0.role == "orchestrator" })
            group("PROJECTS", rows.filter { $0.role == "project" })
            group("USE", rows.filter { $0.role == "desk" || $0.role == "job" })
        }

        // MARK: parked: one group at the foot, shut until opened; the Parked chip lists it alone.
        let parked = parkedItems(items).filter { areaOnly == nil || $0.area == areaOnly }
        if chip == .parked {
            out.removeAll { $0.kind == .focus || $0.id.hasPrefix("focus:") }
            for i in parked { out.append(parkedLine(i, facts: facts[i.row.id] ?? Facts(), depth: 0, selected: i.row.id == selectedTab)) }
        } else if !parked.isEmpty {
            let open = manualOpen["parked"] ?? false
            out.append(SidebarLine(id: "parked", kind: .parked, chevron: open, title: "Parked",
                                   trailing: "\(parked.count)", toggle: "parked", dim: true))
            if open {
                for i in parked { out.append(parkedLine(i, facts: facts[i.row.id] ?? Facts(), depth: 1, selected: i.row.id == selectedTab)) }
            }
        }
        return out
    }

    static func parkedCount(snapshot: Snapshot?, orchestrators: [TabRow], lanes: [TabRow], workflows: [TabRow],
                            catalog: LaneSnapshot, areaOnly: String?) -> Int {
        guard let snapshot else { return 0 }
        let items = areaItems(snapshot: snapshot, orchestrators: orchestrators, lanes: lanes, workflows: workflows, catalog: catalog)
        return parkedItems(items).filter { areaOnly == nil || $0.area == areaOnly }.count
    }

    /// Parking belongs to the tab, not its tree; live children of a parked owner become roots.
    private static func liveItems(_ items: [AreaItem]) -> [AreaItem] {
        items.flatMap { item -> [AreaItem] in
            let children = liveItems(item.children)
            // A promoted child with no area of its own stays in its parked owner's area.
            if item.park != nil { return children.map { var c = $0; if c.area == "unsorted" { c.area = item.area }; return c } }
            var live = item
            live.children = children
            return [live]
        }
    }

    /// Every parked item, a parked child pulled out from under a live owner too. Newest park first.
    static func parkedItems(_ items: [AreaItem]) -> [AreaItem] {
        var out: [AreaItem] = []
        func walk(_ i: AreaItem) {
            if i.park != nil { out.append(i) }
            i.children.forEach(walk)
        }
        items.forEach(walk)
        return out.sorted { a, b in
            let da = a.park?.at ?? .distantPast, db = b.park?.at ?? .distantPast
            if da != db { return da > db }
            return a.number < b.number
        }
    }

    private static func parkedLine(_ i: AreaItem, facts f: Facts, depth: Int, selected: Bool) -> SidebarLine {
        var l = itemLine(i, facts: f, depth: depth, selected: selected, idPrefix: "parked:")
        l.parked = true
        l.badge = ""
        l.dim = true
        // p6's bulk parks lead with "Parked 2026-10-02 16:30 ET: "; the date is already shown.
        var note = i.park?.note ?? ""
        if note.hasPrefix("Parked "), let colon = note.range(of: ": ") { note = String(note[colon.upperBound...]) }
        let when = ParkActions.when(i.park?.at)
        l.parkNote = [when, note].filter { !$0.isEmpty }.joined(separator: " · ")
        return l
    }

    private static func orderGroup(_ title: String, _ rows: [AreaItem]) -> [AreaItem] {
        rows.sorted { a, b in
            if title == "USE" {
                let da = a.role == "desk" ? 0 : 1
                let db = b.role == "desk" ? 0 : 1
                if da != db { return da < db }
            }
            let ca = a.stage == "closed" ? 1 : 0
            let cb = b.stage == "closed" ? 1 : 0
            if ca != cb { return ca < cb }
            if a.number != b.number { return a.number < b.number }
            return a.row.id < b.row.id
        }
    }

    private static func flatItems(_ i: AreaItem) -> [AreaItem] {
        [i] + i.children.flatMap(flatItems)
    }

    private static func areaItems(snapshot s: Snapshot, orchestrators: [TabRow], lanes: [TabRow],
                                 workflows: [TabRow], catalog: LaneSnapshot) -> [AreaItem] {
        let facts = Self.facts(s)
        let number = Dictionary(s.tabs.map { ($0.tab_id, $0.number) }, uniquingKeysWith: { a, _ in a })
        let space = Dictionary(s.tabs.map { ($0.tab_id, $0.workspace_id) }, uniquingKeysWith: { a, _ in a })
        func item(_ r: TabRow) -> AreaItem {
            let lane = catalog.lanes[r.id]
            let ws = space[r.id] ?? ""
            let f = facts[r.id] ?? Facts()
            let area = catalog.areaId(tab: r.id, workspace: ws, lane: lane)
            return AreaItem(row: r, area: area,
                            role: catalog.role(tab: r.id, lane: lane), stage: lane?.section,
                            name: catalog.displayName(tab: r.id, lane: lane, fallback: r.label),
                            color: catalog.areaColor(area),
                            number: number[r.id] ?? 0, failed: f.failed, devLoop: f.devLoop,
                            park: catalog.parked[r.id],
                            children: r.children.map(item))
        }
        return (orchestrators + lanes + workflows).map(item)
    }

    private static func groupLines(_ i: AreaItem, facts: [String: Facts], depth: Int, selectedTab: String?,
                                  manualOpen: [String: Bool]) -> [SidebarLine] {
        let f = facts[i.row.id] ?? Facts()
        var i = i
        i.children.removeAll { $0.park != nil }
        let attn = i.children.contains { facts[$0.row.id]?.attention == true }
        let open = i.children.isEmpty ? false : (manualOpen["tab:\(i.row.id)"] ?? attn)
        var line = itemLine(i, facts: f, depth: depth, selected: i.row.id == selectedTab, idPrefix: "tab:")
        if !i.children.isEmpty {
            line.chevron = open
            line.toggle = "tab:\(i.row.id)"
        }
        var out = [line]
        if open {
            for c in i.children {
                var child = itemLine(c, facts: facts[c.row.id] ?? Facts(), depth: depth + 1, selected: c.row.id == selectedTab, idPrefix: "tab:")
                child.dim = child.dim || i.stage == "closed"
                out.append(child)
            }
        }
        return out
    }

    private static func itemLine(_ i: AreaItem, facts f: Facts, depth: Int, selected: Bool, idPrefix: String) -> SidebarLine {
        let d = dot(f.finished && i.row.status != "done" ? "done" : i.row.status, failed: i.failed || f.failed)
        let kind: SidebarLine.Kind
        switch i.row.kind { case .orchestrator: kind = .orchestrator; case .lane: kind = .lane; case .workflow: kind = .workflow }
        let l = SidebarLine(id: idPrefix + i.row.id, kind: kind, depth: depth, glyph: d.0, glyphTone: d.1,
                            title: i.name + (i.devLoop || f.devLoop ? " ⟳" : ""), titleKind: i.row.kind,
                            tab: i.row.id, selected: selected, dim: i.stage == "closed",
                            area: i.area, role: i.role, stage: i.stage,
                            badge: badge(stage: i.stage, role: i.role), color: i.color)
        return l
    }
}

// MARK: window side

extension MainWindowController {
    /// The lines the sidebar shows now (also the state dump's `sidebar_lines`).
    var sidebarLines: [SidebarLine] {
        if state.mode == .spaces {
            return SidebarModel.build(snapshot: model.snapshot, orchestrators: model.orchestrators, lanes: model.lanes,
                                      workflows: model.workflows, selectedTab: state.selectedTab, manualOpen: state.manualOpen,
                                      parked: Set(model.catalog.snapshot.parked.keys))
        }
        return SidebarModel.buildAreas(snapshot: model.snapshot, orchestrators: model.orchestrators, lanes: model.lanes,
                                       workflows: model.workflows, catalog: model.catalog.snapshot, chip: state.chip,
                                       areaOnly: state.areaOnly, folded: state.foldedAreas, focusExpanded: state.focusExpanded,
                                       focusCursor: state.focusCursor, selectedTab: state.selectedTab, manualOpen: state.manualOpen)
    }

    /// ⌘⇧n: the n-th visible space's target tab. False when there is no such space.
    @discardableResult
    func gotoSpace(_ n: Int) -> Bool {
        guard let s = model.snapshot else { return false }
        let visible = SidebarModel.spaces(s).visible
        guard n >= 1, n <= visible.count, let tab = SidebarModel.targetTab(s, space: visible[n - 1].workspace_id) else { return false }
        selectTab(tab)
        return true
    }
}
