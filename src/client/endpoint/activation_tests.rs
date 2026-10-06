use super::*;

fn endpoint() -> ClientEndpointId {
    ClientEndpointId::Ssh(
        super::super::ProfileId::parse("0123456789abcdef0123456789abcdef").unwrap(),
    )
}

fn lease(id: ClientEndpointId, generation: u64, boot: &str) -> EndpointLease {
    EndpointLease {
        endpoint_id: id,
        generation,
        boot_id: boot.into(),
        minimum_revision: 0,
    }
}

#[derive(Clone)]
struct FakeTransport {
    sent: std::sync::Arc<std::sync::Mutex<Vec<crate::protocol::ClientMessage>>>,
    fail_after_write: bool,
}

impl super::super::EndpointTransport for FakeTransport {
    fn send(&mut self, message: &crate::protocol::ClientMessage) -> std::io::Result<()> {
        self.sent.lock().unwrap().push(message.clone());
        if self.fail_after_write {
            Err(std::io::Error::other("simulated observed write failure"))
        } else {
            Ok(())
        }
    }
}

fn negotiation() -> super::super::EndpointNegotiation {
    super::super::EndpointNegotiation::new(
        vec!["client_shell.surface.set".into()],
        vec![
            crate::protocol::endpoint::SURFACE_INTEREST_CAPABILITY.into(),
            crate::protocol::endpoint::PRESENTATION_EFFECTS_FENCE_CAPABILITY.into(),
        ],
    )
}

fn test_snapshot(boot_id: &str, revision: u64) -> crate::protocol::ClientShellSnapshot {
    crate::protocol::ClientShellSnapshot {
        boot_id: boot_id.into(),
        revision,
        config_diagnostic: None,
        product_announcement: None,
        update_available: None,
        update_install_command: String::new(),
        server_keybindings_toml: None,
        latest_release_notes_available: false,
        integration_updates_available: false,
        worktree_directory: String::new(),
        release_notes: None,
        focused_workspace_id: None,
        focused_tab_id: None,
        focused_pane_id: None,
        tab_bar_right: Vec::new(),
        tab_bar_right_separator: String::new(),
        agent_view_label: None,
        session_name: None,
        active_profile: "default".into(),
        agent_order: Vec::new(),
        workspaces: Vec::new(),
        tabs: Vec::new(),
        panes: Vec::new(),
        agents: Vec::new(),
        commands: Vec::new(),
        pinned_tabs: Vec::new(),
    }
}

type SentMessages = std::sync::Arc<std::sync::Mutex<Vec<crate::protocol::ClientMessage>>>;
type TestFixture = (
    crate::client::ClientShellState,
    EndpointRegistry,
    SentMessages,
    SentMessages,
);

fn shell_and_registry() -> TestFixture {
    shell_and_registry_with_source_failure(false)
}

fn shell_and_registry_with_source_failure(source_fail_after_write: bool) -> TestFixture {
    shell_and_registry_with_config(&crate::config::Config::default(), source_fail_after_write)
}

fn shell_and_registry_with_config(
    config: &crate::config::Config,
    source_fail_after_write: bool,
) -> TestFixture {
    let mut shell =
        crate::client::ClientShellState::new(crate::client::ClientShellConfig::from_config(config));
    let profile = super::super::SavedSshEndpoint {
        id: super::super::ProfileId::parse("0123456789abcdef0123456789abcdef").unwrap(),
        label: "Remote".into(),
        target: "dev@example.com".into(),
        session: "main".into(),
        enabled: true,
    };
    let target = ClientEndpointId::Ssh(profile.id.clone());
    shell.set_endpoint_catalog(&[profile]);
    shell.set_snapshot(Box::new(test_snapshot("local-boot", 1)));
    shell.set_endpoint_status(&target, ClientEndpointStatus::Online);
    shell.set_endpoint_snapshot(&target, Box::new(test_snapshot("remote-boot", 1)));

    let local_sent = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let remote_sent = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut endpoints = EndpointRegistry::new(
        FakeTransport {
            sent: local_sent.clone(),
            fail_after_write: source_fail_after_write,
        },
        1,
        negotiation(),
    );
    endpoints.insert(
        target,
        FakeTransport {
            sent: remote_sent.clone(),
            fail_after_write: false,
        },
        7,
        negotiation(),
        false,
    );
    (shell, endpoints, local_sent, remote_sent)
}

fn surface_success(id: &str, active: bool, projection_revision: u64) -> Vec<u8> {
    serde_json::to_vec(&crate::api::schema::SuccessResponse {
        id: id.into(),
        result: crate::api::schema::ResponseResult::ClientShellSurfaceSet {
            active,
            projection_revision,
        },
    })
    .unwrap()
}

fn workspace_focus_success(id: &str, workspace_id: &str) -> Vec<u8> {
    serde_json::to_vec(&crate::api::schema::SuccessResponse {
        id: id.into(),
        result: crate::api::schema::ResponseResult::WorkspaceInfo {
            workspace: crate::api::schema::WorkspaceInfo {
                workspace_id: workspace_id.into(),
                number: 1,
                label: workspace_id.into(),
                focused: true,
                pane_count: 1,
                tab_count: 1,
                active_tab_id: "tab".into(),
                agent_status: crate::api::schema::AgentStatus::Unknown,
                work_status: None,
                tokens: Default::default(),
                worktree: None,
                orchestrator_mode: false,
                profiles: Vec::new(),
            },
        },
    })
    .unwrap()
}

fn failure(id: &str, message: &str) -> Vec<u8> {
    serde_json::to_vec(&crate::api::schema::ErrorResponse {
        id: id.into(),
        error: crate::api::schema::ErrorBody {
            code: "surface_rejected".into(),
            message: message.into(),
        },
    })
    .unwrap()
}

fn surface_set_active(message: &crate::protocol::ClientMessage) -> Option<bool> {
    let crate::protocol::ClientMessage::ClientShellEndpointRequest { request, .. } = message else {
        return None;
    };
    let request: crate::api::schema::Request = serde_json::from_str(request).ok()?;
    match request.method {
        crate::api::schema::Method::ClientShellSurfaceSet(params) => Some(params.active),
        _ => None,
    }
}

fn resize() -> crate::protocol::ClientMessage {
    crate::protocol::ClientMessage::ClientShellResize {
        cell_width_px: 8,
        cell_height_px: 16,
        surface_size: crate::protocol::ClientSurfaceSize { cols: 80, rows: 24 },
        pixel_mouse: false,
    }
}

fn surface(boot_id: &str, revision: u64, pane: &str) -> crate::protocol::PaneSurfaceFrame {
    crate::protocol::PaneSurfaceFrame {
        boot_id: boot_id.into(),
        projection_revision: revision,
        surface_revision: revision,
        frame: crate::protocol::FrameData {
            cells: Vec::new(),
            width: 80,
            height: 24,
            cursor: None,
            hyperlinks: Vec::new(),
            graphics: Vec::new(),
        },
        panes: vec![crate::protocol::PaneSurfacePane {
            pane_id: pane.into(),
            content_revision: revision,
            rect: crate::protocol::SurfaceRect {
                x: 0,
                y: 0,
                width: 80,
                height: 24,
            },
            inner_rect: crate::protocol::SurfaceRect {
                x: 0,
                y: 0,
                width: 80,
                height: 24,
            },
            scrollbar_rect: None,
            scroll: None,
            focused: true,
            mouse_reporting: false,
            sgr_pixel_mouse: false,
            alternate_screen_active: false,
            pixel_width: 0,
            pixel_height: 0,
        }],
        splits: Vec::new(),
        popup: None,
        graphics: Default::default(),
    }
}

fn machine() -> PendingEndpointActivation {
    PendingEndpointActivation {
        source: lease(ClientEndpointId::Local, 1, "local-boot"),
        source_available: true,
        target: lease(endpoint(), 7, "remote-boot"),
        focus: None,
        host_focused: true,
        source_resize: resize(),
        target_resize: resize(),
        phase: ActivationPhase::ActivatingTarget {
            request_id: "client-shell-surface:3:on".into(),
            acknowledged_revision: Some(1),
            focus_request_id: None,
            focus_request_target: None,
            focus_acknowledged: true,
            evidence: ActivationEvidence::default(),
        },
        deadline: Instant::now() + ACTIVATION_TIMEOUT,
        epoch: 3,
        next_focus_serial: 0,
        rollback_error: None,
        successor: None,
    }
}

