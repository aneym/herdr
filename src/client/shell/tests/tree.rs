//! The unified space/tab/agent tree and attention-aware agent cycling.
//! Ported from `docs/fork/port-0.9/orig/src/ui/sidebar.rs` and
//! `orig/src/app/actions.rs` onto the client shell.

use super::*;
use crate::client::shell::tree::{tree_list_entries, AgentPanelListEntry, ClientTreeChrome};

fn tab(tab_id: &str, workspace_id: &str, number: usize, label: &str) -> ClientShellTab {
    ClientShellTab {
        tab_id: tab_id.into(),
        workspace_id: workspace_id.into(),
        number,
        label: label.into(),
        custom_label: true,
        zoomed: false,
        focused: false,
        agent_status: AgentStatus::Idle,
    }
}

fn agent(
    pane_id: &str,
    workspace_id: &str,
    tab_id: &str,
    status: AgentStatus,
    state_change_seq: u64,
) -> ClientShellAgent {
    ClientShellAgent {
        pane_id: pane_id.into(),
        workspace_id: workspace_id.into(),
        tab_id: tab_id.into(),
        name: Some(pane_id.into()),
        display_agent: Some(pane_id.into()),
        agent: None,
        title: None,
        terminal_title: None,
        terminal_title_stripped: None,
        agent_status: status,
        state_change_seq,
        state_labels: Vec::new(),
        tokens: Vec::new(),
        focused: false,
    }
}

/// Two spaces; the first has two tabs with one agent each, the second one tab
/// with one agent.
fn tree_snapshot() -> ClientShellSnapshot {
    let mut snapshot = snapshot();
    snapshot.workspaces.push(ClientShellWorkspace {
        workspace_id: "ws_2".into(),
        active_tab_id: "tab_3".into(),
        new_workspace_cwd: "/other".into(),
        number: 2,
        label: "beta".into(),
        custom_label: true,
        branch: None,
        git_ahead_behind: None,
        tokens: Vec::new(),
        worktree: None,
        focused: false,
        agent_status: AgentStatus::Idle,
    });
    snapshot.workspaces[0].label = "alpha".into();
    snapshot.workspaces[0].custom_label = true;
    snapshot.tabs = vec![
        tab("tab_1", "ws_1", 1, "one"),
        tab("tab_2", "ws_1", 2, "two"),
        tab("tab_3", "ws_2", 1, "three"),
    ];
    snapshot.tabs[0].focused = true;
    snapshot.panes = vec![
        ClientShellPane {
            pane_id: "pane_1".into(),
            workspace_id: "ws_1".into(),
            tab_id: "tab_1".into(),
            label: None,
            cwd: None,
            foreground_cwd: None,
            focused: true,
            right_click_passthrough: false,
        },
        ClientShellPane {
            pane_id: "pane_2".into(),
            workspace_id: "ws_1".into(),
            tab_id: "tab_2".into(),
            label: None,
            cwd: None,
            foreground_cwd: None,
            focused: false,
            right_click_passthrough: false,
        },
        ClientShellPane {
            pane_id: "pane_3".into(),
            workspace_id: "ws_2".into(),
            tab_id: "tab_3".into(),
            label: None,
            cwd: None,
            foreground_cwd: None,
            focused: false,
            right_click_passthrough: false,
        },
    ];
    snapshot.agents = vec![
        agent("pane_1", "ws_1", "tab_1", AgentStatus::Working, 1),
        agent("pane_2", "ws_1", "tab_2", AgentStatus::Blocked, 2),
        agent("pane_3", "ws_2", "tab_3", AgentStatus::Done, 3),
    ];
    snapshot.agents[0].focused = true;
    snapshot
}

