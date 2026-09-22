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
    state.overlay = None;
    state.profile_menu_load = None;
    state.receive_profile_menu_roster(generation, vec!["default".into(), "work".into()]);
    state.receive_profile_menu_workspace_membership(generation, vec!["default".into()]);
    assert!(state.overlay.is_none());
}
