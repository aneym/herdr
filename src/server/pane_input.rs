use bytes::Bytes;
use crossterm::event::{KeyCode, KeyEventKind, KeyModifiers, MouseEventKind};

use crate::protocol::{AttachScrollDirection, AttachScrollSource, ClientPaneInputEvent};

pub(super) fn downgrade_ineligible_pixel_mouse(
    events: &mut [ClientPaneInputEvent],
    pixel_mouse: bool,
    runtime_size: (u16, u16),
    runtime_pixels: Option<(u32, u32)>,
) {
    let (runtime_rows, runtime_cols) = runtime_size;
    for event in events {
        let ClientPaneInputEvent::Mouse {
            position, geometry, ..
        } = event
        else {
            continue;
        };
        let crate::protocol::ClientMousePosition::Pixels { x, y, column, row } = *position else {
            continue;
        };
        let exact = pixel_mouse
            && geometry.is_some_and(|geometry| {
                (runtime_rows, runtime_cols) == (geometry.rows, geometry.cols)
                    && runtime_pixels == Some((geometry.width_px, geometry.height_px))
                    && column < geometry.cols
                    && row < geometry.rows
                    && x > 0
                    && y > 0
                    && x <= geometry.width_px
                    && y <= geometry.height_px
            });
        if !exact {
            *position = crate::protocol::ClientMousePosition::Cell { column, row };
            *geometry = None;
        }
    }
}

pub(super) fn terminal_attach_mouse_position(
    runtime: &crate::terminal::TerminalRuntime,
    terminal_size: (u16, u16),
    cell_size: crate::kitty_graphics::HostCellSize,
    pixel_mouse: bool,
    host_sgr_pixels_active: bool,
    position: crate::protocol::ClientMousePosition,
    geometry: Option<crate::protocol::ClientMouseGeometry>,
) -> Option<crate::protocol::ClientMousePosition> {
    let runtime_size = runtime.current_size();
    let cell_fallback = |column, row| {
        (column < runtime_size.1 && row < runtime_size.0)
            .then_some(crate::protocol::ClientMousePosition::Cell { column, row })
    };
    let (x, y, column, row) = match position {
        crate::protocol::ClientMousePosition::Cell { column, row } => {
            return cell_fallback(column, row);
        }
        crate::protocol::ClientMousePosition::Pixels { x, y, column, row } => (x, y, column, row),
    };
    let Some(geometry) = geometry else {
        return cell_fallback(column, row);
    };
    let host_geometry = crate::input::mouse::HostGeometry::new(
        geometry.cols,
        geometry.rows,
        geometry.width_px,
        geometry.height_px,
    )?;
    if host_geometry.cell(x, y) != Some((column, row)) {
        return None;
    }
    let exact = (|| {
        let average_width = (geometry.width_px / u32::from(geometry.cols)).max(1);
        let average_height = (geometry.height_px / u32::from(geometry.rows)).max(1);
        let (child_width_px, child_height_px) = runtime.pixel_size()?;
        if !pixel_mouse
            || !host_sgr_pixels_active
            || !runtime.sgr_pixel_mouse_enabled()
            || terminal_size != (geometry.cols, geometry.rows)
            || runtime_size != (geometry.rows, geometry.cols)
            || !cell_size.is_known()
            || average_width != cell_size.width_px
            || average_height != cell_size.height_px
        {
            return None;
        }
        let crate::input::mouse::Position::Pixels { x, y } = (crate::input::mouse::HostPixels {
            x,
            y,
            geometry: host_geometry,
        })
        .pane_position(
            ratatui::layout::Rect::new(0, 0, geometry.cols, geometry.rows),
            child_width_px,
            child_height_px,
        )?
        else {
            return None;
        };
        Some(crate::protocol::ClientMousePosition::Pixels { x, y, column, row })
    })();
    exact.or_else(|| cell_fallback(column, row))
}

pub(super) fn apply_terminal_attach_scroll(
    runtime: &crate::terminal::TerminalRuntime,
    source: AttachScrollSource,
    direction: AttachScrollDirection,
    lines: u16,
    column: Option<u16>,
    row: Option<u16>,
    modifiers: u8,
) -> Result<(), String> {
    apply_scroll(
        runtime,
        source,
        direction,
        lines,
        crate::input::mouse::Position::Cell {
            column: column.unwrap_or(0),
            row: row.unwrap_or(0),
        },
        modifiers,
    )
}

