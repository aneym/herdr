//! Snapshot/input boundary regressions for acknowledged spring requests.
//! Drive real move-mode input and recorded fixture snapshots; no endpoint
//! implementation is mocked. The observable contract is drag survival and
//! outbound methods, rather than the preview's internal request bookkeeping.
use super::pane_drag::{request_id, sent_methods};
use super::pane_drag_fixture::{COLS, ROWS};
use super::*;
use crate::api::schema::Method;

fn state() -> ClientShellState {
    let mut state = super::pane_drag_fixture::three_pane_state(true);
    state.config.reduce_motion = true;
    let mut snapshot = state.snapshot.as_deref().cloned().expect("snapshot");
    let third = snapshot
        .tabs
        .iter_mut()
        .find(|tab| tab.tab_id == "tab_3")
        .expect("third fixture tab");
    third.workspace_id = "ws_1".into();
    third.number = 3;
    state.set_snapshot(Box::new(snapshot));
    state.compose(COLS, ROWS).expect("frame");
    state.handle_input_bytes(&[2]);
    state.handle_input_bytes(b"m");
    assert_eq!(state.mode, ClientShellMode::Move);
    state
}

fn spring(state: &mut ClientShellState, key: &[u8], tab: &str) -> String {
    let output = state.handle_input_bytes(key);
    assert!(
        matches!(&sent_methods(&output)[..], [Method::TabFocus(target)] if target.tab_id == tab)
    );
    request_id(&output)
}

fn spring_next(state: &mut ClientShellState, tab: &str) -> String {
    spring(state, b"]", tab)
}

fn reply(state: &mut ClientShellState, id: &str, ok: bool) {
    let result = if ok {
        Ok(crate::api::schema::ResponseResult::Ok {})
    } else {
        Err(ClientShellEndpointError {
            code: Some("rejected".into()),
            reason: None,
            message: "focus refused".into(),
        })
    };
    let (_, actions) = state.handle_endpoint_result("boot-1", id, result);
    assert!(actions.is_empty(), "focus replies must send nothing");
}

fn show(state: &mut ClientShellState, tab: &str) {
    let mut snapshot = state.snapshot.as_deref().cloned().expect("snapshot");
    snapshot.revision += 1;
    snapshot.focused_tab_id = Some(tab.into());
    for entry in &mut snapshot.tabs {
        entry.focused = entry.tab_id == tab;
    }
    snapshot.workspaces[0].active_tab_id = tab.into();
    state.set_snapshot(Box::new(snapshot));
}

#[test]
fn foreign_focus_on_acknowledged_earlier_tab_ends_drag_without_restore() {
    let mut state = state();
    let second = spring_next(&mut state, "tab_2");
    show(&mut state, "tab_2");
    reply(&mut state, &second, true);
    assert_eq!(state.mode, ClientShellMode::Move);
    let third = spring_next(&mut state, "tab_3");
    show(&mut state, "tab_3");
    reply(&mut state, &third, true);
    assert_eq!(state.mode, ClientShellMode::Move);

    show(&mut state, "tab_2");
    assert!(state.pane_drag.is_none(), "foreign focus must end the lift");
    assert!(state.chrome_drag.is_none());
    assert_eq!(state.mode, ClientShellMode::Terminal);
    let output = state.handle_input_bytes(b"\x1b");
    assert!(sent_methods(&output).is_empty(), "no origin restore");
}

#[test]
fn origin_snapshot_before_spring_acknowledgement_keeps_drag_alive() {
    let mut state = state();
    spring_next(&mut state, "tab_2");
    show(&mut state, "tab_1");
    assert_eq!(state.mode, ClientShellMode::Move);
    assert!(state.pane_drag.is_some());
    assert!(state.chrome_drag.is_some());
    show(&mut state, "tab_2");
    assert_eq!(state.mode, ClientShellMode::Move);
    assert!(state.pane_drag.is_some());
    assert!(state.chrome_drag.is_some());
}

fn assert_alive(state: &ClientShellState) {
    assert_eq!(state.mode, ClientShellMode::Move);
    assert!(state.pane_drag.is_some());
    assert!(state.chrome_drag.is_some());
}

#[test]
fn stale_origin_snapshot_does_not_acknowledge_fast_round_trip() {
    let mut state = state();
    let second = spring_next(&mut state, "tab_2");
    let origin = spring(&mut state, b"[", "tab_1");
    show(&mut state, "tab_1");
    assert_alive(&state);
    reply(&mut state, &second, true);
    show(&mut state, "tab_2");
    assert_alive(&state);
    reply(&mut state, &origin, true);
    show(&mut state, "tab_1");
    assert_alive(&state);
}

#[test]
fn repeated_spring_to_same_tab_remains_pending_until_its_own_reply() {
    let mut state = state();
    let second = spring_next(&mut state, "tab_2");
    let origin = spring(&mut state, b"[", "tab_1");
    let second_again = spring_next(&mut state, "tab_2");
    reply(&mut state, &second, true);
    show(&mut state, "tab_2");
    assert_alive(&state);
    reply(&mut state, &origin, true);
    show(&mut state, "tab_1");
    assert_alive(&state);
    reply(&mut state, &second_again, true);
    show(&mut state, "tab_2");
    assert_alive(&state);
}

#[test]
fn replies_prune_visited_tabs_even_without_matching_snapshots() {
    let mut state = state();
    let second = spring_next(&mut state, "tab_2");
    let third = spring_next(&mut state, "tab_3");
    reply(&mut state, &second, true);
    reply(&mut state, &third, true);
    show(&mut state, "tab_2");
    assert!(state.pane_drag.is_none());
    assert!(state.chrome_drag.is_none());
    assert!(sent_methods(&state.handle_input_bytes(b"\x1b")).is_empty());
}

#[test]
fn refused_spring_focus_reply_ends_drag_quietly() {
    let mut state = state();
    let second = spring_next(&mut state, "tab_2");
    reply(&mut state, &second, false);
    assert!(state.pane_drag.is_none());
    assert!(state.chrome_drag.is_none());
    assert_eq!(state.mode, ClientShellMode::Terminal);
    assert!(state.endpoint_notice_seen.is_empty());
    assert!(sent_methods(&state.handle_input_bytes(b"\x1b")).is_empty());
}

#[test]
fn previous_drag_reply_cannot_cancel_or_acknowledge_new_drag() {
    let mut state = state();
    let old = spring_next(&mut state, "tab_2");
    state.cancel_pane_drag();
    state.handle_input_bytes(&[2]);
    state.handle_input_bytes(b"m");
    let current = spring_next(&mut state, "tab_2");
    reply(&mut state, &old, false);
    assert_alive(&state);
    show(&mut state, "tab_1");
    assert_alive(&state);
    reply(&mut state, &current, true);
    show(&mut state, "tab_2");
    assert_alive(&state);
}
