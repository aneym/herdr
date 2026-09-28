use super::*;
use crate::factory_overlay::{
    FactoryOverlay, Panel, PanelRow, PanelSection, RowStyle, TabKind, TabTag,
};
use crossterm::event::MouseEvent;
use std::sync::Arc;

fn ready() -> ClientShellState {
    let mut config = Config::default();
    config.ui.factory.enabled = true;
    config.keys.toggle_factory_overview = crate::config::BindingConfig::one("alt+o");
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&config));
    state.set_snapshot(Box::new(snapshot()));
    state.set_endpoint_status(
        &state.active_endpoint_id.clone(),
        ClientEndpointStatus::Online,
    );
    state.factory_overlay = Some(Arc::new(FactoryOverlay {
        version: 1,
        tabs: [("tab_1".into(), TabTag { kind: TabKind::Lane, ..Default::default() })].into(),
        panels: [("overview".into(), Panel {
            title: "Factory overview".into(), subtitle: Some("active work".into()),
            sections: vec![PanelSection {
                title: "RUNNING".into(), right: Some("2 items".into()),
                rows: vec![
                    PanelRow { text: "Very long workflow title that must eventually truncate at the edge of this narrow panel".into(), right: Some("PC".into()), target: Some("ws_1:p12".into()), style: RowStyle::Warn, ..Default::default() },
                    PanelRow { text: "ordinary row".into(), style: RowStyle::Normal, ..Default::default() },
                ],
            }],
            actions: vec![PanelRow { text: "open orchestrator".into(), target: Some("ws_1:t1".into()), ..Default::default() }],
        })].into(),
        ..Default::default()
    }));
    state
}

fn key(state: &mut ClientShellState, code: KeyCode, modifiers: KeyModifiers) -> ClientShellInput {
    state.handle_raw_events(vec![RawInputEvent::Key(crate::input::TerminalKey::new(
        code, modifiers,
    ))])
}

fn click(state: &mut ClientShellState, x: u16, y: u16) -> ClientShellInput {
    state.handle_raw_events(vec![
        RawInputEvent::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: x,
            row: y,
            modifiers: KeyModifiers::NONE,
        }),
        RawInputEvent::Mouse(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: x,
            row: y,
            modifiers: KeyModifiers::NONE,
        }),
    ])
}

#[test]
fn detail_panel_layout_preserves_off_and_mobile_surfaces() {
    let mut state = ready();
    let closed = state.layout(110, 30);
    assert_eq!(closed.detail_panel.width, 0);
    let mut result = ClientShellInput::default();
    state.toggle_factory_overview(&mut result);
    assert!(result.resize && result.repaint);
    let opened = state.layout(110, 30);
    assert_eq!(opened.detail_panel.x, closed.sidebar.right());
    assert_eq!(opened.detail_panel.width, state.config.factory.panel_width);
    assert_eq!(
        opened.pane_surface.width,
        closed.pane_surface.width - opened.detail_panel.width
    );
    assert_eq!(state.surface_size(110, 30).cols, opened.pane_surface.width);
    assert_eq!(state.layout(44, 20).detail_panel.width, 0);
    state.config.factory.enabled = false;
    assert_eq!(state.layout(110, 30).pane_surface, closed.pane_surface);
}

