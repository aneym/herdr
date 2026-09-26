//! Durable ownership and orchestrator groups in the agent sidebar. Ported from
//! `docs/fork/port-0.9/orig/src/ui/sidebar.rs` (`arrange_agent_hierarchy`).

use super::*;
use crate::client::shell::tree::{arrange_agent_hierarchy, ClientTreeChrome};

fn owned_snapshot() -> ClientShellSnapshot {
    let mut snapshot = snapshot();
    snapshot.tabs = vec![ClientShellTab {
        tab_id: "tab_1".into(),
        workspace_id: "ws_1".into(),
        number: 1,
        label: "one".into(),
        custom_label: true,
        zoomed: false,
        focused: true,
        agent_status: AgentStatus::Idle,
    }];
    snapshot.panes.clear();
    snapshot.agents.clear();
    for (pane, owner) in [
        ("owner", None),
        ("child_a", Some("owner")),
        ("child_b", Some("owner")),
        ("grandchild", Some("child_a")),
        ("root", None),
    ] {
        snapshot.panes.push(ClientShellPane {
            pane_id: pane.into(),
            workspace_id: "ws_1".into(),
            tab_id: "tab_1".into(),
            label: None,
            cwd: None,
            foreground_cwd: None,
            focused: pane == "owner",
            right_click_passthrough: false,
        });
        snapshot.agents.push(ClientShellAgent {
            pane_id: pane.into(),
            workspace_id: "ws_1".into(),
            tab_id: "tab_1".into(),
            name: Some(pane.into()),
            display_agent: Some(pane.into()),
            agent: None,
            title: None,
            terminal_title: None,
            terminal_title_stripped: None,
            agent_status: AgentStatus::Idle,
            state_change_seq: 1,
            state_labels: Vec::new(),
            tokens: Vec::new(),
            focused: pane == "owner",
            owner_pane_id: owner.map(str::to_owned),
            orphaned: false,
            group: Default::default(),
        });
    }
    snapshot
}

fn arranged(snapshot: &ClientShellSnapshot, tree: &ClientTreeChrome) -> Vec<(String, u8, bool)> {
    let config = ClientShellConfig::from_config(&Config::default());
    let rows = crate::client::shell::agent_sidebar::agent_rows(snapshot, &config, None);
    arrange_agent_hierarchy(snapshot, tree, rows)
        .into_iter()
        .map(|row| (row.pane_id, row.group.depth, row.group.last_in_group))
        .collect()
}

#[test]
fn hierarchy_nests_children_beneath_their_owner_in_order() {
    let snapshot = owned_snapshot();
    let tree = ClientTreeChrome::default();

    assert_eq!(
        arranged(&snapshot, &tree),
        [
            ("owner".to_owned(), 0, false),
            ("child_a".to_owned(), 1, false),
            ("grandchild".to_owned(), 2, true),
            ("child_b".to_owned(), 1, true),
            ("root".to_owned(), 0, false),
        ]
    );
}

#[test]
fn collapsed_group_hides_its_descendants_and_counts_them() {
    let snapshot = owned_snapshot();
    let mut tree = ClientTreeChrome::default();
    tree.collapsed_agent_groups.insert("owner".into());

    let config = ClientShellConfig::from_config(&Config::default());
    let rows = crate::client::shell::agent_sidebar::agent_rows(&snapshot, &config, None);
    let arranged = arrange_agent_hierarchy(&snapshot, &tree, rows);

    assert_eq!(
        arranged
            .iter()
            .map(|row| row.pane_id.as_str())
            .collect::<Vec<_>>(),
        ["owner", "root"]
    );
    let owner = &arranged[0];
    assert_eq!(owner.group.expanded, Some(false));
    // Both children and the grandchild.
    assert_eq!(owner.group.hidden_children, 3);
}

#[test]
fn an_owner_with_no_children_gets_no_chevron() {
    let mut snapshot = owned_snapshot();
    snapshot
        .agents
        .retain(|agent| agent.owner_pane_id.is_none());
    let tree = ClientTreeChrome::default();
    let config = ClientShellConfig::from_config(&Config::default());
    let rows = crate::client::shell::agent_sidebar::agent_rows(&snapshot, &config, None);

    let arranged = arrange_agent_hierarchy(&snapshot, &tree, rows);

    assert!(arranged.iter().all(|row| row.group.expanded.is_none()));
}