fn tree_state(tree: ClientTreeChrome) -> ClientShellState {
    let mut config = ClientShellConfig::from_config(&Config::default());
    config.agent_panel_sort = crate::config::AgentPanelSortConfig::Tree;
    let mut state = ClientShellState::new(config);
    state
        .tree_chrome
        .insert(crate::client::endpoint::ClientEndpointId::Local, tree);
    state.set_snapshot(Box::new(tree_snapshot()));
    state
}

fn shape(state: &ClientShellState, tree: &ClientTreeChrome) -> Vec<String> {
    let snapshot = state.snapshot.as_deref().expect("snapshot");
    let rows = crate::client::shell::agent_sidebar::agent_rows(snapshot, &state.config, None);
    tree_list_entries(snapshot, tree, rows)
        .iter()
        .map(|entry| match entry {
            AgentPanelListEntry::SpaceHeader(header) => {
                format!("space:{}", header.label)
            }
            AgentPanelListEntry::TabHeader(header) => format!("tab:{}", header.label),
            AgentPanelListEntry::Agent(row) => format!("agent:{}", row.pane_id),
            AgentPanelListEntry::HiddenSpacesHeader { count, collapsed } => {
                format!(
                    "hidden:{count}:{}",
                    if *collapsed { "closed" } else { "open" }
                )
            }
        })
        .collect()
}

#[test]
fn tree_nests_spaces_then_tabs_then_agents_in_workspace_order() {
    let tree = ClientTreeChrome::default();
    let state = tree_state(tree.clone());

    assert_eq!(
        shape(&state, &tree),
        [
            "space:alpha",
            "tab:one",
            "agent:pane_1",
            "tab:two",
            "agent:pane_2",
            "space:beta",
            "tab:three",
            "agent:pane_3",
        ]
    );
}

#[test]
fn collapsed_space_hides_its_agents_and_keeps_its_header() {
    let mut tree = ClientTreeChrome::default();
    tree.collapsed_spaces.insert("ws_1".into());
    let state = tree_state(tree.clone());

    assert_eq!(
        shape(&state, &tree),
        ["space:alpha", "space:beta", "tab:three", "agent:pane_3"]
    );
}

#[test]
fn collapsed_tab_hides_only_that_tabs_agents() {
    let mut tree = ClientTreeChrome::default();
    tree.collapsed_tabs.insert("ws_1#1".into());
    let state = tree_state(tree.clone());

    assert_eq!(
        shape(&state, &tree),
        [
            "space:alpha",
            "tab:one",
            "tab:two",
            "agent:pane_2",
            "space:beta",
            "tab:three",
            "agent:pane_3",
        ]
    );
}

#[test]
fn collapsed_tab_header_carries_one_dot_per_hidden_agent() {
    let mut tree = ClientTreeChrome::default();
    tree.collapsed_tabs.insert("ws_1#1".into());
    let state = tree_state(tree.clone());
    let snapshot = state.snapshot.as_deref().expect("snapshot");
    let rows = crate::client::shell::agent_sidebar::agent_rows(snapshot, &state.config, None);

    let dots = tree_list_entries(snapshot, &tree, rows)
        .into_iter()
        .find_map(|entry| match entry {
            AgentPanelListEntry::TabHeader(header) if header.label == "one" => {
                Some(header.child_states)
            }
            _ => None,
        })
        .expect("collapsed tab header");

    assert_eq!(dots, [AgentStatus::Working]);
}

#[test]
fn tree_layer_toggles_drop_their_header_rows() {
    let mut tree = ClientTreeChrome {
        show_tabs: false,
        ..ClientTreeChrome::default()
    };
    let state = tree_state(tree.clone());
    assert_eq!(
        shape(&state, &tree),
        [
            "space:alpha",
            "agent:pane_1",
            "agent:pane_2",
            "space:beta",
            "agent:pane_3",
        ]
    );

    tree.show_tabs = true;
    tree.show_spaces = false;
    assert_eq!(
        shape(&state, &tree),
        [
            "tab:one",
            "agent:pane_1",
            "tab:two",
            "agent:pane_2",
            "tab:three",
            "agent:pane_3",
        ]
    );
}

