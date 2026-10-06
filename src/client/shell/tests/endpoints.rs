use super::*;

#[path = "workspace_navigation.rs"]
mod workspace_navigation;
use crate::client::endpoint::{
    ClientEndpointId, ClientEndpointStatus, ProfileId, SavedSshEndpoint,
};
use crossterm::event::{KeyModifiers, MouseButton, MouseEventKind};

fn remote_profile() -> SavedSshEndpoint {
    SavedSshEndpoint {
        id: ProfileId::parse("0123456789abcdef0123456789abcdef").unwrap(),
        label: "Build".into(),
        target: "dev@build.example".into(),
        session: "agents".into(),
        enabled: true,
    }
}

fn agent(
    name: &str,
    status: crate::api::schema::AgentStatus,
    state_change_seq: u64,
) -> ClientShellAgent {
    ClientShellAgent {
        pane_id: "pane_1".into(),
        workspace_id: "ws_1".into(),
        tab_id: "tab_1".into(),
        name: Some(name.into()),
        display_agent: None,
        agent: Some("pi".into()),
        title: None,
        terminal_title: None,
        terminal_title_stripped: None,
        agent_status: status,
        state_change_seq,
        state_labels: Vec::new(),
        tokens: Vec::new(),
        focused: true,
        visible_in_profile: true,
        owner_pane_id: None,
        orphaned: false,
        group: Default::default(),
    }
}

fn current_workspace_view() -> crate::api::schema::AgentViewSetParams {
    use crate::api::schema::{
        AgentViewBuiltinField, AgentViewContext, AgentViewField, AgentViewFilter, AgentViewValue,
    };

    crate::api::schema::AgentViewSetParams {
        source: "example.views".into(),
        label: Some("current space".into()),
        filter: Some(AgentViewFilter::Eq {
            field: AgentViewField::Builtin(AgentViewBuiltinField::WorkspaceId),
            value: AgentViewValue::Context {
                context: AgentViewContext::CurrentWorkspaceId,
            },
        }),
        sort: Vec::new(),
    }
}

#[derive(Clone)]
struct CapturingTransport(std::sync::Arc<std::sync::Mutex<Vec<crate::protocol::ClientMessage>>>);

impl crate::client::endpoint::EndpointTransport for CapturingTransport {
    fn send(&mut self, message: &crate::protocol::ClientMessage) -> std::io::Result<()> {
        self.0.lock().unwrap().push(message.clone());
        Ok(())
    }
}

fn state_with_remote() -> (ClientShellState, ClientEndpointId) {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    let profile = remote_profile();
    let endpoint_id = ClientEndpointId::Ssh(profile.id.clone());
    state.set_endpoint_catalog(&[profile]);
    state.set_endpoint_status(&endpoint_id, ClientEndpointStatus::Online);
    state.set_snapshot(Box::new(snapshot()));
    state.set_pane_surface(surface());
    let mut remote = snapshot();
    remote.boot_id = "remote-boot".into();
    remote.workspaces[0].label = "remote-workspace".into();
    // Another machine's space shows only while it holds a chat.
    remote
        .agents
        .push(agent("remote", crate::api::schema::AgentStatus::Idle, 0));
    state.set_endpoint_snapshot(&endpoint_id, Box::new(remote));
    (state, endpoint_id)
}

#[test]
fn pinned_chats_survive_aggregate_sidebar_and_route_to_their_endpoint() {
    let (mut state, remote) = state_with_remote();
    let mut snapshot = state
        .endpoints
        .iter()
        .find(|endpoint| endpoint.endpoint_id == remote)
        .and_then(|endpoint| endpoint.snapshot.clone())
        .expect("remote snapshot");
    snapshot
        .pinned_tabs
        .push(crate::protocol::ClientShellPinnedTab {
            role: None,
            tab_id: "tab_1".into(),
            workspace_id: "ws_1".into(),
        });
    state.set_endpoint_snapshot(&remote, snapshot);
    state.compose(100, 28).expect("aggregate frame");
    let (rect, pin, endpoint, _) = state
        .hits
        .endpoint_pins
        .iter()
        .find(|(rect, pin, endpoint, _)| *rect != *pin && *endpoint == remote)
        .cloned()
        .expect("remote pin at top");
    assert_eq!(rect.y, state.hits.sidebar_divider.y + 1);
    // Pinned rows carry no pin toggle; a left press anywhere opens the chat.
    assert!(pin.is_empty());
    // Unpin from the row's context menu, routed to the chat's own endpoint.
    state.handle_raw_events(vec![crate::raw_input::RawInputEvent::Mouse(
        crossterm::event::MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Right),
            column: rect.right() - 2,
            row: rect.y,
            modifiers: KeyModifiers::empty(),
        },
    )]);
    let Some(ClientShellOverlay::ContextMenu(menu)) = state.overlay.as_ref() else {
        panic!("context menu open");
    };
    let labels = menu
        .items()
        .into_iter()
        .map(|item| item.label)
        .collect::<Vec<_>>();
    assert_eq!(labels, ["Unpin"]);
    let mut outcome = ClientShellInput::default();
    state.activate_context_menu_item(0, &mut outcome);
    assert!(outcome.actions.iter().any(|action| matches!(action,
        ClientShellAction::Endpoint { endpoint_id, request, .. }
            if *endpoint_id == endpoint && matches!(&request.method,
                crate::api::schema::Method::TabSetPinned(params) if !params.pinned && params.tab_id == "tab_1")
    )));
    // Local holds the surface; the unpin still reaches the remote that owns it.
    let sent = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut endpoints = crate::client::endpoint::EndpointRegistry::new(
        CapturingTransport(Default::default()),
        1,
        Default::default(),
    );
    endpoints.insert(
        remote.clone(),
        CapturingTransport(sent.clone()),
        1,
        Default::default(),
        false,
    );
    let mut commands = crate::client::endpoint_commands::EndpointCommands::default();
    crate::client::shell_runtime::dispatch_client_shell_actions(
        outcome.actions,
        &mut commands,
        &mut endpoints,
        Some(&mut state),
        &mut Vec::new(),
        &mut None,
        None,
    )
    .unwrap();
    let sent = sent.lock().unwrap().clone();
    assert!(
        matches!(sent.as_slice(), [crate::protocol::ClientMessage::ClientShellEndpointRequest {
        boot_id, request }] if boot_id == "remote-boot" && request.contains("tab.set_pinned")),
        "remote received {sent:?}"
    );
    let mut outcome = ClientShellInput::default();
    assert!(
        state.handle_endpoint_navigation(crate::input::KeybindAction::SwitchTab(0), &mut outcome)
    );
    assert!(outcome.actions.iter().any(|action| matches!(action,
        ClientShellAction::ActivateEndpoint { endpoint_id, target: Some(ClientEndpointFocusTarget::Tab(tab)), .. }
            if *endpoint_id == remote && tab == "tab_1"
    )));
    // Past the last pin, the digits fall through to the focused space's tabs.
    let mut outcome = ClientShellInput::default();
    assert!(
        state.indexed_navigation_target_exists(&crate::input::KeybindMatch::Action(
            crate::input::KeybindAction::SwitchTab(1)
        ))
    );
    assert!(
        state.handle_endpoint_navigation(crate::input::KeybindAction::SwitchTab(1), &mut outcome)
    );
    assert!(outcome.actions.iter().any(|action| matches!(action,
        ClientShellAction::ActivateEndpoint { endpoint_id, target: Some(ClientEndpointFocusTarget::Tab(tab)), .. }
            if *endpoint_id == ClientEndpointId::Local && tab == "tab_1"
    )), "actions: {:?}", outcome.actions);
}

#[test]
fn factory_overlay_control_updates_active_endpoint_and_ignores_stale_revisions() {
    let (mut state, remote) = state_with_remote();
    let overlay =
        crate::factory_overlay::parse(br#"{"version":1,"tabs":{"remote-tab":{"kind":"lane"}}}"#)
            .unwrap();
    let crate::protocol::ServerMessage::EndpointControl { kind, data } =
        crate::protocol::endpoint::factory_overlay_message("remote-boot", 2, Some(&overlay))
            .unwrap()
    else {
        panic!("expected factory overlay control");
    };
    let crate::client::endpoint::EndpointControlMessage::FactoryOverlay(decoded) =
        crate::client::endpoint::decode_endpoint_control(&kind, &data).unwrap()
    else {
        panic!("expected decoded factory overlay");
    };
    assert!(!state.set_endpoint_factory_overlay_for_generation(&remote, 4, decoded.clone()));
    // The remote snapshot in this fixture has no connection generation; install one to
    // exercise the same generation boundary as a live endpoint.
    let mut remote_snapshot = snapshot();
    remote_snapshot.boot_id = "remote-boot".into();
    state.set_endpoint_snapshot_for_generation(&remote, 4, Box::new(remote_snapshot));
    assert!(state.activate_endpoint_projection(&remote));
    assert!(state.set_endpoint_factory_overlay_for_generation(&remote, 4, decoded));
    assert_eq!(state.factory_overlay.as_deref(), Some(&overlay));

    let stale = crate::protocol::endpoint::EndpointFactoryOverlay {
        boot_id: "remote-boot".into(),
        revision: 1,
        overlay: None,
    };
    assert!(!state.set_endpoint_factory_overlay_for_generation(&remote, 4, stale));
    assert_eq!(state.factory_overlay.as_deref(), Some(&overlay));
    assert!(state.activate_endpoint_projection(&ClientEndpointId::Local));
    assert!(state.factory_overlay.is_none());
    assert!(state.activate_endpoint_projection(&remote));
    assert_eq!(state.factory_overlay.as_deref(), Some(&overlay));
    let clear = crate::protocol::endpoint::EndpointFactoryOverlay {
        boot_id: "remote-boot".into(),
        revision: 3,
        overlay: None,
    };
    assert!(state.set_endpoint_factory_overlay_for_generation(&remote, 4, clear));
    assert!(state.factory_overlay.is_none());
}

#[test]
fn machine_diagnostic_badge_reopens_notice() {
    let (mut state, id) = state_with_remote();
    state.set_endpoint_status(&id, ClientEndpointStatus::Attention);
    state.set_machine_diagnostic(&id, "Permission denied (keyboard-interactive)".into());
    for _ in 0..2 {
        state.compose(120, 40).unwrap();
        let hit = state
            .hits
            .machines
            .iter()
            .find(|hit| hit.endpoint_id == id)
            .unwrap();
        let mouse = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: hit.status_badge.x,
            row: hit.status_badge.y,
            modifiers: KeyModifiers::NONE,
        };
        let outcome = state.handle_raw_events(vec![RawInputEvent::Mouse(mouse)]);
        assert!(outcome.repaint);
        let notice = state.visible_endpoint_notice.take().unwrap();
        assert!(notice.body.contains("Permission denied"));
        assert!(notice
            .title
            .contains("herdr machine reconnect 0123456789abcdef0123456789abcdef"));
    }
    state.set_endpoint_status(&id, ClientEndpointStatus::Online);
    state.compose(120, 40).unwrap();
    assert!(!state.machine_diagnostics.required_for(
        state
            .endpoints
            .iter()
            .find(|endpoint| endpoint.endpoint_id == id)
            .unwrap()
    ));
}

fn state_with_scrollable_agents() -> (ClientShellState, ClientEndpointId) {
    let (mut state, remote) = state_with_remote();
    for endpoint_id in [ClientEndpointId::Local, remote.clone()] {
        let mut projection = state
            .endpoints
            .iter()
            .find(|endpoint| endpoint.endpoint_id == endpoint_id)
            .unwrap()
            .snapshot
            .clone()
            .unwrap();
        projection.agents = (0..8)
            .map(|index| ClientShellAgent {
                pane_id: format!("pane_{}", index + 1),
                focused: index == 0,
                ..agent(&format!("agent {index}"), AgentStatus::Idle, 1)
            })
            .collect();
        projection.panes = projection
            .agents
            .iter()
            .map(|agent| ClientShellPane {
                pane_id: agent.pane_id.clone(),
                focused: agent.focused,
                ..projection.panes[0].clone()
            })
            .collect();
        state.set_endpoint_snapshot(&endpoint_id, projection);
    }
    state.compose(100, 28).unwrap();
    state.agent_scroll = 6;
    state.compose(100, 28).unwrap();
    assert_eq!(state.agent_scroll, 6);
    (state, remote)
}

#[test]
fn agent_navigation_reveals_offscreen_targets() {
    use crate::input::KeybindAction;

    for action in [
        KeybindAction::NextAgent,
        KeybindAction::PreviousAgent,
        KeybindAction::FocusAgent(0),
    ] {
        let (mut state, remote) = state_with_scrollable_agents();
        let (endpoint_id, pane_id) = match action {
            KeybindAction::NextAgent => (ClientEndpointId::Local, "pane_2"),
            KeybindAction::PreviousAgent => (remote, "pane_8"),
            _ => (ClientEndpointId::Local, "pane_1"),
        };
        state.agent_scroll = if action == KeybindAction::PreviousAgent {
            0
        } else {
            state.hits.agent_max_scroll
        };
        state.compose(100, 28).unwrap();
        assert!(!state
            .hits
            .endpoint_agents
            .iter()
            .any(|(_, endpoint, pane)| { endpoint == &endpoint_id && pane == pane_id }));

        let mut outcome = ClientShellInput::default();
        assert!(state.handle_endpoint_navigation(action, &mut outcome));
        assert!(outcome.repaint, "agent navigation must request a frame");
        if endpoint_id != state.active_endpoint_id {
            assert!(state.activate_endpoint_projection(&endpoint_id));
        }
        state.compose(100, 28).unwrap();
        assert!(
            state
                .hits
                .endpoint_agents
                .iter()
                .any(|(_, endpoint, pane)| { endpoint == &endpoint_id && pane == pane_id }),
            "{action:?} must reveal the selected agent"
        );
    }
}

#[test]
fn agent_navigation_reveals_target_using_destination_sort() {
    use crate::api::schema::{
        AgentViewBuiltinSortField, AgentViewSort, AgentViewSortField, AgentViewSortOrder,
    };

    let (mut state, remote) = state_with_scrollable_agents();
    for (endpoint_id, base) in [(ClientEndpointId::Local, 0), (remote.clone(), 8)] {
        let mut projection = state
            .endpoints
            .iter()
            .find(|endpoint| endpoint.endpoint_id == endpoint_id)
            .unwrap()
            .snapshot
            .clone()
            .unwrap();
        for (index, agent) in projection.agents.iter_mut().enumerate() {
            agent.state_change_seq = base + index as u64;
        }
        if endpoint_id == remote {
            projection.agent_view_label = Some("recent".into());
        }
        state.set_endpoint_snapshot(&endpoint_id, projection);
    }
    state.set_test_endpoint_agent_view(&ClientEndpointId::Local, None);
    let mut view = current_workspace_view();
    view.label = Some("recent".into());
    view.filter = None;
    view.sort = vec![AgentViewSort {
        field: AgentViewSortField::Builtin(AgentViewBuiltinSortField::StateChangeSeq),
        order: AgentViewSortOrder::Desc,
    }];
    state.set_test_endpoint_agent_view(&remote, Some(view));
    state.compose(100, 28).unwrap();

    let mut outcome = ClientShellInput::default();
    assert!(state
        .handle_endpoint_navigation(crate::input::KeybindAction::FocusAgent(15), &mut outcome,));
    assert!(matches!(
        outcome.actions.as_slice(),
        [ClientShellAction::ActivateEndpoint {
            endpoint_id,
            target: Some(ClientEndpointFocusTarget::Pane(pane_id)),
        }] if endpoint_id == &remote && pane_id == "pane_8"
    ));
    // A superseded handoff restores its source before activating the new target.
    assert!(state.activate_endpoint_projection(&ClientEndpointId::Local));
    state.compose(100, 28).unwrap();
    assert!(state.activate_endpoint_projection(&remote));
    state.compose(100, 28).unwrap();
    assert!(state
        .hits
        .endpoint_agents
        .iter()
        .any(|(_, endpoint, pane)| { endpoint == &remote && pane == "pane_8" }));
}

#[test]
fn agent_navigation_reveal_is_cancelled_by_another_selection() {
    for select_pane in [false, true] {
        let (mut state, remote) = state_with_scrollable_agents();
        let scroll = state.agent_scroll;
        let mut outcome = ClientShellInput::default();
        assert!(state
            .handle_endpoint_navigation(crate::input::KeybindAction::PreviousAgent, &mut outcome,));
        assert_eq!(state.agent_scroll, scroll);
        if select_pane {
            assert!(state.focus_or_activate(
                remote.clone(),
                ClientEndpointFocusTarget::Pane("pane_1".into()),
                &mut outcome,
            ));
        } else {
            assert!(state.activate_endpoint(remote.clone(), &mut outcome));
        }
        assert!(state.activate_endpoint_projection(&remote));
        state.compose(100, 28).unwrap();
        assert_eq!(state.agent_scroll, scroll);
    }
}

