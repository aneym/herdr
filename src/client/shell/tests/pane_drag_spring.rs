//! Owner-written check for pane drag slice S8, spring-loaded tabs on the TUI
//! (spec `pane-drag-rearrange-2026-10-07`, sections "Spring-loaded tabs",
//! "Cancel", "Keyboard", "Where it lands" and "S8"). Implementers may not edit
//! this file.
//!
//! The one interface S8 adds for this check:
//!
//! ```text
//! impl ClientShellState {
//!     pub(crate) fn tick_pane_spring(&mut self, now: std::time::Instant) -> ClientShellInput;
//! }
//! ```
//!
//! It runs in the client's tick chain (`src/client/mod.rs`, beside
//! `tick_pane_motion`), and `timer_delay` wakes the client by the dwell's
//! deadline. When the pointer of a lifted pane drag has rested on a tab-bar tab
//! or a sidebar tab row of the same endpoint for `motion.springLoadMs`
//! (450 ms) with no travel of 1 cell or more, the tick returns one
//! `tab.focus` for that tab. Ticks are pure in `now`: a test may tick far
//! ahead and then feed input stamped with the earlier wall clock.
//!
//! Until S8 adds the inherent method, the `SpringSeam` stand-in below makes
//! this file compile and each dwell test fail with the missing seam's name.
//! Inherent methods win method resolution, so once S8's method exists the
//! stand-in is never called.
//!
//! Time is injected, never slept. Input stamps the dwell with the wall clock,
//! so each test brackets the input with `Instant::now()` and ticks relative to
//! the bracket: a tick before `before + 450 ms` is early for sure, a tick at
//! `after + 450 ms` is due for sure.
//!
//! Everything else runs through the real input path (`handle_raw_events`,
//! `handle_input_bytes`), the real composer and the S3 fixture: tab `tab_1`
//! shows `B | C` over `A`, and `tab_2` is the other tab of workspace `ws_1`.
//! When the client asks for `tab.focus`, the test plays the server: the next
//! snapshot focuses that tab and its surface shows `X | Y`. Motion is reduced,
//! so ghosts and cancels apply at once; `pane_motion.rs` owns motion.

use super::pane_drag::{lift_a_to, sent_methods};
use super::pane_drag_fixture::{
    fixture_surface, mouse, three_pane_panes, three_pane_rects, three_pane_splits, FixturePane, A,
    B, COLS, ROWS,
};
use super::*;
use crate::api::schema::{Method, PaneDirection, PanePlaceTarget};
use std::time::{Duration, Instant};

/// The other tab of `ws_1`, and its two panes once the server shows it.
const TAB_2: &str = "tab_2";
const ORIGIN: &str = "tab_1";
const X: &str = "pane_2";
const Y: &str = "pane_y";
/// `motion.springLoadMs`.
const DWELL: Duration = Duration::from_millis(450);

const PREFIX: &[u8] = &[0x02];
const ENTER: &[u8] = b"\r";
const SHIFT_RIGHT: &[u8] = b"\x1b[1;2C";

/// Stand-in for S8's `ClientShellState::tick_pane_spring`; see the module docs.
// Never called once S8 adds the inherent method, which wins method resolution.
#[allow(dead_code)]
trait SpringSeam {
    fn tick_pane_spring(&mut self, _now: Instant) -> ClientShellInput {
        panic!(
            "S8 seam missing: ClientShellState::tick_pane_spring(&mut self, now: Instant) -> ClientShellInput"
        )
    }
}
impl SpringSeam for ClientShellState {}

fn ms(millis: u64) -> Duration {
    Duration::from_millis(millis)
}

fn state() -> ClientShellState {
    let mut state = super::pane_drag_fixture::three_pane_state(true);
    state.config.reduce_motion = true;
    state
}

fn compose(state: &mut ClientShellState) {
    state.compose(COLS, ROWS).expect("frame");
}

fn surface(state: &ClientShellState) -> Rect {
    state.layout(COLS, ROWS).pane_surface
}