#[test]
fn hidden_agents_leave_only_space_and_tab_headers() {
    let tree = ClientTreeChrome {
        show_agents: false,
        ..ClientTreeChrome::default()
    };
    let state = tree_state(tree.clone());

    assert_eq!(
        shape(&state, &tree),
        [
            "space:alpha",
            "tab:one",
            "tab:two",
            "space:beta",
            "tab:three"
        ]
    );
}

#[test]
fn hiding_all_tree_layers_yields_an_empty_list() {
    let tree = ClientTreeChrome {
        show_spaces: false,
        show_tabs: false,
        show_agents: false,
        ..ClientTreeChrome::default()
    };
    let state = tree_state(tree.clone());

    assert!(shape(&state, &tree).is_empty());
}

#[test]
fn tree_rows_drop_the_labels_their_headers_already_carry() {
    let tree = ClientTreeChrome::default();
    let state = tree_state(tree.clone());
    let snapshot = state.snapshot.as_deref().expect("snapshot");
    let rows = crate::client::shell::agent_sidebar::agent_rows(snapshot, &state.config, None);

    let row = tree_list_entries(snapshot, &tree, rows)
        .into_iter()
        .find_map(|entry| match entry {
            AgentPanelListEntry::Agent(row) if row.pane_id == "pane_1" => Some(row),
            _ => None,
        })
        .expect("agent row");

    assert!(row.rows.iter().flatten().all(|token| !matches!(
        token.kind,
        crate::ui::ResolvedTokenKind::Workspace(_) | crate::ui::ResolvedTokenKind::Tab(_)
    )));
    // Two layers of headers above it, so the row indents twice.
    assert_eq!(row.indent, 2);
}

#[test]
fn agent_cycle_skips_agents_inside_a_collapsed_space() {
    let mut tree = ClientTreeChrome::default();
    tree.collapsed_spaces.insert("ws_2".into());
    let state = tree_state(tree);
    let snapshot = state.snapshot.as_deref().expect("snapshot");

    let panes = state
        .agent_cycle_candidates(snapshot)
        .into_iter()
        .map(|entry| entry.pane_id)
        .collect::<Vec<_>>();

    assert_eq!(panes, ["pane_1", "pane_2"]);
}

#[test]
fn agent_cycle_prefers_the_most_demanding_peer_over_the_next_in_order() {
    // Spaces order keeps a stable list, so the key has to rank for itself:
    // blocked outranks an unread completion, which outranks a working agent.
    let mut config = ClientShellConfig::from_config(&Config::default());
    config.agent_panel_sort = crate::config::AgentPanelSortConfig::Spaces;
    let mut state = ClientShellState::new(config);
    state.set_snapshot(Box::new(tree_snapshot()));
    let snapshot = state.snapshot.as_deref().expect("snapshot");
    let candidates = state.agent_cycle_candidates(snapshot);

    // Focused: pane_1 (working). pane_2 is blocked, pane_3 is done.
    let next = state
        .agent_cycle_target(snapshot, &candidates, true)
        .expect("cycle target");
    assert_eq!(candidates[next].pane_id, "pane_2");
}

#[test]
fn agent_cycle_never_returns_the_focused_agent() {
    let mut config = ClientShellConfig::from_config(&Config::default());
    config.agent_panel_sort = crate::config::AgentPanelSortConfig::Spaces;
    let mut state = ClientShellState::new(config);
    let mut snapshot = tree_snapshot();
    // The focused agent is the only blocked one, so ranking it alongside the
    // rest would wrap the search straight back onto it.
    snapshot.agents[0].agent_status = AgentStatus::Blocked;
    state.set_snapshot(Box::new(snapshot));
    let snapshot = state.snapshot.as_deref().expect("snapshot");
    let candidates = state.agent_cycle_candidates(snapshot);

    let next = state
        .agent_cycle_target(snapshot, &candidates, true)
        .expect("cycle target");
    assert_ne!(candidates[next].pane_id, "pane_1");
}