#[test]
fn agent_navigation_keeps_scroll_when_target_is_visible() {
    let (mut state, _) = state_with_scrollable_agents();
    let (_, endpoint_id, pane_id) = state.hits.endpoint_agents[1].clone();
    let targets = super::super::aggregate_navigation::online_agent_targets(
        &state.endpoints,
        &state.active_endpoint_id,
        state.config.agent_panel_sort,
    );
    let index = targets
        .iter()
        .position(|target| target.endpoint_id == endpoint_id && target.pane_id == pane_id)
        .unwrap();
    let scroll = state.agent_scroll;
    assert!(state.handle_endpoint_navigation(
        crate::input::KeybindAction::FocusAgent(index),
        &mut ClientShellInput::default(),
    ));
    state.compose(100, 28).unwrap();
    assert_eq!(state.agent_scroll, scroll);
}

#[test]
fn switching_machines_preserves_aggregate_agent_scroll_and_visible_rows() {
    let (mut state, remote) = state_with_scrollable_agents();
    for endpoint_id in [remote.clone(), ClientEndpointId::Local, remote] {
        let visible = state.hits.endpoint_agents.clone();
        let (rect, _, pane_id) = visible
            .iter()
            .find(|(_, endpoint, _)| endpoint == &endpoint_id)
            .expect("destination agent remains visible");
        let click = state.handle_raw_events(vec![RawInputEvent::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: rect.x + 2,
            row: rect.y,
            modifiers: KeyModifiers::NONE,
        })]);
        assert!(matches!(
            click.actions.as_slice(),
            [ClientShellAction::ActivateEndpoint {
                endpoint_id: target,
                target: Some(ClientEndpointFocusTarget::Pane(target_pane)),
            }] if target == &endpoint_id && target_pane == pane_id
        ));

        state.workspace_scroll = 3;
        state.tab_scroll = 2;
        assert!(state.activate_endpoint_projection(&endpoint_id));
        assert_eq!(state.agent_scroll, 6);
        assert_eq!(state.workspace_scroll, 0);
        assert_eq!(state.tab_scroll, 0);
        assert!(state.pane_surface.is_none());

        let mut next_surface = surface();
        next_surface.boot_id = state.endpoint_boot_id(&endpoint_id).unwrap().into();
        state.set_pane_surface(next_surface);
        state.compose(100, 28).unwrap();
        assert_eq!(state.agent_scroll, 6);
        assert_eq!(state.hits.endpoint_agents, visible);
    }
}

#[test]
fn local_agent_click_can_cancel_a_pending_remote_switch() {
    for reconnecting in [false, true] {
        let (mut state, remote) = state_with_scrollable_agents();
        assert!(state.activate_endpoint(remote, &mut ClientShellInput::default()));
        if reconnecting {
            state.mark_endpoint_disconnected(&ClientEndpointId::Local);
        }
        state.compose(100, 28).unwrap();
        let (rect, _, pane_id) = state
            .hits
            .endpoint_agents
            .iter()
            .find(|(_, endpoint, _)| endpoint.is_local())
            .unwrap()
            .clone();
        let outcome = state.handle_raw_events(vec![RawInputEvent::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: rect.x + 2,
            row: rect.y,
            modifiers: KeyModifiers::NONE,
        })]);
        assert!(
            matches!(outcome.actions.as_slice(), [ClientShellAction::ActivateEndpoint {
            endpoint_id: ClientEndpointId::Local,
            target: Some(ClientEndpointFocusTarget::Pane(target)),
        }] if target == &pane_id)
        );
    }
}

#[test]
fn aggregate_agent_scroll_still_clamps_when_rows_shrink_on_activation() {
    let (mut state, remote) = state_with_scrollable_agents();
    for endpoint_id in [ClientEndpointId::Local, remote.clone()] {
        let mut projection = state
            .endpoints
            .iter()
            .find(|endpoint| endpoint.endpoint_id == endpoint_id)
            .unwrap()
            .snapshot
            .clone()
            .unwrap();
        projection.revision += 1;
        projection.agents.truncate(1);
        state.set_endpoint_snapshot(&endpoint_id, projection);
    }
    assert!(state.activate_endpoint_projection(&remote));
    state.compose(100, 28).unwrap();
    assert_eq!(state.agent_scroll, 0);
    assert_eq!(state.hits.agent_max_scroll, 0);
    assert_eq!(state.hits.endpoint_agents.len(), 2);
}

#[test]
fn same_machine_reboot_still_resets_agent_scroll() {
    let (mut state, _) = state_with_scrollable_agents();
    let mut projection = state.snapshot.clone().unwrap();
    projection.boot_id = "restarted-local".into();
    state.cache_endpoint_snapshot(&ClientEndpointId::Local, projection);
    assert!(state.activate_endpoint_projection(&ClientEndpointId::Local));
    assert_eq!(state.agent_scroll, 0);
}

#[test]
fn switching_machines_from_copy_mode_restores_terminal_input() {
    let (mut state, remote) = state_with_remote();
    let mut local_surface = surface();
    local_surface.panes[0].scroll = Some(crate::protocol::PaneSurfaceScrollMetrics {
        offset_from_bottom: 0,
        max_offset_from_bottom: 20,
        viewport_rows: 2,
    });
    state.set_pane_surface(local_surface);
    state.compose(100, 28).unwrap();
    assert!(state.enter_copy_mode(&mut ClientShellInput::default()));
    assert_eq!(state.mode, ClientShellMode::Copy);

    assert!(state.activate_endpoint_projection(&remote));
    let mut remote_surface = surface();
    remote_surface.boot_id = "remote-boot".into();
    state.set_pane_surface(remote_surface);
    state.compose(100, 28).unwrap();

    assert!(state.copy_mode.is_none());
    assert_eq!(state.mode, ClientShellMode::Terminal);
    let input = state.handle_raw_events(vec![RawInputEvent::Key(crate::input::TerminalKey::new(
        KeyCode::Char('x'),
        KeyModifiers::NONE,
    ))]);
    assert!(matches!(
        input.requests.as_slice(),
        [ClientMessage::ClientShellPaneInput { pane_id, events }]
            if pane_id == "pane_1" && events.len() == 1
    ));
}

#[test]
fn live_catalog_rename_preserves_snapshot_and_disable_reenable_clears_it() {
    let (mut state, remote) = state_with_remote();
    let mut profile = remote_profile();
    profile.label = "Renamed".into();
    state.set_endpoint_catalog(&[profile.clone()]);
    assert_eq!(state.endpoint_label(&remote), "Renamed");
    assert!(state.endpoint_is_online(&remote));
    assert_eq!(state.endpoint_boot_id(&remote), Some("remote-boot"));
    profile.enabled = false;
    state.set_endpoint_catalog(&[profile.clone()]);
    assert_eq!(
        state.endpoint_status(&remote),
        Some(ClientEndpointStatus::Disabled)
    );
    assert!(!state.endpoint_has_snapshot(&remote));
    profile.enabled = true;
    state.set_endpoint_catalog(&[profile]);
    assert_eq!(
        state.endpoint_status(&remote),
        Some(ClientEndpointStatus::Connecting)
    );
    assert!(!state.endpoint_has_snapshot(&remote));
}

#[test]
fn live_catalog_active_removal_does_not_retain_remote_projection_or_input() {
    let (mut state, remote) = state_with_remote();
    assert!(state.activate_endpoint_projection(&remote));
    state.set_pane_surface(surface());
    state.mode = ClientShellMode::Prefix;
    state.overlay = Some(ClientShellOverlay::Onboarding);
    state.select_unavailable_local();
    state.retire_endpoint(&remote);
    state.set_endpoint_catalog(&[]);
    assert!(state.endpoint_is_active(&ClientEndpointId::Local));
    assert!(state.snapshot.is_none());
    assert!(state.pane_surface.is_none());
    assert!(state.pending_pane_surface.is_none());
    assert!(state.overlay.is_none());
    assert_eq!(state.mode, ClientShellMode::Terminal);
    assert!(state.endpoint_has_snapshot(&ClientEndpointId::Local));
    let frame = state.compose(100, 30).unwrap();
    let buffer = frame.to_ratatui_buffer().unwrap();
    let text = buffer
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(!text.contains("remote-workspace"));
}

#[test]
fn machine_navigation_does_not_require_a_local_snapshot_or_surface() {
    for (cols, rows) in [(100, 28), (36, 18)] {
        let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
        let profile = remote_profile();
        let remote = ClientEndpointId::Ssh(profile.id.clone());
        state.set_endpoint_catalog(&[profile]);
        state.set_endpoint_status(&ClientEndpointId::Local, ClientEndpointStatus::Reconnecting);
        state.set_endpoint_status(&remote, ClientEndpointStatus::Online);
        let mut remote_snapshot = snapshot();
        remote_snapshot
            .agents
            .push(agent("remote", crate::api::schema::AgentStatus::Idle, 0));
        state.set_endpoint_snapshot(&remote, Box::new(remote_snapshot));
        assert!(state.snapshot.is_none());
        assert!(state.pane_surface.is_none());
        let frame = state
            .compose(cols, rows)
            .expect("connection chrome without Local");
        let buffer = frame.to_ratatui_buffer().unwrap();
        let text = buffer
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(!text.contains(" machines"), "no machines section");
        // The remote chat's space is reachable on its own, without Local.
        let hit = state
            .hits
            .workspaces
            .iter()
            .find(|hit| hit.endpoint_id == remote)
            .expect("remote space row")
            .rect;
        let mut outcome = ClientShellInput::default();
        for kind in [
            MouseEventKind::Down(MouseButton::Left),
            MouseEventKind::Up(MouseButton::Left),
        ] {
            state.handle_mouse(
                crossterm::event::MouseEvent {
                    kind,
                    column: hit.x + 5,
                    row: hit.y,
                    modifiers: KeyModifiers::NONE,
                },
                &mut outcome,
            );
        }
        assert!(
            matches!(outcome.actions.as_slice(), [ClientShellAction::ActivateEndpoint { endpoint_id, .. }] if endpoint_id == &remote),
            "{:?}",
            outcome.actions
        );
        assert!(
            state.snapshot.is_none(),
            "selection is committed only by coherent activation"
        );
    }
}

#[test]
fn sidebar_renders_local_and_saved_ssh_endpoints_with_status() {
    let (mut state, remote_id) = state_with_remote();
    let frame = state.compose(100, 28).expect("combined endpoint frame");
    let buffer = frame.to_ratatui_buffer().expect("frame should reconstruct");
    let line = |rect: Rect| {
        (rect.x..rect.right())
            .map(|x| buffer[(x, rect.y)].symbol())
            .collect::<String>()
    };
    let text = (0..frame.height)
        .map(|y| line(Rect::new(0, y, frame.width, 1)))
        .collect::<Vec<_>>()
        .join("\n");
    // One spaces list: no machines section and no machine rows.
    assert!(!text.contains(" machines"), "frame: {text}");
    assert!(text.contains(" spaces"), "frame: {text}");
    // The only machine hits left are one-line badges on rows.
    assert!(state.hits.machines.iter().all(|hit| hit.status_badge.height == 1));
    let local = state
        .hits
        .workspaces
        .iter()
        .find(|hit| hit.endpoint_id.is_local())
        .expect("local space row")
        .rect;
    let remote = state
        .hits
        .workspaces
        .iter()
        .find(|hit| hit.endpoint_id == remote_id)
        .expect("remote space row")
        .rect;
    assert!(!line(local).contains("Build"), "local rows carry no badge");
    assert!(line(remote).contains("remote-work"), "{}", line(remote));
    assert!(
        line(remote).trim_end().ends_with("◇ Build"),
        "{}",
        line(remote)
    );
    let badge = state
        .hits
        .machines
        .iter()
        .find(|hit| hit.endpoint_id == remote_id)
        .expect("remote badge")
        .status_badge;
    assert_eq!(badge.y, remote.y);
    assert_eq!(buffer[(badge.x, badge.y)].fg, state.config.palette.overlay0);

    // A machine that drops dims its badge; nothing else marks it.
    state.set_endpoint_status(&remote_id, ClientEndpointStatus::Reconnecting);
    let frame = state.compose(100, 28).expect("reconnecting frame");
    let buffer = frame.to_ratatui_buffer().expect("frame should reconstruct");
    let remote = state
        .hits
        .workspaces
        .iter()
        .find(|hit| hit.endpoint_id == remote_id)
        .expect("remote space row while reconnecting")
        .rect;
    let row = (remote.x..remote.right())
        .map(|x| buffer[(x, remote.y)].symbol())
        .collect::<String>();
    assert!(row.trim_end().ends_with("◌ Build"), "{row}");

    state.set_endpoint_status(&remote_id, ClientEndpointStatus::Online);
    state.sidebar_collapsed = true;
    let frame = state.compose(100, 28).expect("collapsed endpoint frame");
    let buffer = frame.to_ratatui_buffer().expect("frame should reconstruct");
    let rows = state
        .hits
        .workspaces
        .iter()
        .map(|hit| {
            (
                hit.endpoint_id.clone(),
                (hit.rect.x..hit.rect.right())
                    .map(|x| buffer[(x, hit.rect.y)].symbol())
                    .collect::<String>(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), 2, "{rows:?}");
    assert!(
        rows[0].0.is_local() && rows[0].1.starts_with(" 1"),
        "{rows:?}"
    );
    assert!(
        rows[1].0 == remote_id && rows[1].1.starts_with(" ◇"),
        "{rows:?}"
    );
}

#[test]
fn saved_machine_preserves_endpoint_scoped_worktree_collapses() {
    fn add_worktree_group(snapshot: &mut ClientShellSnapshot, parent_id: &str, child_id: &str) {
        snapshot.workspaces[0].workspace_id = parent_id.into();
        snapshot.workspaces[0].worktree = Some(ClientShellWorktree {
            key: "repo".into(),
            label: "repo".into(),
            is_linked_worktree: false,
        });
        let mut child = snapshot.workspaces[0].clone();
        child.workspace_id = child_id.into();
        child.active_tab_id = format!("tab_{child_id}");
        child.number = 2;
        child.label = "feature".into();
        child.focused = false;
        child.agent_status = AgentStatus::Blocked;
        child.worktree = Some(ClientShellWorktree {
            key: "repo".into(),
            label: "repo".into(),
            is_linked_worktree: true,
        });
        snapshot.workspaces.push(child);
    }

    let (mut state, remote_id) = state_with_remote();
    let mut local = snapshot();
    add_worktree_group(&mut local, "ws_1", "ws_2");
    state.set_snapshot(Box::new(local));
    let mut remote = snapshot();
    remote.boot_id = "remote-boot".into();
    remote.focused_workspace_id = Some("remote_ws_1".into());
    add_worktree_group(&mut remote, "remote_ws_1", "remote_ws_2");
    // Distinct labels and a chat in each, so the remote spaces keep rows of their own.
    for workspace in &mut remote.workspaces {
        workspace.label = format!("remote {}", workspace.label);
        let mut chat = agent("remote", AgentStatus::Idle, 0);
        chat.workspace_id = workspace.workspace_id.clone();
        chat.pane_id = format!("pane_{}", workspace.workspace_id);
        remote.agents.push(chat);
    }
    state.set_endpoint_snapshot(&remote_id, Box::new(remote));

    state.open_workspace_context_menu("ws_1".into(), 0, 0);
    let toggle_index = match state.overlay.as_ref() {
        Some(ClientShellOverlay::ContextMenu(menu)) => menu
            .items()
            .iter()
            .position(|item| item.action == ClientContextMenuAction::ToggleGroup)
            .expect("collapse menu item"),
        _ => panic!("workspace context menu"),
    };
    state.activate_context_menu_item(toggle_index, &mut ClientShellInput::default());

    let frame = state
        .compose(100, 28)
        .expect("collapsed local worktree group");
    assert!(!state
        .hits
        .workspaces
        .iter()
        .any(|hit| { hit.endpoint_id == ClientEndpointId::Local && hit.workspace_id == "ws_2" }));
    assert!(state
        .hits
        .workspaces
        .iter()
        .any(|hit| hit.endpoint_id == remote_id && hit.workspace_id == "remote_ws_2"));
    let local_parent = state
        .hits
        .workspaces
        .iter()
        .find(|hit| hit.endpoint_id == ClientEndpointId::Local && hit.workspace_id == "ws_1")
        .expect("local parent workspace");
    let (local_toggle, key) = local_parent
        .group_toggle
        .as_ref()
        .expect("local worktree group marker");
    assert_eq!(key, "repo");
    let buffer = frame.to_ratatui_buffer().expect("frame should reconstruct");
    assert_eq!(buffer[(local_toggle.x, local_toggle.y)].symbol(), "▸");
    assert!((local_parent.rect.x..local_parent.rect.right())
        .any(|x| buffer[(x, local_parent.rect.y)].fg == state.config.palette.red));

    assert!(state.activate_endpoint_projection(&remote_id));
    let mut remote_surface = surface();
    remote_surface.boot_id = "remote-boot".into();
    state.set_pane_surface(remote_surface);
    state
        .compose(100, 28)
        .expect("active remote worktree group");
    let mut switch = ClientShellInput::default();
    state.record_binding(
        crate::input::KeybindMatch::Action(crate::input::KeybindAction::SwitchWorkspace(1)),
        &mut switch,
    );
    assert!(matches!(
        &switch.actions[..],
        [ClientShellAction::Endpoint { request, .. }]
            if matches!(
                &request.method,
                crate::api::schema::Method::WorkspaceFocus(target)
                    if target.workspace_id == "remote_ws_2"
            )
    ));
    let remote_parent = state
        .hits
        .workspaces
        .iter()
        .find(|hit| hit.endpoint_id == remote_id && hit.workspace_id == "remote_ws_1")
        .expect("visible remote worktree parent")
        .rect;
    let remote_child = state
        .hits
        .workspaces
        .iter()
        .find(|hit| hit.endpoint_id == remote_id && hit.workspace_id == "remote_ws_2")
        .expect("visible remote worktree child")
        .rect;
    state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: remote_parent.x + 3,
        row: remote_parent.y,
        modifiers: KeyModifiers::empty(),
    })]);
    state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
        kind: MouseEventKind::Drag(MouseButton::Left),
        column: remote_child.x + 3,
        row: remote_child.bottom(),
        modifiers: KeyModifiers::empty(),
    })]);
    assert!(matches!(
        state.chrome_drag,
        Some(ClientChromeDrag::Workspace {
            target: Some(_),
            ..
        })
    ));
    state.chrome_drag = None;
    state.workspace_press = None;

    let remote_toggle = state
        .hits
        .workspaces
        .iter()
        .find(|hit| hit.endpoint_id == remote_id && hit.workspace_id == "remote_ws_1")
        .and_then(|hit| hit.group_toggle.as_ref())
        .expect("remote worktree group marker")
        .0;
    state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: remote_toggle.x,
        row: remote_toggle.y,
        modifiers: KeyModifiers::empty(),
    })]);
    state.compose(100, 28).expect("both groups collapsed");
    assert!(!state
        .hits
        .workspaces
        .iter()
        .any(|hit| { hit.endpoint_id == remote_id && hit.workspace_id == "remote_ws_2" }));

    let local_toggle = state
        .hits
        .workspaces
        .iter()
        .find(|hit| hit.endpoint_id == ClientEndpointId::Local && hit.workspace_id == "ws_1")
        .and_then(|hit| hit.group_toggle.as_ref())
        .expect("collapsed local group marker")
        .0;
    state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: local_toggle.x,
        row: local_toggle.y,
        modifiers: KeyModifiers::empty(),
    })]);
    state.compose(100, 28).expect("only remote group collapsed");
    assert!(state
        .hits
        .workspaces
        .iter()
        .any(|hit| { hit.endpoint_id == ClientEndpointId::Local && hit.workspace_id == "ws_2" }));
    assert!(!state
        .hits
        .workspaces
        .iter()
        .any(|hit| { hit.endpoint_id == remote_id && hit.workspace_id == "remote_ws_2" }));
}

