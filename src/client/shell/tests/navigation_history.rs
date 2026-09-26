use super::*;

fn two_pane_snapshot(focused: &str, revision: u64) -> ClientShellSnapshot {
    let mut snapshot = snapshot();
    snapshot.revision = revision;
    snapshot.focused_pane_id = Some(focused.into());
    snapshot.panes[0].focused = focused == "pane_1";
    let mut second = snapshot.panes[0].clone();
    second.pane_id = "pane_2".into();
    second.focused = focused == "pane_2";
    snapshot.panes.push(second);
    snapshot
}

fn endpoint_requests(outcome: ClientShellInput) -> Vec<(String, crate::api::schema::Method)> {
    outcome
        .actions
        .into_iter()
        .filter_map(|action| match action {
            ClientShellAction::Endpoint { request, .. } => Some((request.id, request.method)),
            _ => None,
        })
        .collect()
}

fn endpoint_methods(outcome: ClientShellInput) -> Vec<crate::api::schema::Method> {
    endpoint_requests(outcome)
        .into_iter()
        .map(|(_, method)| method)
        .collect()
}

#[test]
fn focus_history_navigates_back_and_forward_with_stable_pane_ids() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(two_pane_snapshot("pane_1", 1)));
    state.set_snapshot(Box::new(two_pane_snapshot("pane_2", 2)));

    let mut outcome = ClientShellInput::default();
    state.record_binding(
        crate::input::KeybindMatch::Action(crate::input::KeybindAction::FocusBack),
        &mut outcome,
    );
    assert!(matches!(
        &endpoint_methods(outcome)[..],
        [crate::api::schema::Method::PaneFocus(target)] if target.pane_id == "pane_1"
    ));

    state.set_snapshot(Box::new(two_pane_snapshot("pane_1", 3)));
    let mut outcome = ClientShellInput::default();
    state.record_binding(
        crate::input::KeybindMatch::Action(crate::input::KeybindAction::FocusForward),
        &mut outcome,
    );
    assert!(matches!(
        &endpoint_methods(outcome)[..],
        [crate::api::schema::Method::PaneFocus(target)] if target.pane_id == "pane_2"
    ));
}

#[test]
fn failed_focus_history_request_restores_the_target() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(two_pane_snapshot("pane_1", 1)));
    state.set_snapshot(Box::new(two_pane_snapshot("pane_2", 2)));
    let mut outcome = ClientShellInput::default();
    state.record_binding(
        crate::input::KeybindMatch::Action(crate::input::KeybindAction::FocusBack),
        &mut outcome,
    );
    let request_id = endpoint_requests(outcome).remove(0).0;
    state.handle_endpoint_result(
        "boot-1",
        &request_id,
        Err(ClientShellEndpointError {
            code: Some("stale_target".into()),
            message: "target changed".into(),
        }),
    );
    let mut retry = ClientShellInput::default();
    state.record_binding(
        crate::input::KeybindMatch::Action(crate::input::KeybindAction::FocusBack),
        &mut retry,
    );
    assert!(matches!(
        &endpoint_methods(retry)[..],
        [crate::api::schema::Method::PaneFocus(target)] if target.pane_id == "pane_1"
    ));
}

#[test]
fn mouse_navigation_uses_configured_focus_history_action_on_press_only() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(two_pane_snapshot("pane_1", 1)));
    state.set_snapshot(Box::new(two_pane_snapshot("pane_2", 2)));

    let release = state.handle_raw_events(vec![RawInputEvent::MouseNavButton {
        button: crate::raw_input::MouseNavButton::Back,
        pressed: false,
    }]);
    assert!(endpoint_methods(release).is_empty());
    let press = state.handle_raw_events(vec![RawInputEvent::MouseNavButton {
        button: crate::raw_input::MouseNavButton::Back,
        pressed: true,
    }]);
    assert!(matches!(
        &endpoint_methods(press)[..],
        [crate::api::schema::Method::PaneFocus(target)] if target.pane_id == "pane_1"
    ));
}

#[test]
fn passive_snapshots_preserve_folds_but_explicit_focus_reveals_already_active_pane() {
    let mut config = Config::default();
    config.ui.agent_panel_sort = crate::config::AgentPanelSortConfig::Tree;
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&config));
    state.set_snapshot(Box::new(two_pane_snapshot("pane_1", 1)));
    state
        .tree_chrome_mut()
        .collapsed_spaces
        .insert("ws_1".into());
    state
        .tree_chrome_mut()
        .collapsed_tabs
        .insert(super::super::tree::tab_key("ws_1", 1));

    state.set_snapshot(Box::new(two_pane_snapshot("pane_1", 2)));
    assert!(state.tree_chrome_mut().collapsed_spaces.contains("ws_1"));

    let mut outcome = ClientShellInput::default();
    state.push_endpoint_method(
        crate::api::schema::Method::PaneFocus(crate::api::schema::PaneTarget {
            pane_id: "pane_1".into(),
        }),
        &mut outcome,
    );
    let request_id = endpoint_requests(outcome).remove(0).0;
    // An unrelated snapshot while the request is in flight must not unfold it.
    state.set_snapshot(Box::new(two_pane_snapshot("pane_1", 3)));
    assert!(state.tree_chrome_mut().collapsed_spaces.contains("ws_1"));
    state.handle_endpoint_result(
        "boot-1",
        &request_id,
        Ok(crate::api::schema::ResponseResult::Ok {}),
    );
    let tree = state.tree_chrome_mut();
    assert!(!tree.collapsed_spaces.contains("ws_1"));
    assert!(!tree.collapsed_tabs.contains("ws_1#1"));
}

