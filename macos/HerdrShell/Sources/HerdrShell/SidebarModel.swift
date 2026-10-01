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
    enum Kind: String { case header, space, hidden, orchestrator, lane, workflow, background, note }
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

    /// The row as text, the way the mock draws it. Fold state is the chevron.
    var text: String {
        var left = [String]()
        if let c = chevron { left.append(c ? "▾" : "▸") }
        if !glyph.isEmpty { left.append(glyph) }
        left.append(title)
        var s = String(repeating: "  ", count: depth) + left.joined(separator: " ")
        if !trailing.isEmpty { s += "  " + trailing }
        if let h = host { s += "  [\(h)]" }
        return s
    }

    var dump: [String: Any] {
        ["id": id, "kind": kind.rawValue, "depth": depth, "text": text, "title": title,
         "chevron": chevron.map { $0 as Any } ?? NSNull(), "glyph": glyph, "trailing": trailing,
         "host": host ?? NSNull(), "tab": tab ?? NSNull(), "space": space ?? NSNull(),
         "selected": selected, "dim": dim]
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
                      selectedTab: String?, manualOpen: [String: Bool]) -> [SidebarLine] {
        guard let s else { return [] }
        let facts = Self.facts(s)
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

// MARK: window side

extension MainWindowController {
    /// The lines the sidebar shows now (also the state dump's `sidebar_lines`).
    var sidebarLines: [SidebarLine] {
        SidebarModel.build(snapshot: model.snapshot, orchestrators: model.orchestrators, lanes: model.lanes,
                           workflows: model.workflows, selectedTab: state.selectedTab, manualOpen: state.manualOpen)
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
