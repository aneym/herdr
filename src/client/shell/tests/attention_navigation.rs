use super::*;
use crate::client::shell::tree::ClientTreeChrome;
use crate::factory_overlay::{Attention, FactoryOverlay, TabKind, TabMode, TabTag};
use crate::input::KeybindAction;

fn attention_state(statuses: &[AgentStatus]) -> ClientShellState {
    let mut config = ClientShellConfig::from_config(&Config::default());
    config.factory.enabled = true;
    config.agent_panel_sort = crate::config::AgentPanelSortConfig::Tree;
    let mut state = ClientShellState::new(config);
    let mut projected = snapshot();
    projected.agents.clear();
    projected.tabs.clear();
    projected.panes.clear();
    for (index, status) in statuses.iter().copied().enumerate() {
        let pane_id = format!("pane_{index}");
        let tab_id = format!("tab_{index}");
        projected.tabs.push(ClientShellTab {
            sort_rank: 0,
            desk_count: 0,
            tab_id: tab_id.clone(),
            workspace_id: "ws_1".into(),
            number: index + 1,
            label: tab_id.clone(),
            custom_label: true,
            zoomed: false,
            focused: index == 0,
            agent_status: status,
            work_status: None,
        });
        projected.panes.push(ClientShellPane {
            tokens: Default::default(),
            pane_id: pane_id.clone(),
            workspace_id: "ws_1".into(),
            tab_id: tab_id.clone(),
            label: None,
            cwd: None,
            foreground_cwd: None,
            focused: index == 0,
            right_click_passthrough: false,
            machine: None,
        });
        projected.agents.push(ClientShellAgent {
            pane_id,
            workspace_id: "ws_1".into(),
            tab_id,
            name: Some(format!("agent {index}")),
            display_agent: None,
            agent: Some("pi".into()),
            title: None,
            terminal_title: None,
            terminal_title_stripped: None,
            agent_status: if status == AgentStatus::Done {
                AgentStatus::Working
            } else {
                status
            },
            state_change_seq: index as u64,
            state_labels: Vec::new(),
            tokens: Vec::new(),
            focused: index == 0,
            visible_in_profile: true,
            owner_pane_id: None,
            orphaned: false,
            group: Default::default(),
        });
    }
    projected.focused_pane_id = Some("pane_0".into());
    projected.focused_tab_id = Some("tab_0".into());
    state.set_snapshot(Box::new(projected));
    if statuses.contains(&AgentStatus::Done) {
        let mut completed = state.snapshot.as_deref().unwrap().clone();
        completed.revision += 1;
        for (agent, status) in completed.agents.iter_mut().zip(statuses) {
            agent.agent_status = if *status == AgentStatus::Done {
                AgentStatus::Idle
            } else {
                *status
            };
            if *status == AgentStatus::Done {
                agent.state_change_seq += 1;
            }
        }
        state.set_snapshot(Box::new(completed));
    }
    state.factory_overlay = Some(std::sync::Arc::new(FactoryOverlay::default()));
    state
}

fn focused_pane(state: &mut ClientShellState, action: KeybindAction) -> Option<String> {
    let mut input = ClientShellInput::default();
    state.record_binding(crate::input::KeybindMatch::Action(action), &mut input);
    let target = input.actions.iter().find_map(|action| match action {
        ClientShellAction::Endpoint { request, .. } => match &request.method {
            crate::api::schema::Method::PaneFocus(target) => Some(target.pane_id.clone()),
            _ => None,
        },
        _ => None,
    });
    if let Some(pane_id) = &target {
        let mut projected = (**state.snapshot.as_ref().unwrap()).clone();
        projected.focused_pane_id = Some(pane_id.clone());
        projected.focused_tab_id = projected
            .panes
            .iter()
            .find(|pane| &pane.pane_id == pane_id)
            .map(|pane| pane.tab_id.clone());
        for pane in &mut projected.panes {
            pane.focused = &pane.pane_id == pane_id;
        }
        for agent in &mut projected.agents {
            agent.focused = &agent.pane_id == pane_id;
        }
        projected.revision += 1;
        let overlay = state.factory_overlay.clone();
        state.set_snapshot(Box::new(projected));
        state.factory_overlay = overlay;
    }
    target
}