#[test]
fn expanded_machine_sidebar_reveals_newly_focused_workspace() {
    let (mut state, remote_id) = state_with_remote();
    let mut initial = snapshot();
    let template = initial.workspaces[0].clone();
    initial.workspaces = (1..=12)
        .map(|number| ClientShellWorkspace {
            sort_rank: 0,
            parked: false,
            visible_in_profile: true,
            workspace_id: format!("ws_{number}"),
            number,
            label: format!("space-{number}"),
            focused: number == 1,
            ..template.clone()
        })
        .collect();
    // Reuse workspace IDs across machines so revealing must be endpoint-scoped.
    let mut remote = initial.clone();
    remote.boot_id = "remote-boot".into();
    remote.workspaces.push(ClientShellWorkspace {
        sort_rank: 0,
        parked: false,
        workspace_id: "ws_13".into(),
        number: 13,
        focused: false,
        ..template.clone()
    });
    state.set_endpoint_snapshot(&remote_id, Box::new(remote));
    state.set_snapshot(Box::new(initial));
    state.compose(106, 20).expect("full machines sidebar");
    assert!(state.hits.workspace_max_scroll > 0);

    let mut update = state.snapshot.as_deref().expect("snapshot").clone();
    update.revision = 2;
    update.workspaces.push(ClientShellWorkspace {
        sort_rank: 0,
        parked: false,
        workspace_id: "ws_13".into(),
        number: 13,
        label: "new-space".into(),
        ..template
    });
    update.focused_workspace_id = Some("ws_13".into());
    for workspace in &mut update.workspaces {
        workspace.focused = workspace.workspace_id == "ws_13";
    }
    state.set_snapshot(Box::new(update));
    let mut updated_surface = surface();
    updated_surface.projection_revision = 2;
    state.set_pane_surface(updated_surface);
    state.compose(106, 2).expect("zero-height workspace body");
    assert!(state.reveal_focused_workspace);
    state.compose(106, 20).expect("new workspace revealed");
    assert!(state
        .hits
        .workspaces
        .iter()
        .any(|hit| { hit.endpoint_id == ClientEndpointId::Local && hit.workspace_id == "ws_13" }));

    state.workspace_scroll = 0;
    state.compose(106, 20).expect("manual scroll");
    assert_eq!(state.workspace_scroll, 0);
    assert!(!state
        .hits
        .workspaces
        .iter()
        .any(|hit| { hit.endpoint_id == ClientEndpointId::Local && hit.workspace_id == "ws_13" }));
    let unchanged = state.snapshot.as_deref().expect("snapshot").clone();
    state.set_snapshot(Box::new(unchanged));
    state
        .compose(106, 20)
        .expect("unchanged focus preserves scroll");
    assert_eq!(state.workspace_scroll, 0);
}

#[test]
fn expanded_spaces_list_applies_space_row_gap_across_machines() {
    let (mut state, remote_id) = state_with_remote();
    state.config.spaces.row_gap = 1;

    let add_second_workspace = |snapshot: &mut ClientShellSnapshot| {
        let mut workspace = snapshot.workspaces[0].clone();
        workspace.workspace_id = "ws_2".into();
        workspace.active_tab_id = "tab_2".into();
        workspace.number = 2;
        workspace.label = "second-workspace".into();
        workspace.focused = false;
        snapshot.workspaces.push(workspace);
    };
    let mut local = snapshot();
    add_second_workspace(&mut local);
    state.set_snapshot(Box::new(local));
    let mut remote = snapshot();
    remote.boot_id = "remote-boot".into();
    remote.workspaces[0].worktree = Some(ClientShellWorktree {
        key: "repo".into(),
        label: "repo".into(),
        is_linked_worktree: false,
    });
    add_second_workspace(&mut remote);
    remote.workspaces[1].worktree = Some(ClientShellWorktree {
        key: "repo".into(),
        label: "repo".into(),
        is_linked_worktree: true,
    });
    let mut third = remote.workspaces[1].clone();
    third.workspace_id = "ws_3".into();
    third.number = 3;
    third.label = "third-workspace".into();
    third.worktree = None;
    remote.workspaces.push(third);
    // Labels of their own and a chat in each, so no remote space joins a local one.
    for (index, workspace) in remote.workspaces.iter_mut().enumerate() {
        workspace.label = format!("remote-{index}");
        let mut chat = agent("remote", crate::api::schema::AgentStatus::Idle, 0);
        chat.workspace_id = workspace.workspace_id.clone();
        chat.pane_id = format!("pane_{index}");
        remote.agents.push(chat);
    }
    state.set_endpoint_snapshot(&remote_id, Box::new(remote));

    state.compose(100, 40).expect("combined endpoint frame");
    let local_workspaces = state
        .hits
        .workspaces
        .iter()
        .filter(|hit| hit.endpoint_id.is_local())
        .collect::<Vec<_>>();
    assert_eq!(local_workspaces.len(), 2);
    assert_eq!(
        local_workspaces[1].rect.y,
        local_workspaces[0].rect.bottom() + 1
    );

    // One list: the remote spaces follow the local ones with the same gaps, and a
    // worktree child stays tight under its parent.
    let remote_workspaces = state
        .hits
        .workspaces
        .iter()
        .filter(|hit| hit.endpoint_id == remote_id)
        .collect::<Vec<_>>();
    assert_eq!(remote_workspaces.len(), 3);
    assert_eq!(
        remote_workspaces[0].rect.y,
        local_workspaces[1].rect.bottom() + 1
    );
    assert_eq!(
        remote_workspaces[1].rect.y,
        remote_workspaces[0].rect.bottom()
    );
    assert_eq!(
        remote_workspaces[2].rect.y,
        remote_workspaces[1].rect.bottom() + 1
    );

    state.workspace_scroll = usize::MAX;
    state.compose(100, 18).expect("scrolled endpoint frame");
    let metrics = state
        .hits
        .workspace_scroll_metrics
        .expect("workspace scroll metrics");
    assert!(metrics.max_offset_from_bottom > 0);
    assert_eq!(metrics.offset_from_bottom, 0);
    assert_eq!(state.workspace_scroll, metrics.max_offset_from_bottom);
    let visible_remote = state
        .hits
        .workspaces
        .iter()
        .filter(|hit| hit.endpoint_id == remote_id)
        .collect::<Vec<_>>();
    assert_eq!(visible_remote.len(), 3);
    let gap_y = visible_remote[1].rect.bottom();
    assert_eq!(visible_remote[2].rect.y, gap_y + 1);
    assert!(visible_remote[2].rect.bottom() <= state.hits.workspace_body.bottom());
    assert!(state
        .hits
        .workspaces
        .iter()
        .all(|hit| gap_y < hit.rect.top() || gap_y >= hit.rect.bottom()));
}

#[test]
fn active_workspace_is_the_only_highlight_on_another_machine() {
    let (mut state, endpoint_id) = state_with_remote();
    assert!(state.activate_endpoint_projection(&endpoint_id));
    let mut remote_surface = surface();
    remote_surface.boot_id = "remote-boot".into();
    state.set_pane_surface(remote_surface);

    let frame = state.compose(100, 28).expect("combined endpoint frame");
    let local = state
        .hits
        .workspaces
        .iter()
        .find(|hit| hit.endpoint_id.is_local())
        .expect("local workspace hit")
        .rect;
    let workspace = state
        .hits
        .workspaces
        .iter()
        .find(|hit| hit.endpoint_id == endpoint_id)
        .expect("remote workspace hit")
        .rect;
    let buffer = frame.to_ratatui_buffer().expect("frame should reconstruct");
    assert_ne!(
        buffer[(local.x + 2, local.y)].bg,
        state.config.palette.active_row_bg
    );
    assert_eq!(
        buffer[(workspace.x + 2, workspace.y)].bg,
        state.config.palette.active_row_bg
    );
}

#[test]
fn aggregate_agents_badge_remote_rows_and_keep_status_colors() {
    use crate::api::schema::AgentStatus;
    use crate::config::{AgentSidebarToken, StatusIndicatorStyle};

    let mut config = Config::default();
    config.ui.status_indicators = StatusIndicatorStyle::Symbols;
    config.ui.sidebar.agents.rows = vec![vec![
        AgentSidebarToken::StateIcon,
        AgentSidebarToken::Machine,
        AgentSidebarToken::Agent,
    ]];
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&config));
    let profile = remote_profile();
    let endpoint_id = ClientEndpointId::Ssh(profile.id.clone());
    state.set_endpoint_catalog(&[profile]);
    state.set_endpoint_status(&endpoint_id, ClientEndpointStatus::Online);

    let mut local = snapshot();
    local.agents = vec![agent("local agent", AgentStatus::Idle, 1)];
    state.set_snapshot(Box::new(local));
    state.set_pane_surface(surface());
    let mut remote = snapshot();
    remote.boot_id = "remote-boot".into();
    remote.agents = vec![agent("remote", AgentStatus::Blocked, 1)];
    state.set_endpoint_snapshot(&endpoint_id, Box::new(remote));

    let frame = state.compose(100, 28).expect("combined endpoint frame");
    let text = frame
        .cells
        .chunks(frame.width as usize)
        .map(|row| {
            row.iter()
                .map(|cell| cell.symbol.as_str())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    // The machine is a badge on the remote row, not a token on either row.
    assert!(text.contains("○ local agent "), "frame: {text}");
    assert!(text.contains("× remote "), "frame: {text}");
    assert!(!text.contains("Local · "), "frame: {text}");
    assert!(
        text.lines()
            .any(|line| line.contains("× remote ") && line.contains("◇ Build")),
        "frame: {text}"
    );
    assert!(text.contains("grouped"), "frame: {text}");
    let toggle = state.hits.agent_sort_toggle;
    assert!(!toggle.is_empty());
    let click = state.handle_raw_events(vec![RawInputEvent::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: toggle.x,
        row: toggle.y,
        modifiers: KeyModifiers::empty(),
    })]);
    assert_eq!(
        state.config.agent_panel_sort,
        crate::config::AgentPanelSortConfig::Spaces
    );
    assert!(matches!(
        state.overlay,
        Some(ClientShellOverlay::ContextMenu(_))
    ));
    assert!(click.actions.is_empty());

    let buffer = frame
        .to_ratatui_buffer()
        .expect("aggregate frame should reconstruct");
    assert!(buffer
        .content()
        .iter()
        .any(|cell| cell.symbol() == "×" && cell.fg == state.config.palette.red));
}

#[test]
fn current_workspace_agent_view_excludes_same_workspace_id_on_other_machine() {
    use crate::api::schema::AgentStatus;
    use crate::config::AgentSidebarToken;

    let mut config = Config::default();
    config.ui.sidebar.agents.rows =
        vec![vec![AgentSidebarToken::Machine, AgentSidebarToken::Agent]];
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&config));
    let profile = remote_profile();
    let endpoint_id = ClientEndpointId::Ssh(profile.id.clone());
    state.set_endpoint_catalog(&[profile]);
    state.set_endpoint_status(&endpoint_id, ClientEndpointStatus::Online);

    let mut local = snapshot();
    local.agent_view_label = Some("current space".into());
    local.agent_order = vec!["pane_1".into()];
    local.agents = vec![agent("local agent", AgentStatus::Idle, 1)];
    state.set_snapshot(Box::new(local));
    state.set_pane_surface(surface());

    let mut remote = snapshot();
    remote.boot_id = "remote-boot".into();
    remote.agent_view_label = Some("current space".into());
    remote.agent_order = vec!["pane_1".into()];
    remote.agents = vec![agent("remote agent", AgentStatus::Idle, 1)];
    state.set_endpoint_snapshot(&endpoint_id, Box::new(remote));
    let view = current_workspace_view();
    state.set_test_endpoint_agent_view(&ClientEndpointId::Local, Some(view.clone()));
    state.set_test_endpoint_agent_view(&endpoint_id, Some(view));

    let frame = state.compose(100, 28).expect("combined endpoint frame");
    let text = frame
        .cells
        .chunks(frame.width as usize)
        .map(|row| {
            row.iter()
                .map(|cell| cell.symbol.as_str())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("local agent"), "frame: {text}");
    assert!(!text.contains("remote agent"), "frame: {text}");

    assert!(state.activate_endpoint_projection(&endpoint_id));
    let mut remote_surface = surface();
    remote_surface.boot_id = "remote-boot".into();
    state.set_pane_surface(remote_surface);
    let frame = state.compose(100, 28).expect("remote endpoint frame");
    let text = frame
        .cells
        .chunks(frame.width as usize)
        .map(|row| {
            row.iter()
                .map(|cell| cell.symbol.as_str())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!text.contains("local agent"), "frame: {text}");
    assert!(text.contains("remote agent"), "frame: {text}");
}

#[test]
fn current_workspace_or_blocked_keeps_foreign_attention_only() {
    use crate::api::schema::{
        AgentStatus, AgentViewBuiltinField, AgentViewField, AgentViewFilter, AgentViewValue,
    };
    use crate::config::AgentSidebarToken;

    let mut config = Config::default();
    config.ui.sidebar.agents.rows =
        vec![vec![AgentSidebarToken::Machine, AgentSidebarToken::Agent]];
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&config));
    let profile = remote_profile();
    let endpoint_id = ClientEndpointId::Ssh(profile.id.clone());
    state.set_endpoint_catalog(&[profile]);
    state.set_endpoint_status(&endpoint_id, ClientEndpointStatus::Online);

    let mut local = snapshot();
    local.agent_view_label = Some("focus".into());
    local.agents = vec![agent("local agent", AgentStatus::Idle, 1)];
    state.set_snapshot(Box::new(local));
    state.set_pane_surface(surface());

    let mut remote = snapshot();
    remote.boot_id = "remote-boot".into();
    remote.agents = vec![
        agent("far idle", AgentStatus::Idle, 1),
        ClientShellAgent {
            pane_id: "pane_2".into(),
            name: Some("far blocked".into()),
            agent_status: AgentStatus::Blocked,
            focused: false,
            ..agent("far blocked", AgentStatus::Blocked, 2)
        },
    ];
    remote.panes.push(ClientShellPane {
        pane_id: "pane_2".into(),
        focused: false,
        ..remote.panes[0].clone()
    });
    state.set_endpoint_snapshot(&endpoint_id, Box::new(remote));

    let mut view = current_workspace_view();
    view.label = Some("focus".into());
    view.filter = Some(AgentViewFilter::Any {
        filters: vec![
            view.filter.take().expect("current workspace filter"),
            AgentViewFilter::Eq {
                field: AgentViewField::Builtin(AgentViewBuiltinField::Status),
                value: AgentViewValue::String("blocked".into()),
            },
        ],
    });
    state.set_test_endpoint_agent_view(&ClientEndpointId::Local, Some(view));

    let frame = state.compose(100, 28).expect("combined endpoint frame");
    let text = frame
        .cells
        .chunks(frame.width as usize)
        .map(|row| {
            row.iter()
                .map(|cell| cell.symbol.as_str())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("local agent"), "frame: {text}");
    assert!(!text.contains("far idle"), "frame: {text}");
    assert!(text.contains("far blocked"), "frame: {text}");
}