fn on_screen(state: &ClientShellState, id: &str) -> Rect {
    let area = surface(state);
    let (_, rect) = three_pane_rects(area.width, area.height)
        .into_iter()
        .find(|(pane_id, _)| *pane_id == id)
        .expect("fixture pane");
    Rect::new(area.x + rect.x, area.y + rect.y, rect.width, rect.height)
}

fn centre(rect: Rect) -> (u16, u16) {
    (rect.x + rect.width / 2, rect.y + rect.height / 2)
}

/// The tab-bar cell `column` cells into `tab_2`'s tab.
fn tab_2_in_bar(state: &ClientShellState, column: u16) -> (u16, u16) {
    let (rect, _) = state
        .hits
        .tabs
        .iter()
        .find(|(_, tab_id)| tab_id == TAB_2)
        .expect("tab_2 in the tab bar");
    assert!(rect.width >= 3, "tab_2's tab is wide enough to move within: {rect:?}");
    (rect.x + column, rect.y)
}

/// A plain cell of `tab_2`'s row in the sidebar tree.
fn tab_2_in_sidebar(state: &ClientShellState) -> (u16, u16) {
    let row = state
        .hits
        .tree_headers
        .iter()
        .find(|hit| hit.tab_id.as_deref() == Some(TAB_2))
        .expect("tab_2's row in the sidebar tree");
    let column = (row.rect.x..row.rect.right())
        .find(|x| {
            ![row.chevron, row.plus, row.pin]
                .iter()
                .any(|control| contains(*control, (*x, row.rect.y)))
        })
        .expect("a plain cell on tab_2's row");
    (column, row.rect.y)
}

fn drag_to(state: &mut ClientShellState, point: (u16, u16)) -> ClientShellInput {
    state.handle_raw_events(vec![mouse(
        MouseEventKind::Drag(MouseButton::Left),
        point.0,
        point.1,
    )])
}

/// Run `input` and return its outcome with the wall clock just before and
/// just after it: the window the dwell's stamp falls in.
fn bracket(
    state: &mut ClientShellState,
    input: impl FnOnce(&mut ClientShellState) -> ClientShellInput,
) -> (Instant, ClientShellInput, Instant) {
    let before = Instant::now();
    let outcome = input(state);
    let after = Instant::now();
    (before, outcome, after)
}

fn is_tab_focus(method: &Method, tab: &str) -> bool {
    matches!(method, Method::TabFocus(target) if target.tab_id == tab)
}

/// Exactly one `tab.focus` for `tab`, and nothing else.
fn assert_only_tab_focus(outcome: &ClientShellInput, tab: &str, step: &str) {
    let sent = sent_methods(outcome);
    assert!(
        matches!(&sent[..], [method] if is_tab_focus(method, tab)),
        "{step}: exactly one tab.focus({tab}) is sent: {sent:?}"
    );
}

fn assert_nothing_sent(outcome: &ClientShellInput, step: &str) {
    assert!(
        sent_methods(outcome).is_empty(),
        "{step}: nothing is sent: {:?}",
        sent_methods(outcome)
    );
}

fn assert_no_pane_input(outcome: &ClientShellInput, step: &str) {
    assert!(
        !outcome.requests.iter().any(|request| matches!(
            request,
            ClientMessage::ClientShellPaneInput { .. } | ClientMessage::ClientShellPopupInput { .. }
        )),
        "{step}: a pane program received input: {:?}",
        outcome.requests
    );
}

fn assert_lifted(state: &ClientShellState, step: &str) {
    assert!(
        matches!(&state.chrome_drag, Some(ClientChromeDrag::Pane { source_pane_id, .. }) if source_pane_id == A),
        "{step}: A is still lifted"
    );
}

/// The server's answer to `tab.focus(tab)`: the next snapshot focuses `tab`
/// and its surface shows `panes`.
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

/// Tab 2's surface: `X | Y`, side by side, full height.
fn tab_2_panes(state: &ClientShellState) -> Vec<FixturePane> {
    let area = surface(state);
    let half = area.width / 2;
    vec![
        FixturePane {
            pane_id: X.into(),
            rect: SurfaceRect { x: 0, y: 0, width: half, height: area.height },
            fill: 'x',
            mouse_reporting: false,
        },
        FixturePane {
            pane_id: Y.into(),
            rect: SurfaceRect { x: half, y: 0, width: area.width - half, height: area.height },
            fill: 'y',
            mouse_reporting: false,
        },
    ]
}

