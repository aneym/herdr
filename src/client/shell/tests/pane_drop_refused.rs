//! Real input/result/patch boundary regression: refusal preserves notices and
//! releases both the ghost and incremental rendering, with or without motion.
use super::pane_drag::{dry_run_answer, lift_a_to, request_id};
use super::pane_drag_fixture::{mouse, three_pane_state, B, COLS, ROWS};
use super::*;
#[test]
fn refused_drop_releases_the_ghost_and_patch_fast_path() {
    for reduced in [true, false] {
        for code in [
            Some("rejected"),
            Some("endpoint_timeout"),
            Some("server_unavailable"),
            None,
        ] {
            let mut state = three_pane_state(true);
            state.config.reduce_motion = reduced;
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
            if let Some(m) = state.pane_motion.first() {
                state.tick_pane_motion(m.started_at + m.duration);
            }
            let drop = state.handle_raw_events(vec![mouse(
                MouseEventKind::Up(MouseButton::Left),
                point.0,
                point.1,
            )]);
            let id = request_id(&drop);
            assert!(state.pane_drag.as_ref().is_some_and(|p| p.committed));
            let result = if let Some(code) = code {
                Err(ClientShellEndpointError {
                    code: Some(code.into()),
                    reason: None,
                    message: "drop refused".into(),
                })
            } else {
                let (mut answer, _) = dry_run_answer(state.layout(COLS, ROWS).pane_surface, (0, 0));
                if let crate::api::schema::ResponseResult::PanePlace { place } = &mut answer {
                    place.changed = false;
                    place.dry_run = false;
                }
                Ok(answer)
            };
            state.handle_endpoint_result("boot-1", &id, result);
            if let Some(code) = code {
                let notice = state
                    .visible_endpoint_notice
                    .as_ref()
                    .expect("refused drop must show an endpoint notice");
                assert_eq!(
                    notice.title,
                    match code {
                        "endpoint_timeout" => "Server timed out",
                        "server_unavailable" => "Server unavailable",
                        _ => "Action rejected",
                    }
                );
                state.tick_notifications(notice.deadline);
            } else {
                assert!(
                    state.visible_endpoint_notice.is_none(),
                    "no-op replies are not errors"
                );
            }
            if !reduced {
                assert!(state.pane_drag.as_ref().is_some_and(|p| !p.committed));
                let motion = state.pane_motion.first().expect("animated refusal");
                assert!(matches!(
                    motion.kind,
                    super::super::pane_motion::PaneMotionKind::Cancel
                ));
                state.tick_pane_motion(motion.started_at + motion.duration);
            }
            assert!(
                state.pane_drag.is_none(),
                "refusal must end its lift and ghost"
            );
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
    }
}

#[test]
fn unsupported_drop_is_not_committed() {
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
    for endpoint in &mut state.endpoints {
        if let Some(methods) = &mut endpoint.methods {
            methods.retain(|m| m != "pane.swap");
        }
    }
    let drop = state.handle_raw_events(vec![mouse(
        MouseEventKind::Up(MouseButton::Left),
        point.0,
        point.1,
    )]);
    assert!(super::pane_drag::sent_methods(&drop).is_empty());
    assert!(
        state.pane_drag.is_none(),
        "an unsent drop cannot wait for a surface forever"
    );
    assert_eq!(
        state
            .visible_endpoint_notice
            .as_ref()
            .expect("unsupported notice")
            .title,
        "Action unavailable"
    );
}
