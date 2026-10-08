//! Snapshot/input boundary regressions for acknowledged spring requests.
//! Drive real move-mode input and recorded fixture snapshots; no endpoint
//! implementation is mocked. The observable contract is drag survival and
//! outbound methods, rather than the preview's internal request bookkeeping.
use super::pane_drag::sent_methods;
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

fn spring_next(state: &mut ClientShellState, tab: &str) {
    let output = state.handle_input_bytes(b"]");
    assert!(
        matches!(&sent_methods(&output)[..], [Method::TabFocus(target)] if target.tab_id == tab)
    );
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
    spring_next(&mut state, "tab_2");
    show(&mut state, "tab_2");
    assert_eq!(state.mode, ClientShellMode::Move);
    spring_next(&mut state, "tab_3");
    show(&mut state, "tab_3");
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