#[test]
fn malformed_owner_cycle_does_not_block_explicit_reveal() {
    let mut config = Config::default();
    config.ui.agent_panel_sort = crate::config::AgentPanelSortConfig::Tree;
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&config));
    let mut snapshot = two_pane_snapshot("pane_1", 1);
    for (pane_id, owner) in [("pane_1", "pane_2"), ("pane_2", "pane_1")] {
        snapshot.agents.push(ClientShellAgent {
            pane_id: pane_id.into(),
            workspace_id: "ws_1".into(),
            tab_id: "tab_1".into(),
            name: Some(pane_id.into()),
            display_agent: None,
            agent: None,
            title: None,
            terminal_title: None,
            terminal_title_stripped: None,
            agent_status: AgentStatus::Idle,
            state_change_seq: 1,
            state_labels: Vec::new(),
            tokens: Vec::new(),
            focused: pane_id == "pane_1",
            owner_pane_id: Some(owner.into()),
            orphaned: false,
            group: Default::default(),
        });
    }
    state.set_snapshot(Box::new(snapshot));
    state
        .tree_chrome_mut()
        .collapsed_agent_groups
        .extend(["pane_1".into(), "pane_2".into()]);

    assert!(state.reveal_tree_ancestors_for_pane("pane_1"));
    assert!(state.tree_chrome_mut().collapsed_agent_groups.is_empty());
}

#[test]
fn workspace_and_tab_focus_reveal_an_already_active_split_pane() {
    for method in [
        crate::api::schema::Method::WorkspaceFocus(crate::api::schema::WorkspaceTarget {
            workspace_id: "ws_1".into(),
        }),
        crate::api::schema::Method::TabFocus(crate::api::schema::TabTarget {
            tab_id: "tab_1".into(),
        }),
    ] {
        let mut config = Config::default();
        config.ui.agent_panel_sort = crate::config::AgentPanelSortConfig::Tree;
        let mut state = ClientShellState::new(ClientShellConfig::from_config(&config));
        state.set_snapshot(Box::new(two_pane_snapshot("pane_2", 1)));
        state
            .tree_chrome_mut()
            .collapsed_spaces
            .insert("ws_1".into());
        state
            .tree_chrome_mut()
            .collapsed_tabs
            .insert("ws_1#1".into());
        let mut outcome = ClientShellInput::default();
        state.push_endpoint_method(method, &mut outcome);
        let request_id = endpoint_requests(outcome).remove(0).0;
        state.handle_endpoint_result(
            "boot-1",
            &request_id,
            Ok(crate::api::schema::ResponseResult::Ok {}),
        );
        assert!(!state.tree_chrome_mut().collapsed_spaces.contains("ws_1"));
        assert!(!state.tree_chrome_mut().collapsed_tabs.contains("ws_1#1"));
        assert!(state.pending_focus_reveals.is_empty());
    }
}

#[test]
fn explicit_reveal_waits_for_matching_target_after_success() {
    let mut config = Config::default();
    config.ui.agent_panel_sort = crate::config::AgentPanelSortConfig::Tree;
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&config));
    state.set_snapshot(Box::new(two_pane_snapshot("pane_1", 1)));
    state
        .tree_chrome_mut()
        .collapsed_spaces
        .insert("ws_1".into());
    let mut outcome = ClientShellInput::default();
    state.push_endpoint_method(
        crate::api::schema::Method::PaneFocus(crate::api::schema::PaneTarget {
            pane_id: "pane_2".into(),
        }),
        &mut outcome,
    );
    let request_id = endpoint_requests(outcome).remove(0).0;
    state.handle_endpoint_result(
        "boot-1",
        &request_id,
        Ok(crate::api::schema::ResponseResult::Ok {}),
    );
    state.set_snapshot(Box::new(two_pane_snapshot("pane_1", 2)));
    assert!(state.tree_chrome_mut().collapsed_spaces.contains("ws_1"));
    state.set_snapshot(Box::new(two_pane_snapshot("pane_2", 3)));
    assert!(!state.tree_chrome_mut().collapsed_spaces.contains("ws_1"));
}