#[test]
fn detail_panel_renders_sections_actions_alignment_and_truncation() {
    let mut state = ready();
    let mut result = ClientShellInput::default();
    state.toggle_factory_overview(&mut result);
    let layout = state.layout(100, 25);
    let mut buffer = Buffer::empty(Rect::new(0, 0, 100, 25));
    let mut hits = ShellHitMap::default();
    super::super::detail_panel::render_panel(
        &mut buffer,
        layout.detail_panel,
        state.factory_overlay.as_deref().unwrap(),
        state.snapshot.as_deref().unwrap(),
        state.detail_panel.as_mut().unwrap(),
        &state.config.palette,
        &mut hits,
    );
    let frame = FrameData::from_ratatui_buffer_with_hyperlinks(&buffer, None, &[]);
    let rows = frame_rows(&frame);
    let panel_text = rows
        .iter()
        .map(|line| {
            line.chars()
                .skip(layout.detail_panel.x as usize)
                .take(layout.detail_panel.width as usize)
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(panel_text.contains("Factory overview"));
    assert!(panel_text.contains("RUNNING"));
    assert!(panel_text.contains("[ open orchestrator ↵ ]"));
    assert!(panel_text.contains('…'));
    let section = rows.iter().find(|row| row.contains("RUNNING")).unwrap();
    assert!(section.find("2 items").unwrap() > section.find("RUNNING").unwrap() + 8);
    let content = rows.iter().find(|row| row.contains("PC")).unwrap();
    assert!(content.find("PC").unwrap() > content.find('…').unwrap());
    assert_eq!(hits.detail_rows.len(), 2);
}

#[test]
fn overview_key_esc_filter_and_enter_target() {
    let mut state = ready();
    let open = key(&mut state, KeyCode::Char('o'), KeyModifiers::ALT);
    assert!(open.resize && !state.detail_panel.as_ref().unwrap().focused);
    // Clicking a non-row spot inside the panel gives it keyboard focus.
    state.last_composed_size = Some((110, 30));
    let panel = state.layout(110, 30).detail_panel;
    click(&mut state, panel.x + 3, 2);
    assert!(state.detail_panel.is_some());
    assert!(state.detail_panel.as_ref().unwrap().focused);
    key(&mut state, KeyCode::Down, KeyModifiers::NONE);
    key(&mut state, KeyCode::Up, KeyModifiers::NONE);
    assert_eq!(state.detail_panel.as_ref().unwrap().selected, Some(0));
    let filtered = key(&mut state, KeyCode::Char('e'), KeyModifiers::NONE);
    assert!(filtered.requests.is_empty());
    assert!(state.detail_panel.as_ref().unwrap().exceptions_only);
    let enter = key(&mut state, KeyCode::Enter, KeyModifiers::NONE);
    assert!(enter
        .actions
        .iter()
        .any(|action| format!("{action:?}").contains("ws_1:p12")));
    assert!(state.detail_panel.is_none());
    key(&mut state, KeyCode::Char('o'), KeyModifiers::ALT);
    let esc = key(&mut state, KeyCode::Esc, KeyModifiers::NONE);
    assert!(esc.requests.is_empty());
    assert!(state.detail_panel.is_none());
    key(&mut state, KeyCode::Char('o'), KeyModifiers::ALT);
    key(&mut state, KeyCode::Char('o'), KeyModifiers::ALT);
    assert!(state.detail_panel.is_none());
    state.change_detail_panel(
        crate::factory_overlay::tab_panel_key("tab_1"),
        false,
        &mut ClientShellInput::default(),
    );
    assert!(!state.detail_panel.as_ref().unwrap().focused);
    // Unfocused: the pane keeps every key except Esc, which closes the panel.
    let other = key(&mut state, KeyCode::Char('x'), KeyModifiers::NONE);
    assert!(!other.requests.is_empty());
    assert!(state.detail_panel.is_some());
    let esc = key(&mut state, KeyCode::Esc, KeyModifiers::NONE);
    assert!(esc.requests.is_empty() && esc.resize);
    assert!(state.detail_panel.is_none());
}

#[test]
fn alt_o_defaults_on_with_overlay_and_passes_through_when_off() {
    let mut state = ready();
    state.config.keybinds.keybinds.toggle_factory_overview = Default::default();
    key(&mut state, KeyCode::Char('o'), KeyModifiers::ALT);
    assert_eq!(state.detail_panel.as_ref().unwrap().key, "overview");
    key(&mut state, KeyCode::Char('o'), KeyModifiers::ALT);
    assert!(state.detail_panel.is_none());

    state.config.factory.enabled = false;
    let passed = key(&mut state, KeyCode::Char('o'), KeyModifiers::ALT);
    assert!(state.detail_panel.is_none());
    assert!(!passed.requests.is_empty());
}

#[test]
fn missing_panel_shows_title_and_no_details_yet() {
    let mut state = ready();
    state.set_pane_surface(surface());
    state.change_detail_panel(
        crate::factory_overlay::tab_panel_key("tab_1"),
        false,
        &mut ClientShellInput::default(),
    );
    let rows = frame_rows(&state.compose(110, 30).expect("composed shell"));
    assert!(rows.iter().any(|line| line.contains("no details yet")));
    assert!(rows.iter().any(|line| line.contains("esc")));
    state.detail_panel = None;
    state.change_detail_panel("tab:gone".into(), false, &mut ClientShellInput::default());
    let rows = frame_rows(&state.compose(110, 30).expect("composed shell"));
    assert!(rows.iter().any(|line| line.contains("Factory overview")));
    assert!(rows.iter().any(|line| line.contains("no details yet")));
}

#[test]
fn tagged_tab_click_toggles_panel_without_focusing_tab() {
    let mut state = ready();
    state.last_composed_size = Some((100, 30));
    state.hits.tree_headers.push(TreeHeaderHit {
        rect: Rect::new(2, 4, 12, 1),
        chevron: Rect::default(),
        plus: Rect::default(),
        pin: Rect::default(),
        group: None,
        workspace_id: "ws_1".into(),
        tab_id: Some("tab_1".into()),
        key: "ws_1#1".into(),
        pinned: false,
    });
    let first = click(&mut state, 4, 4);
    assert!(first.requests.is_empty());
    assert_eq!(state.detail_panel.as_ref().unwrap().key, "tab:tab_1");
    state.hits.tree_headers.push(TreeHeaderHit {
        rect: Rect::new(2, 4, 12, 1),
        chevron: Rect::default(),
        plus: Rect::default(),
        pin: Rect::default(),
        group: None,
        workspace_id: "ws_1".into(),
        tab_id: Some("tab_1".into()),
        key: "ws_1#1".into(),
        pinned: false,
    });
    let second = click(&mut state, 4, 4);
    assert!(second.requests.is_empty());
    assert!(state.detail_panel.is_none());
    state.factory_overlay = Some(Arc::new(FactoryOverlay {
        version: 1,
        ..Default::default()
    }));
    state.hits.tree_headers.push(TreeHeaderHit {
        rect: Rect::new(2, 4, 12, 1),
        chevron: Rect::default(),
        plus: Rect::default(),
        pin: Rect::default(),
        group: None,
        workspace_id: "ws_1".into(),
        tab_id: Some("tab_1".into()),
        key: "ws_1#1".into(),
        pinned: false,
    });
    let untagged = click(&mut state, 4, 4);
    assert!(untagged
        .actions
        .iter()
        .any(|action| format!("{action:?}").contains("TabFocus")));
}

#[test]
fn detail_panel_click_focus_scroll_and_pane_click() {
    let mut state = ready();
    state.set_pane_surface(surface());
    state.change_detail_panel("overview".into(), false, &mut ClientShellInput::default());
    let frame = state.compose(110, 30).expect("composed shell");
    assert!(frame_rows(&frame)
        .iter()
        .any(|line| line.contains("Factory overview")));
    let panel = state.layout(110, 30).detail_panel;
    let point = (panel.x + 2, panel.y + 2);
    let focus = click(&mut state, point.0, point.1);
    assert!(focus.requests.is_empty());
    assert!(state.detail_panel.as_ref().unwrap().focused);
    let pane = state.layout(110, 30).pane_surface;
    state.handle_raw_events(vec![RawInputEvent::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: pane.x + 1,
        row: pane.y + 1,
        modifiers: KeyModifiers::NONE,
    })]);
    assert!(!state.detail_panel.as_ref().unwrap().focused);
    let before = state.detail_panel.as_ref().unwrap().scroll;
    state.handle_raw_events(vec![RawInputEvent::Mouse(MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column: point.0,
        row: point.1,
        modifiers: KeyModifiers::NONE,
    })]);
    assert!(state.detail_panel.as_ref().unwrap().scroll > before);
    state.compose(110, 30).expect("repaint");
    let target = state.hits.detail_rows[0].0;
    let selected = click(&mut state, target.x + 1, target.y);
    assert!(selected
        .actions
        .iter()
        .any(|action| format!("{action:?}").contains("PaneFocus")));
    assert!(state.detail_panel.is_none());
}

