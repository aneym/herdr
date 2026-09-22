use super::*;

#[test]
fn usage_response_after_dismiss_does_not_reopen_overlay() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(snapshot()));
    let mut outcome = ClientShellInput::default();
    state.toggle_usage_overlay(&mut outcome);
    let request_id = outcome
        .actions
        .iter()
        .find_map(|action| match action {
            ClientShellAction::Endpoint { request, .. } => Some(request.id.clone()),
            _ => None,
        })
        .expect("usage request");
    state.toggle_usage_overlay(&mut ClientShellInput::default());
    let boot_id = state.snapshot.as_ref().unwrap().boot_id.clone();
    state.handle_endpoint_result(
        &boot_id,
        &request_id,
        Ok(crate::api::schema::ResponseResult::AgentUsage { usage: Vec::new() }),
    );
    assert!(state.overlay.is_none());
    assert!(state.usage_refresh_deadline.is_none());
}

#[test]
fn profile_load_cancel_cannot_resurrect_menu() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(snapshot()));
    let mut outcome = ClientShellInput::default();
    state.open_profile_context_menu("ws_1".into(), None, true, 2, 3, &mut outcome);
    let generation = state.next_profile_menu_generation;
    state.handle_input_bytes(b"\x1b");
    state.receive_profile_menu_roster(generation, vec!["default".into(), "work".into()]);
    state.receive_profile_menu_workspace_membership(generation, vec!["default".into()]);
    assert!(state.overlay.is_none());
}

#[test]
fn profile_share_retains_other_selected_memberships() {
    let entries = vec![
        ClientProfileMenuEntry {
            profile: Some("default".into()),
            label: "default".into(),
            selected: true,
        },
        ClientProfileMenuEntry {
            profile: Some("work".into()),
            label: "work".into(),
            selected: true,
        },
        ClientProfileMenuEntry {
            profile: Some("review".into()),
            label: "review".into(),
            selected: false,
        },
    ];
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(snapshot()));
    let mut outcome = ClientShellInput::default();
    state.activate_profile_context_action(
        None,
        "ws_1".into(),
        true,
        entries,
        ClientContextMenuAction::ProfileSelect(2),
        &mut outcome,
    );
    assert!(outcome.actions.iter().any(|action| matches!(action,
        ClientShellAction::Endpoint { request, .. }
            if matches!(&request.method, crate::api::schema::Method::WorkspaceSetProfiles(params)
                if params.profiles == ["default", "work", "review"])
    )));
}

#[test]
fn usage_sidebar_left_click_and_refresh_apply_without_timer_spin() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(snapshot()));
    state.hits.agent_usage = Rect::new(2, 1, 6, 1);
    let mut outcome = ClientShellInput::default();
    state.handle_mouse(
        MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: 3,
            row: 1,
            modifiers: crossterm::event::KeyModifiers::empty(),
        },
        &mut outcome,
    );
    assert!(matches!(state.overlay, Some(ClientShellOverlay::Usage(_))));
    let request_id = outcome
        .actions
        .iter()
        .find_map(|action| match action {
            ClientShellAction::Endpoint { request, .. } => Some(request.id.clone()),
            _ => None,
        })
        .unwrap();
    let now = std::time::Instant::now() + std::time::Duration::from_secs(3);
    assert!(state.tick_usage_overlay(now).actions.is_empty());
    assert!(state.timer_delay(now) > std::time::Duration::ZERO);
    state.handle_endpoint_result(
        "boot-1",
        &request_id,
        Ok(crate::api::schema::ResponseResult::AgentUsage { usage: Vec::new() }),
    );
    let refresh = state.tick_usage_overlay(now + std::time::Duration::from_secs(3));
    let id = refresh
        .actions
        .iter()
        .find_map(|action| match action {
            ClientShellAction::Endpoint { request, .. } => Some(request.id.clone()),
            _ => None,
        })
        .unwrap();
    let row = crate::api::schema::AgentUsageInfo {
        pane_id: "pane_1".into(),
        workspace_id: "ws_1".into(),
        tab_id: "tab_1".into(),
        agent: Some("codex".into()),
        title: Some("test".into()),
        cpu_percent: 12.5,
        mem_bytes: 1024,
        process_count: 2,
    };
    state.handle_endpoint_result(
        "boot-1",
        &id,
        Ok(crate::api::schema::ResponseResult::AgentUsage { usage: vec![row] }),
    );
    assert!(
        matches!(&state.overlay, Some(ClientShellOverlay::Usage(usage)) if usage.rows.len() == 1 && usage.rows[0].cpu_percent == 12.5)
    );
}
