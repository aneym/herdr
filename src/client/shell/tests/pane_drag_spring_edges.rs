//! Input/result/snapshot boundary regressions requested by post-merge review.
use super::pane_drag::{dry_run_answer, request_id, sent_methods};
use super::pane_drag_fixture::{
    fixture_surface, three_pane_panes, three_pane_rects, three_pane_splits, FixturePane, COLS, ROWS,
};
use super::*;
use crate::api::schema::Method;
const ORIGIN: &str = "tab_1";
const X: &str = "pane_2";
const Y: &str = "pane_y";
fn surface(s: &ClientShellState) -> Rect {
    s.layout(COLS, ROWS).pane_surface
}
fn compose(s: &mut ClientShellState) {
    s.compose(COLS, ROWS).expect("frame");
}
fn state() -> ClientShellState {
    let mut s = super::pane_drag_fixture::three_pane_state(true);
    s.config.reduce_motion = true;
    compose(&mut s);
    s
}
fn key(s: &mut ClientShellState, code: KeyCode) -> ClientShellInput {
    s.handle_raw_events(vec![RawInputEvent::Key(crate::input::TerminalKey::new(
        code,
        KeyModifiers::empty(),
    ))])
}
fn open(s: &mut ClientShellState) {
    s.handle_input_bytes(&[2]);
    s.handle_input_bytes(b"m");
    assert_eq!(s.mode, ClientShellMode::Move);
}
fn focus(out: &ClientShellInput, id: &str) {
    assert!(matches!(&sent_methods(out)[..], [Method::TabFocus(t)] if t.tab_id == id));
}
fn show(s: &mut ClientShellState, tab: &str) {
    let area = surface(s);
    let panes = if tab == ORIGIN {
        three_pane_panes(three_pane_rects(area.width, area.height))
    } else {
        vec![
            FixturePane {
                pane_id: X.into(),
                rect: SurfaceRect {
                    x: 0,
                    y: 0,
                    width: area.width / 2,
                    height: area.height,
                },
                fill: 'x',
                mouse_reporting: false,
            },
            FixturePane {
                pane_id: Y.into(),
                rect: SurfaceRect {
                    x: area.width / 2,
                    y: 0,
                    width: area.width - area.width / 2,
                    height: area.height,
                },
                fill: 'y',
                mouse_reporting: false,
            },
        ]
    };
    server_shows(s, tab, &panes);
}
fn server_shows(state: &mut ClientShellState, tab: &str, panes: &[FixturePane]) {
    let area = surface(state);
    let mut snapshot = state.snapshot.as_deref().cloned().expect("snapshot");
    snapshot.revision += 1;
    snapshot.focused_tab_id = Some(tab.into());
    for t in &mut snapshot.tabs {
        t.focused = t.tab_id == tab;
    }
    for workspace in &mut snapshot.workspaces {
        if workspace.workspace_id == "ws_1" {
            workspace.active_tab_id = tab.into();
        }
    }
    if !snapshot.panes.iter().any(|pane| pane.pane_id == Y) {
        let mut y = snapshot
            .panes
            .iter()
            .find(|pane| pane.pane_id == X)
            .cloned()
            .expect("tab_2's pane in the snapshot");
        y.pane_id = Y.into();
        snapshot.panes.push(y);
    }
    let focused = panes.first().map(|pane| pane.pane_id.clone());
    snapshot.focused_pane_id = focused.clone();
    for pane in &mut snapshot.panes {
        pane.focused = Some(&pane.pane_id) == focused.as_ref();
    }
    let splits = if tab == ORIGIN {
        three_pane_splits(area.width, area.height, area.height / 2)
    } else {
        Vec::new()
    };
    let mut frame = fixture_surface(area.width, area.height, panes, splits);
    frame.projection_revision = snapshot.revision;
    state.set_snapshot(Box::new(snapshot));
    state.set_pane_surface(frame);
    compose(state);
}