#[test]
fn overview_key_is_opt_in() {
    let config = Config::default();
    assert!(crate::input::resolve_direct_binding(
        &config.keybinds(),
        &crate::input::TerminalKey::new(KeyCode::Char('o'), KeyModifiers::ALT)
    )
    .is_none());
    let config: Config = toml::from_str("[keys]\ntoggle_factory_overview = 'alt+o'").unwrap();
    let binding = crate::input::resolve_direct_binding(
        &config.keybinds(),
        &crate::input::TerminalKey::new(KeyCode::Char('o'), KeyModifiers::ALT),
    );
    assert!(matches!(
        binding,
        Some(crate::input::KeybindMatch::Action(
            crate::input::KeybindAction::ToggleFactoryOverview
        ))
    ));
}

#[test]
fn alt_o_leaves_typing_with_the_live_pane() {
    let mut state = ready();
    key(&mut state, KeyCode::Char('o'), KeyModifiers::ALT);
    assert!(!state.detail_panel.as_ref().unwrap().focused);
    let typed = key(&mut state, KeyCode::Char('x'), KeyModifiers::NONE);
    assert!(!typed.requests.is_empty());
    let text = state.handle_raw_events(vec![RawInputEvent::Text(crate::input::TextCommit::new("x"))]);
    assert!(!text.requests.is_empty());
    assert!(state.detail_panel.is_some());
}