#[test]
fn an_owner_cycle_still_lists_every_agent_once() {
    let mut snapshot = owned_snapshot();
    // owner -> child_a -> owner, a state that should never occur but must not
    // drop rows or loop.
    for agent in &mut snapshot.agents {
        if agent.pane_id == "owner" {
            agent.owner_pane_id = Some("child_a".into());
        }
    }
    let tree = ClientTreeChrome::default();

    let mut panes = arranged(&snapshot, &tree)
        .into_iter()
        .map(|(pane_id, _, _)| pane_id)
        .collect::<Vec<_>>();
    panes.sort();

    assert_eq!(panes, ["child_a", "child_b", "grandchild", "owner", "root"]);
}

#[test]
fn an_unresolved_owner_marks_the_row_orphaned() {
    let mut snapshot = owned_snapshot();
    for agent in &mut snapshot.agents {
        if agent.pane_id == "child_a" {
            agent.owner_pane_id = None;
            agent.orphaned = true;
        }
    }
    let tree = ClientTreeChrome::default();
    let config = ClientShellConfig::from_config(&Config::default());
    let rows = crate::client::shell::agent_sidebar::agent_rows(&snapshot, &config, None);

    let arranged = arrange_agent_hierarchy(&snapshot, &tree, rows);
    let orphan = arranged
        .iter()
        .find(|row| row.pane_id == "child_a")
        .expect("orphaned row");

    assert!(orphan.orphaned);
    // It flattens to a root rather than vanishing.
    assert_eq!(orphan.group.depth, 0);
}

#[test]
fn orchestrator_mode_groups_the_workspace_under_its_first_tab() {
    let mut snapshot = owned_snapshot();
    snapshot.workspaces[0].orchestrator_mode = true;
    snapshot.workspaces[0].tab_count = 4;
    let tree = ClientTreeChrome::default();
    let config = ClientShellConfig::from_config(&Config::default());
    let rows = crate::client::shell::agent_sidebar::agent_rows(&snapshot, &config, None);

    let arranged = arrange_agent_hierarchy(&snapshot, &tree, rows);

    assert_eq!(
        arranged
            .iter()
            .map(|row| (row.pane_id.as_str(), row.group.depth))
            .collect::<Vec<_>>(),
        [
            ("owner", 0),
            ("child_a", 1),
            ("grandchild", 2),
            ("child_b", 1),
            // The formerly top-level root is adopted by the orchestrator.
            ("root", 1),
        ]
    );
    // The open-tab count sits on the orchestrator row.
    assert_eq!(arranged[0].group.group_count, Some(3));
    assert_eq!(arranged[0].group.group_key.as_deref(), Some("orch:ws_1"));
}

#[test]
fn orchestrator_mode_off_keeps_flat_rows() {
    let mut snapshot = owned_snapshot();
    snapshot
        .agents
        .retain(|agent| agent.owner_pane_id.is_none());
    snapshot.workspaces[0].orchestrator_mode = false;
    let tree = ClientTreeChrome::default();

    assert_eq!(
        arranged(&snapshot, &tree)
            .iter()
            .map(|(_, depth, _)| *depth)
            .collect::<Vec<_>>(),
        [0, 0]
    );
}

fn click(state: &mut ClientShellState, rect: ratatui::layout::Rect) -> ClientShellInput {
    state.handle_raw_events(vec![RawInputEvent::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: rect.right().saturating_sub(1),
        row: rect.y,
        modifiers: KeyModifiers::empty(),
    })])
}

fn group_collapse_requests(outcome: &ClientShellInput) -> Vec<(String, bool)> {
    outcome
        .actions
        .iter()
        .filter_map(|action| match action {
            ClientShellAction::Endpoint { request, .. } => match &request.method {
                crate::api::schema::Method::AgentGroupCollapse(params) => {
                    Some((params.target.clone(), params.collapsed))
                }
                _ => None,
            },
            _ => None,
        })
        .collect()
}

fn local_folds(state: &ClientShellState) -> std::collections::HashSet<String> {
    state
        .tree_chrome
        .get(&crate::client::endpoint::ClientEndpointId::Local)
        .map(|tree| tree.collapsed_agent_groups.clone())
        .unwrap_or_default()
}

fn composed(snapshot: ClientShellSnapshot) -> ClientShellState {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(snapshot));
    state.set_pane_surface(surface());
    state.compose(106, 40).expect("composed frame");
    state
}