#[test]
fn next_attention_ranks_blocked_done_and_ask_and_skips_non_attention() {
    let mut state = attention_state(&[
        AgentStatus::Working,
        AgentStatus::Idle,
        AgentStatus::Unknown,
        AgentStatus::Done,
        AgentStatus::Done,
        AgentStatus::Working,
        AgentStatus::Blocked,
        AgentStatus::Blocked,
    ]);
    let mut overlay = FactoryOverlay::default();
    overlay.tabs.insert(
        "tab_1".into(),
        TabTag {
            attention: Attention::Act,
            ..TabTag::default()
        },
    );
    overlay.tabs.insert(
        "tab_4".into(),
        TabTag {
            busy: true,
            ..TabTag::default()
        },
    );
    overlay.tabs.insert(
        "tab_7".into(),
        TabTag {
            done: true,
            ..TabTag::default()
        },
    );
    state.factory_overlay = Some(std::sync::Arc::new(overlay));
    assert_eq!(
        state.snapshot.as_ref().unwrap().agents[3].agent_status,
        AgentStatus::Done
    );
    assert_eq!(
        focused_pane(&mut state, KeybindAction::NextAttention).as_deref(),
        Some("pane_6")
    );
    assert_eq!(
        state.snapshot.as_ref().unwrap().agents[3].agent_status,
        AgentStatus::Done
    );
    // Once the blocked pane has focus it is excluded; the unread completion wins.
    assert_eq!(
        focused_pane(&mut state, KeybindAction::NextAttention).as_deref(),
        Some("pane_3")
    );
    // The blocked pane outranks the ask again once focus leaves it.
    assert_eq!(
        focused_pane(&mut state, KeybindAction::NextAttention).as_deref(),
        Some("pane_6")
    );
    // Remove the blocked and done candidates to expose the lower-rank ask.
    let mut overlay = (*state.factory_overlay.as_ref().unwrap()).as_ref().clone();
    overlay.tabs.insert(
        "tab_3".into(),
        TabTag {
            done: true,
            ..TabTag::default()
        },
    );
    overlay.tabs.insert(
        "tab_6".into(),
        TabTag {
            done: true,
            ..TabTag::default()
        },
    );
    state.factory_overlay = Some(std::sync::Arc::new(overlay));
    assert_eq!(
        focused_pane(&mut state, KeybindAction::NextAttention).as_deref(),
        Some("pane_1")
    );
}

#[test]
fn next_attention_visits_both_done_panes_in_order_even_inside_folds() {
    let mut state = attention_state(&[AgentStatus::Done, AgentStatus::Done]);
    let mut overlay = FactoryOverlay::default();
    overlay.tabs.insert(
        "tab_0".into(),
        TabTag {
            kind: TabKind::Lane,
            ..TabTag::default()
        },
    );
    overlay.tabs.insert(
        "tab_1".into(),
        TabTag {
            kind: TabKind::Workflow,
            parent: Some("tab_0".into()),
            ..TabTag::default()
        },
    );
    state.factory_overlay = Some(std::sync::Arc::new(overlay));
    let mut tree = ClientTreeChrome::default();
    tree.collapsed_spaces.insert("ws_1".into());
    tree.factory_collapsed_lanes.insert("tab_0".into());
    state
        .tree_chrome
        .insert(crate::client::endpoint::ClientEndpointId::Local, tree);
    assert_eq!(
        focused_pane(&mut state, KeybindAction::NextAttention).as_deref(),
        Some("pane_1")
    );
    let tree = state
        .tree_chrome
        .get(&crate::client::endpoint::ClientEndpointId::Local)
        .unwrap();
    assert!(!tree.collapsed_spaces.contains("ws_1"));
    assert!(!tree.factory_collapsed_lanes.contains("tab_0"));
    assert!(tree.factory_expanded_lanes.contains("tab_0"));
    assert_eq!(
        focused_pane(&mut state, KeybindAction::NextAttention).as_deref(),
        Some("pane_0")
    );
}

#[test]
fn next_attention_rotates_after_focused_background_tab() {
    for tag in [
        TabTag {
            kind: TabKind::Advisor,
            ..TabTag::default()
        },
        TabTag {
            done: true,
            ..TabTag::default()
        },
    ] {
        let mut state =
            attention_state(&[AgentStatus::Done, AgentStatus::Working, AgentStatus::Done]);
        let mut projected = (**state.snapshot.as_ref().unwrap()).clone();
        projected.focused_pane_id = Some("pane_1".into());
        projected.focused_tab_id = Some("tab_1".into());
        for pane in &mut projected.panes {
            pane.focused = pane.pane_id == "pane_1";
        }
        projected.revision += 1;
        state.set_snapshot(Box::new(projected));
        let mut overlay = FactoryOverlay::default();
        overlay.tabs.insert("tab_1".into(), tag);
        state.factory_overlay = Some(std::sync::Arc::new(overlay));
        assert_eq!(
            focused_pane(&mut state, KeybindAction::NextAttention).as_deref(),
            Some("pane_2")
        );
    }
}