#[test]
fn alt_o_fallback_does_not_override_another_binding() {
    let mut state = ready();
    state.config.keybinds.keybinds.toggle_factory_overview = Default::default();
    state.config.keybinds.keybinds.detach =
        crate::config::ActionKeybinds::from_labels(&["alt+o".to_owned()]).expect("binding");
    let outcome = key(&mut state, KeyCode::Char('o'), KeyModifiers::ALT);
    assert!(outcome.detach);
    assert!(state.detail_panel.is_none());
}

#[test]
fn panel_width_is_fixed_at_88_columns() {
    let mut state = ready();
    state.toggle_factory_overview(&mut ClientShellInput::default());
    let layout = state.layout(88, 30);
    assert_eq!(layout.detail_panel.width, state.config.factory.panel_width);
    assert_eq!(layout.detail_panel.width, 46);
    assert_eq!(
        layout.pane_surface.x,
        layout.detail_panel.x + layout.detail_panel.width
    );
}

fn tagged_row_hit() -> TreeHeaderHit {
    TreeHeaderHit {
        rect: Rect::new(2, 4, 12, 1),
        chevron: Rect::default(),
        plus: Rect::default(),
        pin: Rect::default(),
        group: None,
        workspace_id: "ws_1".into(),
        tab_id: Some("tab_1".into()),
        key: "ws_1#1".into(),
        pinned: false,
    }
}