#[test]
fn selected_default_view_ignores_inactive_endpoint_projection() {
    use crate::api::schema::{
        AgentStatus, AgentViewBuiltinField, AgentViewField, AgentViewFilter, AgentViewValue,
    };
    use crate::config::AgentSidebarToken;

    let mut config = Config::default();
    config.ui.sidebar.agents.rows =
        vec![vec![AgentSidebarToken::Machine, AgentSidebarToken::Agent]];
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&config));
    let profile = remote_profile();
    let endpoint_id = ClientEndpointId::Ssh(profile.id.clone());
    state.set_endpoint_catalog(&[profile]);
    state.set_endpoint_status(&endpoint_id, ClientEndpointStatus::Online);

    let mut local = snapshot();
    local.agents = vec![agent("local agent", AgentStatus::Idle, 1)];
    state.set_snapshot(Box::new(local));
    state.set_pane_surface(surface());
    let mut remote = snapshot();
    remote.boot_id = "remote-boot".into();
    remote.agent_view_label = Some("blocked".into());
    remote.agent_order.clear();
    remote.agents = vec![agent("remote agent", AgentStatus::Idle, 1)];
    state.set_endpoint_snapshot(&endpoint_id, Box::new(remote));
    state.set_test_endpoint_agent_view(&ClientEndpointId::Local, None);
    state.set_test_endpoint_agent_view(
        &endpoint_id,
        Some(crate::api::schema::AgentViewSetParams {
            source: "remote.views".into(),
            label: Some("blocked".into()),
            filter: Some(AgentViewFilter::Eq {
                field: AgentViewField::Builtin(AgentViewBuiltinField::Status),
                value: AgentViewValue::String("blocked".into()),
            }),
            sort: Vec::new(),
        }),
    );

    let frame = state.compose(100, 28).expect("combined endpoint frame");
    let text = frame
        .cells
        .chunks(frame.width as usize)
        .map(|row| {
            row.iter()
                .map(|cell| cell.symbol.as_str())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("local agent"), "frame: {text}");
    assert!(text.contains("remote agent"), "frame: {text}");
    assert!(text.contains("grouped"), "frame: {text}");
}

#[test]
fn newer_snapshot_does_not_reuse_stale_agent_view_projection() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    let mut local = snapshot();
    local.agent_view_label = Some("current space".into());
    state.set_snapshot(Box::new(local.clone()));
    state.set_test_endpoint_agent_view(&ClientEndpointId::Local, Some(current_workspace_view()));
    let endpoint = state
        .endpoints
        .iter()
        .find(|endpoint| endpoint.endpoint_id.is_local())
        .expect("local endpoint");
    assert!(matches!(
        ClientShellState::endpoint_agent_view(endpoint),
        Some(Ok(Some(_)))
    ));

    state.set_test_endpoint_agent_view_projection(
        &ClientEndpointId::Local,
        "foreign-boot",
        99,
        None,
    );
    let endpoint = state
        .endpoints
        .iter()
        .find(|endpoint| endpoint.endpoint_id.is_local())
        .expect("local endpoint");
    assert!(matches!(
        ClientShellState::endpoint_agent_view(endpoint),
        Some(Ok(Some(_)))
    ));

    local.revision += 1;
    state.set_snapshot(Box::new(local));
    let endpoint = state
        .endpoints
        .iter()
        .find(|endpoint| endpoint.endpoint_id.is_local())
        .expect("local endpoint");
    assert!(ClientShellState::endpoint_agent_view(endpoint).is_none());
}

#[test]
fn legacy_custom_views_keep_v1_per_endpoint_projection() {
    use crate::api::schema::AgentStatus;
    use crate::config::AgentSidebarToken;

    let mut config = Config::default();
    config.ui.sidebar.agents.rows =
        vec![vec![AgentSidebarToken::Machine, AgentSidebarToken::Agent]];
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&config));
    let profile = remote_profile();
    let endpoint_id = ClientEndpointId::Ssh(profile.id.clone());
    state.set_endpoint_catalog(&[profile]);
    state.set_endpoint_status(&endpoint_id, ClientEndpointStatus::Online);

    let mut local = snapshot();
    local.agent_view_label = Some("current space".into());
    local.agent_order = vec!["pane_1".into()];
    local.agents = vec![agent("local agent", AgentStatus::Idle, 1)];
    state.set_snapshot(Box::new(local));
    state.set_pane_surface(surface());
    let mut remote = snapshot();
    remote.boot_id = "remote-boot".into();
    remote.agent_view_label = Some("current space".into());
    remote.agent_order = vec!["pane_1".into()];
    remote.agents = vec![agent("remote agent", AgentStatus::Idle, 1)];
    state.set_endpoint_snapshot(&endpoint_id, Box::new(remote));

    let frame = state.compose(100, 28).expect("legacy combined frame");
    let text = frame
        .cells
        .chunks(frame.width as usize)
        .map(|row| {
            row.iter()
                .map(|cell| cell.symbol.as_str())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("local agent"), "frame: {text}");
    assert!(text.contains("remote agent"), "frame: {text}");
}

#[test]
fn selected_custom_sort_orders_rendering_and_indexed_navigation() {
    use crate::api::schema::{
        AgentStatus, AgentViewBuiltinSortField, AgentViewSort, AgentViewSortField,
        AgentViewSortOrder,
    };
    use crate::config::AgentSidebarToken;

    let mut config = Config::default();
    config.ui.agent_panel_sort = crate::config::AgentPanelSortConfig::Priority;
    config.ui.sidebar.agents.rows =
        vec![vec![AgentSidebarToken::Machine, AgentSidebarToken::Agent]];
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&config));
    let profile = remote_profile();
    let endpoint_id = ClientEndpointId::Ssh(profile.id.clone());
    state.set_endpoint_catalog(&[profile]);
    state.set_endpoint_status(&endpoint_id, ClientEndpointStatus::Online);

    let mut local = snapshot();
    local.agent_view_label = Some("recent".into());
    local.agents = vec![agent("local blocked", AgentStatus::Blocked, 1)];
    state.set_snapshot(Box::new(local));
    state.set_pane_surface(surface());
    let mut remote = snapshot();
    remote.boot_id = "remote-boot".into();
    remote.agents = vec![agent("remote idle", AgentStatus::Idle, 9)];
    state.set_endpoint_snapshot(&endpoint_id, Box::new(remote));

    let mut view = current_workspace_view();
    view.label = Some("recent".into());
    view.filter = None;
    view.sort = vec![AgentViewSort {
        field: AgentViewSortField::Builtin(AgentViewBuiltinSortField::StateChangeSeq),
        order: AgentViewSortOrder::Desc,
    }];
    state.set_test_endpoint_agent_view(&ClientEndpointId::Local, Some(view));

    let frame = state.compose(100, 28).expect("custom sorted frame");
    let text = frame
        .cells
        .chunks(frame.width as usize)
        .map(|row| {
            row.iter()
                .map(|cell| cell.symbol.as_str())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        text.find("remote idle").expect("remote row")
            < text.find("local blocked").expect("local row"),
        "frame: {text}"
    );

    let mut outcome = ClientShellInput::default();
    assert!(
        state.handle_endpoint_navigation(crate::input::KeybindAction::FocusAgent(0), &mut outcome,)
    );
    assert!(matches!(
        outcome.actions.as_slice(),
        [ClientShellAction::ActivateEndpoint {
            endpoint_id: selected,
            target: Some(ClientEndpointFocusTarget::Pane(pane_id)),
        }] if selected == &endpoint_id && pane_id == "pane_1"
    ));
}

#[test]
fn selected_position_sort_uses_public_tab_and_pane_numbers() {
    use crate::api::schema::{
        AgentStatus, AgentViewBuiltinSortField, AgentViewSort, AgentViewSortField,
        AgentViewSortOrder,
    };

    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    let mut selected = snapshot();
    selected.agent_view_label = Some("positions".into());

    let mut tab_nine = selected.tabs[0].clone();
    tab_nine.tab_id = "ws_1:t9".into();
    tab_nine.number = 9;
    let mut tab_two = tab_nine.clone();
    tab_two.tab_id = "ws_1:t2".into();
    tab_two.number = 2;
    selected.tabs = vec![tab_nine, tab_two];

    let mut pane_tab_nine = selected.panes[0].clone();
    pane_tab_nine.tab_id = "ws_1:t9".into();
    pane_tab_nine.pane_id = "ws_1:p1".into();
    let mut pane_nine = pane_tab_nine.clone();
    pane_nine.tab_id = "ws_1:t2".into();
    pane_nine.pane_id = "ws_1:p9".into();
    let mut pane_two = pane_nine.clone();
    pane_two.pane_id = "ws_1:p2".into();
    selected.panes = vec![pane_tab_nine, pane_nine, pane_two];

    let mut late_tab = agent("tab nine", AgentStatus::Idle, 1);
    late_tab.tab_id = "ws_1:t9".into();
    late_tab.pane_id = "ws_1:p1".into();
    let mut late_pane = agent("pane nine", AgentStatus::Idle, 1);
    late_pane.tab_id = "ws_1:t2".into();
    late_pane.pane_id = "ws_1:p9".into();
    let mut early_pane = agent("pane two", AgentStatus::Idle, 1);
    early_pane.tab_id = "ws_1:t2".into();
    early_pane.pane_id = "ws_1:p2".into();
    selected.agents = vec![late_tab, late_pane, early_pane];
    state.set_snapshot(Box::new(selected));

    let mut view = current_workspace_view();
    view.label = Some("positions".into());
    view.filter = None;
    view.sort = vec![
        AgentViewSort {
            field: AgentViewSortField::Builtin(AgentViewBuiltinSortField::TabOrder),
            order: AgentViewSortOrder::Asc,
        },
        AgentViewSort {
            field: AgentViewSortField::Builtin(AgentViewBuiltinSortField::PaneOrder),
            order: AgentViewSortOrder::Asc,
        },
    ];
    state.set_test_endpoint_agent_view(&ClientEndpointId::Local, Some(view));

    let names = aggregate_navigation::aggregate_agent_rows(
        &state.endpoints,
        &state.active_endpoint_id,
        crate::config::AgentPanelSortConfig::Priority,
    )
    .into_iter()
    .map(|row| row.agent.name.as_deref().expect("agent name"))
    .collect::<Vec<_>>();
    assert_eq!(names, ["pane two", "pane nine", "tab nine"]);
}

#[test]
fn aggregate_priority_uses_client_observed_recency_across_machines() {
    use crate::api::schema::AgentStatus;
    use crate::config::AgentSidebarToken;

    let mut config = Config::default();
    config.ui.agent_panel_sort = crate::config::AgentPanelSortConfig::Priority;
    config.ui.sidebar.agents.rows =
        vec![vec![AgentSidebarToken::Machine, AgentSidebarToken::Agent]];
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&config));
    let profile = remote_profile();
    let endpoint_id = ClientEndpointId::Ssh(profile.id.clone());
    state.set_endpoint_catalog(&[profile]);
    state.set_endpoint_status(&endpoint_id, ClientEndpointStatus::Online);

    let mut local = snapshot();
    local.agents = vec![agent("local agent", AgentStatus::Idle, 1)];
    state.set_snapshot(Box::new(local));
    state.set_pane_surface(surface());
    let mut remote = snapshot();
    remote.boot_id = "remote-boot".into();
    remote.agents = vec![agent("remote agent", AgentStatus::Idle, 1)];
    state.set_endpoint_snapshot(&endpoint_id, Box::new(remote.clone()));

    let mut local = snapshot();
    local.agents = vec![agent("local agent", AgentStatus::Idle, 2)];
    state.set_snapshot(Box::new(local));
    let frame_text = |state: &mut ClientShellState| {
        let frame = state.compose(100, 28).expect("combined endpoint frame");
        frame
            .cells
            .chunks(frame.width as usize)
            .map(|row| {
                row.iter()
                    .map(|cell| cell.symbol.as_str())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let text = frame_text(&mut state);
    assert!(
        text.find("local agent").expect("local agent")
            < text.find("remote agent").expect("remote agent")
    );

    remote.agents = vec![agent("remote agent", AgentStatus::Working, 2)];
    state.set_endpoint_snapshot(&endpoint_id, Box::new(remote.clone()));
    let text = frame_text(&mut state);
    assert!(
        text.find("remote agent").expect("remote agent")
            < text.find("local agent").expect("local agent")
    );

    remote.agents = vec![agent("remote agent", AgentStatus::Idle, 3)];
    state.set_endpoint_snapshot(&endpoint_id, Box::new(remote));
    let text = frame_text(&mut state);
    assert!(
        text.find("remote agent").expect("remote agent")
            < text.find("local agent").expect("local agent")
    );
    let mut outcome = ClientShellInput::default();
    assert!(
        state.handle_endpoint_navigation(crate::input::KeybindAction::FocusAgent(0), &mut outcome,)
    );
    assert!(matches!(
        outcome.actions.as_slice(),
        [ClientShellAction::ActivateEndpoint {
            endpoint_id: activated,
            target: Some(ClientEndpointFocusTarget::Pane(pane_id)),
        }] if activated == &endpoint_id && pane_id == "pane_1"
    ));
}

#[test]
fn unselected_endpoint_completion_projects_done_client_side() {
    use crate::api::schema::AgentStatus;

    let (mut state, endpoint_id) = state_with_remote();
    let mut remote = snapshot();
    remote.boot_id = "remote-boot".into();
    remote.agents = vec![agent("background agent", AgentStatus::Working, 2)];
    state.set_endpoint_snapshot(&endpoint_id, Box::new(remote.clone()));
    remote.revision = 2;
    remote.agents = vec![agent("background agent", AgentStatus::Idle, 3)];

    state.set_endpoint_snapshot(&endpoint_id, Box::new(remote));

    let status = state
        .endpoints
        .iter()
        .find(|endpoint| endpoint.endpoint_id == endpoint_id)
        .and_then(|endpoint| endpoint.snapshot.as_deref())
        .and_then(|snapshot| snapshot.agents.first())
        .map(|agent| agent.agent_status);
    assert_eq!(status, Some(AgentStatus::Done));
    assert_eq!(state.active_endpoint_id, ClientEndpointId::Local);
}

#[test]
fn remote_chat_joins_the_space_its_label_names_with_a_badge_and_routes_to_its_machine() {
    use crate::api::schema::AgentStatus;

    let (mut state, endpoint_id) = state_with_remote();
    let mut local = snapshot();
    local.workspaces[0].label = "rails".into();
    local.agents = vec![agent("local chat", AgentStatus::Idle, 1)];
    state.set_snapshot(Box::new(local));
    let mut remote = snapshot();
    remote.boot_id = "remote-boot".into();
    // The same space on the other machine, spelled in another case.
    remote.workspaces[0].label = "Rails".into();
    remote.workspaces[0].agent_status = AgentStatus::Working;
    remote.agents = vec![agent("remote chat", AgentStatus::Working, 1)];
    state.set_endpoint_snapshot(&endpoint_id, Box::new(remote));

    let frame = state.compose(100, 28).expect("combined endpoint frame");
    let rows = frame_rows(&frame);
    let width = state.hits.sidebar_divider.x as usize;
    let sidebar = rows
        .iter()
        .map(|row| row.chars().take(width).collect::<String>())
        .collect::<Vec<_>>();
    assert!(
        !sidebar.iter().any(|row| row.contains(" machines")),
        "{sidebar:#?}"
    );
    // One space row for both machines; the remote space adds no row of its own.
    assert_eq!(state.hits.workspaces.len(), 1, "{sidebar:#?}");
    assert!(state.hits.workspaces[0].endpoint_id.is_local());
    let space = state.hits.workspaces[0].rect;
    // The space rolls up the remote chat's state.
    let buffer = frame.to_ratatui_buffer().expect("frame buffer");
    assert!(
        (space.x..space.right()).any(|x| buffer[(x, space.y)].fg == state.config.palette.working)
    );
    // The remote chat names its space and carries its machine's badge; the
    // local chat has none.
    let chat_row = |endpoint: &ClientEndpointId| {
        state
            .hits
            .endpoint_agents
            .iter()
            .find(|(_, owner, _)| owner == endpoint)
            .map(|(rect, _, _)| *rect)
            .expect("chat row")
    };
    let local_rect = chat_row(&ClientEndpointId::Local);
    let rect = chat_row(&endpoint_id);
    // Inside the space, the remote chat follows the local one.
    assert!(rect.y > local_rect.y, "{sidebar:#?}");
    let lines =|rect: Rect| sidebar[usize::from(rect.y)..usize::from(rect.bottom())].join("\n");
    assert!(
        lines(local_rect).contains("local chat"),
        "{}",
        lines(local_rect)
    );
    assert!(!lines(local_rect).contains("◇"), "{}", lines(local_rect));
    assert!(lines(rect).contains("remote chat"), "{}", lines(rect));
    assert!(lines(rect).contains("Rails"), "{}", lines(rect));
    assert!(
        sidebar[usize::from(rect.y)]
            .trim_end()
            .ends_with("◇ Build ⚲"),
        "{}",
        sidebar[usize::from(rect.y)]
    );
    let outcome = state.handle_raw_events(vec![RawInputEvent::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: rect.x + 3,
        row: rect.y,
        modifiers: KeyModifiers::empty(),
    })]);
    assert!(
        matches!(
            outcome.actions.as_slice(),
            [ClientShellAction::ActivateEndpoint { endpoint_id: activated, target: Some(_) }]
                if activated == &endpoint_id
        ),
        "{:?}",
        outcome.actions
    );
    // Routing asks the runtime to switch; the projection stays Local until it does.
    assert_eq!(state.active_endpoint_id, ClientEndpointId::Local);
    assert_eq!(
        state
            .snapshot
            .as_deref()
            .map(|snapshot| snapshot.boot_id.as_str()),
        Some("boot-1")
    );
}