fn set_group(
    snapshot: &mut ClientShellSnapshot,
    pane: &str,
    group: crate::protocol::ClientShellAgentGroup,
) {
    snapshot
        .agents
        .iter_mut()
        .find(|agent| agent.pane_id == pane)
        .expect("agent")
        .group = group;
}

#[test]
fn clicking_a_group_chevron_asks_the_endpoint_to_fold_it() {
    let mut state = composed(owned_snapshot());
    let hit = state
        .hits
        .agent_groups
        .first()
        .cloned()
        .expect("group chevron hit");
    assert_eq!(hit.key, "owner");

    let outcome = click(&mut state, hit.rect);

    // The endpoint holds the fold so the CLI and every client agree.
    assert_eq!(
        group_collapse_requests(&outcome),
        [("owner".to_owned(), true)]
    );
    assert!(local_folds(&state).is_empty());
}

#[test]
fn clicking_a_server_folded_chevron_opens_it_on_the_endpoint_and_locally() {
    let mut snapshot = owned_snapshot();
    set_group(
        &mut snapshot,
        "owner",
        crate::protocol::ClientShellAgentGroup {
            collapsed: true,
            ..Default::default()
        },
    );
    let mut state = composed(snapshot);
    state
        .tree_chrome
        .entry(crate::client::endpoint::ClientEndpointId::Local)
        .or_default()
        .collapsed_agent_groups
        .insert("owner".into());
    let hit = state
        .hits
        .agent_groups
        .first()
        .cloned()
        .expect("group chevron hit");
    assert!(!hit.expanded);

    let outcome = click(&mut state, hit.rect);

    assert_eq!(
        group_collapse_requests(&outcome),
        [("owner".to_owned(), false)]
    );
    assert!(local_folds(&state).is_empty());
}

#[test]
fn an_endpoint_without_the_method_folds_the_group_locally() {
    let mut state = composed(owned_snapshot());
    state.set_endpoint_methods(Some(vec!["pane.focus".into()]));
    let hit = state
        .hits
        .agent_groups
        .first()
        .cloned()
        .expect("group chevron hit");

    let outcome = click(&mut state, hit.rect);

    assert!(group_collapse_requests(&outcome).is_empty());
    assert!(local_folds(&state).contains("owner"));
}

#[test]
fn a_hands_on_agent_is_a_root_and_is_never_adopted() {
    let mut snapshot = owned_snapshot();
    snapshot.workspaces[0].orchestrator_mode = true;
    snapshot.workspaces[0].tab_count = 2;
    set_group(
        &mut snapshot,
        "child_a",
        crate::protocol::ClientShellAgentGroup {
            hands_on: true,
            ..Default::default()
        },
    );
    set_group(
        &mut snapshot,
        "root",
        crate::protocol::ClientShellAgentGroup {
            hands_on: true,
            ..Default::default()
        },
    );

    let arranged = arranged(&snapshot, &ClientTreeChrome::default())
        .into_iter()
        .map(|(pane, depth, _)| (pane, depth))
        .collect::<Vec<_>>();

    assert_eq!(
        arranged,
        [
            ("owner".to_owned(), 0),
            ("child_b".to_owned(), 1),
            // Pinned out of its owner's group, it keeps its own children.
            ("child_a".to_owned(), 0),
            ("grandchild".to_owned(), 1),
            // The orchestrator would adopt this root; the pin keeps it out.
            ("root".to_owned(), 0),
        ]
    );
}

#[test]
fn an_explicit_parent_wins_over_the_owner() {
    let mut snapshot = owned_snapshot();
    set_group(
        &mut snapshot,
        "grandchild",
        crate::protocol::ClientShellAgentGroup {
            parent_pane_id: Some("root".into()),
            ..Default::default()
        },
    );

    assert_eq!(
        arranged(&snapshot, &ClientTreeChrome::default()),
        [
            ("owner".to_owned(), 0, false),
            ("child_a".to_owned(), 1, false),
            ("child_b".to_owned(), 1, true),
            ("root".to_owned(), 0, false),
            ("grandchild".to_owned(), 1, true),
        ]
    );
}

