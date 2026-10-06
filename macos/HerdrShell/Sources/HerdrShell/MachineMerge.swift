import Foundation

/// Other machines' chats inside the one spaces tree (Alex, 2026-10-06: "just a badge with an
/// icon on the machine that's running the chat, and everything appearing together under the
/// proper space").
///
/// Pure over `SpacesInput`, so `scripts/check_machines_merge.py` drives it with SpacesTree alone.
/// A remote workspace whose label matches a local space's label (trimmed, any case) joins that
/// space; any other remote workspace is its own space after the local ones, and SpacesTree puts
/// it in its areas.json group by label like a local space. Remote tabs follow the local tabs of
/// a space. Only chats travel: a remote tab shows when it has an agent or a pin, so a machine
/// with no agents adds nothing. Remote pins follow the local pins, machine by machine, in each
/// machine's own pin order, as the Cmd digits number them.
enum MachineMerge {
    struct Machine: Codable {
        var name: String
        /// nil when healthy; otherwise "unreachable" or "needs update", shown only on the badge.
        var health: String?
        var spaces: [SpacesInput.Space]
        var tabs: [SpacesInput.Tab]
    }

    static func key(_ label: String) -> String { label.trimmingCharacters(in: .whitespaces).lowercased() }

    static func merge(_ local: SpacesInput, machines: [Machine]) -> SpacesInput {
        var out = local
        var byLabel: [String: String] = [:]
        for space in local.spaces where byLabel[key(space.name)] == nil { byLabel[key(space.name)] = space.id }
        var nextPin = (local.tabs.compactMap(\.pinIndex).max() ?? -1) + 1
        for machine in machines {
            let shown = machine.tabs.filter { !$0.agents.isEmpty || $0.pinIndex != nil }
            // Pins renumber after every pin before them, keeping this machine's order.
            var pins: [String: Int] = [:]
            for tab in shown.filter({ $0.pinIndex != nil }).sorted(by: { ($0.pinIndex ?? 0) < ($1.pinIndex ?? 0) }) {
                pins[tab.id] = nextPin
                nextPin += 1
            }
            for var tab in shown {
                guard let space = machine.spaces.first(where: { $0.id == tab.space }) else { continue }
                if let home = byLabel[key(space.name)] {
                    tab.space = home
                } else {
                    // A space of its own; a later workspace with the same label joins it.
                    byLabel[key(space.name)] = space.id
                    out.spaces.append(space)
                }
                tab.pinIndex = pins[tab.id]
                tab.focused = tab.id == local.focusedTab
                out.tabs.append(tab)
            }
        }
        return out
    }

    /// Puts the owning machine's badge on every row that stands for one of its chats: tab rows in
    /// a space and pinned rows. Local rows are untouched.
    static func badge(_ rows: [SpacesRow], machines: [Machine]) -> [SpacesRow] {
        let health = Dictionary(machines.map { ($0.name, $0.health) }, uniquingKeysWith: { a, _ in a })
        return rows.map { row in
            guard row.kind == .tab, let tab = row.tab, let slash = tab.firstIndex(of: "/"), slash != tab.startIndex else { return row }
            let name = String(tab[..<slash])
            guard let state = health[name] else { return row }
            var r = row
            r.badge = name
            r.badgeState = state
            return r
        }
    }
}