#[test]
fn clicking_local_can_cancel_a_remote_switch_while_local_is_still_displayed() {
    let (mut state, remote) = state_with_remote();
    state.compose(100, 28).unwrap();
    let mut pending = ClientShellInput::default();
    assert!(state.activate_endpoint(remote, &mut pending));
    assert_eq!(state.active_endpoint_id, ClientEndpointId::Local);
    let rect = state
        .hits
        .workspaces
        .iter()
        .find(|hit| hit.endpoint_id.is_local())
        .unwrap()
        .rect;
    let outcome = state.handle_raw_events(vec![
        RawInputEvent::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: rect.x + 5,
            row: rect.y,
            modifiers: KeyModifiers::empty(),
        }),
        RawInputEvent::Mouse(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: rect.x + 5,
            row: rect.y,
            modifiers: KeyModifiers::empty(),
        }),
    ]);
    assert!(matches!(
        outcome.actions.as_slice(),
        [ClientShellAction::ActivateEndpoint {
            endpoint_id: ClientEndpointId::Local,
            target: Some(_),
        }]
    ));
}

#[test]
fn reconnecting_local_selection_still_reaches_the_runtime() {
    let (mut state, _) = state_with_remote();
    state.mark_endpoint_disconnected(&ClientEndpointId::Local);
    let mut outcome = ClientShellInput::default();
    state.focus_or_activate(
        ClientEndpointId::Local,
        ClientEndpointFocusTarget::Workspace("local-workspace".into()),
        &mut outcome,
    );
    assert!(
        matches!(outcome.actions.as_slice(), [ClientShellAction::ActivateEndpoint {
        endpoint_id: ClientEndpointId::Local,
        target: Some(ClientEndpointFocusTarget::Workspace(id)),
    }] if id == "local-workspace")
    );
}

#[test]
fn context_menu_lookup_ignores_inactive_endpoint_workspaces() {
    let (mut state, endpoint_id) = state_with_remote();
    state.compose(100, 28).expect("combined endpoint frame");
    let remote = state
        .hits
        .workspaces
        .iter()
        .find(|hit| hit.endpoint_id == endpoint_id)
        .expect("remote workspace")
        .rect;
    let local = state
        .hits
        .workspaces
        .iter()
        .find(|hit| hit.endpoint_id.is_local())
        .expect("local workspace")
        .rect;

    assert_eq!(
        state.active_endpoint_workspace_at((remote.x, remote.y)),
        None
    );
    assert_eq!(
        state.active_endpoint_workspace_at((local.x, local.y)),
        Some("ws_1".into())
    );
}

#[test]
fn future_surface_waits_for_its_exact_snapshot_revision() {
    let (mut state, _) = state_with_remote();
    let mut future = surface();
    future.projection_revision = 2;
    future.surface_revision = 2;
    state.set_pane_surface(future);
    assert_eq!(
        state
            .pane_surface
            .as_ref()
            .map(|surface| surface.projection_revision),
        Some(1)
    );
    assert_eq!(
        state
            .pending_pane_surface
            .as_ref()
            .map(|surface| surface.projection_revision),
        Some(2)
    );

    let mut next = snapshot();
    next.revision = 2;
    state.set_snapshot(Box::new(next));
    assert_eq!(
        state
            .pane_surface
            .as_ref()
            .map(|surface| surface.projection_revision),
        Some(2)
    );
    assert!(state.pending_pane_surface.is_none());
}

#[test]
fn inactive_endpoint_snapshot_cache_never_regresses_revision() {
    let (mut state, endpoint_id) = state_with_remote();
    let mut newest = snapshot();
    newest.boot_id = "remote-boot".into();
    newest.revision = 3;
    newest.workspaces[0].label = "newest".into();
    state.set_endpoint_snapshot(&endpoint_id, Box::new(newest));
    let mut delayed = snapshot();
    delayed.boot_id = "remote-boot".into();
    delayed.revision = 2;
    delayed.workspaces[0].label = "delayed".into();

    state.set_endpoint_snapshot(&endpoint_id, Box::new(delayed));

    let label = state
        .endpoints
        .iter()
        .find(|endpoint| endpoint.endpoint_id == endpoint_id)
        .and_then(|endpoint| endpoint.snapshot.as_deref())
        .and_then(|snapshot| snapshot.workspaces.first())
        .map(|workspace| workspace.label.as_str());
    assert_eq!(label, Some("newest"));
}

#[test]
fn new_connection_generation_accepts_a_lower_same_boot_projection_revision() {
    let (mut state, endpoint_id) = state_with_remote();
    let mut previous = snapshot();
    previous.boot_id = "shared-server-boot".into();
    previous.revision = 9;
    previous.workspaces[0].label = "old connection".into();
    state.cache_endpoint_snapshot_inactive_for_generation(&endpoint_id, 4, Box::new(previous));
    let mut reconnected = snapshot();
    reconnected.boot_id = "shared-server-boot".into();
    reconnected.revision = 1;
    reconnected.workspaces[0].label = "new connection".into();

    state.cache_endpoint_snapshot_inactive_for_generation(&endpoint_id, 5, Box::new(reconnected));

    let endpoint = state
        .endpoints
        .iter()
        .find(|endpoint| endpoint.endpoint_id == endpoint_id)
        .expect("remote endpoint");
    assert_eq!(endpoint.snapshot_generation, Some(5));
    assert_eq!(endpoint.snapshot.as_ref().unwrap().revision, 1);
    assert_eq!(
        endpoint.snapshot.as_ref().unwrap().workspaces[0].label,
        "new connection"
    );
}

#[test]
fn reconnect_same_endpoint_accepts_new_generation_surface_revision() {
    for previous_revision in [9, 1] {
        let (mut state, endpoint_id) = state_with_remote();
        let mut previous = snapshot();
        previous.boot_id = "shared-server-boot".into();
        previous.revision = previous_revision;
        state.cache_endpoint_snapshot_inactive_for_generation(&endpoint_id, 4, Box::new(previous));
        assert!(state.activate_endpoint_projection(&endpoint_id));
        let mut previous_surface = surface();
        previous_surface.boot_id = "shared-server-boot".into();
        previous_surface.projection_revision = previous_revision;
        previous_surface.surface_revision = 9;
        state.set_pane_surface(previous_surface.clone());
        previous_surface.projection_revision += 1;
        state.set_pane_surface(previous_surface);
        assert!(state.pending_pane_surface.is_some());
        state.agent_scroll = 7;

        state.mark_endpoint_disconnected(&endpoint_id);
        let mut reconnected = snapshot();
        reconnected.boot_id = "shared-server-boot".into();
        reconnected.revision = 1;
        state.cache_endpoint_snapshot_inactive_for_generation(
            &endpoint_id,
            5,
            Box::new(reconnected),
        );
        assert_eq!(state.snapshot.as_ref().unwrap().revision, previous_revision);
        assert_eq!(state.pane_surface.as_ref().unwrap().surface_revision, 9);

        state.set_endpoint_status(&endpoint_id, ClientEndpointStatus::Online);
        assert!(state.activate_endpoint_projection(&endpoint_id));
        assert!(state.compose(106, 20).is_none());
        let mut reconnected_surface = surface();
        reconnected_surface.boot_id = "shared-server-boot".into();
        reconnected_surface.projection_revision = 1;
        reconnected_surface.surface_revision = 1;
        state.set_pane_surface(reconnected_surface);

        assert_eq!(state.snapshot.as_ref().unwrap().revision, 1);
        assert_eq!(state.pane_surface.as_ref().unwrap().projection_revision, 1);
        assert_eq!(state.pane_surface.as_ref().unwrap().surface_revision, 1);
        assert!(state.pending_pane_surface.is_none());
        assert_eq!(state.agent_scroll, 7);
        assert!(state.compose(106, 20).is_some());
    }
}

#[test]
fn on_unfocus_navigation_acknowledges_only_the_displayed_completion_generation() {
    let mut config = Config::default();
    config.ui.attention_read = crate::config::AttentionReadConfig::OnUnfocus;
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&config));

    let mut working = snapshot();
    working.agents = vec![agent("worker", AgentStatus::Working, 1)];
    state.set_endpoint_snapshot(&ClientEndpointId::Local, Box::new(working));

    let mut completed = state.snapshot.as_deref().unwrap().clone();
    completed.revision = 2;
    completed.agents[0].agent_status = AgentStatus::Idle;
    completed.agents[0].state_change_seq = 2;
    state.set_endpoint_snapshot(&ClientEndpointId::Local, Box::new(completed));
    let mut presented = surface();
    presented.projection_revision = 2;
    state.set_pane_surface(presented);
    assert_eq!(
        state.snapshot.as_deref().unwrap().agents[0].agent_status,
        AgentStatus::Done
    );

    let mut navigation = state.snapshot.as_deref().unwrap().clone();
    navigation.revision = 3;
    navigation.focused_pane_id = Some("pane_2".into());
    state.set_endpoint_snapshot(&ClientEndpointId::Local, Box::new(navigation));
    assert_eq!(
        state.snapshot.as_deref().unwrap().agents[0].agent_status,
        AgentStatus::Idle,
        "changing panes immediately consumes the completion displayed while focused"
    );

    let mut working_again = state.snapshot.as_deref().unwrap().clone();
    working_again.revision = 4;
    working_again.agents[0].agent_status = AgentStatus::Working;
    working_again.agents[0].state_change_seq = 3;
    working_again.focused_pane_id = Some("pane_1".into());
    state.set_endpoint_snapshot(&ClientEndpointId::Local, Box::new(working_again));
    let mut presented_working = surface();
    presented_working.projection_revision = 4;
    state.set_pane_surface(presented_working);
    let mut completed_again = state.snapshot.as_deref().unwrap().clone();
    completed_again.revision = 5;
    completed_again.agents[0].agent_status = AgentStatus::Idle;
    completed_again.agents[0].state_change_seq = 4;
    state.set_endpoint_snapshot(&ClientEndpointId::Local, Box::new(completed_again));
    assert_eq!(
        state.snapshot.as_deref().unwrap().agents[0].agent_status,
        AgentStatus::Done
    );

    let mut newer_navigation = state.snapshot.as_deref().unwrap().clone();
    newer_navigation.revision = 6;
    newer_navigation.focused_pane_id = Some("pane_2".into());
    state.set_endpoint_snapshot(&ClientEndpointId::Local, Box::new(newer_navigation));
    assert_eq!(
        state.snapshot.as_deref().unwrap().agents[0].agent_status,
        AgentStatus::Done,
        "a completion that arrived after the displayed generation remains unread"
    );
}

#[test]
fn on_unfocus_navigation_preserves_read_mark_when_next_surface_arrives_first() {
    let mut config = Config::default();
    config.ui.attention_read = crate::config::AttentionReadConfig::OnUnfocus;
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&config));

    let mut working = snapshot();
    working.agents = vec![agent("worker", AgentStatus::Working, 1)];
    let mut second_pane = working.panes[0].clone();
    second_pane.pane_id = "pane_2".into();
    second_pane.focused = false;
    working.panes.push(second_pane);
    state.set_endpoint_snapshot(&ClientEndpointId::Local, Box::new(working));
    let mut completed = state.snapshot.as_deref().unwrap().clone();
    completed.revision = 2;
    completed.agents = vec![agent("worker", AgentStatus::Idle, 2)];
    state.set_endpoint_snapshot(&ClientEndpointId::Local, Box::new(completed));
    let mut presented = surface();
    presented.projection_revision = 2;
    state.set_pane_surface(presented);
    assert_eq!(
        state.snapshot.as_ref().unwrap().tabs[0].agent_status,
        AgentStatus::Done
    );

    // A newer surface for another pane may precede its focus snapshot. The old
    // chat was already displayed; this frame must not erase its pending mark.
    let mut next_surface = surface();
    next_surface.projection_revision = 2;
    next_surface.surface_revision = 2;
    next_surface.panes[0].pane_id = "pane_2".into();
    state.set_pane_surface(next_surface);
    assert_eq!(
        state.pane_surface.as_ref().unwrap().panes[0].pane_id,
        "pane_2"
    );
    assert_eq!(
        state.snapshot.as_ref().unwrap().tabs[0].agent_status,
        AgentStatus::Done
    );
    let mut navigated = state.snapshot.as_deref().unwrap().clone();
    navigated.revision = 3;
    navigated.focused_pane_id = Some("pane_2".into());
    navigated.agents = vec![agent("worker", AgentStatus::Idle, 2)];
    state.set_endpoint_snapshot(&ClientEndpointId::Local, Box::new(navigated));

    assert_eq!(
        state.snapshot.as_ref().unwrap().agents[0].agent_status,
        AgentStatus::Idle
    );
    assert_eq!(
        state.snapshot.as_ref().unwrap().tabs[0].agent_status,
        AgentStatus::Idle
    );
}

#[test]
fn reconnect_snapshot_waits_for_coherent_activation_before_replacing_projection() {
    let (mut state, endpoint_id) = state_with_remote();
    assert!(state.activate_endpoint_projection(&endpoint_id));
    assert_eq!(state.snapshot.as_deref().unwrap().boot_id, "remote-boot");

    state.mark_endpoint_disconnected(&endpoint_id);
    let mut replacement = snapshot();
    replacement.boot_id = "replacement-boot".into();
    state.cache_endpoint_snapshot(&endpoint_id, Box::new(replacement));
    assert_eq!(state.snapshot.as_deref().unwrap().boot_id, "remote-boot");

    state.set_endpoint_status(&endpoint_id, ClientEndpointStatus::Online);
    assert!(state.activate_endpoint_projection(&endpoint_id));
    assert_eq!(
        state.snapshot.as_deref().unwrap().boot_id,
        "replacement-boot"
    );
}