fn apply_scroll(
    runtime: &crate::terminal::TerminalRuntime,
    source: AttachScrollSource,
    direction: AttachScrollDirection,
    lines: u16,
    position: crate::input::mouse::Position,
    modifiers: u8,
) -> Result<(), String> {
    let wheel_kind = match direction {
        AttachScrollDirection::Up => MouseEventKind::ScrollUp,
        AttachScrollDirection::Down => MouseEventKind::ScrollDown,
    };
    if let AttachScrollSource::PageKey { input } = source {
        let host_scroll = runtime
            .plain_page_keys_use_host_scrollback()
            .unwrap_or(false);
        if host_scroll {
            match direction {
                AttachScrollDirection::Up => runtime.scroll_up(lines.max(1) as usize),
                AttachScrollDirection::Down => runtime.scroll_down(lines.max(1) as usize),
            }
            return Ok(());
        }
        return apply_terminal_attach_input(runtime, input);
    }

    match runtime.wheel_routing() {
        Some(crate::pane::WheelRouting::MouseReport) => {
            runtime.scroll_reset();
            let Some(bytes) = runtime.encode_mouse_wheel(
                wheel_kind,
                position,
                KeyModifiers::from_bits_truncate(modifiers),
            ) else {
                return Err(format!(
                    "failed to encode terminal attach mouse wheel event: {wheel_kind:?}"
                ));
            };
            runtime
                .try_send_bytes(Bytes::from(bytes))
                .map_err(|err| format!("terminal attach mouse wheel input failed: {err}"))?;
        }
        Some(crate::pane::WheelRouting::AlternateScroll) => {
            runtime.scroll_reset();
            let Some(bytes) = runtime.encode_alternate_scroll(wheel_kind) else {
                return Ok(());
            };
            runtime
                .try_send_bytes(Bytes::from(bytes))
                .map_err(|err| format!("terminal attach alternate scroll input failed: {err}"))?;
        }
        Some(crate::pane::WheelRouting::HostScroll) | None => match direction {
            AttachScrollDirection::Up => runtime.scroll_up(lines.max(1) as usize),
            AttachScrollDirection::Down => runtime.scroll_down(lines.max(1) as usize),
        },
    }
    Ok(())
}

pub(super) fn apply_terminal_attach_input(
    runtime: &crate::terminal::TerminalRuntime,
    data: Vec<u8>,
) -> Result<(), String> {
    runtime.record_human_bytes(&data);
    runtime.scroll_reset();
    if let Some(text) = crate::raw_input::complete_text_bracketed_paste(&data) {
        runtime
            .try_send_paste(text.to_owned())
            .map_err(|err| format!("terminal attach paste failed: {err}"))
    } else {
        runtime
            .try_send_bytes(Bytes::from(data))
            .map_err(|err| format!("terminal attach input failed: {err}"))
    }
}

pub(super) fn apply_client_pane_input_events(
    runtime: &crate::terminal::TerminalRuntime,
    events: &[ClientPaneInputEvent],
) -> Result<(), String> {
    apply_client_terminal_input_events(runtime, events, true)
}

pub(super) fn apply_client_popup_input_events(
    runtime: &crate::terminal::TerminalRuntime,
    events: &[ClientPaneInputEvent],
) -> Result<(), String> {
    apply_client_terminal_input_events(runtime, events, false)
}

