import Foundation

struct HomeTag: Codable, Equatable {
    let entity: String
    let `class`: String
    let state: String
    let seats: Int
    let stale: Bool
    let progress_at: Double?
    let finished_at: Double?
    let sub: String
    let order: Int
    let folded: Bool
}

enum HomeRows {
    static func represented(_ input: SpacesInput, overlay: Overlay) -> Set<String> {
        var ids = Set(input.tabs.filter { (overlay.tabs[$0.id]?.home ?? $0.home) != nil }.map(\.id))
        var changed = true
        while changed {
            changed = false
            for tab in input.tabs where !ids.contains(tab.id) {
                let parent = overlay.tabs[tab.id]?.parent ?? tab.agents.first?.explicitParent
                if let parent, ids.contains(parent) {
                    ids.insert(tab.id)
                    changed = true
                }
            }
        }
        return ids
    }

    static func rows(_ input: SpacesInput, overlay: Overlay, chrome: SpacesChrome, now: Double) -> [SpacesRow] {
        let entities = input.tabs.compactMap { tab -> (SpacesInput.Tab, HomeTag)? in
            (overlay.tabs[tab.id]?.home ?? tab.home).map { (tab, $0) }
        }.sorted { $0.1.order < $1.1.order }
        var out: [SpacesRow] = []
        func append(_ members: [(SpacesInput.Tab, HomeTag)], key: String) {
            let owners = members.filter { $0.1.class == "owner" }
            let others = members.filter { $0.1.class != "owner" }
            func row(_ member: (SpacesInput.Tab, HomeTag)) -> SpacesRow {
                let (tab, home) = member
                let progress = home.progress_at.map { SpacesTree.age(now - $0) } ?? "unknown"
                return SpacesRow(id: "home:" + tab.id, kind: .tab, glyph: "●", tone: home.state,
                                 title: tab.label, alert: home.stale ? "act" : "none", tab: tab.id,
                                 seats: home.seats, homeState: home.state, badge: overlay.tabs[tab.id]?.badge,
                                 hover: home.sub + " · last progress " + progress)
            }
            out += owners.map(row)
            if !owners.isEmpty && !others.isEmpty {
                out.append(SpacesRow(id: "home-divider:" + key, kind: .divider, title: ""))
            }
            out += others.map(row)
        }
        for (state, title) in [("needs_you", "NEEDS YOU"), ("working", "WORKING"), ("finished", "FINISHED"), ("utility", "UTILITIES")] {
            let members = entities.filter { state == "utility" ? $0.1.class == "utility" : $0.1.class != "utility" && $0.1.state == state }
            let current = members.filter { !$0.1.folded }
            out.append(SpacesRow(id: "home-section:" + state, kind: .section, title: title, trailing: String(current.count)))
            let folded = state == "finished" ? members.filter { $0.1.folded } : []
            append(current, key: state)
            if !folded.isEmpty {
                let open = chrome.expandedGroups.contains("home-earlier")
                out.append(SpacesRow(id: "home-earlier", kind: .group, chevron: open ? "open" : "closed", title: "Earlier " + String(folded.count), toggleKey: "group:home-earlier"))
                if open { append(folded, key: "earlier") }
            }
        }
        return out
    }
}