#[test]
fn disconnected_active_endpoint_freezes_surface_and_marks_cached_ui_stale() {
    use crate::api::schema::AgentStatus;
    use crate::config::{AgentSidebarToken, StatusIndicatorStyle};

    let (mut state, endpoint_id) = state_with_remote();
    state.config.status_indicators = StatusIndicatorStyle::Symbols;
    state.config.agents.rows = vec![vec![
        AgentSidebarToken::StateIcon,
        AgentSidebarToken::Machine,
        AgentSidebarToken::Agent,
    ]];
    let endpoint = state
        .endpoints
        .iter_mut()
        .find(|endpoint| endpoint.endpoint_id == endpoint_id)
        .expect("remote endpoint");
    endpoint.snapshot.as_mut().expect("remote snapshot").agents =
        vec![agent("remote", AgentStatus::Blocked, 1)];
    assert!(state.activate_endpoint_projection(&endpoint_id));
    let mut remote_surface = surface();
    remote_surface.boot_id = "remote-boot".into();
    state.set_pane_surface(remote_surface);
    state.pending_integration_installs = 2;

    state.mark_endpoint_disconnected(&endpoint_id);
    let frame = state.compose(100, 28).expect("frozen endpoint frame");
    let text = frame
        .cells
        .chunks(frame.width as usize)
        .map(|row| {
            row.iter()
                .map(|cell| cell.symbol.as_str())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    assert_eq!(state.pending_integration_installs, 0);
    assert_eq!(
        state.endpoint_status(&endpoint_id),
        Some(ClientEndpointStatus::Reconnecting)
    );
    assert!(text.contains("◐ Build · reconnecting"), "frame: {text}");
    assert!(text.contains("× remote "), "frame: {text}");
    assert!(
        text.contains("LIVE"),
        "frozen surface should remain: {text}"
    );
    assert!(state.hits.panes.is_empty());
    assert!(frame.cursor.is_none());
    let buffer = frame.to_ratatui_buffer().expect("frame should reconstruct");
    let stale_icon = buffer
        .content()
        .iter()
        .find(|cell| cell.symbol() == "×")
        .expect("stale blocked icon");
    assert_eq!(stale_icon.fg, state.config.palette.overlay0);
}

#[cfg(unix)]
#[test]
fn graphics_scope_qualifies_colliding_boot_ids_by_endpoint() {
    let (mut state, endpoint_id) = state_with_remote();
    let local_scope = state.graphics_scope().to_owned();
    let mut remote = snapshot();
    remote.boot_id = "boot-1".into();
    state.set_endpoint_snapshot(&endpoint_id, Box::new(remote));
    assert!(state.activate_endpoint_projection(&endpoint_id));
    let remote_scope = state.graphics_scope();
    assert_ne!(local_scope, remote_scope);
    assert!(remote_scope.starts_with("ssh:0123456789abcdef0123456789abcdef:"));
}

#[cfg(unix)]
#[test]
fn local_direct_graphics_accept_server_ids_across_endpoint_switches_and_restarts() {
    use crate::kitty_graphics::surface::{direct_upload_control, host_image_id};
    use crate::protocol::{SurfaceGraphicsAssetKey, SurfaceGraphicsFormat, SurfaceGraphicsSource};

    let (mut state, remote_id) = state_with_remote();
    let asset = SurfaceGraphicsAssetKey {
        source: SurfaceGraphicsSource::PaneLayer {
            pane_id: "pane_1".into(),
            layer_id: "primary".into(),
        },
        image_width: 2,
        image_height: 2,
        format: SurfaceGraphicsFormat::Rgba,
        data_len: 16,
        data_fingerprint: 17,
    };
    let (server_image_id, _) = direct_upload_control("boot-1", &asset);
    assert_eq!(
        host_image_id(state.graphics_scope(), &asset),
        server_image_id
    );
    assert!(state.trust_direct_graphics_asset(&asset, server_image_id));
    assert!(!state.trust_direct_graphics_asset(&asset, server_image_id + 1));

    let mut remote = snapshot();
    remote.boot_id = "boot-1".into();
    state.set_endpoint_snapshot(&remote_id, Box::new(remote));
    assert!(state.activate_endpoint_projection(&remote_id));
    assert!(!state.trust_direct_graphics_asset(&asset, server_image_id));
    assert!(state.activate_endpoint_projection(&ClientEndpointId::Local));
    assert!(state.trust_direct_graphics_asset(&asset, server_image_id));

    let mut restarted = snapshot();
    restarted.boot_id = "replacement-boot".into();
    state.set_snapshot(Box::new(restarted));
    let (restarted_image_id, _) = direct_upload_control("replacement-boot", &asset);
    assert!(!state.trust_direct_graphics_asset(&asset, server_image_id));
    assert_eq!(
        host_image_id(state.graphics_scope(), &asset),
        restarted_image_id
    );
    assert!(state.trust_direct_graphics_asset(&asset, restarted_image_id));
}

#[test]
fn compact_palette_shows_profile_context_and_activates_foreign_pane() {
    let (mut state, remote_id) = state_with_remote();
    let mut remote = snapshot();
    remote.active_profile = "work".into();
    remote.workspaces[0].label = "overseas".into();
    remote.agents.push(agent(
        "builder",
        crate::api::schema::AgentStatus::Working,
        1,
    ));
    remote.agents[0].title = Some("Live build title".into());
    state.set_endpoint_snapshot(&remote_id, Box::new(remote));
    state.open_navigator_search_overlay();
    let Some(ClientShellOverlay::Navigator(navigator)) = state.overlay.as_ref() else {
        panic!("search palette");
    };
    assert!(navigator.search_focused && navigator.search_entry);
    let rows =
        render::client_navigator_rows(&state.endpoints, &state.active_endpoint_id, navigator);
    assert!(rows
        .iter()
        .all(|row| !matches!(row.target, ClientNavigatorTarget::Machine { .. })));
    let workspace = rows
        .iter()
        .find(|row| {
            matches!(&row.target,
                ClientNavigatorTarget::Workspace { endpoint_id, .. } if endpoint_id == &remote_id
            )
        })
        .expect("foreign workspace");
    assert!(workspace.meta.contains("work"));
    let pane = rows
        .iter()
        .find(|row| {
            matches!(&row.target,
                ClientNavigatorTarget::Pane { endpoint_id, .. } if endpoint_id == &remote_id
            )
        })
        .expect("foreign pane");
    assert_eq!(pane.label, "Live build title");
    assert!(pane.meta.contains("work"));
    let mut rendered = state.compose(106, 30).expect("palette");
    assert_eq!(state.hits.navigator_popup.width, 76);
    for (rect, _) in &state.hits.navigator_rows {
        let row = rendered.cells[rect.y as usize * rendered.width as usize + rect.x as usize..]
            .iter()
            .take(rect.width as usize)
            .map(|cell| cell.symbol.as_str())
            .collect::<String>();
        assert!(!row.contains("├─") && !row.contains("└─"), "{row}");
    }
    let outcome = state.handle_input_bytes(b"overseas");
    assert!(outcome.actions.is_empty());
    let Some(ClientShellOverlay::Navigator(navigator)) = state.overlay.as_mut() else {
        panic!("search palette");
    };
    navigator.selected = Some(ClientNavigatorTarget::Pane {
        endpoint_id: remote_id.clone(),
        pane_id: "pane_1".into(),
    });
    rendered = state.compose(106, 30).expect("filtered palette");
    assert!(rendered
        .cursor
        .as_ref()
        .is_some_and(|cursor| cursor.visible));
    let accepted = state.handle_input_bytes(b"\r");
    assert!(accepted.actions.iter().any(|action| matches!(action,
        ClientShellAction::ActivateEndpoint { endpoint_id, target: Some(ClientEndpointFocusTarget::Pane(pane_id)) }
        if endpoint_id == &remote_id && pane_id == "pane_1"
    )));
}

#[test]
fn navigator_uses_machine_parents_only_for_federated_clients() {
    let (mut state, _) = state_with_remote();
    state.open_navigator_overlay();
    let ClientShellOverlay::Navigator(navigator) = state.overlay.as_ref().expect("navigator")
    else {
        panic!("expected navigator");
    };
    let rows =
        render::client_navigator_rows(&state.endpoints, &state.active_endpoint_id, navigator);
    let machines = rows
        .iter()
        .filter(|row| matches!(row.target, ClientNavigatorTarget::Machine { .. }))
        .collect::<Vec<_>>();
    assert_eq!(
        machines
            .iter()
            .map(|row| row.label.as_str())
            .collect::<Vec<_>>(),
        vec!["Local", "Build"]
    );
    assert!(rows.iter().all(|row| {
        matches!(row.target, ClientNavigatorTarget::Machine { .. })
            || (!row.label.contains("Local ·") && !row.label.contains("Build ·"))
    }));
    assert!(rows.iter().all(|row| match row.target {
        ClientNavigatorTarget::Machine { .. } => row.depth == 0 && row.status.is_none(),
        ClientNavigatorTarget::Workspace { .. } => row.depth == 1 && row.status.is_none(),
        ClientNavigatorTarget::Pane { .. } => row.depth == 2 && row.status.is_some(),
    }));
    assert_eq!(rows.iter().filter(|row| row.current).count(), 1);

    let frame = state.compose(106, 30).expect("federated navigator");
    for (rect, target) in &state.hits.navigator_rows {
        let expected = match target {
            ClientNavigatorTarget::Machine { .. } => " ",
            ClientNavigatorTarget::Workspace { .. } => "   ",
            ClientNavigatorTarget::Pane { .. } => "   └─ ",
        };
        let prefix = frame.cells[rect.y as usize * frame.width as usize + rect.x as usize..]
            .iter()
            .take(expected.chars().count())
            .map(|cell| cell.symbol.as_str())
            .collect::<String>();
        assert_eq!(prefix, expected, "{target:?}");
    }

    let mut local = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    local.set_snapshot(Box::new(snapshot()));
    local.set_pane_surface(surface());
    let frame = local.compose(100, 28).expect("local-only sidebar");
    assert!(local.hits.machines.is_empty());
    assert!(!frame
        .cells
        .chunks(frame.width as usize)
        .map(|row| {
            row.iter()
                .map(|cell| cell.symbol.as_str())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
        .contains(" machines"));
    local.open_navigator_overlay();
    let ClientShellOverlay::Navigator(navigator) = local.overlay.as_ref().expect("navigator")
    else {
        panic!("expected navigator");
    };
    let rows =
        render::client_navigator_rows(&local.endpoints, &local.active_endpoint_id, navigator);
    assert!(rows
        .iter()
        .all(|row| !matches!(row.target, ClientNavigatorTarget::Machine { .. })));
    assert!(rows.iter().all(|row| match row.target {
        ClientNavigatorTarget::Workspace { .. } => row.depth == 0,
        ClientNavigatorTarget::Pane { .. } => row.depth == 1,
        ClientNavigatorTarget::Machine { .. } => false,
    }));
}

#[test]
fn navigator_fuzzy_search_ranks_fragmented_endpoint_qualified_panes() {
    let (mut state, remote_id) = state_with_remote();
    let mut remote = state
        .endpoints
        .iter()
        .find(|endpoint| endpoint.endpoint_id == remote_id)
        .and_then(|endpoint| endpoint.snapshot.clone())
        .expect("remote snapshot");
    remote.agents = vec![
        ClientShellAgent {
            pane_id: "pane_1".into(),
            name: Some("code review".into()),
            ..agent("code review", AgentStatus::Working, 1)
        },
        ClientShellAgent {
            pane_id: "pane_2".into(),
            name: Some("component revision".into()),
            ..agent("component revision", AgentStatus::Working, 1)
        },
    ];
    remote.panes.push(ClientShellPane {
        pane_id: "pane_2".into(),
        focused: false,
        ..remote.panes[0].clone()
    });
    state.set_endpoint_snapshot(&remote_id, remote);
    state.open_navigator_overlay();
    let ClientShellOverlay::Navigator(navigator) = state.overlay.as_mut().expect("navigator")
    else {
        panic!("expected navigator");
    };
    navigator.query = "revi".into();

    let rows =
        render::client_navigator_rows(&state.endpoints, &state.active_endpoint_id, navigator);
    let panes = rows
        .iter()
        .filter_map(|row| match &row.target {
            ClientNavigatorTarget::Pane {
                endpoint_id,
                pane_id,
            } => Some((endpoint_id, pane_id.as_str())),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        panes,
        vec![(&remote_id, "pane_1"), (&remote_id, "pane_2")],
        "fragmented query remains endpoint-qualified despite duplicate local pane ids"
    );
}

#[test]
fn navigator_keeps_saved_machine_visible_before_metadata_arrives() {
    let (mut state, endpoint_id) = state_with_remote();
    let endpoint = state
        .endpoints
        .iter_mut()
        .find(|endpoint| endpoint.endpoint_id == endpoint_id)
        .expect("saved remote endpoint");
    endpoint.snapshot = None;
    endpoint.status = ClientEndpointStatus::Connecting;
    state.open_navigator_overlay();
    let ClientShellOverlay::Navigator(navigator) = state.overlay.as_ref().expect("navigator")
    else {
        panic!("expected navigator");
    };

    let rows =
        render::client_navigator_rows(&state.endpoints, &state.active_endpoint_id, navigator);

    assert!(rows.iter().any(|row| {
        matches!(
            &row.target,
            ClientNavigatorTarget::Machine { endpoint_id: target } if target == &endpoint_id
        ) && row.label == "Build"
            && row.stale
    }));
    assert!(!rows.iter().any(|row| match &row.target {
        ClientNavigatorTarget::Machine { .. } => false,
        ClientNavigatorTarget::Workspace {
            endpoint_id: target,
            ..
        }
        | ClientNavigatorTarget::Pane {
            endpoint_id: target,
            ..
        } => target == &endpoint_id,
    }));
}

#[test]
fn navigator_machine_selection_opens_its_remembered_view() {
    let (mut state, endpoint_id) = state_with_remote();
    state.open_navigator_overlay();
    let selected = {
        let ClientShellOverlay::Navigator(navigator) = state.overlay.as_ref().expect("navigator")
        else {
            panic!("expected navigator");
        };
        render::client_navigator_rows(&state.endpoints, &state.active_endpoint_id, navigator)
            .into_iter()
            .find(|row| {
                matches!(
                    &row.target,
                    ClientNavigatorTarget::Machine { endpoint_id: target } if target == &endpoint_id
                )
            })
            .map(|row| row.target)
            .expect("remote machine row")
    };
    if let Some(ClientShellOverlay::Navigator(navigator)) = state.overlay.as_mut() {
        navigator.selected = Some(selected);
    }

    let mut outcome = ClientShellInput::default();
    state.accept_navigator_selection(&mut outcome);

    assert!(matches!(
        outcome.actions.as_slice(),
        [ClientShellAction::ActivateEndpoint {
            endpoint_id: activated,
            target: None,
        }] if activated == &endpoint_id
    ));
    assert!(state.overlay.is_none());
}

#[test]
fn navigator_foreign_pane_selection_activates_its_endpoint() {
    let (mut state, endpoint_id) = state_with_remote();
    state.open_navigator_overlay();
    let selected = {
        let ClientShellOverlay::Navigator(navigator) = state.overlay.as_ref().expect("navigator")
        else {
            panic!("expected navigator");
        };
        render::client_navigator_rows(&state.endpoints, &state.active_endpoint_id, navigator)
            .iter()
            .find(|row| {
                matches!(
                    &row.target,
                    ClientNavigatorTarget::Pane {
                        endpoint_id: target_endpoint,
                        pane_id,
                    } if target_endpoint == &endpoint_id && pane_id == "pane_1"
                )
            })
            .map(|row| row.target.clone())
            .expect("remote pane row")
    };
    if let Some(ClientShellOverlay::Navigator(navigator)) = state.overlay.as_mut() {
        navigator.selected = Some(selected);
    }
    let mut local = snapshot();
    let mut inserted = local.workspaces[0].clone();
    inserted.workspace_id = "ws_2".into();
    inserted.focused = false;
    local.workspaces.push(inserted);
    state.set_snapshot(Box::new(local));

    let mut outcome = ClientShellInput::default();
    state.accept_navigator_selection(&mut outcome);

    assert!(matches!(
        outcome.actions.as_slice(),
        [ClientShellAction::ActivateEndpoint {
            endpoint_id: activated,
            target: Some(ClientEndpointFocusTarget::Pane(pane_id)),
        }] if activated == &endpoint_id && pane_id == "pane_1"
    ));
    assert!(state.overlay.is_none());
}

#[test]
fn mobile_foreign_agent_and_workspace_targets_activate_their_endpoint() {
    use crate::api::schema::AgentStatus;

    let (mut state, endpoint_id) = state_with_remote();
    state
        .endpoints
        .iter_mut()
        .find(|endpoint| endpoint.endpoint_id == endpoint_id)
        .expect("remote endpoint")
        .snapshot
        .as_mut()
        .expect("remote snapshot")
        .agents = vec![agent("remote agent", AgentStatus::Working, 2)];
    state.mode = ClientShellMode::Navigate;
    state.compose(44, 30).expect("mobile switcher");
    let remote_agent = state
        .hits
        .mobile_targets
        .iter()
        .find_map(|(rect, target)| {
            matches!(
                target,
                ClientMobileTarget::Agent {
                    endpoint_id: target_endpoint,
                    pane_id,
                } if target_endpoint == &endpoint_id && pane_id == "pane_1"
            )
            .then_some(*rect)
        })
        .expect("remote agent target");
    let click = |rect: Rect| {
        RawInputEvent::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: rect.x,
            row: rect.y,
            modifiers: KeyModifiers::empty(),
        })
    };
    let outcome = state.handle_raw_events(vec![click(remote_agent)]);
    assert!(matches!(
        outcome.actions.as_slice(),
        [ClientShellAction::ActivateEndpoint {
            endpoint_id: activated,
            target: Some(ClientEndpointFocusTarget::Pane(pane_id)),
        }] if activated == &endpoint_id && pane_id == "pane_1"
    ));

    state.mode = ClientShellMode::Navigate;
    state.compose(44, 30).expect("mobile switcher");
    let remote_workspace = state
        .hits
        .mobile_targets
        .iter()
        .find_map(|(rect, target)| {
            matches!(
                target,
                ClientMobileTarget::Workspace {
                    endpoint_id: target_endpoint,
                    workspace_id,
                } if target_endpoint == &endpoint_id && workspace_id == "ws_1"
            )
            .then_some(*rect)
        })
        .expect("remote workspace target");
    let outcome = state.handle_raw_events(vec![click(remote_workspace)]);
    assert!(matches!(
        outcome.actions.as_slice(),
        [ClientShellAction::ActivateEndpoint {
            endpoint_id: activated,
            target: Some(ClientEndpointFocusTarget::Workspace(workspace_id)),
        }] if activated == &endpoint_id && workspace_id == "ws_1"
    ));
}

#[test]
fn cached_offline_navigator_and_mobile_targets_are_dimmed_and_disabled() {
    use crate::api::schema::AgentStatus;

    let (mut state, endpoint_id) = state_with_remote();
    state
        .endpoints
        .iter_mut()
        .find(|endpoint| endpoint.endpoint_id == endpoint_id)
        .expect("remote endpoint")
        .snapshot
        .as_mut()
        .expect("remote snapshot")
        .agents = vec![agent("remote agent", AgentStatus::Blocked, 2)];
    state.set_endpoint_status(&endpoint_id, ClientEndpointStatus::Reconnecting);

    state.open_navigator_overlay();
    let selected = {
        let ClientShellOverlay::Navigator(navigator) = state.overlay.as_ref().expect("navigator")
        else {
            panic!("expected navigator");
        };
        let rows =
            render::client_navigator_rows(&state.endpoints, &state.active_endpoint_id, navigator);
        let machine = rows
            .iter()
            .find(|row| {
                matches!(
                    &row.target,
                    ClientNavigatorTarget::Machine { endpoint_id: target } if target == &endpoint_id
                )
            })
            .expect("cached remote machine row");
        assert!(machine.stale);
        let row = rows
            .iter()
            .find(|row| {
                matches!(
                    &row.target,
                    ClientNavigatorTarget::Pane {
                        endpoint_id: target_endpoint,
                        pane_id,
                    } if target_endpoint == &endpoint_id && pane_id == "pane_1"
                )
            })
            .expect("cached remote pane row");
        assert!(row.stale);
        assert!(!row.meta.contains("reconnecting"));
        row.target.clone()
    };
    if let Some(ClientShellOverlay::Navigator(navigator)) = state.overlay.as_mut() {
        navigator.selected = Some(selected);
    }
    let mut navigator_outcome = ClientShellInput::default();
    state.accept_navigator_selection(&mut navigator_outcome);
    assert!(navigator_outcome.actions.is_empty());
    assert!(matches!(
        state.overlay,
        Some(ClientShellOverlay::Navigator(_))
    ));

    state.overlay = None;
    state.mode = ClientShellMode::Navigate;
    let frame = state.compose(44, 30).expect("offline mobile switcher");
    let mobile_target = state
        .hits
        .mobile_targets
        .iter()
        .find_map(|(rect, target)| {
            matches!(
                target,
                ClientMobileTarget::Workspace {
                    endpoint_id: target_endpoint,
                    workspace_id,
                } if target_endpoint == &endpoint_id && workspace_id == "ws_1"
            )
            .then_some(*rect)
        })
        .expect("cached remote workspace target");
    let buffer = frame.to_ratatui_buffer().expect("mobile frame buffer");
    assert_eq!(
        buffer[(mobile_target.x, mobile_target.y)].fg,
        state.config.palette.overlay0
    );
    let outcome = state.handle_raw_events(vec![RawInputEvent::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: mobile_target.x,
        row: mobile_target.y,
        modifiers: KeyModifiers::empty(),
    })]);
    assert!(outcome.actions.is_empty());
    assert_eq!(state.mode, ClientShellMode::Navigate);
    assert_eq!(state.active_endpoint_id, ClientEndpointId::Local);
}

#[test]
fn focus_agent_index_uses_online_aggregate_rows() {
    use crate::api::schema::AgentStatus;

    let (mut state, endpoint_id) = state_with_remote();
    state
        .endpoints
        .iter_mut()
        .find(|endpoint| endpoint.endpoint_id == endpoint_id)
        .expect("remote endpoint")
        .snapshot
        .as_mut()
        .expect("remote snapshot")
        .agents = vec![agent("remote agent", AgentStatus::Working, 2)];
    let focus_agent =
        |index| crate::input::KeybindMatch::Action(crate::input::KeybindAction::FocusAgent(index));

    assert!(state.indexed_navigation_target_exists(&focus_agent(0)));
    assert!(!state.indexed_navigation_target_exists(&focus_agent(1)));

    state.set_endpoint_status(&endpoint_id, ClientEndpointStatus::Reconnecting);
    assert!(!state.indexed_navigation_target_exists(&focus_agent(0)));
}

#[test]
fn workspace_drag_rejects_foreign_endpoint_slots() {
    let (mut state, endpoint_id) = state_with_remote();
    state.compose(100, 28).expect("aggregate sidebar");
    let local = state
        .hits
        .workspaces
        .iter()
        .find(|hit| hit.endpoint_id.is_local())
        .expect("local workspace")
        .rect;
    let remote = state
        .hits
        .workspaces
        .iter()
        .find(|hit| hit.endpoint_id == endpoint_id)
        .expect("remote workspace")
        .rect;
    let mouse = |kind, rect: Rect| {
        RawInputEvent::Mouse(MouseEvent {
            kind,
            column: rect.x.saturating_add(1),
            row: rect.y,
            modifiers: KeyModifiers::empty(),
        })
    };

    state.handle_raw_events(vec![mouse(MouseEventKind::Down(MouseButton::Left), local)]);
    state.handle_raw_events(vec![mouse(MouseEventKind::Drag(MouseButton::Left), remote)]);

    assert!(state.chrome_drag.is_none());
}

#[test]
fn collapsed_sidebar_orders_agents_before_spaces_for_local_and_remote_views() {
    let mut config = Config::default();
    config.ui.sidebar.section_order = [
        crate::config::SidebarSection::Agents,
        crate::config::SidebarSection::Spaces,
    ];
    config
        .ui
        .sidebar
        .agents
        .state_icons
        .insert("working".into(), "W".into());
    for aggregate in [false, true] {
        let mut state = ClientShellState::new(ClientShellConfig::from_config(&config));
        let mut local = snapshot();
        local.agents = vec![agent("active agent", AgentStatus::Working, 1)];
        if aggregate {
            let profile = remote_profile();
            let remote_id = ClientEndpointId::Ssh(profile.id.clone());
            state.set_endpoint_catalog(&[profile]);
            state.set_endpoint_status(&remote_id, ClientEndpointStatus::Online);
            let mut remote = snapshot();
            remote.boot_id = "remote-boot".into();
            state.set_endpoint_snapshot(&remote_id, Box::new(remote));
        }
        state.set_snapshot(Box::new(local));
        state.set_pane_surface(surface());
        state.sidebar_collapsed = true;
        let frame = state.compose(100, 28).expect("collapsed frame");
        let workspace_y = state
            .hits
            .workspaces
            .iter()
            .find(|hit| hit.endpoint_id.is_local())
            .expect("local workspace")
            .rect
            .y;
        let agent_y = if aggregate {
            state
                .hits
                .endpoint_agents
                .iter()
                .find(|(_, endpoint, _)| endpoint.is_local())
                .expect("local agent")
                .0
                .y
        } else {
            state.hits.agents.first().expect("agent").0.y
        };
        let (x, y) = cell_symbol_position(&frame, Rect::new(0, agent_y, 5, 1), "W");
        assert_eq!(y, agent_y);
        assert_eq!(
            frame.cells[usize::from(y) * usize::from(frame.width) + usize::from(x)].symbol,
            "W"
        );
        assert!(
            agent_y < workspace_y,
            "aggregate={aggregate}: agents should render above spaces"
        );
    }
}

#[test]
fn collapsed_aggregate_workspace_status_uses_its_status_color() {
    use crate::api::schema::AgentStatus;

    let (mut state, endpoint_id) = state_with_remote();
    state
        .endpoints
        .iter_mut()
        .find(|endpoint| endpoint.endpoint_id == endpoint_id)
        .expect("remote endpoint")
        .snapshot
        .as_mut()
        .expect("remote snapshot")
        .workspaces[0]
        .agent_status = AgentStatus::Blocked;
    state.sidebar_collapsed = true;

    let frame = state.compose(100, 28).expect("collapsed aggregate sidebar");
    let workspace = state
        .hits
        .workspaces
        .iter()
        .find(|hit| hit.endpoint_id == endpoint_id)
        .expect("remote workspace")
        .rect;
    let buffer = frame.to_ratatui_buffer().expect("frame buffer");
    assert_eq!(
        buffer[(workspace.x.saturating_add(2), workspace.y)].fg,
        state.config.palette.red
    );
}

#[test]
fn navigator_workspace_arrows_cross_machine_headings_without_activating_them() {
    let (mut state, endpoint_id) = state_with_remote();
    state.open_navigator_overlay();
    for (key, expected_endpoint) in [
        (KeyCode::Right, endpoint_id),
        (KeyCode::Left, ClientEndpointId::Local),
    ] {
        let outcome = state.handle_raw_events(vec![RawInputEvent::Key(
            crate::input::TerminalKey::new(key, KeyModifiers::empty()),
        )]);
        assert!(outcome.actions.is_empty());
        let Some(ClientShellOverlay::Navigator(navigator)) = &state.overlay else {
            panic!("navigator");
        };
        assert_eq!(
            navigator.selected,
            Some(ClientNavigatorTarget::Pane {
                endpoint_id: expected_endpoint,
                pane_id: "pane_1".into(),
            })
        );
    }
}

#[test]
fn navigator_foreign_workspace_heading_keeps_the_workspace_target() {
    let (mut state, endpoint_id) = state_with_remote();
    state.open_navigator_overlay();
    let selected = {
        let ClientShellOverlay::Navigator(navigator) = state.overlay.as_ref().expect("navigator")
        else {
            panic!("expected navigator");
        };
        render::client_navigator_rows(&state.endpoints, &state.active_endpoint_id, navigator)
            .iter()
            .find(|row| {
                matches!(
                    &row.target,
                    ClientNavigatorTarget::Workspace {
                        endpoint_id: target_endpoint,
                        workspace_id,
                    } if target_endpoint == &endpoint_id && workspace_id == "ws_1"
                )
            })
            .map(|row| row.target.clone())
            .expect("remote workspace heading")
    };
    if let Some(ClientShellOverlay::Navigator(navigator)) = state.overlay.as_mut() {
        navigator.selected = Some(selected);
    }

    let mut outcome = ClientShellInput::default();
    state.accept_navigator_selection(&mut outcome);

    assert!(matches!(
        outcome.actions.as_slice(),
        [ClientShellAction::ActivateEndpoint {
            endpoint_id: activated,
            target: Some(ClientEndpointFocusTarget::Workspace(workspace_id)),
        }] if activated == &endpoint_id && workspace_id == "ws_1"
    ));
}

/// A remote machine's pins reorder in the multi-machine sidebar: the drag
/// previews within that machine's block only, and the drop reaches that
/// machine while Local holds the surface.
#[test]
fn dragging_a_remote_pin_moves_it_on_its_own_machine() {
    let (mut state, remote) = state_with_remote();
    let mut local = state.snapshot.as_deref().expect("local snapshot").clone();
    local.pinned_tabs = vec![crate::protocol::ClientShellPinnedTab {
        role: None,
        tab_id: "tab_1".into(),
        workspace_id: "ws_1".into(),
    }];
    state.set_snapshot(Box::new(local));
    let mut snapshot = state
        .endpoints
        .iter()
        .find(|endpoint| endpoint.endpoint_id == remote)
        .and_then(|endpoint| endpoint.snapshot.clone())
        .expect("remote snapshot");
    let template = snapshot.tabs[0].clone();
    snapshot.tabs = (1..=3)
        .map(|number| ClientShellTab {
            sort_rank: 0,
            desk_count: 0,
            tab_id: format!("tab_{number}"),
            number,
            label: format!("chat {number}"),
            focused: number == 1,
            ..template.clone()
        })
        .collect();
    snapshot.pinned_tabs = snapshot
        .tabs
        .iter()
        .map(|tab| crate::protocol::ClientShellPinnedTab {
            role: None,
            tab_id: tab.tab_id.clone(),
            workspace_id: tab.workspace_id.clone(),
        })
        .collect();
    state.set_endpoint_snapshot(&remote, snapshot);
    let drawn = |state: &ClientShellState| {
        state
            .hits
            .pinned_rows
            .iter()
            .map(|hit| (hit.endpoint_id.clone(), hit.tab_id.clone()))
            .collect::<Vec<_>>()
    };
    state.compose(100, 30).expect("aggregate frame");
    let rows = state.hits.pinned_rows.clone();
    assert_eq!(rows.len(), 4, "{:?}", drawn(&state));
    assert_eq!(rows[0].endpoint_id, Some(ClientEndpointId::Local));
    let (local_row, remote_first, remote_last) = (rows[0].rect, rows[1].rect, rows[3].rect);
    let mouse = |kind, rect: ratatui::layout::Rect, row: u16| {
        crate::raw_input::RawInputEvent::Mouse(crossterm::event::MouseEvent {
            kind,
            column: rect.x + 4,
            row,
            modifiers: KeyModifiers::empty(),
        })
    };

    // The remote's last pin dragged onto Local's row stops at the top of its
    // own block.
    state.handle_raw_events(vec![mouse(
        MouseEventKind::Down(MouseButton::Left),
        remote_last,
        remote_last.y,
    )]);
    state.handle_raw_events(vec![mouse(
        MouseEventKind::Drag(MouseButton::Left),
        local_row,
        local_row.y,
    )]);
    state.compose(100, 30).expect("aggregate frame");
    let remote_tab = |tab: &str| (Some(remote.clone()), tab.to_owned());
    assert_eq!(
        drawn(&state),
        [
            (Some(ClientEndpointId::Local), "tab_1".to_owned()),
            remote_tab("tab_3"),
            remote_tab("tab_1"),
            remote_tab("tab_2"),
        ]
    );
    // Cmd digits follow the preview across machines at once.
    assert_eq!(
        state.aggregate_numbered_tabs().expect("pins")[..4],
        [
            (ClientEndpointId::Local, "tab_1".to_owned()),
            (remote.clone(), "tab_3".to_owned()),
            (remote.clone(), "tab_1".to_owned()),
            (remote.clone(), "tab_2".to_owned()),
        ]
    );
    let drop = state.handle_raw_events(vec![mouse(
        MouseEventKind::Up(MouseButton::Left),
        remote_first,
        remote_first.y,
    )]);
    assert!(
        !drop
            .actions
            .iter()
            .any(|action| matches!(action, ClientShellAction::ActivateEndpoint { .. })),
        "a drop never activates the machine"
    );

    let sent = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut endpoints = crate::client::endpoint::EndpointRegistry::new(
        CapturingTransport(Default::default()),
        1,
        Default::default(),
    );
    endpoints.insert(
        remote.clone(),
        CapturingTransport(sent.clone()),
        1,
        Default::default(),
        false,
    );
    let mut commands = crate::client::endpoint_commands::EndpointCommands::default();
    crate::client::shell_runtime::dispatch_client_shell_actions(
        drop.actions,
        &mut commands,
        &mut endpoints,
        Some(&mut state),
        &mut Vec::new(),
        &mut None,
        None,
    )
    .unwrap();
    let sent = sent.lock().unwrap().clone();
    assert!(
        matches!(sent.as_slice(), [crate::protocol::ClientMessage::ClientShellEndpointRequest {
            boot_id, request }] if boot_id == "remote-boot"
                && request.contains("tab.pin_move")
                && request.contains("\"tab_id\":\"tab_3\"")
                && request.contains("\"pin_index\":0")),
        "remote received {sent:?}"
    );
}

#[test]
fn aggregate_pins_scroll_within_their_section_and_keep_the_divider_below_them() {
    let (mut state, remote) = state_with_remote();
    let mut snapshot = state
        .endpoints
        .iter()
        .find(|endpoint| endpoint.endpoint_id == remote)
        .and_then(|endpoint| endpoint.snapshot.clone())
        .expect("remote snapshot");
    let template = snapshot.tabs[0].clone();
    snapshot.tabs = (1..=30)
        .map(|number| ClientShellTab {
            sort_rank: 0,
            desk_count: 0,
            tab_id: format!("tab_{number}"),
            number,
            label: format!("chat {number}"),
            focused: number == 1,
            ..template.clone()
        })
        .collect();
    snapshot.pinned_tabs = snapshot
        .tabs
        .iter()
        .map(|tab| crate::protocol::ClientShellPinnedTab {
            role: None,
            tab_id: tab.tab_id.clone(),
            workspace_id: tab.workspace_id.clone(),
        })
        .collect();
    state.set_endpoint_snapshot(&remote, snapshot);
    let shown = |state: &ClientShellState| {
        state
            .hits
            .endpoint_pins
            .iter()
            // Pinned-section rows; a chat row's own pin toggle hit is its pin cell.
            .filter(|(rect, pin, _, _)| rect != pin)
            .map(|(_, _, _, tab_id)| tab_id.clone())
            .collect::<Vec<_>>()
    };

    let frame = state.compose(100, 30).expect("aggregate frame");
    let first = shown(&state);
    assert!(
        first.len() < 30 && first[0] == "tab_1",
        "section capped: {first:?}"
    );
    let header = &frame_rows(&frame)[state.hits.sidebar_divider.y as usize];
    assert!(
        header.contains(&format!("+{} more", 30 - first.len())),
        "header: {header}"
    );
    // The spaces list keeps its rows, and its divider sits on the drawn
    // spaces/agents boundary under the pins.
    let last_pin = state
        .hits
        .endpoint_pins
        .iter()
        .rev()
        .find(|(rect, pin, _, _)| rect != pin)
        .expect("pin rows")
        .0;
    let boundary_row = |state: &mut ClientShellState| {
        let frame = state.compose(100, 30).expect("aggregate frame");
        let width = state.hits.sidebar_divider.x as usize;
        let agents = frame_rows(&frame)
            .iter()
            .position(|row| {
                row.chars()
                    .take(width)
                    .collect::<String>()
                    .starts_with(" agents")
            })
            .expect("agents header drawn") as u16;
        agents - 1
    };
    let boundary = boundary_row(&mut state);
    let divider = state.hits.sidebar_section_divider;
    assert!(
        boundary > last_pin.y,
        "boundary {boundary} over pin {last_pin:?}"
    );
    assert_eq!(divider.y, boundary, "divider hitbox off the drawn boundary");
    // Dragging the divider moves the drawn boundary with the pointer.
    let target = boundary - 3;
    for (kind, row) in [
        (MouseEventKind::Down(MouseButton::Left), divider.y),
        (MouseEventKind::Drag(MouseButton::Left), target),
        (MouseEventKind::Up(MouseButton::Left), target),
    ] {
        state.handle_raw_events(vec![RawInputEvent::Mouse(MouseEvent {
            kind,
            column: divider.x + 2,
            row,
            modifiers: KeyModifiers::NONE,
        })]);
    }
    let dragged = boundary_row(&mut state);
    assert!(
        dragged.abs_diff(target) <= 1,
        "boundary {dragged} did not follow the pointer to {target}"
    );
    assert_eq!(state.hits.sidebar_section_divider.y, dragged);
    assert!(state
        .hits
        .machines
        .iter()
        .any(|hit| hit.endpoint_id == remote));

    // The wheel over the section reaches the last pin.
    let body = state.hits.endpoint_pin_body;
    for _ in 0..40 {
        state.handle_raw_events(vec![RawInputEvent::Mouse(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: body.x + 2,
            row: body.y,
            modifiers: KeyModifiers::NONE,
        })]);
    }
    state.compose(100, 30).expect("scrolled frame");
    let scrolled = shown(&state);
    assert_eq!(scrolled.last().map(String::as_str), Some("tab_30"));
    assert_eq!(scrolled.len(), first.len());
}