#[test]
fn a_server_fold_hides_the_group_and_colors_plus_n_by_the_most_demanding_child() {
    let mut snapshot = owned_snapshot();
    set_group(
        &mut snapshot,
        "owner",
        crate::protocol::ClientShellAgentGroup {
            collapsed: true,
            ..Default::default()
        },
    );
    for agent in &mut snapshot.agents {
        if agent.pane_id == "grandchild" {
            agent.agent_status = AgentStatus::Blocked;
        }
    }
    let config = ClientShellConfig::from_config(&Config::default());
    let rows = crate::client::shell::agent_sidebar::agent_rows(&snapshot, &config, None);

    let arranged = arrange_agent_hierarchy(&snapshot, &ClientTreeChrome::default(), rows);

    assert_eq!(
        arranged
            .iter()
            .map(|row| row.pane_id.as_str())
            .collect::<Vec<_>>(),
        ["owner", "root"]
    );
    let owner = &arranged[0];
    assert_eq!(owner.group.expanded, Some(false));
    assert_eq!(owner.group.hidden_children, 3);
    assert_eq!(owner.group.hidden_status, Some(AgentStatus::Blocked));
    assert!(owner.group.server_collapsed);

    let mut state = composed(snapshot);
    let text = state_text(&mut state);
    assert!(text.contains("+3 \u{25b8}"), "{text}");
}

#[test]
fn a_hands_on_row_shows_the_pin_marker() {
    let mut snapshot = owned_snapshot();
    set_group(
        &mut snapshot,
        "root",
        crate::protocol::ClientShellAgentGroup {
            hands_on: true,
            ..Default::default()
        },
    );

    let text = state_text(&mut composed(snapshot));

    assert!(text.contains("\u{26b2} "), "{text}");
}

#[test]
fn a_tab_header_carries_the_group_chevron_when_agent_rows_are_hidden() {
    let mut snapshot = owned_snapshot();
    set_group(
        &mut snapshot,
        "owner",
        crate::protocol::ClientShellAgentGroup {
            collapsed: true,
            ..Default::default()
        },
    );
    let mut config = ClientShellConfig::from_config(&Config::default());
    config.agent_panel_sort = crate::config::AgentPanelSortConfig::Tree;
    let mut state = ClientShellState::new(config);
    state.tree_chrome.insert(
        crate::client::endpoint::ClientEndpointId::Local,
        ClientTreeChrome {
            show_agents: false,
            ..ClientTreeChrome::default()
        },
    );
    state.set_snapshot(Box::new(snapshot));
    state.set_pane_surface(surface());
    state.compose(106, 40).expect("composed frame");

    let group = state
        .hits
        .tree_headers
        .iter()
        .find(|hit| hit.tab_id.is_some())
        .and_then(|hit| hit.group.clone())
        .expect("tab header group control");
    assert_eq!(group.owner_pane_id, "owner");
    assert!(!group.expanded);
    assert!(state_text(&mut state).contains("+3 \u{25b8}"));

    let outcome = click(&mut state, group.rect);

    assert_eq!(
        group_collapse_requests(&outcome),
        [("owner".to_owned(), false)]
    );
}

#[test]
fn focusing_an_agent_inside_a_server_fold_opens_the_fold_after_the_focus() {
    let mut snapshot = owned_snapshot();
    set_group(
        &mut snapshot,
        "child_a",
        crate::protocol::ClientShellAgentGroup {
            collapsed: true,
            ..Default::default()
        },
    );
    let mut state = composed(snapshot);
    let mut outcome = ClientShellInput::default();

    state.push_endpoint_method(
        crate::api::schema::Method::PaneFocus(crate::api::schema::PaneTarget {
            pane_id: "grandchild".into(),
        }),
        &mut outcome,
    );

    let methods = outcome
        .actions
        .iter()
        .filter_map(|action| match action {
            ClientShellAction::Endpoint { request, .. } => {
                Some(crate::api::api_method_name(&request.method))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(methods, ["pane.focus", "agent.group.collapse"]);
    // Only the folded ancestor opens; the expanded owner above it is untouched.
    assert_eq!(
        group_collapse_requests(&outcome),
        [("child_a".to_owned(), false)]
    );
}

fn menu_labels(state: &ClientShellState) -> Vec<String> {
    let Some(ClientShellOverlay::ContextMenu(menu)) = state.overlay.as_ref() else {
        panic!("context menu open");
    };
    menu.items().into_iter().map(|item| item.label).collect()
}

fn group_set_requests(outcome: &ClientShellInput) -> Vec<crate::api::schema::AgentGroupSetParams> {
    outcome
        .actions
        .iter()
        .filter_map(|action| match action {
            ClientShellAction::Endpoint { request, .. } => match &request.method {
                crate::api::schema::Method::AgentGroupSet(params) => Some(params.clone()),
                _ => None,
            },
            _ => None,
        })
        .collect()
}

#[test]
fn right_clicking_an_agent_row_pins_it_hands_on_through_the_endpoint() {
    let mut state = composed(owned_snapshot());
    let (rect, _) = state
        .hits
        .agents
        .iter()
        .find(|(_, pane_id)| pane_id == "child_b")
        .cloned()
        .expect("child_b row");
    state.handle_raw_events(vec![RawInputEvent::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Right),
        column: rect.x + 2,
        row: rect.y,
        modifiers: KeyModifiers::empty(),
    })]);
    let labels = menu_labels(&state);
    assert_eq!(
        labels[..4],
        ["Focus", "Rename pane", "Pin hands-on", "Nest under..."]
    );
    let pin = labels
        .iter()
        .position(|label| label == "Pin hands-on")
        .expect("pin item");

    let mut outcome = ClientShellInput::default();
    state.activate_context_menu_item(pin, &mut outcome);

    assert_eq!(
        group_set_requests(&outcome),
        [crate::api::schema::AgentGroupSetParams {
            target: "child_b".into(),
            placement: crate::api::schema::AgentGroupPlacementKind::HandsOn,
            parent: None,
        }]
    );
}