#[test]
fn triage_sort_orders_tiers_and_oldest_state_change_first() {
    let mut snapshot = tree_snapshot();
    snapshot.agents = vec![
        agent("p1", "ws_1", "tab_1", AgentStatus::Working, 2),
        agent("p2", "ws_1", "tab_1", AgentStatus::Done, 8),
        agent("p3", "ws_1", "tab_1", AgentStatus::Unknown, 0),
        agent("p4", "ws_1", "tab_1", AgentStatus::Idle, 3),
        agent("p5", "ws_1", "tab_1", AgentStatus::Blocked, 5),
        agent("p6", "ws_1", "tab_1", AgentStatus::Done, 1),
    ];

    let ordered = crate::client::shell::agent_sidebar::ordered_agent_pane_ids(
        &snapshot,
        crate::config::AgentPanelSortConfig::Triage,
    );

    assert_eq!(ordered, ["p5", "p6", "p2", "p4", "p1", "p3"]);
}

#[test]
fn tree_chevron_click_collapses_the_space_and_persists_it() {
    let tree = ClientTreeChrome::default();
    let mut state = tree_state(tree);
    state.set_pane_surface(surface());
    state.compose(106, 40).expect("composed frame");

    let header = state
        .hits
        .tree_headers
        .iter()
        .find(|hit| hit.key == "ws_1" && hit.tab_id.is_none())
        .expect("space header hit");
    let chevron = header.chevron;
    assert!(chevron.width > 0);

    state.handle_raw_events(vec![RawInputEvent::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: chevron.x,
        row: chevron.y,
        modifiers: KeyModifiers::empty(),
    })]);

    assert!(state
        .tree_chrome
        .get(&crate::client::endpoint::ClientEndpointId::Local)
        .expect("local tree chrome")
        .collapsed_spaces
        .contains("ws_1"));
}

#[test]
fn tree_tab_header_click_focuses_that_tab() {
    let tree = ClientTreeChrome::default();
    let mut state = tree_state(tree);
    state.set_pane_surface(surface());
    state.compose(106, 40).expect("composed frame");

    let header = state
        .hits
        .tree_headers
        .iter()
        .find(|hit| hit.tab_id.as_deref() == Some("tab_2"))
        .expect("tab header hit");
    let rect = header.rect;

    let outcome = state.handle_raw_events(vec![RawInputEvent::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: rect.x + 1,
        row: rect.y,
        modifiers: KeyModifiers::empty(),
    })]);

    assert!(matches!(
        &outcome.actions[..],
        [ClientShellAction::Endpoint { request, .. }]
            if matches!(
                &request.method,
                crate::api::schema::Method::TabFocus(target) if target.tab_id == "tab_2"
            )
    ));
}

#[test]
fn pinned_space_keeps_its_header_without_any_agents() {
    let mut tree = ClientTreeChrome::default();
    tree.pinned_spaces.insert("ws_2".into());
    let mut state = tree_state(tree.clone());
    let mut snapshot = tree_snapshot();
    // Nothing runs in beta any more.
    snapshot.agents.retain(|agent| agent.workspace_id != "ws_2");
    state.set_snapshot(Box::new(snapshot));

    assert_eq!(
        shape(&state, &tree),
        [
            "space:alpha",
            "tab:one",
            "agent:pane_1",
            "tab:two",
            "agent:pane_2",
            "space:beta",
        ]
    );
}

#[test]
fn unpinned_agentless_space_drops_out_of_the_tree() {
    let tree = ClientTreeChrome::default();
    let mut state = tree_state(tree.clone());
    let mut snapshot = tree_snapshot();
    snapshot.agents.retain(|agent| agent.workspace_id != "ws_2");
    state.set_snapshot(Box::new(snapshot));

    assert!(!shape(&state, &tree).contains(&"space:beta".to_owned()));
}

