//! Post-review lifecycle regressions: real input, surface, endpoint and compose
//! boundaries. Existing drag tests cover only an unchanged layout and success.
use super::pane_drag_fixture::{mouse, three_pane_state, A, B, C, COLS, ROWS};
use super::*;
use crate::api::schema::{Method, PanePlaceTarget};

fn start(state: &mut ClientShellState, target: &str) -> (ClientShellInput, (u16, u16)) {
    state.compose(COLS, ROWS).expect("frame");
    let a = state
        .hits
        .panes
        .iter()
        .find(|h| h.pane_id == A)
        .expect("source")
        .rect;
    let b = state
        .hits
        .panes
        .iter()
        .find(|h| h.pane_id == target)
        .expect("target")
        .rect;
    let point = (b.right() - 4, b.y + b.height / 2);
    state.handle_raw_events(vec![mouse(
        MouseEventKind::Down(MouseButton::Left),
        a.x + (a.width - 2) / 2,
        a.y,
    )]);
    (move_to(state, point), point)
}
fn move_to(state: &mut ClientShellState, point: (u16, u16)) -> ClientShellInput {
    state.handle_raw_events(vec![mouse(
        MouseEventKind::Drag(MouseButton::Left),
        point.0,
        point.1,
    )])
}
fn request(outcome: &ClientShellInput) -> (&str, &Method) {
    outcome
        .actions
        .iter()
        .find_map(|a| match a {
            ClientShellAction::Endpoint { request, .. } => {
                Some((request.id.as_str(), &request.method))
            }
            _ => None,
        })
        .expect("endpoint request")
}

#[test]
fn pane_drag_closed_target_cancels_before_old_area_move() {
    let mut state = three_pane_state(true);
    let (_, point) = start(&mut state, C);
    let mut surface = state.pane_surface.as_ref().expect("surface").clone();
    surface.panes.retain(|p| p.pane_id != C);
    surface.surface_revision += 1;
    state.set_pane_surface(surface);
    state.compose(COLS, ROWS).expect("closed pane frame");
    let moved = move_to(&mut state, point);
    assert!(state.pane_drag.is_none(), "closed target cancels drag");
    assert!(!moved.actions.iter().any(|a| matches!(a,
        ClientShellAction::Endpoint { request, .. } if matches!(&request.method,
            Method::PanePlace(p) if p.target == PanePlaceTarget::Pane { pane_id: C.into() }))));
}

#[test]
fn pane_drag_resize_reissues_same_zone_dry_run() {
    let mut state = three_pane_state(true);
    let (first, point) = start(&mut state, B);
    let first_id = request(&first).0.to_owned();
    // Resolve and cache an error, then resize without changing ids/split paths.
    state.handle_endpoint_result(
        "boot-1",
        &first_id,
        Err(ClientShellEndpointError {
            reason: None,
            code: None,
            message: "temporary failure".into(),
        }),
    );
    let mut surface = state.pane_surface.as_ref().expect("surface").clone();
    surface
        .panes
        .iter_mut()
        .find(|p| p.pane_id == B)
        .expect("B")
        .rect
        .width += 1;
    surface.surface_revision += 1;
    state.set_pane_surface(surface);
    state.compose(COLS, ROWS).expect("resized frame");
    let moved = move_to(&mut state, point);
    let (id, method) = request(&moved);
    assert_ne!(id, first_id);
    assert!(
        matches!(method, Method::PanePlace(p) if p.dry_run && p.target == PanePlaceTarget::Pane { pane_id: B.into() })
    );
}

#[test]
fn pane_drag_output_patch_recomposes_ghost_corners() {
    let mut state = three_pane_state(true);
    start(&mut state, B);
    let ghost = state
        .pane_drag
        .as_ref()
        .expect("drag")
        .ghost
        .expect("estimated ghost");
    state.compose(COLS, ROWS).expect("drag frame");
    let surface = state.pane_surface.as_ref().expect("surface");
    let mut pane = surface
        .panes
        .iter()
        .find(|p| p.pane_id == B)
        .expect("B")
        .clone();
    pane.content_revision += 1;
    let x = pane.inner_rect.x;
    let y = pane.inner_rect.y;
    let patch = crate::protocol::PaneSurfacePatch {
        boot_id: surface.boot_id.clone(),
        projection_revision: surface.projection_revision,
        base_surface_revision: surface.surface_revision,
        surface_revision: surface.surface_revision + 1,
        rows: vec![crate::protocol::PaneSurfacePatchRow {
            x,
            y,
            cells: vec![surface.frame.cells
                [usize::from(y) * usize::from(surface.frame.width) + usize::from(x)]
            .clone()],
        }],
        panes: vec![pane],
        cursor: None,
    };
    assert!(
        matches!(
            state.apply_pane_surface_patch(patch),
            ClientPaneSurfacePatchOutcome::Applied(None)
        ),
        "drag must block direct frame patch"
    );
    let frame = state.compose(COLS, ROWS).expect("patched drag frame");
    let symbol = |x, y| {
        frame.cells[usize::from(y) * usize::from(frame.width) + usize::from(x)]
            .symbol
            .as_str()
    };
    assert_eq!(symbol(ghost.x, ghost.y), "╭");
    assert_eq!(symbol(ghost.right() - 1, ghost.bottom() - 1), "╯");
}

#[test]
fn pane_drag_dry_run_error_is_not_no_change() {
    let mut state = three_pane_state(true);
    let (outcome, _) = start(&mut state, B);
    state.handle_endpoint_result(
        "boot-1",
        request(&outcome).0,
        Err(ClientShellEndpointError {
            reason: None,
            code: Some("endpoint_timeout".into()),
            message: "timeout".into(),
        }),
    );
    let preview = state.pane_drag.as_ref().expect("drag");
    assert!(matches!(
        &preview.answers[..],
        [(_, super::super::pane_drag::PaneDragAnswer::Error)]
    ));
    assert!(
        preview.ghost.is_none(),
        "error cannot display an approved placement"
    );
}