fn focused_tab_ids(input: &ClientShellInput) -> Vec<String> {
    input
        .actions
        .iter()
        .filter_map(|action| match action {
            ClientShellAction::Endpoint { request, .. } => match &request.method {
                crate::api::schema::Method::TabFocus(target) => Some(target.tab_id.clone()),
                _ => None,
            },
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn overlay_file_reaches_the_panel_through_the_server_poll_and_client_control() {
    let dir = std::env::temp_dir().join(format!(
        "herdr-overlay-panel-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("overlay.json");
    let document = {
        let overlay = ready().factory_overlay.take().unwrap();
        serde_json::to_string(overlay.as_ref()).unwrap()
    };
    std::fs::write(&path, document).unwrap();

    let mut config = Config::default();
    config.ui.factory.enabled = true;
    config.ui.factory.overlay_file = path.to_string_lossy().into_owned();
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&config));
    let active = state.active_endpoint_id.clone();
    state.set_endpoint_snapshot_for_generation(&active, 1, Box::new(snapshot()));
    assert!(state.factory_overlay.is_none());
    state.handle_raw_events(vec![RawInputEvent::Key(crate::input::TerminalKey::new(
        KeyCode::Char('o'),
        KeyModifiers::ALT,
    ))]);
    assert!(state.detail_panel.is_none(), "no overlay yet, Alt-O passes through");

    // Server side: the real headless loop. A poll tick reaches the attached client shell
    // through send_to_client_shells, and a client connecting later is seeded through
    // send_to_client.
    let mut server =
        crate::server::headless::tests::OverlayServerHarness::new(&path.to_string_lossy());
    // The endpoint snapshot carries the server's boot id, as it does after a real attach.
    let mut seeded = snapshot();
    seeded.boot_id = server.boot_id();
    state.set_endpoint_snapshot_for_generation(&active, 1, Box::new(seeded));
    let overlay_messages = |messages: Vec<crate::protocol::ServerMessage>| {
        messages
            .into_iter()
            .filter_map(|message| match message {
                crate::protocol::ServerMessage::EndpointControl { kind, data }
                    if kind == crate::protocol::endpoint::FACTORY_OVERLAY_KIND =>
                {
                    Some((kind, data))
                }
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let ticked = overlay_messages(server.poll_tick());
    assert_eq!(ticked.len(), 1, "poll tick must deliver the overlay to the client shell");
    let connected = overlay_messages(server.connect());
    assert_eq!(connected.len(), 1, "a connecting client must be seeded with the overlay");
    // Client side: decode and apply exactly as the attach loop does.
    let apply = |state: &mut ClientShellState, (kind, data): &(String, String)| {
        let crate::client::endpoint::EndpointControlMessage::FactoryOverlay(decoded) =
            crate::client::endpoint::decode_endpoint_control(kind, data).unwrap()
        else {
            panic!("expected factory overlay control");
        };
        state.set_endpoint_factory_overlay_for_generation(&active, 1, decoded)
    };
    // Both paths carry the same document and revision.
    assert_eq!(connected, ticked);
    assert!(apply(&mut state, &ticked[0]));
    assert!(state.factory_overlay.is_some());

    // Alt-O (default binding) opens the panel with the file's contents.
    state.set_pane_surface(surface());
    state.set_endpoint_status(&active, ClientEndpointStatus::Online);
    let open = key(&mut state, KeyCode::Char('o'), KeyModifiers::ALT);
    assert!(open.resize);
    assert_eq!(state.detail_panel.as_ref().unwrap().key, "overview");
    let rows = frame_rows(&state.compose(110, 30).expect("composed shell"));
    assert!(rows.iter().any(|line| line.contains("Factory overview")));
    assert!(rows.iter().any(|line| line.contains("open orchestrator")));

    // A changed file is picked up on the next poll.
    let mut changed: FactoryOverlay =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    changed.panels.get_mut("overview").unwrap().title = "Renamed overview".into();
    std::fs::write(&path, serde_json::to_string(&changed).unwrap()).unwrap();
    let updated = overlay_messages(server.poll_tick());
    assert_eq!(updated.len(), 1);
    assert!(apply(&mut state, &updated[0]));
    assert_eq!(
        state.factory_overlay.as_ref().unwrap().panels["overview"].title,
        "Renamed overview"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn enter_after_tagged_row_click_focuses_that_tab() {
    let mut state = ready();
    state.last_composed_size = Some((100, 30));
    state.hits.tree_headers.push(tagged_row_hit());
    let opened = click(&mut state, 4, 4);
    assert!(opened.requests.is_empty());
    assert_eq!(state.detail_panel.as_ref().unwrap().key, "tab:tab_1");
    let enter = key(&mut state, KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(focused_tab_ids(&enter), ["tab_1"]);
    assert!(state.detail_panel.is_none());
}

#[test]
fn blank_panel_click_keeps_a_selection_so_enter_acts() {
    let mut state = ready();
    key(&mut state, KeyCode::Char('o'), KeyModifiers::ALT);
    assert_eq!(state.detail_panel.as_ref().unwrap().selected, None);
    // Compose first so the hit map holds the rendered rows and action.
    state.compose(110, 30).expect("composed shell");
    let panel = state.layout(110, 30).detail_panel;
    assert!(!state.hits.detail_rows.is_empty());
    let covered = |state: &ClientShellState, x: u16, y: u16| {
        state.hits.detail_rows.iter().any(|(rect, _)| {
            x >= rect.x && x < rect.right() && y >= rect.y && y < rect.bottom()
        })
    };
    // Pick the lowest interior cell (inside the border) that no row or action covers.
    let x = panel.x + 3;
    let blank = (panel.y + 1..panel.bottom() - 1)
        .rev()
        .find(|&row| !covered(&state, x, row))
        .expect("a blank panel cell");
    click(&mut state, x, blank);
    let opened = state.detail_panel.as_ref().unwrap();
    assert!(opened.focused && opened.selected.is_some());
    let enter = key(&mut state, KeyCode::Enter, KeyModifiers::NONE);
    assert!(enter
        .actions
        .iter()
        .any(|action| format!("{action:?}").contains("PaneFocus")));
}

#[test]
fn panel_keeps_its_configured_width_and_hides_when_it_cannot_fit() {
    let mut state = ready();
    state.toggle_factory_overview(&mut ClientShellInput::default());
    let sidebar = state.layout(80, 30).sidebar.width;
    let wide_enough = sidebar + state.config.factory.panel_width + 10;
    let fit = state.layout(wide_enough, 30);
    assert_eq!(fit.detail_panel.width, 46);
    assert!(fit.pane_surface.width >= 10);
    let tight = state.layout(wide_enough - 1, 30);
    assert_eq!(tight.detail_panel.width, 0);
    assert_eq!(tight.pane_surface.width, wide_enough - 1 - sidebar);
    let narrow = state.layout(80, 30);
    assert_ne!(narrow.detail_panel.width, 44);
}