#[test]
fn inactive_machine_pin_rolls_up_from_that_machines_factory_overlay() {
    // Local holds the surface; the remote's pinned lane has a live run in the
    // remote's own overlay. Its pin shows that work before the remote is
    // selected, and selecting it changes nothing.
    let (mut state, remote) = state_with_remote();
    state.config.factory.enabled = true;
    let mut remote_snapshot = snapshot();
    remote_snapshot.boot_id = "remote-boot".into();
    remote_snapshot.pinned_tabs = vec![crate::protocol::ClientShellPinnedTab {
        role: None,
        tab_id: "tab_1".into(),
        workspace_id: "ws_1".into(),
    }];
    state.set_endpoint_snapshot_for_generation(&remote, 4, Box::new(remote_snapshot));
    let overlay = crate::factory_overlay::parse(
        br#"{"version":1,"tabs":{"tab_1":{"kind":"lane","runs":[{"id":"wf_live"}]}}}"#,
    )
    .unwrap();
    let crate::protocol::ServerMessage::EndpointControl { kind, data } =
        crate::protocol::endpoint::factory_overlay_message("remote-boot", 2, Some(&overlay))
            .unwrap()
    else {
        panic!("expected factory overlay control");
    };
    let crate::client::endpoint::EndpointControlMessage::FactoryOverlay(decoded) =
        crate::client::endpoint::decode_endpoint_control(&kind, &data).unwrap()
    else {
        panic!("expected decoded factory overlay");
    };
    state.set_endpoint_factory_overlay_for_generation(&remote, 4, decoded);
    let remote_pin_mark = |state: &mut ClientShellState| {
        let frame = state.compose(100, 28).expect("aggregate frame");
        let rect = state
            .hits
            .endpoint_pins
            .iter()
            .find(|(_, _, endpoint, _)| *endpoint == remote)
            .map(|(rect, ..)| *rect)
            .expect("remote pin row");
        let buffer = frame.to_ratatui_buffer().expect("frame buffer");
        let cell = &buffer[(rect.x + 1, rect.y)];
        (cell.symbol().to_owned(), cell.fg)
    };
    let inactive = remote_pin_mark(&mut state);
    assert_eq!(inactive.1, state.config.palette.working, "{inactive:?}");
    assert!(state.activate_endpoint_projection(&remote));
    assert_eq!(remote_pin_mark(&mut state), inactive);
}