#[test]
fn source_off_request_is_distinct_and_precedes_target_on_phase() {
    let activation = PendingEndpointActivation {
        source: lease(ClientEndpointId::Local, 1, "local-boot"),
        source_available: true,
        target: lease(endpoint(), 7, "remote-boot"),
        focus: None,
        host_focused: true,
        source_resize: resize(),
        target_resize: resize(),
        phase: ActivationPhase::ReleasingSource {
            request_id: "client-shell-surface:9:off".into(),
        },
        deadline: Instant::now() + ACTIVATION_TIMEOUT,
        epoch: 9,
        next_focus_serial: 0,
        rollback_error: None,
        successor: None,
    };
    assert!(activation.accepts_response(
        &ClientEndpointId::Local,
        1,
        "local-boot",
        "client-shell-surface:9:off"
    ));
    assert!(!activation.accepts_response(
        &endpoint(),
        7,
        "remote-boot",
        "client-shell-surface:9:on"
    ));
}

#[test]
fn active_source_requires_metadata_from_its_current_connection_generation() {
    let (mut shell, mut endpoints, local_sent, remote_sent) = shell_and_registry();
    shell.set_endpoint_snapshot_for_generation(
        &ClientEndpointId::Local,
        99,
        Box::new(test_snapshot("stale-local-boot", 1)),
    );

    let result = PendingEndpointActivation::begin(
        &shell,
        &mut endpoints,
        endpoint(),
        None,
        resize(),
        26,
        Instant::now(),
    );

    assert!(
        matches!(result, Err(ActivationBeginError::Preflight(message)) if message.contains("this connection"))
    );
    assert!(local_sent.lock().unwrap().is_empty());
    assert!(remote_sent.lock().unwrap().is_empty());
    assert!(endpoints.active_surface_available());
}

#[test]
fn observed_begin_write_failure_returns_recoverable_partial_activation() {
    let (shell, mut endpoints, local_sent, remote_sent) =
        shell_and_registry_with_source_failure(true);
    let result = PendingEndpointActivation::begin(
        &shell,
        &mut endpoints,
        endpoint(),
        None,
        resize(),
        10,
        Instant::now(),
    );

    let ActivationBeginError::Partial {
        mut activation,
        error,
    } = (match result {
        Err(error) => error,
        Ok(_) => panic!("an observed source write must return partial lifecycle state"),
    })
    else {
        panic!("an observed source write must return partial lifecycle state");
    };
    assert!(error.contains("focus revoke"));
    assert_eq!(
        local_sent.lock().unwrap().first(),
        Some(&crate::protocol::ClientMessage::ClientShellFocus { focused: false })
    );
    assert!(remote_sent.lock().unwrap().is_empty());
    assert!(matches!(
        activation.rollback(&mut endpoints, error, false),
        ActivationRollback::Unavailable(_)
    ));
}

#[test]
fn source_release_is_sent_and_acknowledged_before_target_activation() {
    let (shell, mut endpoints, local_sent, remote_sent) = shell_and_registry();
    let target = endpoint();
    let mut activation = PendingEndpointActivation::begin(
        &shell,
        &mut endpoints,
        target.clone(),
        None,
        resize(),
        11,
        Instant::now(),
    )
    .unwrap();
    let local = local_sent.lock().unwrap();
    assert_eq!(
        local.first(),
        Some(&crate::protocol::ClientMessage::ClientShellFocus { focused: false }),
        "source focus is revoked before source-off"
    );
    assert_eq!(
        local
            .as_slice()
            .iter()
            .filter_map(surface_set_active)
            .collect::<Vec<_>>(),
        vec![false],
        "the source is released first"
    );
    drop(local);
    assert!(remote_sent.lock().unwrap().is_empty());
    assert!(
        !endpoints.active_surface_available(),
        "pane input is blocked while frozen"
    );

    assert_eq!(
        activation.receive_response(
            &ClientEndpointId::Local,
            1,
            "client-shell-surface:11:off",
            &surface_success("client-shell-surface:11:off", false, 1),
            &mut endpoints,
        ),
        SurfaceActivationProgress::Pending
    );
    let remote = remote_sent.lock().unwrap();
    assert!(matches!(
        remote[0],
        crate::protocol::ClientMessage::ClientShellResize { .. }
    ));
    assert_eq!(
        remote.get(2),
        Some(&crate::protocol::ClientMessage::ClientShellFocus { focused: true })
    );
    assert_eq!(remote.get(1).and_then(surface_set_active), Some(true));
}

#[test]
fn activation_requires_an_exact_snapshot_surface_revision_pair() {
    let mut activation = machine();
    let target = endpoint();
    let snapshot = crate::protocol::ClientShellSnapshot {
        boot_id: "remote-boot".into(),
        revision: 2,
        config_diagnostic: None,
        product_announcement: None,
        update_available: None,
        update_install_command: String::new(),
        server_keybindings_toml: None,
        latest_release_notes_available: false,
        integration_updates_available: false,
        worktree_directory: String::new(),
        release_notes: None,
        focused_workspace_id: None,
        focused_tab_id: None,
        focused_pane_id: None,
        tab_bar_right: Vec::new(),
        tab_bar_right_separator: String::new(),
        agent_view_label: None,
        session_name: None,
        active_profile: "default".into(),
        agent_order: Vec::new(),
        workspaces: Vec::new(),
        tabs: Vec::new(),
        panes: Vec::new(),
        agents: Vec::new(),
        commands: Vec::new(),
        pinned_tabs: Vec::new(),
    };
    assert_eq!(
        activation.receive_snapshot(&target, 7, &snapshot),
        SurfaceActivationProgress::Pending
    );
    assert_eq!(
        activation.receive_surface(&target, 7, surface("remote-boot", 1, "pane")),
        SurfaceActivationProgress::Pending
    );
    assert_eq!(
        activation.receive_surface(&target, 7, surface("remote-boot", 2, "pane")),
        SurfaceActivationProgress::Ready
    );
}

#[test]
fn typed_target_ack_sets_a_floor_for_same_boot_activation_evidence() {
    let (shell, mut endpoints, _local_sent, _remote_sent) = shell_and_registry();
    let target = endpoint();
    let mut activation = PendingEndpointActivation::begin(
        &shell,
        &mut endpoints,
        target.clone(),
        None,
        resize(),
        16,
        Instant::now(),
    )
    .unwrap();
    let _ = activation.receive_response(
        &ClientEndpointId::Local,
        1,
        "client-shell-surface:16:off",
        &surface_success("client-shell-surface:16:off", false, 1),
        &mut endpoints,
    );
    assert_eq!(
        activation.receive_response(
            &target,
            7,
            "client-shell-surface:16:on",
            &surface_success("client-shell-surface:16:on", true, 4),
            &mut endpoints,
        ),
        SurfaceActivationProgress::Pending
    );
    assert_eq!(
        activation.receive_snapshot(&target, 7, &test_snapshot("remote-boot", 3)),
        SurfaceActivationProgress::Pending
    );
    assert_eq!(
        activation.receive_surface(&target, 7, surface("remote-boot", 3, "pane")),
        SurfaceActivationProgress::Pending,
        "a delayed same-boot surface below the acknowledgement floor is not evidence"
    );
    assert_eq!(
        activation.receive_snapshot(&target, 7, &test_snapshot("remote-boot", 4)),
        SurfaceActivationProgress::Pending
    );
    assert_eq!(
        activation.receive_surface(&target, 7, surface("remote-boot", 4, "pane")),
        SurfaceActivationProgress::Ready
    );
}

#[test]
fn stale_generation_and_boot_are_not_activation_evidence() {
    let mut activation = machine();
    assert_eq!(
        activation.receive_surface(&endpoint(), 6, surface("remote-boot", 1, "pane")),
        SurfaceActivationProgress::Stale
    );
    assert_eq!(
        activation.receive_surface(&endpoint(), 7, surface("old-boot", 1, "pane")),
        SurfaceActivationProgress::Stale
    );
}

