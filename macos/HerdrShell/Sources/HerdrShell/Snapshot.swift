import Foundation

/// Live herdr state: the `session.snapshot` shape (what `herdr api snapshot` prints), read by HerdrClient.
struct Snapshot: Decodable {
    struct Workspace: Decodable {
        let workspace_id: String; let label: String?; let number: Int; let orchestrator_mode: Bool?
        // P10: the space list follows herdr's own focus and tokens (`pinned`, `hidden`).
        let focused: Bool?; let active_tab_id: String?; let tokens: [String: String]?
        var sort_rank: UInt32? = nil; var parked: Bool? = nil
    }
    /// `work_status` is herdr's one answer to "is this chat working" (server app/work_status.rs); older servers omit it.
    struct Tab: Decodable { let tab_id: String; let workspace_id: String; let label: String?; let number: Int; let agent_status: String?; let pane_count: Int?; let pin_index: Int?; var work_status: String? = nil; var role: String? = nil; var desk: DeskInfo? = nil; var sort_rank: UInt32? = nil }
    struct Pane: Decodable {
        var restore_error: String? = nil
        var tokens: [String: String]? = nil
        let pane_id: String; let tab_id: String; let terminal_id: String; let agent_status: String?; let focused: Bool?
        // Titles the quick switcher matches. Older snapshots omit them.
        let title: String?; let terminal_title: String?; let terminal_title_stripped: String?
    }
    struct Owner: Decodable { let pane_id: String? }
    struct Ownership: Decodable { let current: Owner? }
    struct Agent: Decodable {
        let pane_id: String; let tab_id: String; let agent: String?; let agent_status: String?; var work_status: String? = nil
        let tokens: [String: String]?; let ownership: Ownership?
        let title: String?; let terminal_title: String?; let terminal_title_stripped: String?
    }
    struct Rect: Decodable { let x: Double; let y: Double; let width: Double; let height: Double }
    struct LayoutPane: Decodable { let pane_id: String; let rect: Rect }
    struct Split: Decodable { let id: String; let direction: String; let ratio: Double; let rect: Rect }
    struct Layout: Decodable {
        let tab_id: String; let area: Rect; let panes: [LayoutPane]; let splits: [Split]?
        let zoomed: Bool?; let focused_pane_id: String?
    }

    let workspaces: [Workspace]
    let tabs: [Tab]
    let panes: [Pane]
    let agents: [Agent]
    let layouts: [Layout]
    let version: String?
    /// The server's wire protocol. Another machine whose protocol differs from the local
    /// server's cannot attach its terminals here, so its header says "needs update".
    let `protocol`: Int?
}