fn tab_1_panes(state: &ClientShellState) -> Vec<FixturePane> {
    let area = surface(state);
    three_pane_panes(three_pane_rects(area.width, area.height))
}

/// Y's rect on screen while tab 2 shows.
fn y_rect(state: &ClientShellState) -> Rect {
    let area = surface(state);
    let half = area.width / 2;
    Rect::new(area.x + half, area.y, area.width - half, area.height)
}

/// Lift A, rest on `tab_2` in the tab bar for the dwell, and play the
/// server's switch. Returns with tab 2 shown and A still lifted.
fn spring_to_tab_2(state: &mut ClientShellState) {
    let over = tab_2_in_bar(state, 1);
    let (_, moved, after) = bracket(state, |s| lift_a_to(s, over).1);
    assert_nothing_sent(&moved, "lifting onto tab_2");
    let fired = state.tick_pane_spring(after + DWELL);
    assert_only_tab_focus(&fired, TAB_2, "the dwell");
    assert_lifted(state, "after the spring-load");
    let panes = tab_2_panes(state);
    server_shows(state, TAB_2, &panes);
    assert_lifted(state, "once the server shows tab_2");
}

/// Dwell 450 ms over a tab: one `tab.focus(tab_2)`, on time and once. The
/// drag continues into tab 2, whose zones apply: an edge of one of its panes
/// asks a dry run against that pane, and the release places A there, not
/// into the tab.
#[test]
fn spring_dwell_focuses_the_tab_and_the_drop_uses_its_zones() {
    let mut state = state();
    let over = tab_2_in_bar(&state, 1);
    let (before, moved, after) = bracket(&mut state, |s| lift_a_to(s, over).1);
    assert_nothing_sent(&moved, "lifting onto tab_2");
    assert_lifted(&state, "over tab_2");
    assert!(
        state.timer_delay(after + ms(400)) <= ms(50),
        "the client wakes for the dwell's deadline, got {:?}",
        state.timer_delay(after + ms(400))
    );
    assert_nothing_sent(
        &state.tick_pane_spring(before + DWELL - ms(1)),
        "1 ms before the dwell ends",
    );
    let fired = state.tick_pane_spring(after + DWELL);
    assert_only_tab_focus(&fired, TAB_2, "the dwell");
    assert_no_pane_input(&fired, "the dwell");
    assert_lifted(&state, "after the spring-load");
    assert_nothing_sent(
        &state.tick_pane_spring(after + DWELL + ms(100)),
        "the spring-load fires once",
    );

    let panes = tab_2_panes(&state);
    server_shows(&mut state, TAB_2, &panes);
    assert_lifted(&state, "once the server shows tab_2");
    assert_nothing_sent(
        &state.tick_pane_spring(after + ms(2000)),
        "resting on the tab now shown springs nothing",
    );

    let y = y_rect(&state);
    let band = (y.right() - 4, y.y + y.height / 2);
    let edge = drag_to(&mut state, band);
    assert_no_pane_input(&edge, "Y's right band");
    assert!(
        matches!(&sent_methods(&edge)[..], [Method::PanePlace(params)]
            if params.pane_id == A
                && params.target == PanePlaceTarget::Pane { pane_id: Y.into() }
                && params.side == PaneDirection::Right
                && params.dry_run),
        "tab 2's zones apply: one dry run for Y's right edge: {:?}",
        sent_methods(&edge)
    );
    let drop = state.handle_raw_events(vec![mouse(
        MouseEventKind::Up(MouseButton::Left),
        band.0,
        band.1,
    )]);
    assert_no_pane_input(&drop, "release");
    assert!(
        matches!(&sent_methods(&drop)[..], [Method::PanePlace(params)]
            if params.pane_id == A
                && params.target == PanePlaceTarget::Pane { pane_id: Y.into() }
                && params.side == PaneDirection::Right
                && !params.dry_run
                && params.focus),
        "the release places A right of Y, not into the tab: {:?}",
        sent_methods(&drop)
    );
}