#[test]
fn stale_response_boot_is_not_consumed() {
    let mut activation = machine();
    let (_shell, mut endpoints, _local_sent, _remote_sent) = shell_and_registry();
    assert_eq!(
        activation.receive_response_for_boot(
            &endpoint(),
            7,
            "old-boot",
            "client-shell-surface:3:on",
            &surface_success("client-shell-surface:3:on", true, 2),
            &mut endpoints,
        ),
        SurfaceActivationProgress::Stale
    );
}

#[test]
fn same_target_retarget_is_latest_wins() {
    let (shell, mut endpoints, _local_sent, remote_sent) = shell_and_registry();
    let target = endpoint();
    let mut activation = PendingEndpointActivation::begin(
        &shell,
        &mut endpoints,
        target.clone(),
        Some(crate::client::shell::ClientEndpointFocusTarget::Workspace(
            "old".into(),
        )),
        resize(),
        12,
        Instant::now(),
    )
    .unwrap();
    let _ = activation.receive_response(
        &ClientEndpointId::Local,
        1,
        "client-shell-surface:12:off",
        &surface_success("client-shell-surface:12:off", false, 1),
        &mut endpoints,
    );
    let old_focus = remote_sent
        .lock()
        .unwrap()
        .iter()
        .find_map(|message| match message {
            crate::protocol::ClientMessage::ClientShellEndpointRequest { request, .. }
                if surface_set_active(message).is_none() =>
            {
                Some(
                    serde_json::from_str::<crate::api::schema::Request>(request)
                        .unwrap()
                        .id,
                )
            }
            _ => None,
        })
        .unwrap();
    let sent_before_retarget = remote_sent.lock().unwrap().len();
    activation
        .retarget(
            Some(crate::client::shell::ClientEndpointFocusTarget::Workspace(
                "new".into(),
            )),
            &mut endpoints,
        )
        .unwrap();
    assert_eq!(remote_sent.lock().unwrap().len(), sent_before_retarget);
    assert!(activation.accepts_response(&target, 7, "remote-boot", &old_focus));
    assert_eq!(
        activation.receive_response(
            &target,
            7,
            &old_focus,
            &workspace_focus_success(&old_focus, "old"),
            &mut endpoints,
        ),
        SurfaceActivationProgress::Pending
    );
    let latest_focus = remote_sent
        .lock()
        .unwrap()
        .last()
        .and_then(|message| match message {
            crate::protocol::ClientMessage::ClientShellEndpointRequest { request, .. } => Some(
                serde_json::from_str::<crate::api::schema::Request>(request)
                    .unwrap()
                    .id,
            ),
            _ => None,
        })
        .unwrap();
    assert_ne!(latest_focus, old_focus);
    assert!(activation.accepts_response(&target, 7, "remote-boot", &latest_focus));
}

#[test]
fn latest_host_focus_is_replayed_to_the_eventual_target() {
    let (shell, mut endpoints, _local_sent, remote_sent) = shell_and_registry();
    let mut activation = PendingEndpointActivation::begin(
        &shell,
        &mut endpoints,
        endpoint(),
        None,
        resize(),
        25,
        Instant::now(),
    )
    .unwrap();
    activation.update_host_focus(false, &mut endpoints).unwrap();
    assert!(remote_sent.lock().unwrap().is_empty());

    let _ = activation.receive_response(
        &ClientEndpointId::Local,
        1,
        "client-shell-surface:25:off",
        &surface_success("client-shell-surface:25:off", false, 1),
        &mut endpoints,
    );
    assert_eq!(
        remote_sent.lock().unwrap().get(2),
        Some(&crate::protocol::ClientMessage::ClientShellFocus { focused: false })
    );

    activation.update_host_focus(true, &mut endpoints).unwrap();
    assert_eq!(
        remote_sent.lock().unwrap().last(),
        Some(&crate::protocol::ClientMessage::ClientShellFocus { focused: true })
    );
}

#[test]
fn host_focus_change_restarts_an_issued_presentation_effects_fence() {
    let (_shell, mut endpoints, _local_sent, remote_sent) = shell_and_registry();
    let mut activation = machine();
    activation.phase = ActivationPhase::AwaitingPresentationEffects {
        lease: lease(endpoint(), 7, "remote-boot"),
        token: "old-token".into(),
        ready: false,
        completion: Box::new(ActivationCompletion::Activated),
    };

    activation.update_host_focus(false, &mut endpoints).unwrap();

    assert!(matches!(
        activation.phase,
        ActivationPhase::SynchronizingPresentation { .. }
    ));
    assert_eq!(
        activation.receive_presentation_effects_ready(&endpoint(), 7, "old-token"),
        SurfaceActivationProgress::Stale
    );
    assert_eq!(
        remote_sent
            .lock()
            .unwrap()
            .iter()
            .filter_map(surface_set_active)
            .collect::<Vec<_>>(),
        vec![true]
    );
}

#[test]
fn source_release_rejection_restores_the_source_coherently() {
    let (shell, mut endpoints, local_sent, _remote_sent) = shell_and_registry();
    let mut activation = PendingEndpointActivation::begin(
        &shell,
        &mut endpoints,
        endpoint(),
        None,
        resize(),
        13,
        Instant::now(),
    )
    .unwrap();
    assert_eq!(
        activation.receive_response(
            &ClientEndpointId::Local,
            1,
            "client-shell-surface:13:off",
            &failure("client-shell-surface:13:off", "source rejected release"),
            &mut endpoints,
        ),
        SurfaceActivationProgress::Rejected {
            message: "source rejected release".into(),
            source_release_rejected: true,
        }
    );
    assert_eq!(
        activation.rollback(&mut endpoints, "source rejected release".into(), true),
        ActivationRollback::Pending
    );
    assert_eq!(endpoints.active_id(), &ClientEndpointId::Local);
    assert!(!endpoints.active_surface_available());
    assert_eq!(
        local_sent
            .lock()
            .unwrap()
            .iter()
            .filter_map(surface_set_active)
            .collect::<Vec<_>>(),
        vec![false, true]
    );
}

#[test]
fn source_release_timeout_starts_an_acknowledged_source_restore() {
    let (shell, mut endpoints, local_sent, _remote_sent) = shell_and_registry();
    let mut activation = PendingEndpointActivation::begin(
        &shell,
        &mut endpoints,
        endpoint(),
        None,
        resize(),
        14,
        Instant::now(),
    )
    .unwrap();
    assert_eq!(
        activation.rollback(&mut endpoints, "source release timed out".into(), false),
        ActivationRollback::Pending
    );
    let sent = local_sent.lock().unwrap();
    assert_eq!(
        sent.iter()
            .filter_map(surface_set_active)
            .collect::<Vec<_>>(),
        vec![false, true]
    );
    assert!(activation.accepts_response(
        &ClientEndpointId::Local,
        1,
        "local-boot",
        "client-shell-surface:14:rollback-source-on"
    ));
}

#[test]
fn resize_invalidates_already_recorded_surface_evidence() {
    let (_shell, mut endpoints, _local_sent, _remote_sent) = shell_and_registry();
    let mut activation = machine();
    assert_eq!(
        activation.receive_snapshot(&endpoint(), 7, &test_snapshot("remote-boot", 1)),
        SurfaceActivationProgress::Pending
    );
    assert_eq!(
        activation.receive_surface(&endpoint(), 7, surface("remote-boot", 1, "pane")),
        SurfaceActivationProgress::Ready
    );
    let resize = crate::protocol::ClientMessage::ClientShellResize {
        cell_width_px: 9,
        cell_height_px: 17,
        surface_size: crate::protocol::ClientSurfaceSize {
            cols: 100,
            rows: 30,
        },
        pixel_mouse: true,
    };
    activation.update_resize(resize, &mut endpoints).unwrap();
    assert_eq!(activation.progress(), SurfaceActivationProgress::Pending);
}