#[test]
fn collapsed_spaces_move_into_the_hidden_section() {
    let mut tree = ClientTreeChrome {
        show_hidden_spaces: true,
        ..ClientTreeChrome::default()
    };
    tree.collapsed_spaces.insert("ws_1".into());
    let state = tree_state(tree.clone());

    assert_eq!(
        shape(&state, &tree),
        ["space:beta", "tab:three", "agent:pane_3", "hidden:1:closed"]
    );
}

#[test]
fn expanding_the_hidden_section_lists_the_folded_spaces() {
    let mut tree = ClientTreeChrome {
        show_hidden_spaces: true,
        hidden_spaces_expanded: true,
        ..ClientTreeChrome::default()
    };
    tree.collapsed_spaces.insert("ws_1".into());
    let state = tree_state(tree.clone());

    assert_eq!(
        shape(&state, &tree),
        [
            "space:beta",
            "tab:three",
            "agent:pane_3",
            "hidden:1:open",
            "space:alpha",
        ]
    );
}

#[test]
fn no_hidden_section_without_the_reveal() {
    let mut tree = ClientTreeChrome::default();
    tree.collapsed_spaces.insert("ws_1".into());
    let state = tree_state(tree.clone());

    assert!(!shape(&state, &tree)
        .iter()
        .any(|row| row.starts_with("hidden:")));
}

#[test]
fn hidden_section_counts_spaces_not_agents() {
    let mut tree = ClientTreeChrome {
        show_hidden_spaces: true,
        ..ClientTreeChrome::default()
    };
    tree.collapsed_spaces.insert("ws_1".into());
    tree.collapsed_spaces.insert("ws_2".into());
    let state = tree_state(tree.clone());

    assert_eq!(shape(&state, &tree), ["hidden:2:closed"]);
}

#[test]
fn collapsed_space_header_hides_its_status_dots() {
    let mut tree = ClientTreeChrome {
        show_tabs: false,
        show_agents: false,
        ..ClientTreeChrome::default()
    };
    tree.collapsed_spaces.insert("ws_1".into());
    let state = tree_state(tree.clone());
    let snapshot = state.snapshot.as_deref().expect("snapshot");
    let rows = crate::client::shell::agent_sidebar::agent_rows(snapshot, &state.config, None);
    let headers = tree_list_entries(snapshot, &tree, rows)
        .into_iter()
        .filter_map(|entry| match entry {
            AgentPanelListEntry::SpaceHeader(header) => Some((header.label, header.child_states)),
            _ => None,
        })
        .collect::<Vec<_>>();

    let beta_status = snapshot
        .agents
        .iter()
        .find(|agent| agent.pane_id == "pane_3")
        .expect("beta agent")
        .agent_status;
    assert_eq!(
        headers,
        [
            ("alpha".to_owned(), Vec::new()),
            ("beta".to_owned(), vec![beta_status]),
        ]
    );
}

#[test]
fn space_order_moves_whole_space_blocks() {
    let tree = ClientTreeChrome {
        space_order: vec!["ws_2".into(), "ws_1".into()],
        ..ClientTreeChrome::default()
    };
    let state = tree_state(tree.clone());

    assert_eq!(
        shape(&state, &tree),
        [
            "space:beta",
            "tab:three",
            "agent:pane_3",
            "space:alpha",
            "tab:one",
            "agent:pane_1",
            "tab:two",
            "agent:pane_2",
        ]
    );
}

