//! Real input/result/patch boundary regression for refused drops. Successful
//! drops have coverage already; refusal must release incremental rendering.
use super::pane_drag::{lift_a_to, request_id};
use super::pane_drag_fixture::{mouse, three_pane_state, B, COLS, ROWS};
use super::*;
#[test]
fn refused_drop_releases_the_ghost_and_patch_fast_path() {
    let mut state = three_pane_state(true);
    state.config.reduce_motion = true;
    state.compose(COLS, ROWS);
    let b = state
        .hits
        .panes
        .iter()
        .find(|h| h.pane_id == B)
        .expect("B")
        .rect;
    let point = (b.x + b.width / 2, b.y + b.height / 2);
    lift_a_to(&mut state, point);
    let drop = state.handle_raw_events(vec![mouse(
        MouseEventKind::Up(MouseButton::Left),
        point.0,
        point.1,
    )]);
    let id = request_id(&drop);
    assert!(state.pane_drag.as_ref().is_some_and(|p| p.committed));
    state.handle_endpoint_result(
        "boot-1",
        &id,
        Err(ClientShellEndpointError {
            code: Some("rejected".into()),
            reason: None,
            message: "drop refused".into(),
        }),
    );
    assert!(
        state.pane_drag.is_none(),
        "a refused drop must end its lift and ghost"
    );
    state.visible_endpoint_notice = None;
    state.compose(COLS, ROWS);
    let s = state.pane_surface.as_ref().expect("surface");
    let patch = crate::protocol::PaneSurfacePatch {
        boot_id: s.boot_id.clone(),
        projection_revision: s.projection_revision,
        base_surface_revision: s.surface_revision,
        surface_revision: s.surface_revision + 1,
        rows: vec![],
        panes: vec![],
        cursor: s.frame.cursor.clone(),
    };
    assert!(
        matches!(
            state.apply_pane_surface_patch(patch),
            super::super::surface_patch::ClientPaneSurfacePatchOutcome::Applied(Some(_))
        ),
        "refusal must release patch fast path"
    );
}