#[test]
fn refused_spring_drop_restores_origin() {
    for error in [false, true] {
        let mut s = state();
        open(&mut s);
        focus(&s.handle_input_bytes(b"]"), "tab_2");
        show(&mut s, "tab_2");
        s.handle_raw_events(vec![RawInputEvent::Key(crate::input::TerminalKey::new(
            KeyCode::Right,
            KeyModifiers::SHIFT,
        ))]);
        let drop = key(&mut s, KeyCode::Enter);
        let id = request_id(&drop);
        let result = if error {
            Err(ClientShellEndpointError {
                code: Some("rejected".into()),
                reason: None,
                message: "refused".into(),
            })
        } else {
            let (mut result, _) = dry_run_answer(surface(&s), (0, 0));
            if let crate::api::schema::ResponseResult::PanePlace { place } = &mut result {
                place.changed = false;
                place.dry_run = false;
            }
            Ok(result)
        };
        let (repaint, actions) = s.handle_endpoint_result("boot-1", &id, result);
        assert!(repaint);
        let out = ClientShellInput {
            actions,
            ..Default::default()
        };
        focus(&out, ORIGIN);
        assert!(s.pane_drag.is_none());
    }
}
#[test]
fn fast_brackets_keep_lift_through_earlier_replies() {
    let mut s = state();
    open(&mut s);
    focus(&s.handle_input_bytes(b"]"), "tab_2");
    focus(&s.handle_input_bytes(b"]"), ORIGIN);
    show(&mut s, "tab_2");
    assert_eq!(s.mode, ClientShellMode::Move);
    show(&mut s, ORIGIN);
    assert_eq!(s.mode, ClientShellMode::Move);
    assert!(s.chrome_drag.is_some());
}
#[test]
fn foreign_focus_ends_drag_without_restore() {
    let mut s = state();
    open(&mut s);
    focus(&s.handle_input_bytes(b"]"), "tab_2");
    show(&mut s, "tab_2");
    let mut snap = s.snapshot.as_deref().cloned().expect("snapshot");
    snap.focused_tab_id = Some("foreign".into());
    s.set_snapshot(Box::new(snap));
    assert!(s.pane_drag.is_none());
    assert!(sent_methods(&key(&mut s, KeyCode::Esc)).is_empty());
}
#[test]
fn origin_return_cancel_has_no_redundant_focus() {
    let mut s = state();
    open(&mut s);
    focus(&s.handle_input_bytes(b"]"), "tab_2");
    show(&mut s, "tab_2");
    focus(&s.handle_input_bytes(b"["), ORIGIN);
    show(&mut s, ORIGIN);
    assert!(sent_methods(&key(&mut s, KeyCode::Esc)).is_empty());
    assert!(s.pane_drag.is_none());
}
#[test]
fn esc_exits_orphan_move_without_pane_input() {
    let mut s = state();
    open(&mut s);
    s.chrome_drag = None;
    let out = key(&mut s, KeyCode::Esc);
    assert!(out.requests.is_empty(), "Esc must not reach the program");
    assert_eq!(s.mode, ClientShellMode::Terminal);
}
#[test]
fn own_tab_row_dwells_but_never_offers_drop() {
    for sidebar in [false, true] {
        let mut s = state();
        open(&mut s);
        focus(&s.handle_input_bytes(b"]"), "tab_2");
        show(&mut s, "tab_2");
        let rect = if sidebar {
            s.hits
                .tree_headers
                .iter()
                .find(|h| h.tab_id.as_deref() == Some(ORIGIN))
                .expect("origin row")
                .rect
        } else {
            s.hits
                .tabs
                .iter()
                .find(|(_, id)| id == ORIGIN)
                .expect("origin tab")
                .0
        };
        let point = (rect.x + 1, rect.y);
        s.handle_raw_events(vec![super::pane_drag_fixture::mouse(
            MouseEventKind::Drag(MouseButton::Left),
            point.0,
            point.1,
        )]);
        assert!(
            matches!(
                s.chrome_drag,
                Some(ClientChromeDrag::Pane { target: None, .. })
            ),
            "own tab is not a drop zone"
        );
        focus(
            &s.tick_pane_spring(std::time::Instant::now() + std::time::Duration::from_millis(450)),
            ORIGIN,
        );
    }
}