#[test]
fn next_attention_skips_focused_ask_tab_without_an_agent() {
    let mut config = ClientShellConfig::from_config(&Config::default());
    config.factory.enabled = true;
    let mut state = ClientShellState::new(config);
    let mut projected = snapshot();
    for number in 2..=3 {
        let mut tab = projected.tabs[0].clone();
        tab.tab_id = format!("tab_{number}");
        tab.number = number;
        tab.focused = false;
        projected.tabs.push(tab);
        let mut pane = projected.panes[0].clone();
        pane.pane_id = format!("pane_{number}");
        pane.tab_id = format!("tab_{number}");
        pane.focused = false;
        projected.panes.push(pane);
    }
    state.set_snapshot(Box::new(projected));
    let mut overlay = FactoryOverlay::default();
    for number in [1, 3] {
        overlay.tabs.insert(
            format!("tab_{number}"),
            TabTag {
                attention: Attention::Act,
                ..TabTag::default()
            },
        );
    }
    state.factory_overlay = Some(std::sync::Arc::new(overlay));

    let snapshot = state.snapshot.as_deref().unwrap();
    assert_eq!(
        state.next_attention_target(snapshot),
        Some((None, "tab_3".into()))
    );
    let mut projected = snapshot.clone();
    projected.focused_tab_id = Some("tab_3".into());
    projected.focused_pane_id = Some("pane_3".into());
    assert_eq!(
        state.next_attention_target(&projected),
        Some((None, "tab_1".into()))
    );
}

#[test]
fn parked_and_auto_lanes_are_anchors_except_blocked_auto_panes() {
    let mut state = attention_state(&[
        AgentStatus::Working,
        AgentStatus::Done,
        AgentStatus::Done,
        AgentStatus::Done,
        AgentStatus::Done,
        AgentStatus::Blocked,
        AgentStatus::Done,
    ]);
    let mut overlay = FactoryOverlay::default();
    for (id, mode) in [("tab_1", TabMode::Parked), ("tab_3", TabMode::Auto)] {
        overlay.tabs.insert(
            id.into(),
            TabTag {
                kind: TabKind::Lane,
                mode,
                ..Default::default()
            },
        );
    }
    for (id, parent) in [("tab_2", "tab_1"), ("tab_4", "tab_3")] {
        overlay.tabs.insert(
            id.into(),
            TabTag {
                kind: TabKind::Workflow,
                parent: Some(parent.into()),
                attention: Attention::Act,
                ..Default::default()
            },
        );
    }
    overlay.tabs.insert(
        "tab_5".into(),
        TabTag {
            kind: TabKind::Lane,
            mode: TabMode::Parked,
            done: true,
            ..Default::default()
        },
    );
    overlay.tabs.insert(
        "tab_6".into(),
        TabTag {
            kind: TabKind::Lane,
            ..Default::default()
        },
    );
    state.factory_overlay = Some(std::sync::Arc::new(overlay));
    // With no eligible blocked pane, neither parked nor auto Done panes may win.
    assert_eq!(
        focused_pane(&mut state, KeybindAction::NextAttention).as_deref(),
        Some("pane_6")
    );
    let mut overlay = (*state.factory_overlay.as_ref().unwrap()).as_ref().clone();
    overlay.tabs.get_mut("tab_5").unwrap().mode = TabMode::Auto;
    overlay.tabs.get_mut("tab_5").unwrap().done = false;
    state.factory_overlay = Some(std::sync::Arc::new(overlay));
    assert_eq!(
        focused_pane(&mut state, KeybindAction::NextAttention).as_deref(),
        Some("pane_5")
    );
    assert_eq!(
        focused_pane(&mut state, KeybindAction::NextAttention).as_deref(),
        Some("pane_6")
    );
    let mut overlay = (*state.factory_overlay.as_ref().unwrap()).as_ref().clone();
    overlay.tabs.get_mut("tab_6").unwrap().mode = TabMode::Parked;
    state.factory_overlay = Some(std::sync::Arc::new(overlay));
    assert_eq!(
        focused_pane(&mut state, KeybindAction::NextAttention).as_deref(),
        Some("pane_5")
    );
}

#[test]
fn next_attention_has_no_fallback_to_idle_or_working_agents() {
    let mut state = attention_state(&[
        AgentStatus::Idle,
        AgentStatus::Working,
        AgentStatus::Unknown,
    ]);
    assert_eq!(focused_pane(&mut state, KeybindAction::NextAttention), None);
    assert_eq!(
        state.snapshot.as_ref().unwrap().focused_pane_id.as_deref(),
        Some("pane_0")
    );
}

#[test]
fn next_agent_rotates_normally_with_factory_asks_present() {
    let mut state = attention_state(&[AgentStatus::Idle, AgentStatus::Idle, AgentStatus::Idle]);
    let mut overlay = FactoryOverlay::default();
    overlay.tabs.insert(
        "tab_2".into(),
        TabTag {
            kind: TabKind::Lane,
            attention: Attention::Act,
            ..TabTag::default()
        },
    );
    state.factory_overlay = Some(std::sync::Arc::new(overlay));
    assert_eq!(
        focused_pane(&mut state, KeybindAction::NextAgent).as_deref(),
        Some("pane_1")
    );
}