/// After a spring-load the tab edge is the new tab's edge.
#[test]
fn spring_tab_edge_targets_the_spring_loaded_tab() {
    let mut state = state();
    spring_to_tab_2(&mut state);
    let area = surface(&state);
    let edge = (area.right() - 1, area.y + area.height / 2);
    let moved = drag_to(&mut state, edge);
    assert!(
        matches!(&sent_methods(&moved)[..], [Method::PanePlace(params)]
            if params.pane_id == A
                && params.target == PanePlaceTarget::Tab { tab_id: TAB_2.into() }
                && params.side == PaneDirection::Right
                && params.dry_run),
        "the right tab edge asks a dry run against tab_2: {:?}",
        sent_methods(&moved)
    );
    let drop = state.handle_raw_events(vec![mouse(
        MouseEventKind::Up(MouseButton::Left),
        edge.0,
        edge.1,
    )]);
    assert!(
        matches!(&sent_methods(&drop)[..], [Method::PanePlace(params)]
            if params.target == PanePlaceTarget::Tab { tab_id: TAB_2.into() }
                && params.side == PaneDirection::Right
                && !params.dry_run),
        "the release places A along tab_2's right edge: {:?}",
        sent_methods(&drop)
    );
}

/// A sidebar tab row springs as a tab-bar tab does.
#[test]
fn spring_dwell_over_a_sidebar_tab_row() {
    let mut state = state();
    let over = tab_2_in_sidebar(&state);
    let (before, moved, after) = bracket(&mut state, |s| lift_a_to(s, over).1);
    assert_nothing_sent(&moved, "lifting onto tab_2's row");
    assert_nothing_sent(
        &state.tick_pane_spring(before + DWELL - ms(1)),
        "1 ms before the dwell ends",
    );
    assert_only_tab_focus(
        &state.tick_pane_spring(after + DWELL),
        TAB_2,
        "the dwell on the sidebar row",
    );
    assert_lifted(&state, "after the spring-load");
}

/// Leaving the tab at 300 ms sends no focus, and coming back starts a new
/// 450 ms dwell.
#[test]
fn spring_leaving_before_the_dwell_sends_nothing() {
    let mut state = state();
    let over = tab_2_in_bar(&state, 1);
    let (before, _, after) = bracket(&mut state, |s| lift_a_to(s, over).1);
    assert_nothing_sent(&state.tick_pane_spring(before + ms(300)), "300 ms on tab_2");
    let b_centre = centre(on_screen(&state, B));
    let away = drag_to(&mut state, b_centre);
    assert_nothing_sent(&away, "leaving for B's centre");
    for at in [DWELL, ms(1000)] {
        assert_nothing_sent(
            &state.tick_pane_spring(after + at),
            "after leaving, the dwell is gone",
        );
    }
    assert_lifted(&state, "after leaving");

    let (back_before, back, back_after) = bracket(&mut state, |s| drag_to(s, over));
    assert_nothing_sent(&back, "back on tab_2");
    assert!(back_before > after, "the clock moved between the two visits");
    assert_nothing_sent(
        &state.tick_pane_spring(after + DWELL),
        "the dwell restarts on return; the first visit's deadline is void",
    );
    assert_only_tab_focus(
        &state.tick_pane_spring(back_after + DWELL),
        TAB_2,
        "a full dwell after the return",
    );
}

/// The pointer must stay within one cell: a drag event on the same cell keeps
/// the dwell, a move of one cell along the same tab restarts it.
#[test]
fn spring_travel_of_one_cell_restarts_the_dwell() {
    let mut state = state();
    let over = tab_2_in_bar(&state, 1);
    let (_, _, after) = bracket(&mut state, |s| lift_a_to(s, over).1);
    let still = drag_to(&mut state, over);
    assert_nothing_sent(&still, "a drag event on the same cell");
    assert_only_tab_focus(
        &state.tick_pane_spring(after + DWELL),
        TAB_2,
        "no travel: the first stamp holds",
    );

    let mut state = self::state();
    let (_, _, after) = bracket(&mut state, |s| lift_a_to(s, over).1);
    let next_cell = tab_2_in_bar(&state, 2);
    let (moved_before, moved, moved_after) = bracket(&mut state, |s| drag_to(s, next_cell));
    assert_nothing_sent(&moved, "one cell along tab_2");
    assert!(moved_before > after, "the clock moved between the two events");
    assert_nothing_sent(
        &state.tick_pane_spring(after + DWELL),
        "one cell of travel restarts the dwell",
    );
    assert_only_tab_focus(
        &state.tick_pane_spring(moved_after + DWELL),
        TAB_2,
        "a full dwell after the travel",
    );
}