#[test]
fn the_owner_menu_folds_its_group_and_the_nest_picker_leaves_out_descendants() {
    let mut state = composed(owned_snapshot());
    state.open_agent_context_menu("owner".into(), 0, 0);
    let labels = menu_labels(&state);
    let collapse = labels
        .iter()
        .position(|label| label == "Collapse group")
        .expect("collapse item on a group owner");
    let mut outcome = ClientShellInput::default();
    state.activate_context_menu_item(collapse, &mut outcome);
    assert_eq!(
        group_collapse_requests(&outcome),
        [("owner".to_owned(), true)]
    );

    state.open_agent_context_menu("child_a".into(), 0, 0);
    let nest = menu_labels(&state)
        .iter()
        .position(|label| label == "Nest under...")
        .expect("nest item");
    state.activate_context_menu_item(nest, &mut ClientShellInput::default());
    let picker = menu_labels(&state);
    // child_a's own grandchild cannot become its parent.
    assert_eq!(picker, ["Automatic \u{2713}", "owner", "child_b", "root"]);
    let root = picker
        .iter()
        .position(|label| label == "root")
        .expect("root");
    let mut outcome = ClientShellInput::default();
    state.activate_context_menu_item(root, &mut outcome);

    assert_eq!(
        group_set_requests(&outcome),
        [crate::api::schema::AgentGroupSetParams {
            target: "child_a".into(),
            placement: crate::api::schema::AgentGroupPlacementKind::Under,
            parent: Some("root".into()),
        }]
    );
}

#[test]
fn the_group_key_folds_the_group_holding_a_focused_child() {
    let mut snapshot = owned_snapshot();
    snapshot.focused_pane_id = Some("child_b".into());
    let mut state = composed(snapshot);
    let mut outcome = ClientShellInput::default();

    state.record_binding(
        crate::input::KeybindMatch::Action(crate::input::KeybindAction::ToggleAgentGroup),
        &mut outcome,
    );

    assert_eq!(
        group_collapse_requests(&outcome),
        [("owner".to_owned(), true)]
    );
}

#[test]
fn the_hands_on_key_toggles_the_focused_agent() {
    let mut snapshot = owned_snapshot();
    snapshot.focused_pane_id = Some("root".into());
    set_group(
        &mut snapshot,
        "root",
        crate::protocol::ClientShellAgentGroup {
            hands_on: true,
            ..Default::default()
        },
    );
    let mut state = composed(snapshot);
    let mut outcome = ClientShellInput::default();

    state.record_binding(
        crate::input::KeybindMatch::Action(crate::input::KeybindAction::ToggleHandsOn),
        &mut outcome,
    );

    assert_eq!(
        group_set_requests(&outcome),
        [crate::api::schema::AgentGroupSetParams {
            target: "root".into(),
            placement: crate::api::schema::AgentGroupPlacementKind::Auto,
            parent: None,
        }]
    );
}