fn apply_client_terminal_input_events(
    runtime: &crate::terminal::TerminalRuntime,
    events: &[ClientPaneInputEvent],
    host_page_keys: bool,
) -> Result<(), String> {
    for event in events {
        if let ClientPaneInputEvent::Mouse {
            kind,
            position,
            modifiers,
            lines,
            ..
        } = event
        {
            let kind = kind.to_crossterm();
            let modifiers = KeyModifiers::from_bits_truncate(*modifiers);
            let position = match position {
                crate::protocol::ClientMousePosition::Cell { column, row } => {
                    crate::input::mouse::Position::Cell {
                        column: *column,
                        row: *row,
                    }
                }
                crate::protocol::ClientMousePosition::Pixels { x, y, column, row } => {
                    if runtime.sgr_pixel_mouse_enabled() {
                        crate::input::mouse::Position::Pixels { x: *x, y: *y }
                    } else {
                        crate::input::mouse::Position::Cell {
                            column: *column,
                            row: *row,
                        }
                    }
                }
            };
            let bytes = match kind {
                MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                    let direction = if kind == MouseEventKind::ScrollUp {
                        AttachScrollDirection::Up
                    } else {
                        AttachScrollDirection::Down
                    };
                    apply_scroll(
                        runtime,
                        AttachScrollSource::Wheel,
                        direction,
                        (*lines).max(1),
                        position,
                        modifiers.bits(),
                    )?;
                    continue;
                }
                MouseEventKind::ScrollLeft | MouseEventKind::ScrollRight => runtime
                    .encode_mouse_wheel(kind, position, modifiers)
                    .unwrap_or_default(),
                MouseEventKind::Down(_) | MouseEventKind::Up(_) | MouseEventKind::Drag(_) => {
                    runtime
                        .encode_mouse_button(kind, position, modifiers)
                        .unwrap_or_default()
                }
                MouseEventKind::Moved => runtime
                    .encode_mouse_motion(kind, position, modifiers)
                    .unwrap_or_default(),
            };
            if !bytes.is_empty() {
                if kind != MouseEventKind::Moved {
                    runtime.scroll_reset();
                }
                runtime
                    .try_send_bytes(Bytes::from(bytes))
                    .map_err(|err| format!("targeted pane mouse input failed: {err}"))?;
            }
            continue;
        }

        match event.to_raw_input_event() {
            crate::raw_input::RawInputEvent::Key(key) => {
                runtime.record_human_key(&key);
                let key_event = key.as_key_event();
                if host_page_keys
                    && matches!(key_event.code, KeyCode::PageUp | KeyCode::PageDown)
                    && key_event.modifiers.is_empty()
                    && runtime.plain_page_keys_use_host_scrollback() == Some(true)
                {
                    match key_event.kind {
                        KeyEventKind::Release => continue,
                        KeyEventKind::Press | KeyEventKind::Repeat => {
                            let lines = runtime.current_size().0.max(1) as usize;
                            if key_event.code == KeyCode::PageUp {
                                runtime.scroll_up(lines);
                            } else {
                                runtime.scroll_down(lines);
                            }
                            continue;
                        }
                    }
                }

                runtime.scroll_reset();
                let bytes = runtime.encode_terminal_key(key);
                if !bytes.is_empty() {
                    runtime
                        .try_send_bytes(Bytes::from(bytes))
                        .map_err(|err| format!("targeted pane key input failed: {err}"))?;
                }
            }
            crate::raw_input::RawInputEvent::Text(text) => {
                runtime.record_human_text();
                runtime.scroll_reset();
                runtime
                    .try_send_bytes(Bytes::copy_from_slice(text.as_str().as_bytes()))
                    .map_err(|err| format!("targeted pane text input failed: {err}"))?;
            }
            crate::raw_input::RawInputEvent::Paste(text) => {
                runtime.record_human_text();
                runtime.scroll_reset();
                runtime
                    .try_send_paste(text)
                    .map_err(|err| format!("targeted pane paste failed: {err}"))?;
            }
            // PORT-0.9: mouse back/forward buttons drive focus history in the
            // client shell once ported. (docs/fork/port-0.9/PORT.md)
            crate::raw_input::RawInputEvent::MouseNavButton { .. }
            | crate::raw_input::RawInputEvent::Mouse(_)
            | crate::raw_input::RawInputEvent::OuterFocusGained
            | crate::raw_input::RawInputEvent::OuterFocusLost
            | crate::raw_input::RawInputEvent::HostDefaultColor { .. }
            | crate::raw_input::RawInputEvent::HostPaletteColors { .. }
            | crate::raw_input::RawInputEvent::HostColorSchemeChanged(_)
            | crate::raw_input::RawInputEvent::HostCellSizeReport { .. }
            | crate::raw_input::RawInputEvent::Unsupported => {
                return Err("non-pane input reached targeted pane input".to_owned());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn terminal_attach_stale_geometry_falls_back_to_the_canonical_cell() {
        let runtime = crate::terminal::TerminalRuntime::test_with_screen_bytes(20, 5, b"");
        let position = crate::protocol::ClientMousePosition::Pixels {
            x: 121,
            y: 81,
            column: 12,
            row: 4,
        };

        assert_eq!(
            terminal_attach_mouse_position(
                &runtime,
                (20, 5),
                crate::kitty_graphics::HostCellSize {
                    width_px: 10,
                    height_px: 20,
                },
                true,
                false,
                position,
                Some(crate::protocol::ClientMouseGeometry {
                    cols: 20,
                    rows: 5,
                    width_px: 200,
                    height_px: 100,
                }),
            ),
            Some(crate::protocol::ClientMousePosition::Cell { column: 12, row: 4 })
        );
        assert_eq!(
            terminal_attach_mouse_position(
                &runtime,
                (20, 5),
                crate::kitty_graphics::HostCellSize {
                    width_px: 10,
                    height_px: 20,
                },
                true,
                false,
                crate::protocol::ClientMousePosition::Pixels {
                    x: 120,
                    y: 80,
                    column: 12,
                    row: 4,
                },
                Some(crate::protocol::ClientMouseGeometry {
                    cols: 20,
                    rows: 5,
                    width_px: 200,
                    height_px: 100,
                }),
            ),
            None
        );
        assert_eq!(
            terminal_attach_mouse_position(
                &runtime,
                (80, 24),
                crate::kitty_graphics::HostCellSize::default(),
                false,
                false,
                crate::protocol::ClientMousePosition::Cell { column: 12, row: 4 },
                None,
            ),
            Some(crate::protocol::ClientMousePosition::Cell { column: 12, row: 4 })
        );
    }

    #[test]
    fn ineligible_shell_pixel_mouse_uses_its_canonical_cell_position() {
        let mut events = vec![ClientPaneInputEvent::Mouse {
            kind: crate::protocol::ClientMouseKind::Down(crate::protocol::ClientMouseButton::Left),
            position: crate::protocol::ClientMousePosition::Pixels {
                x: 121,
                y: 81,
                column: 12,
                row: 4,
            },
            geometry: Some(crate::protocol::ClientMouseGeometry {
                cols: 20,
                rows: 5,
                width_px: 200,
                height_px: 100,
            }),
            modifiers: 0,
            lines: 1,
        }];

        downgrade_ineligible_pixel_mouse(&mut events, false, (5, 20), Some((200, 100)));

        assert!(matches!(
            events.as_slice(),
            [ClientPaneInputEvent::Mouse {
                position: crate::protocol::ClientMousePosition::Cell { column: 12, row: 4 },
                ..
            }]
        ));
    }

    #[test]
    fn eligible_shell_pixel_mouse_remains_exact() {
        let position = crate::protocol::ClientMousePosition::Pixels {
            x: 121,
            y: 81,
            column: 12,
            row: 4,
        };
        let mut events = vec![ClientPaneInputEvent::Mouse {
            kind: crate::protocol::ClientMouseKind::Moved,
            position,
            geometry: Some(crate::protocol::ClientMouseGeometry {
                cols: 20,
                rows: 5,
                width_px: 200,
                height_px: 100,
            }),
            modifiers: 0,
            lines: 1,
        }];

        downgrade_ineligible_pixel_mouse(&mut events, true, (5, 20), Some((200, 100)));

        assert!(matches!(
            events.as_slice(),
            [ClientPaneInputEvent::Mouse {
                position: current,
                ..
            }] if *current == position
        ));
    }

    #[test]
    fn stale_shell_pixel_geometry_downgrades_to_its_canonical_cell() {
        let mut events = vec![ClientPaneInputEvent::Mouse {
            kind: crate::protocol::ClientMouseKind::Moved,
            position: crate::protocol::ClientMousePosition::Pixels {
                x: 121,
                y: 81,
                column: 12,
                row: 4,
            },
            geometry: Some(crate::protocol::ClientMouseGeometry {
                cols: 20,
                rows: 5,
                width_px: 200,
                height_px: 100,
            }),
            modifiers: 0,
            lines: 1,
        }];

        downgrade_ineligible_pixel_mouse(&mut events, true, (6, 20), Some((200, 120)));

        assert!(matches!(
            events.as_slice(),
            [ClientPaneInputEvent::Mouse {
                position: crate::protocol::ClientMousePosition::Cell { column: 12, row: 4 },
                geometry: None,
                ..
            }]
        ));
    }
}

#[cfg(test)]
mod polite_send_tests {
    use crate::api::schema::PaneQueueParams;
    use crate::api::schema::{Method, PaneSendKeysParams, PaneSendTextParams, Request};
    use crate::app::App;
    use crate::config::PoliteSendConfig;
    use crate::layout::PaneId;
    use crate::protocol::ClientPaneInputEvent;
    use crate::server::pane_input::{apply_client_pane_input_events, apply_terminal_attach_input};
    use std::time::{Duration, Instant};

    fn fixture(
        mode: PoliteSendConfig,
    ) -> (
        App,
        PaneId,
        String,
        tokio::sync::mpsc::Receiver<bytes::Bytes>,
    ) {
        let (_tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let mut config = crate::config::Config::default();
        config.server.polite_send = mode;
        config.server.polite_send_quiet_secs = 1;
        let mut app = App::new(
            &config,
            crate::app::AppPolicy::TEST,
            None,
            rx,
            crate::api::EventHub::default(),
        );
        app.state.workspaces = vec![crate::workspace::Workspace::test_new("polite")];
        app.state.ensure_test_terminals();
        app.state.active = Some(0);
        let pane = app.state.workspaces[0].tabs[0].root_pane;
        let (runtime, receiver) = crate::terminal::TerminalRuntime::test_with_channel(80, 24);
        let terminal_id = app.state.workspaces[0].terminal_id(pane).unwrap().clone();
        app.terminal_runtimes.insert(terminal_id, runtime);
        let public = app.public_pane_id(0, pane).unwrap();
        (app, pane, public, receiver)
    }

    fn request(app: &mut App, method: Method) -> serde_json::Value {
        serde_json::from_str(&app.handle_api_request(Request {
            id: "polite".into(),
            method,
        }))
        .unwrap()
    }

    #[tokio::test]
    async fn polite_send_raw_reports_do_not_create_a_draft() {
        for report in [
            b"\x1b[I".as_slice(),
            b"\x1b[O",
            b"\x1b[<0;10;5M",
            b"\x1b[M *%",
            b"\x1b[Z",
            b"\x1b]0;title\x07",
            b"\x1bPpayload\x1b\\",
            b"\x1b",
            b"\x1b[200~\x1b[201~",
        ] {
            let (mut app, pane, public, mut rx) = fixture(PoliteSendConfig::All);
            let runtime = app
                .state
                .runtime_for_pane_in_workspace(&app.terminal_runtimes, 0, pane)
                .unwrap();
            apply_terminal_attach_input(runtime, report.to_vec()).unwrap();
            rx.try_recv().unwrap();
            let response = request(
                &mut app,
                Method::PaneSendText(PaneSendTextParams {
                    if_idle: false,
                    human: false,
                    pane_id: public,
                    text: "message".into(),
                }),
            );
            // Recognized keys may start the quiet window, but must never create
            // a draft. Non-key reports do not even start that window.
            if report == b"\x1b[Z" {
                app.flush_polite_sends(Instant::now() + Duration::from_secs(2));
            } else {
                assert_eq!(response["result"]["queued"], false, "{report:?}");
            }
            assert_eq!(rx.try_recv().unwrap().as_ref(), b"message");
        }
    }

    #[tokio::test]
    async fn polite_send_raw_mixed_text_ctrl_j_and_kitty_enter() {
        assert!(crate::input::parse_terminal_key_sequence("\x1b[13u").is_some());
        for draft in [
            b"abc\x1b[I".as_slice(),
            b"\n",
            b"\x1b\r",
            b"\x1b[13;2u",
            b"\x1b[200~pasted\x1b[201~",
        ] {
            let (mut app, pane, public, mut rx) = fixture(PoliteSendConfig::All);
            let runtime = app
                .state
                .runtime_for_pane_in_workspace(&app.terminal_runtimes, 0, pane)
                .unwrap();
            apply_terminal_attach_input(runtime, draft.to_vec()).unwrap();
            rx.try_recv().unwrap();
            let response = request(
                &mut app,
                Method::PaneSendText(PaneSendTextParams {
                    if_idle: false,
                    human: false,
                    pane_id: public,
                    text: "message".into(),
                }),
            );
            assert_eq!(response["result"]["queued"], true);
            app.flush_polite_sends(Instant::now() + Duration::from_secs(2));
            assert!(rx.try_recv().is_err());
            let runtime = app
                .state
                .runtime_for_pane_in_workspace(&app.terminal_runtimes, 0, pane)
                .unwrap();
            apply_terminal_attach_input(runtime, b"\x1b[13u".to_vec()).unwrap();
            rx.try_recv().unwrap();
            app.flush_polite_sends(Instant::now());
            assert!(rx.try_recv().is_err());
            app.flush_polite_sends(Instant::now() + Duration::from_secs(2));
            assert_eq!(rx.try_recv().unwrap().as_ref(), b"message");
        }
    }

    #[tokio::test]
    async fn polite_send_preserves_human_draft_and_fifo_in_both_attach_modes() {
        for raw_attach in [false, true] {
            let (mut app, pane, public, mut rx) = fixture(PoliteSendConfig::All);
            let runtime = app
                .state
                .runtime_for_pane_in_workspace(&app.terminal_runtimes, 0, pane)
                .unwrap();
            if raw_attach {
                apply_terminal_attach_input(runtime, b"draft-abc".to_vec()).unwrap();
            } else {
                apply_client_pane_input_events(
                    runtime,
                    &[ClientPaneInputEvent::TextCommit("draft-abc".into())],
                )
                .unwrap();
            }
            let text = request(
                &mut app,
                Method::PaneSendText(PaneSendTextParams {
                    if_idle: false,
                    human: false,
                    pane_id: public.clone(),
                    text: "MSG-1".into(),
                }),
            );
            let keys = request(
                &mut app,
                Method::PaneSendKeys(PaneSendKeysParams {
                    if_idle: false,
                    human: false,
                    pane_id: public.clone(),
                    keys: vec!["Enter".into()],
                }),
            );
            assert_eq!(text["result"]["queued"], true);
            assert_eq!(text["result"]["queue_position"], 1);
            assert_eq!(keys["result"]["queue_position"], 2);
            assert_eq!(rx.try_recv().unwrap().as_ref(), b"draft-abc");
            assert!(rx.try_recv().is_err());
            let queue = request(
                &mut app,
                Method::PaneQueue(PaneQueueParams {
                    pane_id: public.clone(),
                    flush: false,
                }),
            );
            assert_eq!(queue["result"]["sends"][0]["byte_length"], 5);
            assert!(!queue.to_string().contains("MSG-1"));
            let runtime = app
                .state
                .runtime_for_pane_in_workspace(&app.terminal_runtimes, 0, pane)
                .unwrap();
            if raw_attach {
                apply_terminal_attach_input(runtime, b"\r".to_vec()).unwrap();
            } else {
                let key = crate::input::TerminalKey::new(
                    crossterm::event::KeyCode::Enter,
                    crossterm::event::KeyModifiers::NONE,
                );
                apply_client_pane_input_events(
                    runtime,
                    &[ClientPaneInputEvent::from_terminal_key(key).unwrap()],
                )
                .unwrap();
            }
            app.flush_polite_sends(Instant::now());
            assert_eq!(rx.try_recv().unwrap().as_ref(), b"\r");
            assert!(rx.try_recv().is_err());
            app.flush_polite_sends(Instant::now() + Duration::from_secs(2));
            assert_eq!(rx.try_recv().unwrap().as_ref(), b"MSG-1");
            assert_eq!(rx.try_recv().unwrap().as_ref(), b"\r");
            assert!(rx.try_recv().is_err());
        }
    }

    #[tokio::test]
    async fn polite_send_modes_and_explicit_flush() {
        for (mode, human, queued) in [
            (PoliteSendConfig::All, false, false),
            (PoliteSendConfig::Off, true, false),
            (PoliteSendConfig::Agents, true, false),
            (PoliteSendConfig::All, true, true),
        ] {
            let (mut app, pane, public, mut rx) = fixture(mode);
            if human {
                apply_client_pane_input_events(
                    app.state
                        .runtime_for_pane_in_workspace(&app.terminal_runtimes, 0, pane)
                        .unwrap(),
                    &[ClientPaneInputEvent::Paste("draft".into())],
                )
                .unwrap();
                rx.try_recv().unwrap();
            }
            let response = request(
                &mut app,
                Method::PaneSendText(PaneSendTextParams {
                    if_idle: false,
                    human: false,
                    pane_id: public.clone(),
                    text: "message".into(),
                }),
            );
            assert_eq!(response["result"]["queued"], queued);
            if queued {
                assert!(rx.try_recv().is_err());
                let response = request(
                    &mut app,
                    Method::PaneQueue(PaneQueueParams {
                        pane_id: public,
                        flush: true,
                    }),
                );
                assert_eq!(response["result"]["sends"], serde_json::json!([]));
            }
            assert_eq!(rx.try_recv().unwrap().as_ref(), b"message");
        }
    }

    #[tokio::test]
    async fn polite_send_ctrl_u_clears_draft_and_holds_recent_input() {
        let (mut app, pane, public, mut rx) = fixture(PoliteSendConfig::All);
        let runtime = app
            .state
            .runtime_for_pane_in_workspace(&app.terminal_runtimes, 0, pane)
            .unwrap();
        apply_terminal_attach_input(runtime, b"draft".to_vec()).unwrap();
        apply_terminal_attach_input(runtime, vec![21]).unwrap();
        rx.try_recv().unwrap();
        rx.try_recv().unwrap();
        let response = request(
            &mut app,
            Method::PaneSendText(PaneSendTextParams {
                if_idle: false,
                human: false,
                pane_id: public,
                text: "message".into(),
            }),
        );
        assert_eq!(response["result"]["queued"], true);
        assert!(rx.try_recv().is_err());
        app.flush_polite_sends(Instant::now() + Duration::from_secs(2));
        assert_eq!(rx.try_recv().unwrap().as_ref(), b"message");
    }

    #[tokio::test]
    async fn polite_send_quiet_window_holds_non_draft_input() {
        let (mut app, pane, public, mut rx) = fixture(PoliteSendConfig::All);
        let runtime = app
            .state
            .runtime_for_pane_in_workspace(&app.terminal_runtimes, 0, pane)
            .unwrap();
        apply_terminal_attach_input(runtime, b"\x1b[A".to_vec()).unwrap();
        rx.try_recv().unwrap();
        let response = request(
            &mut app,
            Method::PaneSendText(PaneSendTextParams {
                if_idle: false,
                human: false,
                pane_id: public,
                text: "message".into(),
            }),
        );
        assert_eq!(response["result"]["queued"], true);
        app.flush_polite_sends(Instant::now());
        assert!(rx.try_recv().is_err());
        app.flush_polite_sends(Instant::now() + Duration::from_secs(2));
        assert_eq!(rx.try_recv().unwrap().as_ref(), b"message");
    }
    #[tokio::test]
    async fn polite_send_screen_draft_faint_and_submit_settle() {
        let (mut app, pane, public, mut rx) = fixture(PoliteSendConfig::Agents);
        let terminal_id = app.state.workspaces[0].terminal_id(pane).unwrap().clone();
        app.state
            .terminals
            .get_mut(&terminal_id)
            .unwrap()
            .set_detected_state(
                Some(crate::detect::Agent::Claude),
                crate::detect::AgentState::Idle,
            );
        let runtime = app.terminal_runtimes.get(&terminal_id).unwrap();
        runtime.test_process_pty_bytes(
            "────────\r\n❯ half typed\r\n  continuation\r\n────────".as_bytes(),
        );
        let response = request(
            &mut app,
            Method::PaneSendText(PaneSendTextParams {
                pane_id: public.clone(),
                text: "wake".into(),
                if_idle: false,
                human: false,
            }),
        );
        assert_eq!(response["result"]["queued"], true);
        app.flush_polite_sends(Instant::now() + Duration::from_secs(31));
        assert!(rx.try_recv().is_err());
        let runtime = app.terminal_runtimes.get(&terminal_id).unwrap();
        runtime.test_process_pty_bytes(
            "\x1b[2J\x1b[H────────\r\n❯ \r\n  continuation only\r\n────────".as_bytes(),
        );
        app.flush_polite_sends(Instant::now() + Duration::from_secs(31));
        assert!(rx.try_recv().is_err());
        let runtime = app.terminal_runtimes.get(&terminal_id).unwrap();
        apply_terminal_attach_input(runtime, b"\r".to_vec()).unwrap();
        assert_eq!(rx.try_recv().unwrap().as_ref(), b"\r");
        runtime.test_process_pty_bytes(
            "\x1b[2J\x1b[H────────\r\n❯ \x1b[2mTry \"refactor\"\x1b[0m\r\n────────".as_bytes(),
        );
        app.flush_polite_sends(Instant::now());
        assert!(rx.try_recv().is_err());
        app.flush_polite_sends(Instant::now() + Duration::from_secs(2));
        assert_eq!(rx.try_recv().unwrap().as_ref(), b"wake");
        let runtime = app.terminal_runtimes.get(&terminal_id).unwrap();
        // An empty screen clears a stale key flag (e.g. a dialog choice).
        runtime.record_human_text();
        runtime.test_process_pty_bytes("\x1b[2J\x1b[H────────\r\n❯ \r\n────────".as_bytes());
        let response = request(
            &mut app,
            Method::PaneSendText(PaneSendTextParams {
                pane_id: public,
                text: "next".into(),
                if_idle: false,
                human: false,
            }),
        );
        assert_eq!(response["result"]["queued"], true);
        app.flush_polite_sends(Instant::now() + Duration::from_secs(2));
        assert_eq!(rx.try_recv().unwrap().as_ref(), b"next");
    }

    #[tokio::test]
    async fn polite_send_if_idle_drop_and_human_bypass() {
        let (mut app, pane, public, mut rx) = fixture(PoliteSendConfig::All);
        let send = |if_idle, human, text: &str| {
            Method::PaneSendText(PaneSendTextParams {
                pane_id: public.clone(),
                text: text.into(),
                if_idle,
                human,
            })
        };
        let response = request(&mut app, send(true, false, "idle"));
        assert_eq!(response["result"]["dropped"], false);
        assert_eq!(rx.try_recv().unwrap().as_ref(), b"idle");
        let runtime = app
            .state
            .runtime_for_pane_in_workspace(&app.terminal_runtimes, 0, pane)
            .unwrap();
        runtime.record_human_text();
        let response = request(&mut app, send(true, false, "drop"));
        assert_eq!(response["result"]["dropped"], true);
        assert_eq!(response["result"]["queued"], false);
        assert!(rx.try_recv().is_err());
        request(&mut app, send(false, false, "held"));
        let response = request(&mut app, send(true, false, "drop queued"));
        assert_eq!(response["result"]["dropped"], true);
        let response = request(
            &mut app,
            Method::PaneSendKeys(PaneSendKeysParams {
                pane_id: public.clone(),
                keys: vec!["x".into()],
                if_idle: false,
                human: true,
            }),
        );
        assert_eq!(response["result"]["queued"], false);
        assert_eq!(rx.try_recv().unwrap().as_ref(), b"x");
        app.flush_polite_sends(Instant::now() + Duration::from_secs(2));
        assert!(rx.try_recv().is_err());
        request(
            &mut app,
            Method::PaneSendKeys(PaneSendKeysParams {
                pane_id: public.clone(),
                keys: vec!["ctrl+u".into()],
                if_idle: false,
                human: true,
            }),
        );
        assert_eq!(rx.try_recv().unwrap().as_ref(), b"\x15");
        // Queue alone drops an idle-only send even after the draft clears.
        assert_eq!(
            request(&mut app, send(true, false, "still queued"))["result"]["dropped"],
            true
        );
        app.flush_polite_sends(Instant::now());
        assert!(rx.try_recv().is_err());
        app.flush_polite_sends(Instant::now() + Duration::from_secs(2));
        assert_eq!(rx.try_recv().unwrap().as_ref(), b"held");
    }
}