/// Esc or a right click after a spring-load sends `tab.focus` back to the
/// origin tab and nothing else, whether or not the server's switch has
/// arrived yet. The release that follows sends nothing.
#[test]
fn spring_cancel_restores_the_origin_tab() {
    for (shown, how) in [(true, "esc"), (false, "esc"), (true, "right click")] {
        let mut state = state();
        let step = format!("{how}, tab_2 shown: {shown}");
        let over = tab_2_in_bar(&state, 1);
        let (_, _, after) = bracket(&mut state, |s| lift_a_to(s, over).1);
        assert_only_tab_focus(&state.tick_pane_spring(after + DWELL), TAB_2, &step);
        if shown {
            let panes = tab_2_panes(&state);
            server_shows(&mut state, TAB_2, &panes);
        }
        let point = if shown { centre(y_rect(&state)) } else { over };
        let cancel = if how == "esc" {
            state.handle_raw_events(vec![RawInputEvent::Key(crate::input::TerminalKey::new(
                KeyCode::Esc,
                KeyModifiers::empty(),
            ))])
        } else {
            state.handle_raw_events(vec![mouse(
                MouseEventKind::Down(MouseButton::Right),
                point.0,
                point.1,
            )])
        };
        assert_only_tab_focus(&cancel, ORIGIN, &step);
        assert_no_pane_input(&cancel, &step);
        assert!(
            !matches!(state.chrome_drag, Some(ClientChromeDrag::Pane { .. })),
            "{step}: the cancel ends the drag"
        );
        let release = state.handle_raw_events(vec![mouse(
            MouseEventKind::Up(MouseButton::Left),
            point.0,
            point.1,
        )]);
        assert_nothing_sent(&release, &format!("{step}: the release after the cancel"));
        assert_nothing_sent(
            &state.tick_pane_spring(after + ms(5000)),
            &format!("{step}: nothing springs after the cancel"),
        );
    }
}

fn open_move(state: &mut ClientShellState) {
    assert_nothing_sent(&state.handle_input_bytes(PREFIX), "prefix");
    assert_nothing_sent(&state.handle_input_bytes(b"m"), "prefix+m");
    assert_eq!(state.mode, ClientShellMode::Move, "prefix+m opens move mode");
}

fn esc(state: &mut ClientShellState) -> ClientShellInput {
    state.handle_raw_events(vec![RawInputEvent::Key(crate::input::TerminalKey::new(
        KeyCode::Esc,
        KeyModifiers::empty(),
    ))])
}

/// In move mode `]` and `[` spring-load the next and previous tab of the same
/// workspace at once, without leaving the mode. Esc restores the origin tab.
#[test]
fn spring_move_mode_brackets_and_esc_restores_the_origin() {
    let mut state = state();
    open_move(&mut state);

    let next = state.handle_input_bytes(b"]");
    assert_only_tab_focus(&next, TAB_2, "]");
    assert_no_pane_input(&next, "]");
    assert_eq!(state.mode, ClientShellMode::Move, "] keeps move mode");
    let panes = tab_2_panes(&state);
    server_shows(&mut state, TAB_2, &panes);
    assert_eq!(state.mode, ClientShellMode::Move, "the switch keeps move mode");

    let previous = state.handle_input_bytes(b"[");
    assert_only_tab_focus(&previous, ORIGIN, "[ from tab_2");
    assert_eq!(state.mode, ClientShellMode::Move, "[ keeps move mode");
    let panes = tab_1_panes(&state);
    server_shows(&mut state, ORIGIN, &panes);

    let again = state.handle_input_bytes(b"]");
    assert_only_tab_focus(&again, TAB_2, "] again");
    let panes = tab_2_panes(&state);
    server_shows(&mut state, TAB_2, &panes);
    assert_eq!(state.mode, ClientShellMode::Move);

    let cancel = esc(&mut state);
    assert_only_tab_focus(&cancel, ORIGIN, "Esc in move mode after a spring-load");
    assert_no_pane_input(&cancel, "Esc");
    assert_eq!(state.mode, ClientShellMode::Terminal, "Esc ends move mode");
}

