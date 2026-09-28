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
    assert!(open.resize && state.detail_panel.as_ref().unwrap().focused);
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
    let esc = key(&mut state, KeyCode::Esc, KeyModifiers::NONE);
    assert!(!esc.requests.is_empty());
    assert!(state.detail_panel.is_some());
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
    assert!(frame_rows(&frame).iter().any(|line| line.contains("Factory overview")));
    let panel = state.layout(110, 30).detail_panel;
    let point = (panel.x + 2, panel.y + 2);
    let focus = click(&mut state, point.0, point.1);
    assert!(focus.requests.is_empty());
    assert!(state.detail_panel.as_ref().unwrap().focused);
    let pane = state.layout(110, 30).pane_surface;
    state.handle_raw_events(vec![RawInputEvent::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left), column: pane.x + 1,
        row: pane.y + 1, modifiers: KeyModifiers::NONE,
    })]);
    assert!(!state.detail_panel.as_ref().unwrap().focused);
    let before = state.detail_panel.as_ref().unwrap().scroll;
    state.handle_raw_events(vec![RawInputEvent::Mouse(MouseEvent {
        kind: MouseEventKind::ScrollDown, column: point.0,
        row: point.1, modifiers: KeyModifiers::NONE,
    })]);
    assert!(state.detail_panel.as_ref().unwrap().scroll > before);
    state.compose(110, 30).expect("repaint");
    let target = state.hits.detail_rows[0].0;
    let selected = click(&mut state, target.x + 1, target.y);
    assert!(selected.actions.iter().any(|action| format!("{action:?}").contains("PaneFocus")));
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