#[test]
fn inactive_machine_overlay_change_redraws_its_pin_only_when_pinned() {
    // Local holds the surface. An overlay-only update from the remote arrives with no snapshot
    // behind it, so the overlay handler's own frame is the only redraw its pins get.
    let (mut state, remote) = state_with_remote();
    state.config.factory.enabled = true;
    let overlay_update = |revision: u64, json: &[u8]| {
        let overlay = crate::factory_overlay::parse(json).unwrap();
        let crate::protocol::ServerMessage::EndpointControl { kind, data } =
            crate::protocol::endpoint::factory_overlay_message(
                "remote-boot",
                revision,
                Some(&overlay),
            )
            .unwrap()
        else {
            panic!("expected factory overlay control");
        };
        let crate::client::endpoint::EndpointControlMessage::FactoryOverlay(decoded) =
            crate::client::endpoint::decode_endpoint_control(&kind, &data).unwrap()
        else {
            panic!("expected decoded factory overlay");
        };
        decoded
    };
    let deliver = |state: &mut ClientShellState, revision: u64, json: &[u8]| {
        crate::client::shell_runtime::apply_client_shell_factory_overlay(
            state,
            &remote,
            4,
            overlay_update(revision, json),
            (100, 28),
            (8, 16),
            false,
        )
        .0
    };
    let idle = br#"{"version":1,"tabs":{"tab_1":{"kind":"lane"}}}"#;
    let live = br#"{"version":1,"tabs":{"tab_1":{"kind":"lane","runs":[{"id":"wf_live"}]}}}"#;
    let mut remote_snapshot = snapshot();
    remote_snapshot.boot_id = "remote-boot".into();
    state.set_endpoint_snapshot_for_generation(&remote, 4, Box::new(remote_snapshot.clone()));

    // Nothing of the remote's is on screen: its overlay is cached without a redraw.
    assert!(deliver(&mut state, 1, idle).is_none());

    remote_snapshot.pinned_tabs = vec![crate::protocol::ClientShellPinnedTab {
        role: None,
        tab_id: "tab_1".into(),
        workspace_id: "ws_1".into(),
    }];
    state.set_endpoint_snapshot_for_generation(&remote, 4, Box::new(remote_snapshot));
    state.compose(100, 28).expect("aggregate frame");
    let frame = deliver(&mut state, 2, live).expect("pinned remote redraws on its overlay");
    let rect = state
        .hits
        .endpoint_pins
        .iter()
        .find(|(_, _, endpoint, _)| *endpoint == remote)
        .map(|(rect, ..)| *rect)
        .expect("remote pin row");
    let buffer = frame.to_ratatui_buffer().expect("frame buffer");
    assert_eq!(
        buffer[(rect.x + 1, rect.y)].fg,
        state.config.palette.working,
        "the redraw shows the remote's live run on its pin"
    );
    assert!(state.endpoint_is_active(&ClientEndpointId::Local));
}

/// Aggregate sidebar projection: a remote agent must precede a local plain pin
/// in both displayed rows and Cmd+digit dispatch, despite machine order.
#[test]
fn aggregate_agent_pins_display_and_cmd_digits_agree() {
    let (mut state, remote) = state_with_remote();
    let mut local = state.snapshot.as_deref().unwrap().clone();
    local.pinned_tabs = vec![crate::protocol::ClientShellPinnedTab {
        tab_id: "tab_1".into(),
        workspace_id: "ws_1".into(),
        role: None,
    }];
    state.set_snapshot(Box::new(local));
    let mut snapshot = state
        .endpoints
        .iter()
        .find(|endpoint| endpoint.endpoint_id == remote)
        .unwrap()
        .snapshot
        .as_deref()
        .unwrap()
        .clone();
    snapshot.pinned_tabs = vec![crate::protocol::ClientShellPinnedTab {
        tab_id: "tab_1".into(),
        workspace_id: "ws_1".into(),
        role: Some(crate::api::schema::TabRole::Agent),
    }];
    state.set_endpoint_snapshot(&remote, Box::new(snapshot));
    let frame = state.compose(100, 30).unwrap();
    for (index, hit) in state.hits.pinned_rows.iter().enumerate() {
        let slot = super::super::agent_sidebar::chat_pin_rect(hit.rect);
        let cell =
            usize::from(hit.rect.y) * usize::from(frame.width) + usize::from(slot.right() - 1);
        assert_eq!(frame.cells[cell].symbol, (index + 1).to_string());
    }
    assert_eq!(state.hits.pinned_rows[0].endpoint_id, Some(remote.clone()));
    assert_eq!(
        state.hits.pinned_rows[1].endpoint_id,
        Some(ClientEndpointId::Local)
    );
    let numbered = state.aggregate_numbered_tabs().unwrap();
    assert_eq!(numbered[0], (remote.clone(), "tab_1".into()));
    assert_eq!(numbered[1], (ClientEndpointId::Local, "tab_1".into()));
}

#[test]
fn machine_diagnostic_badge_on_remote_pin_reopens_notice() {
    let (mut state, id) = state_with_remote();
    let mut remote = snapshot();
    remote.boot_id = "remote-boot".into();
    remote.workspaces[0].label = "remote-workspace".into();
    remote
        .pinned_tabs
        .push(crate::protocol::ClientShellPinnedTab {
            workspace_id: "ws_1".into(),
            tab_id: "tab_1".into(),
            role: None,
        });
    state.set_endpoint_snapshot(&id, Box::new(remote));
    state.set_endpoint_status(&id, ClientEndpointStatus::Attention);
    state.set_machine_diagnostic(&id, "Permission denied".into());
    for _ in 0..2 {
        state.compose(120, 40).expect("remote pinned row");
        let pin = state
            .hits
            .endpoint_pins
            .iter()
            .find(|(_, _, endpoint, _)| endpoint == &id)
            .expect("remote pin")
            .0;
        let badge = state
            .hits
            .machines
            .iter()
            .find(|hit| hit.endpoint_id == id && hit.status_badge.y == pin.y)
            .expect("pinned machine diagnostic badge")
            .status_badge;
        state.handle_raw_events(vec![RawInputEvent::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: badge.x,
            row: badge.y,
            modifiers: KeyModifiers::NONE,
        })]);
        assert!(state
            .visible_endpoint_notice
            .take()
            .expect("diagnostic notice")
            .body
            .contains("Permission denied"));
    }
}

#[test]
fn collapsed_sidebar_machine_badge_reopens_diagnostic() {
    let (mut state, id) = state_with_remote();
    state.sidebar_collapsed = true;
    state.set_endpoint_status(&id, ClientEndpointStatus::Attention);
    state.set_machine_diagnostic(&id, "Permission denied".into());
    for _ in 0..2 {
        state.compose(120, 40).expect("collapsed sidebar");
        let workspace = state
            .hits
            .workspaces
            .iter()
            .find(|hit| hit.endpoint_id == id)
            .expect("remote workspace")
            .rect;
        let badge = state
            .hits
            .machines
            .iter()
            .find(|hit| hit.endpoint_id == id && hit.status_badge.y == workspace.y)
            .expect("collapsed machine diagnostic badge")
            .status_badge;
        state.handle_raw_events(vec![RawInputEvent::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: badge.x,
            row: badge.y,
            modifiers: KeyModifiers::NONE,
        })]);
        assert!(state
            .visible_endpoint_notice
            .take()
            .expect("diagnostic notice")
            .body
            .contains("Permission denied"));
    }
}


#[test]
fn remote_spaces_that_share_a_label_keep_rows_of_their_own() {
    let (mut state, remote_id) = state_with_remote();
    let mut remote = snapshot();
    remote.boot_id = "remote-boot".into();
    let mut second = remote.workspaces[0].clone();
    second.workspace_id = "ws_2".into();
    second.number = 2;
    second.focused = false;
    remote.workspaces.push(second);
    for workspace in &mut remote.workspaces {
        workspace.label = "rails".into();
        remote.pinned_tabs.push(crate::protocol::ClientShellPinnedTab {
            workspace_id: workspace.workspace_id.clone(),
            tab_id: format!("tab_{}", workspace.workspace_id),
            role: None,
        });
    }
    state.set_endpoint_snapshot(&remote_id, Box::new(remote));
    state.compose(100, 30).expect("two remote rails spaces");
    let rows = state
        .hits
        .workspaces
        .iter()
        .filter(|hit| hit.endpoint_id == remote_id)
        .map(|hit| hit.workspace_id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(rows, ["ws_1", "ws_2"]);
}

#[test]
fn folded_remote_worktree_group_stays_while_only_a_hidden_child_has_a_chat() {
    let (mut state, remote_id) = state_with_remote();
    let mut remote = snapshot();
    remote.boot_id = "remote-boot".into();
    remote.workspaces[0].label = "remote repo".into();
    remote.workspaces[0].worktree = Some(ClientShellWorktree {
        key: "repo".into(),
        label: "repo".into(),
        is_linked_worktree: false,
    });
    let mut child = remote.workspaces[0].clone();
    child.workspace_id = "ws_2".into();
    child.number = 2;
    child.label = "feature".into();
    child.focused = false;
    child.worktree = Some(ClientShellWorktree {
        key: "repo".into(),
        label: "repo".into(),
        is_linked_worktree: true,
    });
    remote.workspaces.push(child);
    // The parent's agent exited; the only chat left is the child's.
    let mut chat = agent("remote", AgentStatus::Idle, 0);
    chat.workspace_id = "ws_2".into();
    chat.pane_id = "pane_ws_2".into();
    chat.focused = false;
    remote.agents.push(chat);
    state.set_endpoint_snapshot(&remote_id, Box::new(remote));
    state
        .remote_collapsed_groups
        .entry(remote_id.clone())
        .or_default()
        .insert("repo".into());
    state.compose(100, 30).expect("folded remote group");
    assert!(state
        .hits
        .workspaces
        .iter()
        .any(|hit| hit.endpoint_id == remote_id && hit.workspace_id == "ws_1"));
}