/// After `]` the keyboard target is in tab 2: Shift+Right asks a dry run
/// against one of its panes and Enter places A there, not into the tab.
#[test]
fn spring_move_mode_drop_uses_the_new_tabs_panes() {
    let mut state = state();
    open_move(&mut state);
    assert_only_tab_focus(&state.handle_input_bytes(b"]"), TAB_2, "]");
    let panes = tab_2_panes(&state);
    server_shows(&mut state, TAB_2, &panes);

    let edge = state.handle_input_bytes(SHIFT_RIGHT);
    assert_no_pane_input(&edge, "Shift+Right");
    let target = match &sent_methods(&edge)[..] {
        [Method::PanePlace(params)]
            if params.pane_id == A && params.side == PaneDirection::Right && params.dry_run =>
        {
            params.target.clone()
        }
        other => panic!("Shift+Right asks one dry run for an edge in tab_2: {other:?}"),
    };
    assert!(
        matches!(&target, PanePlaceTarget::Pane { pane_id } if pane_id == X || pane_id == Y),
        "the target is a pane of tab_2: {target:?}"
    );
    let drop = state.handle_input_bytes(ENTER);
    assert!(
        matches!(&sent_methods(&drop)[..], [Method::PanePlace(params)]
            if params.pane_id == A
                && params.target == target
                && params.side == PaneDirection::Right
                && !params.dry_run
                && params.focus),
        "Enter places A at that edge: {:?}",
        sent_methods(&drop)
    );
    assert_eq!(state.mode, ClientShellMode::Terminal, "a drop ends move mode");
}

/// S5 review advisory: move mode must not outlive the lift. When the lift is
/// cleared under move mode (a mouse release drops it, or an overlay refresh
/// clears the drag), Esc ends move mode instead of being swallowed, and the
/// keyboard reaches the pane again.
#[test]
fn spring_move_mode_never_outlives_the_lift() {
    for how in ["mouse release", "overlay refresh"] {
        let mut state = state();
        open_move(&mut state);
        if how == "mouse release" {
            let grip = on_screen(&state, A);
            state.handle_raw_events(vec![mouse(
                MouseEventKind::Up(MouseButton::Left),
                grip.x + grip.width / 2,
                grip.y,
            )]);
        } else {
            let mut snapshot = state.snapshot.as_deref().cloned().expect("snapshot");
            snapshot.product_announcement = Some(crate::protocol::ClientShellProductAnnouncement {
                version: "9.9.9".into(),
                id: "spring-check".into(),
                title: "An announcement".into(),
                body: "- one line".into(),
                preview: false,
            });
            state.set_snapshot(Box::new(snapshot.clone()));
            compose(&mut state);
            snapshot.product_announcement = None;
            state.set_snapshot(Box::new(snapshot));
            compose(&mut state);
        }
        assert!(
            !matches!(state.chrome_drag, Some(ClientChromeDrag::Pane { .. })),
            "{how}: precondition: the lift is gone"
        );
        let first = esc(&mut state);
        assert!(
            !sent_methods(&first).iter().any(|method| matches!(
                method,
                Method::PanePlace(_) | Method::PaneSwap(_) | Method::PaneMove(_)
            )),
            "{how}: Esc rearranges nothing: {:?}",
            sent_methods(&first)
        );
        assert_ne!(
            state.mode,
            ClientShellMode::Move,
            "{how}: Esc is not swallowed by a move mode with nothing lifted"
        );
        let typed = state.handle_input_bytes(b"x");
        assert!(
            typed
                .requests
                .iter()
                .any(|request| matches!(request, ClientMessage::ClientShellPaneInput { .. })),
            "{how}: after Esc, typing reaches the pane again: {:?}",
            typed.requests
        );
    }
}