#[test]
fn resize_during_activation_reaches_the_pending_target() {
    let (shell, mut endpoints, _local_sent, remote_sent) = shell_and_registry();
    let mut activation = PendingEndpointActivation::begin(
        &shell,
        &mut endpoints,
        endpoint(),
        None,
        resize(),
        15,
        Instant::now(),
    )
    .unwrap();
    let _ = activation.receive_response(
        &ClientEndpointId::Local,
        1,
        "client-shell-surface:15:off",
        &surface_success("client-shell-surface:15:off", false, 1),
        &mut endpoints,
    );
    let resized = crate::protocol::ClientMessage::ClientShellResize {
        cell_width_px: 9,
        cell_height_px: 17,
        surface_size: crate::protocol::ClientSurfaceSize {
            cols: 100,
            rows: 30,
        },
        pixel_mouse: true,
    };
    activation
        .update_resize(resized.clone(), &mut endpoints)
        .unwrap();
    assert_eq!(remote_sent.lock().unwrap().last(), Some(&resized));
    assert_eq!(
        activation.receive_surface(&endpoint(), 7, surface("remote-boot", 1, "pane")),
        SurfaceActivationProgress::Pending,
        "a surface for the prior geometry cannot commit"
    );
}

fn sent_surface_widths(sent: &SentMessages) -> Vec<u16> {
    sent.lock()
        .unwrap()
        .iter()
        .filter_map(|message| match message {
            crate::protocol::ClientMessage::ClientShellResize { surface_size, .. } => {
                Some(surface_size.cols)
            }
            _ => None,
        })
        .collect()
}

const HANDOFF_SIZE: (u16, u16) = (110, 30);
const HANDOFF_CELL: (u32, u32) = (8, 16);
// The default layout at 110x30: the open detail panel takes 46 of the pane's 84 columns.
const HANDOFF_WIDE: u16 = 84;
const HANDOFF_NARROW: u16 = 38;

fn factory_overlay(
    boot_id: &str,
    revision: u64,
    present: bool,
) -> crate::protocol::endpoint::EndpointFactoryOverlay {
    crate::protocol::endpoint::EndpointFactoryOverlay {
        boot_id: boot_id.into(),
        revision,
        overlay: present.then(crate::factory_overlay::FactoryOverlay::default),
    }
}

fn factory_config() -> crate::config::Config {
    let mut config = crate::config::Config::default();
    config.ui.factory.enabled = true;
    config.keys.toggle_factory_overview = crate::config::BindingConfig::one("alt+o");
    config
}

/// A (Local) shows an overlay with the detail panel open and B (remote) has none; an A to B
/// handoff has released A and sent B its target-on at B's own width.
fn handoff_from_open_panel() -> (
    crate::client::ClientShellState,
    EndpointRegistry,
    SentMessages,
    SentMessages,
    PendingEndpointActivation,
) {
    use crate::client::shell_runtime::{
        apply_client_shell_factory_overlay, client_shell_activation_resize,
    };
    let (mut shell, mut endpoints, local_sent, remote_sent) =
        shell_and_registry_with_config(&factory_config(), false);
    let local = ClientEndpointId::Local;
    shell.set_endpoint_snapshot_for_generation(&local, 1, Box::new(test_snapshot("local-boot", 1)));
    shell.set_endpoint_snapshot_for_generation(
        &endpoint(),
        7,
        Box::new(test_snapshot("remote-boot", 1)),
    );
    let (frame, _) = apply_client_shell_factory_overlay(
        &mut shell,
        &local,
        1,
        factory_overlay("local-boot", 1, true),
        HANDOFF_SIZE,
        HANDOFF_CELL,
        false,
    );
    assert!(frame.is_some());
    shell.handle_raw_events(vec![crate::raw_input::RawInputEvent::Key(
        crate::input::TerminalKey::new(
            crossterm::event::KeyCode::Char('o'),
            crossterm::event::KeyModifiers::ALT,
        ),
    )]);
    assert_eq!(
        shell.surface_size(HANDOFF_SIZE.0, HANDOFF_SIZE.1).cols,
        HANDOFF_NARROW
    );
    local_sent.lock().unwrap().clear();

    let resize = client_shell_activation_resize(
        &shell,
        endpoints.active_id(),
        &endpoint(),
        HANDOFF_SIZE,
        HANDOFF_CELL,
        false,
    );
    let mut activation = PendingEndpointActivation::begin(
        &shell,
        &mut endpoints,
        endpoint(),
        None,
        resize,
        30,
        Instant::now(),
    )
    .unwrap();
    let _ = activation.receive_response(
        &local,
        1,
        "client-shell-surface:30:off",
        &surface_success("client-shell-surface:30:off", false, 1),
        &mut endpoints,
    );
    assert_eq!(
        sent_surface_widths(&remote_sent),
        vec![HANDOFF_WIDE],
        "B is activated at its own width, not A's panel width"
    );
    (shell, endpoints, local_sent, remote_sent, activation)
}

/// Regression: an overlay change on one endpoint during an A to B handoff was laid out with A's
/// geometry and written to B. Each endpoint must be sized for its own overlay, and A's latest
/// geometry must survive for rollback.
#[test]
fn overlay_changes_during_a_handoff_size_each_endpoint_for_its_own_layout() {
    use crate::client::shell_runtime::{apply_client_shell_factory_overlay, route_endpoint_resize};
    const SIZE: (u16, u16) = HANDOFF_SIZE;
    const CELL: (u32, u32) = HANDOFF_CELL;
    const WIDE: u16 = HANDOFF_WIDE;
    const NARROW: u16 = HANDOFF_NARROW;
    let (mut shell, mut endpoints, local_sent, remote_sent, mut activation) =
        handoff_from_open_panel();
    let local = ClientEndpointId::Local;
    let overlay = factory_overlay;

    // A drops its overlay while B is activating: A widens, B's layout is unchanged.
    let (_, resize) = apply_client_shell_factory_overlay(
        &mut shell,
        &local,
        1,
        overlay("local-boot", 2, false),
        SIZE,
        CELL,
        false,
    );
    let resize = resize.expect("A's pane width changed");
    assert!(route_endpoint_resize(&mut endpoints, Some(&mut activation), &local, resize).is_ok());
    assert_eq!(
        sent_surface_widths(&remote_sent),
        vec![WIDE],
        "A's geometry must not reach B"
    );
    assert!(sent_surface_widths(&local_sent).is_empty());

    // B's own overlay arrives while it is the pending target: B narrows to its panel layout.
    let (_, resize) = apply_client_shell_factory_overlay(
        &mut shell,
        &endpoint(),
        7,
        overlay("remote-boot", 1, true),
        SIZE,
        CELL,
        false,
    );
    let resize = resize.expect("B's pane width changed");
    assert!(
        route_endpoint_resize(&mut endpoints, Some(&mut activation), &endpoint(), resize).is_ok()
    );
    assert_eq!(sent_surface_widths(&remote_sent), vec![WIDE, NARROW]);

    // Rolling back restores A at A's latest width, not B's.
    assert_eq!(
        activation.rollback(&mut endpoints, "target failed".into(), false),
        ActivationRollback::Pending
    );
    let _ = activation.receive_response(
        &endpoint(),
        7,
        "client-shell-surface:30:rollback-target-off",
        &surface_success("client-shell-surface:30:rollback-target-off", false, 2),
        &mut endpoints,
    );
    assert_eq!(sent_surface_widths(&local_sent), vec![WIDE]);
}

