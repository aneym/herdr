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

#[test]
fn clicking_a_group_chevron_collapses_that_group() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(owned_snapshot()));
    state.set_pane_surface(surface());
    state.compose(106, 40).expect("composed frame");

    let (rect, key) = state
        .hits
        .agent_groups
        .first()
        .cloned()
        .expect("group chevron hit");
    assert_eq!(key, "owner");

    state.handle_raw_events(vec![RawInputEvent::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: rect.right().saturating_sub(1),
        row: rect.y,
        modifiers: KeyModifiers::empty(),
    })]);

    assert!(state
        .tree_chrome
        .get(&crate::client::endpoint::ClientEndpointId::Local)
        .expect("local tree chrome")
        .collapsed_agent_groups
        .contains("owner"));
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