fn state_text(state: &mut ClientShellState) -> String {
    let frame = state.compose(106, 40).expect("composed frame");
    let buffer = frame.to_ratatui_buffer().expect("buffer");
    let area = buffer.area;
    (area.y..area.bottom())
        .map(|y| {
            (area.x..area.right())
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn close_focus_state(focus: crate::config::AgentCloseFocusConfig) -> ClientShellState {
    let mut config = Config::default();
    config.ui.agent_close_focus = focus;
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&config));
    let mut snapshot = owned_snapshot();
    snapshot
        .agents
        .retain(|agent| agent.owner_pane_id.is_none());
    snapshot
        .panes
        .retain(|pane| pane.pane_id == "owner" || pane.pane_id == "root");
    // One pane per tab, so closing either takes its whole tab.
    snapshot.tabs.push(ClientShellTab {
        tab_id: "tab_2".into(),
        workspace_id: "ws_1".into(),
        number: 2,
        label: "two".into(),
        custom_label: true,
        zoomed: false,
        focused: false,
        agent_status: AgentStatus::Idle,
    });
    for item in &mut snapshot.panes {
        if item.pane_id == "root" {
            item.tab_id = "tab_2".into();
        }
    }
    for agent in &mut snapshot.agents {
        if agent.pane_id == "root" {
            agent.tab_id = "tab_2".into();
        }
    }
    snapshot.focused_pane_id = Some("owner".into());
    state.set_snapshot(Box::new(snapshot));
    state
}

fn close_focus_requests(state: &mut ClientShellState) -> Vec<crate::api::schema::Method> {
    let mut outcome = ClientShellInput::default();
    state.push_endpoint_method(
        crate::api::schema::Method::PaneClose(crate::api::schema::PaneTarget {
            pane_id: "owner".into(),
        }),
        &mut outcome,
    );
    outcome
        .actions
        .into_iter()
        .filter_map(|action| match action {
            ClientShellAction::Endpoint { request, .. } => Some(request.method),
            _ => None,
        })
        .collect()
}

#[test]
fn panel_next_focuses_the_next_agent_after_a_whole_tab_close() {
    let mut state = close_focus_state(crate::config::AgentCloseFocusConfig::PanelNext);

    let methods = close_focus_requests(&mut state);

    assert!(matches!(
        &methods[..],
        [
            crate::api::schema::Method::PaneClose(closed),
            crate::api::schema::Method::PaneFocus(next),
        ] if closed.pane_id == "owner" && next.pane_id == "root"
    ));
}

#[test]
fn stock_close_focus_sends_only_the_close() {
    let mut state = close_focus_state(crate::config::AgentCloseFocusConfig::Stock);

    let methods = close_focus_requests(&mut state);

    assert_eq!(methods.len(), 1);
}

#[test]
fn panel_next_stays_spatial_when_the_tab_keeps_siblings() {
    let mut state = close_focus_state(crate::config::AgentCloseFocusConfig::PanelNext);
    let mut snapshot = state.snapshot.as_deref().cloned().expect("snapshot");
    snapshot.panes.push(ClientShellPane {
        pane_id: "sibling".into(),
        workspace_id: "ws_1".into(),
        tab_id: "tab_1".into(),
        label: None,
        cwd: None,
        foreground_cwd: None,
        focused: false,
        right_click_passthrough: false,
    });
    state.set_snapshot(Box::new(snapshot));

    let methods = close_focus_requests(&mut state);

    assert_eq!(methods.len(), 1);
}

#[test]
fn panel_next_ignores_a_close_of_an_unfocused_pane() {
    let mut state = close_focus_state(crate::config::AgentCloseFocusConfig::PanelNext);
    let mut outcome = ClientShellInput::default();
    state.push_endpoint_method(
        crate::api::schema::Method::PaneClose(crate::api::schema::PaneTarget {
            pane_id: "root".into(),
        }),
        &mut outcome,
    );

    assert_eq!(outcome.actions.len(), 1);
}

#[test]
fn panel_next_does_nothing_with_a_single_agent() {
    let mut state = close_focus_state(crate::config::AgentCloseFocusConfig::PanelNext);
    let mut snapshot = state.snapshot.as_deref().cloned().expect("snapshot");
    snapshot.agents.retain(|agent| agent.pane_id == "owner");
    state.set_snapshot(Box::new(snapshot));

    let methods = close_focus_requests(&mut state);

    assert_eq!(methods.len(), 1);
}