/// Regression: a config reload that changes only the source's layout while B is the committed,
/// still-synchronizing endpoint must refresh A's rollback geometry. B kept 84 columns, so the
/// reload skipped the handoff and a later B disconnect restored A at the stale 38 instead of 34.
#[test]
fn config_reload_during_a_handoff_refreshes_the_rollback_geometry() {
    use crate::client::ClientState;
    let (mut shell, mut endpoints, local_sent, remote_sent, mut activation) =
        handoff_from_open_panel();
    let target_rows = shell
        .surface_size_for_endpoint(&endpoint(), HANDOFF_SIZE.0, HANDOFF_SIZE.1)
        .rows;
    assert_eq!(
        activation.receive_response(
            &endpoint(),
            7,
            "client-shell-surface:30:on",
            &surface_success("client-shell-surface:30:on", true, 2),
            &mut endpoints,
        ),
        SurfaceActivationProgress::Pending
    );
    let snapshot = test_snapshot("remote-boot", 2);
    shell.set_endpoint_snapshot_for_generation(&endpoint(), 7, Box::new(snapshot.clone()));
    let _ = activation.receive_snapshot(&endpoint(), 7, &snapshot);
    let mut target_surface = surface("remote-boot", 2, "pane");
    target_surface.frame.width = HANDOFF_WIDE;
    target_surface.frame.height = target_rows;
    assert_eq!(
        activation.receive_surface(&endpoint(), 7, target_surface),
        SurfaceActivationProgress::Ready
    );
    assert!(matches!(
        activation.complete(&mut shell, &mut endpoints),
        Ok(ActivationCompletion::AwaitingPresentationSync { .. })
    ));
    assert_eq!(endpoints.active_id(), &endpoint());

    let _guard = crate::config::test_config_env_lock().lock().unwrap();
    let dir = std::env::temp_dir().join(format!(
        "herdr-reload-handoff-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config.toml");
    std::fs::write(
        &path,
        "[ui.factory]\nenabled = true\npanel_width = 50\n\n[keys]\ntoggle_factory_overview = \"alt+o\"\n",
    )
    .unwrap();
    std::env::set_var(crate::config::CONFIG_PATH_ENV_VAR, &path);
    let mut state = ClientState::test_new();
    state.reported_size = HANDOFF_SIZE;
    state.reported_cell_size = HANDOFF_CELL;
    state.shell = Some(shell);
    // As `install_pending_activation` does, so the reload composes without painting the terminal.
    state.freeze_presentation();
    let mut pending = Some(activation);
    let result = crate::client::config_reload::apply_reload(
        &mut state,
        &mut endpoints,
        &mut pending,
        &std::sync::atomic::AtomicBool::new(false),
        &std::sync::atomic::AtomicBool::new(false),
        &mut crate::platform::RealPrefixInputSource::default(),
    );
    std::env::remove_var(crate::config::CONFIG_PATH_ENV_VAR);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(result.is_ok());
    assert_eq!(
        sent_surface_widths(&remote_sent),
        vec![HANDOFF_WIDE],
        "B's layout did not change, so B gets no resize"
    );
    assert!(sent_surface_widths(&local_sent).is_empty());

    let mut activation = pending.expect("the reload keeps the handoff pending");
    assert_eq!(
        activation.endpoint_disconnected(&mut endpoints, &endpoint(), "B disconnected".into()),
        ActivationRollback::Pending
    );
    assert_eq!(
        sent_surface_widths(&local_sent),
        vec![34],
        "A is restored at its width under the reloaded 50-column panel"
    );
}

#[test]
fn rapid_a_to_b_to_a_restores_source_before_a_fresh_latest_epoch() {
    let (mut shell, mut endpoints, local_sent, remote_sent) = shell_and_registry();
    let mut activation = PendingEndpointActivation::begin(
        &shell,
        &mut endpoints,
        endpoint(),
        None,
        resize(),
        20,
        Instant::now(),
    )
    .unwrap();
    let _ = activation.receive_response(
        &ClientEndpointId::Local,
        1,
        "client-shell-surface:20:off",
        &surface_success("client-shell-surface:20:off", false, 1),
        &mut endpoints,
    );

    // The latest A-qualified target replaces B while B may already have accepted target-on.
    assert_eq!(
        activation.supersede(
            ClientEndpointId::Local,
            Some(crate::client::shell::ClientEndpointFocusTarget::Pane(
                "local-pane".into(),
            )),
            &mut endpoints,
        ),
        ActivationRollback::Pending
    );
    assert_eq!(
        remote_sent
            .lock()
            .unwrap()
            .iter()
            .filter_map(surface_set_active)
            .collect::<Vec<_>>(),
        vec![true, false],
        "B is released before A can be restored"
    );
    assert_eq!(
        activation.receive_surface(&endpoint(), 7, surface("remote-boot", 2, "pane")),
        SurfaceActivationProgress::Stale,
        "delayed B activation evidence cannot satisfy A restoration"
    );
    let _ = activation.receive_response(
        &endpoint(),
        7,
        "client-shell-surface:20:rollback-target-off",
        &surface_success("client-shell-surface:20:rollback-target-off", false, 1),
        &mut endpoints,
    );
    assert_eq!(
        local_sent
            .lock()
            .unwrap()
            .iter()
            .filter_map(surface_set_active)
            .collect::<Vec<_>>(),
        vec![false, true],
        "source restoration is acknowledged rather than racing target ownership"
    );
    let _ = activation.receive_response(
        &ClientEndpointId::Local,
        1,
        "client-shell-surface:20:rollback-source-on",
        &surface_success("client-shell-surface:20:rollback-source-on", true, 2),
        &mut endpoints,
    );
    let local_snapshot = test_snapshot("local-boot", 2);
    shell.set_snapshot(Box::new(local_snapshot.clone()));
    assert_eq!(
        activation.receive_snapshot(&ClientEndpointId::Local, 1, &local_snapshot),
        SurfaceActivationProgress::Pending
    );
    assert_eq!(
        activation.receive_surface(
            &ClientEndpointId::Local,
            1,
            surface("local-boot", 2, "pane")
        ),
        SurfaceActivationProgress::Ready
    );
    assert!(matches!(
        activation.complete(&mut shell, &mut endpoints),
        Ok(ActivationCompletion::AwaitingPresentationSync {
            endpoint: ClientEndpointId::Local,
            ..
        })
    ));
    let _ = activation.receive_response(
        &ClientEndpointId::Local,
        1,
        "client-shell-surface:20:presentation-sync",
        &surface_success("client-shell-surface:20:presentation-sync", true, 3),
        &mut endpoints,
    );
    let sync_snapshot = test_snapshot("local-boot", 3);
    shell.set_endpoint_snapshot_for_generation(
        &ClientEndpointId::Local,
        1,
        Box::new(sync_snapshot.clone()),
    );
    let _ = activation.receive_snapshot(&ClientEndpointId::Local, 1, &sync_snapshot);
    assert_eq!(
        activation.receive_surface(
            &ClientEndpointId::Local,
            1,
            surface("local-boot", 3, "pane")
        ),
        SurfaceActivationProgress::Ready
    );
    assert_eq!(
        activation.complete(&mut shell, &mut endpoints),
        Ok(ActivationCompletion::AwaitingPresentationEffects)
    );
    assert_eq!(
        activation.receive_presentation_effects_ready(
            &ClientEndpointId::Local,
            1,
            "20:1:local-boot"
        ),
        SurfaceActivationProgress::Ready
    );
    assert!(matches!(
        activation.complete(&mut shell, &mut endpoints),
        Ok(ActivationCompletion::RestoredSource {
            successor: Some(EndpointActivationIntent {
                endpoint_id: ClientEndpointId::Local,
                ..
            }),
            ..
        })
    ));
    assert_eq!(endpoints.active_id(), &ClientEndpointId::Local);

    // Runtime queues this successor with force=true, so even source==target receives a new
    // activation epoch only after restoration committed.
    let _fresh_epoch = PendingEndpointActivation::begin(
        &shell,
        &mut endpoints,
        ClientEndpointId::Local,
        Some(crate::client::shell::ClientEndpointFocusTarget::Pane(
            "local-pane".into(),
        )),
        resize(),
        21,
        Instant::now(),
    )
    .unwrap();
    assert!(local_sent.lock().unwrap().iter().any(|message| {
        matches!(
            message,
            crate::protocol::ClientMessage::ClientShellEndpointRequest { request, .. }
                if serde_json::from_str::<crate::api::schema::Request>(request)
                    .is_ok_and(|request| request.id == "client-shell-surface:21:on")
        )
    }));
}

#[test]
fn local_activation_does_not_wait_for_a_disconnected_stalled_or_failed_remote() {
    for source_state in ["disconnected", "stalled", "write-failed"] {
        local_escape(source_state);
    }
}

fn local_escape(source_state: &str) {
    let (mut shell, mut endpoints, local_sent, remote_sent) = shell_and_registry();
    let disconnected = endpoint();
    endpoints.set_surface_active(&ClientEndpointId::Local, false);
    endpoints.set_surface_active(&disconnected, true);
    assert!(endpoints.set_active(&disconnected));
    assert!(shell.activate_endpoint_projection(&disconnected));
    match source_state {
        "disconnected" => endpoints.disconnect(&disconnected),
        "write-failed" => endpoints.insert(
            disconnected.clone(),
            FakeTransport {
                sent: remote_sent,
                fail_after_write: true,
            },
            7,
            negotiation(),
            true,
        ),
        _ => {}
    }

    let mut activation = PendingEndpointActivation::begin(
        &shell,
        &mut endpoints,
        ClientEndpointId::Local,
        None,
        resize(),
        22,
        Instant::now(),
    )
    .unwrap();
    let local_activations = local_sent
        .lock()
        .unwrap()
        .iter()
        .filter_map(surface_set_active)
        .collect::<Vec<_>>();
    assert_eq!(local_activations, vec![true], "Local must not wait for SSH");
    assert!(!endpoints.active_surface_available());
    assert_eq!(endpoints.active_id(), &disconnected);
    assert_eq!(
        activation.receive_response(
            &disconnected,
            7,
            "client-shell-surface:22:off",
            &surface_success("client-shell-surface:22:off", false, 1),
            &mut endpoints,
        ),
        SurfaceActivationProgress::Stale
    );
    assert_eq!(
        activation.receive_surface(&disconnected, 7, surface("remote-boot", 2, "stale")),
        SurfaceActivationProgress::Stale
    );

    assert_eq!(
        activation.receive_response(
            &ClientEndpointId::Local,
            1,
            "client-shell-surface:22:on",
            &surface_success("client-shell-surface:22:on", true, 2),
            &mut endpoints,
        ),
        SurfaceActivationProgress::Pending
    );
    let snapshot = test_snapshot("local-boot", 2);
    shell.set_endpoint_snapshot_for_generation(
        &ClientEndpointId::Local,
        1,
        Box::new(snapshot.clone()),
    );
    assert_eq!(
        activation.receive_snapshot(&ClientEndpointId::Local, 1, &snapshot),
        SurfaceActivationProgress::Pending
    );
    assert_eq!(
        activation.receive_surface(
            &ClientEndpointId::Local,
            1,
            surface("local-boot", 2, "pane")
        ),
        SurfaceActivationProgress::Ready
    );
    assert!(matches!(
        activation.complete(&mut shell, &mut endpoints),
        Ok(ActivationCompletion::AwaitingPresentationSync {
            previous,
            endpoint: ClientEndpointId::Local,
        }) if previous == disconnected
    ));
    let _ = activation.receive_response(
        &ClientEndpointId::Local,
        1,
        "client-shell-surface:22:presentation-sync",
        &surface_success("client-shell-surface:22:presentation-sync", true, 3),
        &mut endpoints,
    );
    let sync_snapshot = test_snapshot("local-boot", 3);
    shell.set_endpoint_snapshot_for_generation(
        &ClientEndpointId::Local,
        1,
        Box::new(sync_snapshot.clone()),
    );
    let _ = activation.receive_snapshot(&ClientEndpointId::Local, 1, &sync_snapshot);
    assert_eq!(
        activation.receive_surface(
            &ClientEndpointId::Local,
            1,
            surface("local-boot", 3, "pane")
        ),
        SurfaceActivationProgress::Ready
    );
    assert_eq!(
        activation.complete(&mut shell, &mut endpoints),
        Ok(ActivationCompletion::AwaitingPresentationEffects)
    );
    assert_eq!(
        activation.receive_presentation_effects_ready(
            &ClientEndpointId::Local,
            1,
            "22:1:local-boot"
        ),
        SurfaceActivationProgress::Ready
    );
    assert_eq!(
        activation.complete(&mut shell, &mut endpoints),
        Ok(ActivationCompletion::Activated)
    );
    assert_eq!(endpoints.active_id(), &ClientEndpointId::Local);
    assert_ne!(endpoints.active_id(), &disconnected);
    assert!(
        !endpoints.active_surface_available(),
        "runtime opens input only after the effects fence"
    );
}

#[test]
fn local_selection_abandons_every_unfinished_remote_handoff_phase() {
    use crate::client::{
        endpoint_commands::EndpointCommands, shell_runtime::begin_endpoint_activation, ClientState,
    };
    for phase in ["release", "target", "rollback", "restore"] {
        let (shell, mut endpoints, local_sent, _remote_sent) = shell_and_registry();
        let mut abandoned = PendingEndpointActivation::begin(
            &shell,
            &mut endpoints,
            endpoint(),
            None,
            resize(),
            30,
            Instant::now(),
        )
        .unwrap();
        if phase != "release" {
            abandoned.receive_response(
                &ClientEndpointId::Local,
                1,
                "client-shell-surface:30:off",
                &surface_success("client-shell-surface:30:off", false, 1),
                &mut endpoints,
            );
        }
        if matches!(phase, "rollback" | "restore") {
            assert_eq!(
                abandoned.rollback(&mut endpoints, "cancel".into(), false),
                ActivationRollback::Pending
            );
        }
        if phase == "restore" {
            abandoned.receive_response(
                &endpoint(),
                7,
                "client-shell-surface:30:rollback-target-off",
                &surface_success("client-shell-surface:30:rollback-target-off", false, 1),
                &mut endpoints,
            );
        }
        local_sent.lock().unwrap().clear();
        let mut state = ClientState::test_new();
        state.shell = Some(shell);
        let mut commands = EndpointCommands::default();
        let mut pending = Some(abandoned);
        let mut serial = 31;
        let mut scheduled = None;
        for _ in 0..2 {
            begin_endpoint_activation(
                &mut state,
                &mut endpoints,
                &mut commands,
                &mut pending,
                &mut serial,
                ClientEndpointId::Local,
                None,
                false,
                Instant::now(),
                &mut scheduled,
            )
            .unwrap();
        }
        assert_eq!(serial, 32, "repeated Local selection must coalesce");
        let local = pending.as_mut().unwrap();
        assert_eq!(local.target(), &ClientEndpointId::Local);
        assert!(!endpoints.active_surface_available());
        assert!(state.presentation_frozen);
        let activations = local_sent
            .lock()
            .unwrap()
            .iter()
            .filter_map(surface_set_active)
            .collect::<Vec<_>>();
        assert_eq!(activations, vec![false, true], "phase: {phase}");
        assert!(!local.accepts_response(
            &ClientEndpointId::Local,
            1,
            "local-boot",
            "client-shell-surface:30:off"
        ));
        assert_eq!(
            local.receive_surface(&endpoint(), 7, surface("remote-boot", 2, "stale")),
            SurfaceActivationProgress::Stale
        );
    }
}

#[test]
fn local_selection_waits_for_fresh_metadata_without_abandoning_remote() {
    use crate::client::{
        endpoint_commands::EndpointCommands,
        shell_runtime::{begin_endpoint_activation, take_ready_local_activation},
        ClientLoopEvent, ClientState,
    };
    for replaced_generation in [false, true] {
        let (mut shell, mut endpoints, local_sent, remote_sent) = shell_and_registry();
        shell.set_endpoint_snapshot_for_generation(
            &ClientEndpointId::Local,
            1,
            Box::new(test_snapshot("local-boot", 1)),
        );
        let mut state = ClientState::test_new();
        state.shell = Some(shell);
        let mut commands = EndpointCommands::default();
        let mut pending = None;
        let mut serial = 40;
        let mut scheduled = None;
        begin_endpoint_activation(
            &mut state,
            &mut endpoints,
            &mut commands,
            &mut pending,
            &mut serial,
            endpoint(),
            None,
            false,
            Instant::now(),
            &mut scheduled,
        )
        .unwrap();
        if replaced_generation {
            endpoints.insert(
                ClientEndpointId::Local,
                FakeTransport {
                    sent: local_sent.clone(),
                    fail_after_write: false,
                },
                2,
                negotiation(),
                false,
            );
        } else {
            endpoints.disconnect(&ClientEndpointId::Local);
        }
        begin_endpoint_activation(
            &mut state,
            &mut endpoints,
            &mut commands,
            &mut pending,
            &mut serial,
            ClientEndpointId::Local,
            Some(crate::client::shell::ClientEndpointFocusTarget::Workspace(
                "selected-local".into(),
            )),
            false,
            Instant::now(),
            &mut scheduled,
        )
        .unwrap();
        assert_eq!(
            pending.as_ref().map(PendingEndpointActivation::target),
            Some(&endpoint())
        );
        assert!(remote_sent.lock().unwrap().is_empty());
        assert_eq!(serial, 41);
        assert!(state.deferred_local_activation.is_some());
        assert!(take_ready_local_activation(&mut state, &endpoints).is_none());
        if !replaced_generation {
            endpoints.insert(
                ClientEndpointId::Local,
                FakeTransport {
                    sent: local_sent,
                    fail_after_write: false,
                },
                2,
                negotiation(),
                false,
            );
        }
        assert!(take_ready_local_activation(&mut state, &endpoints).is_none());
        state
            .shell
            .as_mut()
            .unwrap()
            .cache_endpoint_snapshot_inactive_for_generation(
                &ClientEndpointId::Local,
                2,
                Box::new(test_snapshot("local-boot", 1)),
            );
        let event = take_ready_local_activation(&mut state, &endpoints).unwrap();
        let ClientLoopEvent::ActivateEndpoint {
            endpoint_id,
            target,
            force,
        } = event
        else {
            panic!("expected retained Local selection");
        };
        assert_eq!(endpoint_id, ClientEndpointId::Local);
        assert_eq!(
            target,
            Some(crate::client::shell::ClientEndpointFocusTarget::Workspace(
                "selected-local".into()
            ))
        );
        begin_endpoint_activation(
            &mut state,
            &mut endpoints,
            &mut commands,
            &mut pending,
            &mut serial,
            endpoint_id,
            target,
            force,
            Instant::now(),
            &mut scheduled,
        )
        .unwrap();
        assert_eq!(pending.as_ref().unwrap().target(), &ClientEndpointId::Local);
        assert_eq!(serial, 42);
        assert!(!endpoints.active_surface_available());
        assert!(state.deferred_local_activation.is_none());
    }
}

#[test]
fn newer_remote_selection_cancels_deferred_local_selection() {
    use crate::client::{
        endpoint_commands::EndpointCommands, shell_runtime::begin_endpoint_activation, ClientState,
    };
    let (shell, mut endpoints, _, _) = shell_and_registry();
    let mut state = ClientState::test_new();
    state.shell = Some(shell);
    let mut commands = EndpointCommands::default();
    let mut pending = None;
    let mut serial = 50;
    let mut scheduled = None;
    endpoints.disconnect(&ClientEndpointId::Local);
    for endpoint_id in [ClientEndpointId::Local, endpoint()] {
        begin_endpoint_activation(
            &mut state,
            &mut endpoints,
            &mut commands,
            &mut pending,
            &mut serial,
            endpoint_id.clone(),
            None,
            false,
            Instant::now(),
            &mut scheduled,
        )
        .unwrap();
        assert_eq!(
            state.deferred_local_activation.is_some(),
            endpoint_id.is_local()
        );
    }
    assert_eq!(pending.as_ref().unwrap().target(), &endpoint());
}

#[test]
fn rollback_keeps_the_latest_intent_even_when_it_returns_to_the_target() {
    let (shell, mut endpoints, _local_sent, _remote_sent) = shell_and_registry();
    let target = endpoint();
    let mut activation = PendingEndpointActivation::begin(
        &shell,
        &mut endpoints,
        target.clone(),
        None,
        resize(),
        23,
        Instant::now(),
    )
    .unwrap();
    let _ = activation.receive_response(
        &ClientEndpointId::Local,
        1,
        "client-shell-surface:23:off",
        &surface_success("client-shell-surface:23:off", false, 1),
        &mut endpoints,
    );
    assert_eq!(
        activation.supersede(
            ClientEndpointId::Local,
            Some(crate::client::shell::ClientEndpointFocusTarget::Pane(
                "local-pane".into()
            )),
            &mut endpoints,
        ),
        ActivationRollback::Pending
    );
    assert!(!activation.can_retarget(&target));
    assert_eq!(
        activation.supersede(
            target.clone(),
            Some(crate::client::shell::ClientEndpointFocusTarget::Pane(
                "remote-pane".into()
            )),
            &mut endpoints,
        ),
        ActivationRollback::Pending
    );
    assert_eq!(
        activation.successor,
        Some(EndpointActivationIntent {
            endpoint_id: target,
            target: Some(crate::client::shell::ClientEndpointFocusTarget::Pane(
                "remote-pane".into()
            )),
        })
    );
}

#[test]
fn unacknowledged_target_release_closes_target_before_restoring_source() {
    let (shell, mut endpoints, local_sent, _remote_sent) = shell_and_registry();
    let target = endpoint();
    let mut activation = PendingEndpointActivation::begin(
        &shell,
        &mut endpoints,
        target.clone(),
        None,
        resize(),
        24,
        Instant::now(),
    )
    .unwrap();
    let _ = activation.receive_response(
        &ClientEndpointId::Local,
        1,
        "client-shell-surface:24:off",
        &surface_success("client-shell-surface:24:off", false, 1),
        &mut endpoints,
    );
    assert_eq!(
        activation.rollback(&mut endpoints, "target activation timed out".into(), false),
        ActivationRollback::Pending
    );
    assert_eq!(
        activation.rollback(&mut endpoints, "target release timed out".into(), false),
        ActivationRollback::Pending
    );
    assert!(endpoints.connection(&target).is_none());
    let failures = endpoints.take_failures();
    assert_eq!(
        failures.len(),
        1,
        "rollback revocation must reach the reconnect owner"
    );
    assert_eq!(failures[0].endpoint_id, target);
    assert_eq!(failures[0].generation, 7);
    assert_eq!(failures[0].kind, std::io::ErrorKind::TimedOut);
    assert_eq!(
        local_sent
            .lock()
            .unwrap()
            .iter()
            .filter_map(surface_set_active)
            .collect::<Vec<_>>(),
        vec![false, true]
    );
}

#[test]
fn target_loss_at_activation_deadline_restores_source_before_timeout() {
    let (shell, mut endpoints, local_sent, _) = shell_and_registry();
    let target = endpoint();
    let mut activation = PendingEndpointActivation::begin(
        &shell,
        &mut endpoints,
        target.clone(),
        None,
        resize(),
        30,
        Instant::now(),
    )
    .unwrap();
    activation.receive_response(
        &ClientEndpointId::Local,
        1,
        "client-shell-surface:30:off",
        &surface_success("client-shell-surface:30:off", false, 1),
        &mut endpoints,
    );
    let now = Instant::now();
    activation.deadline = now;
    assert!(activation.expired(now));
    endpoints.fail(&target, std::io::ErrorKind::UnexpectedEof.into());
    // Match the client timer: apply transport failures before checking phase expiry.
    for failure in endpoints.take_failures() {
        assert_eq!(
            activation.endpoint_disconnected(&mut endpoints, &failure.endpoint_id, failure.message),
            ActivationRollback::Pending
        );
    }
    assert!(matches!(
        activation.phase,
        ActivationPhase::RestoringSource { .. }
    ));
    assert!(!activation.expired(now));
    assert_eq!(
        local_sent
            .lock()
            .unwrap()
            .iter()
            .filter_map(surface_set_active)
            .collect::<Vec<_>>(),
        vec![false, true]
    );
}

#[test]
fn losing_local_during_handoff_does_not_revoke_the_healthy_target() {
    for source_released in [false, true] {
        let (shell, mut endpoints, _local_sent, remote_sent) = shell_and_registry();
        let target = endpoint();
        let mut activation = PendingEndpointActivation::begin(
            &shell,
            &mut endpoints,
            target.clone(),
            None,
            resize(),
            29,
            Instant::now(),
        )
        .unwrap();
        if source_released {
            activation.receive_response(
                &ClientEndpointId::Local,
                1,
                "client-shell-surface:29:off",
                &surface_success("client-shell-surface:29:off", false, 1),
                &mut endpoints,
            );
        }
        if !source_released {
            activation.deadline = Instant::now() - Duration::from_millis(1);
        }
        endpoints.fail(
            &ClientEndpointId::Local,
            std::io::ErrorKind::BrokenPipe.into(),
        );
        assert_eq!(
            activation.endpoint_disconnected(
                &mut endpoints,
                &ClientEndpointId::Local,
                "Local stopped".into()
            ),
            ActivationRollback::Pending
        );
        assert!(!activation.source_available);
        assert!(
            !activation.expired(Instant::now()),
            "starting the healthy target must get a fresh deadline"
        );
        assert!(matches!(
            activation.phase,
            ActivationPhase::ActivatingTarget { .. }
        ));
        assert!(endpoints.connection(&target).is_some());
        assert_eq!(
            remote_sent
                .lock()
                .unwrap()
                .iter()
                .filter_map(surface_set_active)
                .collect::<Vec<_>>(),
            vec![true]
        );
    }
}

#[test]
fn resize_message_preserves_the_latest_surface_dimensions() {
    assert_eq!(
        resize_geometry(&resize()),
        Some(crate::protocol::ClientSurfaceSize { cols: 80, rows: 24 })
    );
}

#[test]
fn pin_in_flight_on_inactive_machine_holds_its_handoff_until_answered() {
    // Local holds the surface. A pin from the aggregate sidebar is running on the remote when
    // the user selects one of the remote's tabs. The remote's server refuses any focus while
    // that pin runs (endpoint_busy), so the handoff's surface and focus must wait for its
    // answer, a snapshot arriving meanwhile must keep the held tab, and a pin sent during the
    // handoff must wait for the handoff.
    use crate::api::schema::{Method, Request, TabSetPinnedParams};
    use crate::client::{
        endpoint_commands::EndpointCommands,
        shell::{ClientEndpointFocusTarget, ClientShellAction},
        shell_runtime::{
            begin_endpoint_activation, dispatch_client_shell_actions,
            install_client_shell_snapshot, selected_endpoint_activation_after_snapshot,
            take_ready_command_activation,
        },
        ClientLoopEvent, ClientState,
    };
    struct NoInputSource;
    impl crate::platform::PrefixInputSource for NoInputSource {
        fn switch_to_ascii(&mut self) {}
        fn restore(&mut self) {}
    }
    let (shell, mut endpoints, local_sent, remote_sent) = shell_and_registry();
    let mut state = ClientState::test_new();
    state.shell = Some(shell);
    let mut commands = EndpointCommands::default();
    let mut pending: Option<PendingEndpointActivation> = None;
    let mut serial = 60;
    let mut scheduled = None;
    let pin = |id: &str| ClientShellAction::Endpoint {
        endpoint_id: endpoint(),
        boot_id: "remote-boot".into(),
        request: Box::new(Request {
            id: id.into(),
            method: Method::TabSetPinned(TabSetPinnedParams {
                tab_id: "remote-tab".into(),
                pinned: true,
                priority: None,
            }),
        }),
    };
    let requests = |sent: &SentMessages| {
        sent.lock()
            .unwrap()
            .iter()
            .filter_map(|message| match message {
                crate::protocol::ClientMessage::ClientShellEndpointRequest { request, .. } => {
                    serde_json::from_str::<Request>(request).ok()
                }
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let remote_ids = || {
        requests(&remote_sent)
            .into_iter()
            .map(|request| request.id)
            .collect::<Vec<_>>()
    };
    // Answer each source release the handoff sent, as Local's server would.
    let release_source = |pending: &mut Option<PendingEndpointActivation>,
                          endpoints: &mut EndpointRegistry| {
        for request in requests(&local_sent) {
            if let (Method::ClientShellSurfaceSet(params), Some(activation)) =
                (&request.method, pending.as_mut())
            {
                if !params.active {
                    activation.receive_response(
                        &ClientEndpointId::Local,
                        1,
                        &request.id,
                        &surface_success(&request.id, false, 2),
                        endpoints,
                    );
                }
            }
        }
        local_sent.lock().unwrap().clear();
    };

    dispatch_client_shell_actions(
        vec![pin("pin-1")],
        &mut commands,
        &mut endpoints,
        state.shell.as_mut(),
        &mut state.detached_process_children,
        &mut scheduled,
        pending.as_ref(),
    )
    .unwrap();
    assert_eq!(remote_ids(), ["pin-1"]);

    let focus = Some(ClientEndpointFocusTarget::Tab("remote-tab".into()));
    begin_endpoint_activation(
        &mut state,
        &mut endpoints,
        &mut commands,
        &mut pending,
        &mut serial,
        endpoint(),
        focus.clone(),
        false,
        Instant::now(),
        &mut scheduled,
    )
    .unwrap();
    release_source(&mut pending, &mut endpoints);
    assert_eq!(
        remote_ids(),
        ["pin-1"],
        "the handoff reached the remote while its pin was still running"
    );
    assert!(take_ready_command_activation(&mut state, &commands).is_none());

    // The remote's snapshot lands before the pin answers. The snapshot path runs whatever
    // activation it schedules, as the client loop does, and the held tab must survive it.
    install_client_shell_snapshot(
        &mut state,
        &endpoint(),
        Box::new(test_snapshot("remote-boot", 2)),
        false,
        &mut endpoints,
        &mut NoInputSource,
    )
    .unwrap();
    let ClientEndpointId::Ssh(profile) = endpoint() else {
        unreachable!()
    };
    if let Some(ClientLoopEvent::ActivateEndpoint {
        endpoint_id,
        target,
        force,
    }) = selected_endpoint_activation_after_snapshot(
        &state,
        &endpoints,
        pending.as_ref(),
        Some(&profile),
    ) {
        begin_endpoint_activation(
            &mut state,
            &mut endpoints,
            &mut commands,
            &mut pending,
            &mut serial,
            endpoint_id,
            target,
            force,
            Instant::now(),
            &mut scheduled,
        )
        .unwrap();
    }
    assert_eq!(remote_ids(), ["pin-1"]);

    // The pin answers; the held selection now starts the handoff with its focus.
    let answer = serde_json::to_vec(&crate::api::schema::SuccessResponse {
        id: "pin-1".into(),
        result: crate::api::schema::ResponseResult::Ok {},
    })
    .unwrap();
    assert!(commands
        .receive_chunk(&endpoint(), 7, "remote-boot", "pin-1", true, answer)
        .unwrap()
        .is_some());
    let Some(ClientLoopEvent::ActivateEndpoint {
        endpoint_id,
        target,
        force,
    }) = take_ready_command_activation(&mut state, &commands)
    else {
        panic!("held selection resumes once the pin answers");
    };
    assert_eq!((&endpoint_id, &target, force), (&endpoint(), &focus, false));
    begin_endpoint_activation(
        &mut state,
        &mut endpoints,
        &mut commands,
        &mut pending,
        &mut serial,
        endpoint_id,
        target,
        force,
        Instant::now(),
        &mut scheduled,
    )
    .unwrap();
    release_source(&mut pending, &mut endpoints);
    let sent = requests(&remote_sent);
    assert!(
        sent.iter().skip(1).any(|request| matches!(&request.method,
            Method::ClientShellSurfaceSet(params) if params.active))
            && sent
                .iter()
                .skip(1)
                .any(|request| matches!(&request.method, Method::TabFocus(_))),
        "handoff after the pin: {:?}",
        sent.iter().map(|request| &request.id).collect::<Vec<_>>()
    );

    // A pin sent to the remote during its handoff waits for the handoff.
    dispatch_client_shell_actions(
        vec![pin("pin-2")],
        &mut commands,
        &mut endpoints,
        state.shell.as_mut(),
        &mut state.detached_process_children,
        &mut scheduled,
        pending.as_ref(),
    )
    .unwrap();
    assert!(!remote_ids().contains(&"pin-2".to_owned()));
}