#[test]
fn pin_click_toggles_the_pin_and_tells_the_endpoint() {
    let tree = ClientTreeChrome::default();
    let mut state = tree_state(tree);
    state.set_pane_surface(surface());
    state.compose(106, 40).expect("composed frame");

    let pin = state
        .hits
        .tree_headers
        .iter()
        .find(|hit| hit.key == "ws_1" && hit.tab_id.is_none())
        .expect("space header hit")
        .pin;
    assert!(pin.width > 0);

    let outcome = state.handle_raw_events(vec![RawInputEvent::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: pin.x,
        row: pin.y,
        modifiers: KeyModifiers::empty(),
    })]);

    assert!(state
        .tree_chrome
        .get(&crate::client::endpoint::ClientEndpointId::Local)
        .expect("local tree chrome")
        .pinned_spaces
        .contains("ws_1"));
    assert!(matches!(
        &outcome.actions[..],
        [ClientShellAction::Endpoint { request, .. }]
            if matches!(
                &request.method,
                crate::api::schema::Method::WorkspaceSetPinned(params)
                    if params.workspace_id == "ws_1" && params.pinned
            )
    ));
}

#[test]
fn dragging_a_space_header_records_the_new_order() {
    let tree = ClientTreeChrome::default();
    let mut state = tree_state(tree);
    state.set_pane_surface(surface());
    state.compose(106, 40).expect("composed frame");

    let (alpha, beta) = {
        let headers = state
            .hits
            .tree_headers
            .iter()
            .filter(|hit| hit.tab_id.is_none())
            .collect::<Vec<_>>();
        (headers[0].rect, headers[1].rect)
    };

    state.handle_raw_events(vec![RawInputEvent::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: alpha.x + 1,
        row: alpha.y,
        modifiers: KeyModifiers::empty(),
    })]);
    assert!(state.tree_space_press.is_some());
    state.handle_raw_events(vec![RawInputEvent::Mouse(MouseEvent {
        kind: MouseEventKind::Drag(MouseButton::Left),
        column: beta.x + 1,
        row: beta.y + 1,
        modifiers: KeyModifiers::empty(),
    })]);
    state.handle_raw_events(vec![RawInputEvent::Mouse(MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column: beta.x + 1,
        row: beta.y + 1,
        modifiers: KeyModifiers::empty(),
    })]);

    assert_eq!(
        state
            .tree_chrome
            .get(&crate::client::endpoint::ClientEndpointId::Local)
            .expect("local tree chrome")
            .space_order,
        ["ws_2", "ws_1"]
    );
}

#[test]
fn right_click_on_the_sort_label_opens_the_view_toggles() {
    let tree = ClientTreeChrome::default();
    let mut state = tree_state(tree);
    state.set_pane_surface(surface());
    state.compose(106, 40).expect("composed frame");
    let toggle = state.hits.agent_sort_toggle;
    assert!(toggle.width > 0);

    state.handle_raw_events(vec![RawInputEvent::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Right),
        column: toggle.x,
        row: toggle.y,
        modifiers: KeyModifiers::empty(),
    })]);

    let Some(crate::client::shell::ClientShellOverlay::ContextMenu(menu)) = state.overlay.as_ref()
    else {
        panic!("right-click on the sort label should open the view menu");
    };
    assert_eq!(
        menu.items()
            .iter()
            .map(|item| item.label)
            .collect::<Vec<_>>(),
        [
            "Hide spaces",
            "Hide tabs",
            "Hide agents",
            "Reveal folded spaces"
        ]
    );
}

#[test]
fn revealing_folded_spaces_starts_the_section_compact() {
    let tree = ClientTreeChrome {
        hidden_spaces_expanded: true,
        show_hidden_spaces: true,
        ..ClientTreeChrome::default()
    };
    let mut state = tree_state(tree);
    state.open_sidebar_view_context_menu(0, 0);
    let mut outcome = ClientShellInput::default();
    state.activate_context_menu_item(3, &mut outcome);

    let tree = state
        .tree_chrome
        .get(&crate::client::endpoint::ClientEndpointId::Local)
        .expect("local tree chrome");
    assert!(!tree.show_hidden_spaces);
    assert!(!tree.hidden_spaces_expanded);
}
