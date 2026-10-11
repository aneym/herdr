import Foundation

/// Settings > Agents: every agent pin with its face and whether it shows in the Agents list.
/// Hidden is herdr's shared pin flag (`tab.set_hidden`), so a toggle here reaches every client.
/// The Windows client builds the same rows in settingsAgents.ts.
enum SettingsAgents {
    struct Row: Equatable, Identifiable {
        var id: String
        var title: String
        /// Space name, then where the chat runs when herdr says (TabInfo.runs_on).
        var meta: String
        var face: SpacesRow.Face
        var shown: Bool
    }

    /// Agent pins in the shared pin order, hidden ones in their own slots (hiding never moves a pin).
    /// `pending` holds toggles the server has not echoed yet: tab id to shown.
    static func rows(_ input: SpacesInput, pending: [String: Bool] = [:]) -> [Row] {
        let names = Dictionary(input.spaces.map { ($0.id, $0.name) }, uniquingKeysWith: { a, _ in a })
        return input.tabs
            .filter { $0.role == "agent" && $0.pinIndex != nil }
            .sorted { ($0.pinIndex ?? 0, $0.id) < ($1.pinIndex ?? 0, $1.id) }
            .map { tab in
                let title = tab.agentName ?? tab.label
                let meta = [names[tab.space], tab.runsOn].compactMap { $0 }.filter { !$0.isEmpty }.joined(separator: " · ")
                return Row(id: tab.id, title: title, meta: meta,
                           face: SpacesTree.face(name: title, avatar: tab.avatar),
                           shown: pending[tab.id] ?? !tab.hidden)
            }
    }

    /// Drop pending toggles the server now agrees with.
    static func settle(_ pending: [String: Bool], _ input: SpacesInput) -> [String: Bool] {
        let hidden = Dictionary(input.tabs.map { ($0.id, $0.hidden) }, uniquingKeysWith: { a, _ in a })
        return pending.filter { id, shown in hidden[id].map { $0 == shown } ?? false }
    }
}
