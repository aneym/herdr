//! Owner-written check for pane drag slice S4, TUI motion (spec
//! `pane-drag-rearrange-2026-10-07`, sections "Motion" and "S4").
//! Implementers may not edit this file.
//!
//! Time is injected, never slept. Input and snapshots stamp each motion with
//! the wall clock (`PaneMotion::started_at`); the test reads that stamp and
//! drives `tick_pane_motion(now)` with instants relative to it, then reads the
//! frame from the real composer. Ticks are pure in `now`, so a test may tick
//! "in the future" and later feed input stamped with an earlier wall clock.
//!
//! The fixture surface draws only square corners (`┌┐└┘`), so every rounded
//! corner (`╭╮╰╯`) in a frame is a client-drawn ghost or settle outline.
//!
//! Geometry (see `pane_drag_fixture.rs`): `B | C` on top, `A` full width below.
//! Sample points rest on the spec's ease, cubic-bezier(0.2, 0, 0, 1): at half
//! the duration it has covered about 88% of the way.

use super::pane_drag::{dry_run_answer, lift_a_to, request_id, sent_methods};
use super::pane_drag_fixture::{
    fixture_surface, mouse, state_with_panes, three_pane_panes, three_pane_rects,
    three_pane_rects_at, three_pane_splits, three_pane_state, A, A_LABEL, B, C, COLS, ROWS,
};
use super::*;
use crate::api::schema::Method;
use std::time::{Duration, Instant};

const CORNERS: [&str; 4] = ["╭", "╮", "╰", "╯"];

fn ms(millis: u64) -> Duration {
    Duration::from_millis(millis)
}

fn surface_area(state: &ClientShellState) -> Rect {
    state.layout(COLS, ROWS).pane_surface
}

fn on_screen(surface: Rect, rect: SurfaceRect) -> Rect {
    Rect::new(
        surface.x + rect.x,
        surface.y + rect.y,
        rect.width,
        rect.height,
    )
}

/// Absolute rect of a fixture pane in `rects`.
fn rect_of(surface: Rect, rects: &[(&'static str, SurfaceRect); 3], id: &str) -> Rect {
    let (_, rect) = rects
        .iter()
        .find(|(pane_id, _)| *pane_id == id)
        .expect("fixture pane");
    on_screen(surface, *rect)
}

/// The fixture's starting rects of A, B and C, on screen.
fn start_rects(state: &ClientShellState) -> (Rect, Rect, Rect) {
    let surface = surface_area(state);
    let rects = three_pane_rects(surface.width, surface.height);
    (
        rect_of(surface, &rects, A),
        rect_of(surface, &rects, B),
        rect_of(surface, &rects, C),
    )
}

fn centre(rect: Rect) -> (u16, u16) {
    (rect.x + rect.width / 2, rect.y + rect.height / 2)
}

fn compose(state: &mut ClientShellState) -> FrameData {
    state.compose(COLS, ROWS).expect("frame").frame.clone()
}

/// Tick the motion to `now`, then compose.
fn frame_at(state: &mut ClientShellState, now: Instant) -> FrameData {
    state.tick_pane_motion(now);
    compose(state)
}

fn cell(frame: &FrameData, x: u16, y: u16) -> &crate::protocol::CellData {
    &frame.cells[usize::from(y) * usize::from(frame.width) + usize::from(x)]
}

/// Every cell inside `area` that shows `glyph`, row by row.
fn glyph_cells(frame: &FrameData, area: Rect, glyph: &str) -> Vec<(u16, u16)> {
    (area.y..area.bottom())
        .flat_map(|y| (area.x..area.right()).map(move |x| (x, y)))
        .filter(|&(x, y)| cell(frame, x, y).symbol == glyph)
        .collect()
}

fn outline_drawn(frame: &FrameData, surface: Rect) -> bool {
    CORNERS
        .iter()
        .any(|glyph| !glyph_cells(frame, surface, glyph).is_empty())
}

/// The one ghost outline in the surface, read from its `╭` and `╯` corners.
fn ghost(frame: &FrameData, surface: Rect, step: &str) -> Rect {
    let top_left = glyph_cells(frame, surface, "╭");
    let bottom_right = glyph_cells(frame, surface, "╯");
    assert!(
        top_left.len() == 1 && bottom_right.len() == 1,
        "{step}: expected exactly one ghost outline, found ╭ at {top_left:?} and ╯ at {bottom_right:?}"
    );
    let ((x0, y0), (x1, y1)) = (top_left[0], bottom_right[0]);
    assert!(x1 >= x0 && y1 >= y0, "{step}: corners out of order");
    Rect::new(x0, y0, x1 - x0 + 1, y1 - y0 + 1)
}

fn edges(rect: Rect) -> [(&'static str, u16); 4] {
    [
        ("left", rect.x),
        ("top", rect.y),
        ("right", rect.right()),
        ("bottom", rect.bottom()),
    ]
}

/// In flight: every edge lies within its from..to range, and the rect is
/// neither end.
fn assert_between(actual: Rect, from: Rect, to: Rect, step: &str) {
    assert!(
        actual != from && actual != to,
        "{step}: {actual:?} should be between {from:?} and {to:?}, not at an end"
    );
    for ((name, value), ((_, a), (_, b))) in edges(actual)
        .into_iter()
        .zip(edges(from).into_iter().zip(edges(to)))
    {
        assert!(
            value >= a.min(b) && value <= a.max(b),
            "{step}: {name} edge {value} is outside {a}..{b} ({actual:?} from {from:?} to {to:?})"
        );
    }
}

fn single_start(state: &ClientShellState, step: &str) -> Instant {
    assert_eq!(
        state.pane_motion.len(),
        1,
        "{step}: expected one motion entry"
    );
    state.pane_motion[0].started_at
}

fn assert_idle_timer(state: &ClientShellState, now: Instant, step: &str) {
    assert_eq!(
        state.timer_delay(now),
        ms(100),
        "{step}: no motion, so the client keeps its 100 ms idle tick"
    );
}

fn assert_frame_timer(state: &ClientShellState, now: Instant, step: &str) {
    assert!(
        state.timer_delay(now) <= ms(16),
        "{step}: a running motion wakes the client at frame rate, got {:?}",
        state.timer_delay(now)
    );
}

fn chip_shown(frame: &FrameData) -> bool {
    frame_rows(frame)
        .iter()
        .any(|row| row.contains(&format!("⠿ {A_LABEL}")))
}

/// Apply a server reflow of the focused tab: the next snapshot revision and a
/// matching surface with the fixture panes at `rects` and the root divider at
/// `top`. This is how a keyboard swap, a CLI `pane place`, another client's
/// drop or a split resize reaches this client.
fn apply_reflow(state: &mut ClientShellState, rects: [(&'static str, SurfaceRect); 3], top: u16) {
    let area = surface_area(state);
    let mut snapshot = state.snapshot.as_deref().expect("snapshot").clone();
    snapshot.revision += 1;
    let mut surface = fixture_surface(
        area.width,
        area.height,
        &three_pane_panes(rects),
        three_pane_splits(area.width, area.height, top),
    );
    surface.projection_revision = snapshot.revision;
    state.set_snapshot(Box::new(snapshot));
    state.set_pane_surface(surface);
}

/// The fixture rects after A and B trade places (sizes stay with positions).
fn swapped_rects(width: u16, height: u16) -> [(&'static str, SurfaceRect); 3] {
    let rects = three_pane_rects(width, height);
    let rect = |id: &str| rects.iter().find(|(pane_id, _)| *pane_id == id).unwrap().1;
    [(A, rect(B)), (B, rect(A)), (C, rect(C))]
}

/// What a client that never saw the move would draw for `rects`.
fn fresh_frame(rects: [(&'static str, SurfaceRect); 3], top: u16) -> FrameData {
    let mut state = state_with_panes(true, &three_pane_panes(rects), |width, height| {
        three_pane_splits(width, height, top)
    });
    compose(&mut state)
}

fn rgb(colour: u32) -> [u32; 3] {
    assert_eq!(colour >> 24, 0x02, "an RGB cell colour, got {colour:#x}");
    [(colour >> 16) & 0xff, (colour >> 8) & 0xff, colour & 0xff]
}

/// At a zone change the ghost leaves the old rect at t0, is in flight at
/// +70 ms and lands on the new rect from +140 ms (`zoneMorphMs`). The first
/// zone of a drag grows out of the source pane's rect. The client wakes at
/// frame rate only while the ghost moves.
#[test]
fn zone_change_eases_the_ghost_over_zone_morph_ms() {
    let mut state = three_pane_state(true);
    let surface = surface_area(&state);
    let (a, b, c) = start_rects(&state);
    let now = Instant::now();
    assert!(state.pane_motion.is_empty());
    assert_idle_timer(&state, now, "idle");

    // First zone: B's centre. The ghost starts as A's own rect.
    lift_a_to(&mut state, centre(b));
    assert!(matches!(
        state.chrome_drag,
        Some(ClientChromeDrag::Pane { .. })
    ));
    let s1 = single_start(&state, "first zone");
    assert_eq!(
        ghost(&compose(&mut state), surface, "first zone, before any tick"),
        a,
        "before a tick, compose draws the stored start rect"
    );
    assert_eq!(
        ghost(&frame_at(&mut state, s1), surface, "first zone t0"),
        a,
        "the first zone grows out of the source rect"
    );
    assert_frame_timer(&state, s1, "first zone in flight");
    assert_between(
        ghost(
            &frame_at(&mut state, s1 + ms(70)),
            surface,
            "first zone +70",
        ),
        a,
        b,
        "first zone +70 ms",
    );
    assert!(
        state.tick_pane_motion(s1 + ms(140)),
        "the landing tick repaints"
    );
    assert_eq!(ghost(&compose(&mut state), surface, "first zone +140"), b);
    assert!(
        state.pane_motion.is_empty(),
        "a landed motion leaves no entry"
    );
    assert_idle_timer(&state, s1 + ms(140), "drag held still");
    assert!(
        !state.tick_pane_motion(s1 + ms(200)),
        "nothing moves, nothing to repaint"
    );

    // Zone change: B's centre to C's centre.
    state.handle_raw_events(vec![mouse(
        MouseEventKind::Drag(MouseButton::Left),
        centre(c).0,
        centre(c).1,
    )]);
    let s2 = single_start(&state, "zone change");
    assert_eq!(
        ghost(&frame_at(&mut state, s2), surface, "zone change t0"),
        b,
        "t0: the ghost is still on the previous zone"
    );
    let mid = ghost(&frame_at(&mut state, s2 + ms(70)), surface, "+70");
    assert_between(mid, b, c, "zone change +70 ms");
    assert!(
        (mid.x - b.x) * 10 > (c.x - b.x) * 6,
        "the ease is front-loaded: at half time the ghost is past 60% of the way \
         (x {} on {}..{})",
        mid.x,
        b.x,
        c.x
    );
    for at in [140, 400] {
        assert_eq!(
            ghost(&frame_at(&mut state, s2 + ms(at)), surface, "landed"),
            c,
            "+{at} ms: the ghost is on the new zone"
        );
    }
    assert_idle_timer(&state, s2 + ms(400), "landed");
}

/// When the dry run answers mid-flight, the ghost turns toward `placed_rect`
/// from wherever it is drawn: no jump back to the estimate or the source.
#[test]
fn dry_run_answer_retargets_the_ghost_mid_flight() {
    let mut state = three_pane_state(true);
    let surface = surface_area(&state);
    let (a, b, _) = start_rects(&state);
    let right_band = (b.right() - 4, b.y + b.height / 2);
    let (_, moved) = lift_a_to(&mut state, right_band);
    let dry_run = request_id(&moved);
    let s1 = single_start(&state, "estimate");
    let first_half = (f32::from(b.width) * 0.5).round() as u16;
    let estimate = Rect::new(b.x + first_half, b.y, b.width - first_half, b.height);
    let held = ghost(
        &frame_at(&mut state, s1 + ms(70)),
        surface,
        "toward estimate",
    );
    assert_between(held, a, estimate, "heading for the local estimate");

    let (answer, placed) = dry_run_answer(surface, (3, 1));
    state.handle_endpoint_result("boot-1", &dry_run, Ok(answer));
    let s2 = single_start(&state, "retarget");
    assert_eq!(
        ghost(&frame_at(&mut state, s2), surface, "retarget t0"),
        held,
        "the retarget starts from the rect last drawn"
    );
    assert_between(
        ghost(&frame_at(&mut state, s2 + ms(70)), surface, "retarget +70"),
        held,
        placed,
        "retarget +70 ms",
    );
    assert_eq!(
        ghost(
            &frame_at(&mut state, s2 + ms(140)),
            surface,
            "retarget +140"
        ),
        placed
    );
}

/// Esc: the chip goes at once, the ghost eases back to the source rect over
/// `cancelMs` (160 ms) with the source still lifted, then the lift clears and
/// the frame is the idle frame again.
#[test]
fn cancel_eases_the_ghost_back_then_clears_the_lift() {
    let mut state = three_pane_state(true);
    let surface = surface_area(&state);
    let (a, b, _) = start_rects(&state);
    let idle = compose(&mut state);
    lift_a_to(&mut state, centre(b));
    let s1 = single_start(&state, "zone");
    assert!(chip_shown(&frame_at(&mut state, s1 + ms(140))));

    let esc = state.handle_raw_events(vec![RawInputEvent::Key(crate::input::TerminalKey::new(
        crossterm::event::KeyCode::Esc,
        KeyModifiers::empty(),
    ))]);
    assert!(sent_methods(&esc).is_empty(), "{:?}", sent_methods(&esc));
    let s2 = single_start(&state, "cancel");
    let dim = Modifier::DIM.bits();
    let content = (a.right() - 10, a.y + 3);
    let t0 = frame_at(&mut state, s2);
    assert!(!chip_shown(&t0), "the chip disappears at once");
    assert_eq!(ghost(&t0, surface, "cancel t0"), b);
    assert_ne!(
        cell(&t0, content.0, content.1).modifier & dim,
        0,
        "the source stays lifted while the ghost returns"
    );
    assert_frame_timer(&state, s2, "cancel in flight");
    let mid = frame_at(&mut state, s2 + ms(80));
    assert_between(ghost(&mid, surface, "cancel +80"), b, a, "cancel +80 ms");
    assert_ne!(cell(&mid, content.0, content.1).modifier & dim, 0);

    let release = state.handle_raw_events(vec![mouse(
        MouseEventKind::Up(MouseButton::Left),
        centre(b).0,
        centre(b).1,
    )]);
    assert!(
        sent_methods(&release).is_empty(),
        "a release after Esc sends nothing: {:?}",
        sent_methods(&release)
    );
    assert!(release.requests.is_empty(), "{:?}", release.requests);

    assert!(
        state.tick_pane_motion(s2 + ms(160)),
        "the last cancel tick repaints"
    );
    assert_eq!(
        compose(&mut state),
        idle,
        "after cancelMs the frame is the idle frame"
    );
    assert!(state.pane_drag.is_none(), "the lift is cleared");
    assert!(state.pane_motion.is_empty());
    assert_idle_timer(&state, s2 + ms(160), "after cancel");
}

/// A swap that reaches this client by snapshot (keyboard prefix+shift+hjkl,
/// CLI `pane swap`, another client) settles: two outlines cross, A's old rect
/// to B's and B's old rect to A's, their colour steps accent -> 50% mix ->
/// `overlay0` (the border colour) over `settleMs` (200 ms), then nothing is
/// drawn and the frame is what a client that never saw the move would draw.
#[test]
fn swap_by_snapshot_settles_with_two_crossing_outlines() {
    let mut state = three_pane_state(true);
    let surface = surface_area(&state);
    let (a, b, _) = start_rects(&state);
    let top = surface.height / 2;
    apply_reflow(
        &mut state,
        swapped_rects(surface.width, surface.height),
        top,
    );

    assert_eq!(
        state.pane_motion.len(),
        2,
        "one settle outline per moved pane"
    );
    let s = state.pane_motion[0].started_at;
    assert!(
        state.pane_motion.iter().all(|m| m.started_at == s),
        "one event, one start"
    );
    let top_rights = |frame: &FrameData| glyph_cells(frame, surface, "╮");

    let t0 = frame_at(&mut state, s);
    let mut at_t0 = top_rights(&t0);
    at_t0.sort_unstable();
    let mut expected = vec![(a.right() - 1, a.y), (b.right() - 1, b.y)];
    expected.sort_unstable();
    assert_eq!(
        at_t0, expected,
        "t0: each outline sits on its pane's old rect"
    );
    assert_frame_timer(&state, s, "settle in flight");

    let mut mid = top_rights(&frame_at(&mut state, s + ms(100)));
    assert_eq!(mid.len(), 2, "+100 ms: two outlines, {mid:?}");
    mid.sort_unstable_by_key(|&(_, y)| y);
    let (upper, lower) = (mid[0], mid[1]);
    assert!(
        b.y < upper.1 && upper.1 < lower.1 && lower.1 < a.y,
        "+100 ms: both outlines are between the two rows, A's rising, B's falling: {mid:?}"
    );
    assert!(
        upper.0 < lower.0,
        "+100 ms: A's outline narrows toward B's rect while B's widens toward A's: {mid:?}"
    );

    // Colour: exactly three steps, in order, each drawn on every corner.
    let accent = crate::protocol::color_to_u32(state.config.palette.accent);
    let border = crate::protocol::color_to_u32(state.config.palette.overlay0);
    let mut steps: Vec<u32> = Vec::new();
    for at in (0..200).step_by(10) {
        let frame = frame_at(&mut state, s + ms(at));
        let mut colours = CORNERS
            .iter()
            .flat_map(|glyph| glyph_cells(&frame, surface, glyph))
            .map(|(x, y)| cell(&frame, x, y).fg)
            .collect::<Vec<_>>();
        colours.dedup();
        assert_eq!(
            colours.len(),
            1,
            "+{at} ms: one outline colour at a time, got {colours:x?}"
        );
        if steps.last() != Some(&colours[0]) {
            steps.push(colours[0]);
        }
    }
    assert_eq!(steps.len(), 3, "the colour steps 3 times: {steps:x?}");
    assert_eq!(steps[0], accent, "settle starts in accent");
    assert_eq!(steps[2], border, "settle ends in the border colour");
    let (from, mid_colour, to) = (rgb(accent), rgb(steps[1]), rgb(border));
    for channel in 0..3 {
        let (lo, hi) = (
            from[channel].min(to[channel]),
            from[channel].max(to[channel]),
        );
        assert!(
            (lo..=hi).contains(&mid_colour[channel]),
            "the middle step mixes accent and border: {:x?} vs {:x?} / {:x?}",
            mid_colour,
            from,
            to
        );
    }

    assert!(
        state.tick_pane_motion(s + ms(200)),
        "the last settle tick repaints"
    );
    let settled = compose(&mut state);
    assert!(
        !outline_drawn(&settled, surface),
        "after settleMs nothing is drawn"
    );
    assert_eq!(
        settled,
        fresh_frame(swapped_rects(surface.width, surface.height), top),
        "a settled frame is byte-identical to a client that never saw the move"
    );
    assert!(state.pane_motion.is_empty());
    assert!(!state.tick_pane_motion(s + ms(300)));
    assert_idle_timer(&state, s + ms(300), "settled");
}

/// A split resize is not a move: no settle while the divider is dragged, and
/// none for the final ratio that lands after the release.
#[test]
fn split_drag_resize_does_not_settle() {
    let mut state = three_pane_state(true);
    let surface = surface_area(&state);
    let top = surface.height / 2;
    let (w, h) = (surface.width, surface.height);
    let press = (surface.x + 5, surface.y + top - 1);
    state.handle_raw_events(vec![mouse(
        MouseEventKind::Down(MouseButton::Left),
        press.0,
        press.1,
    )]);
    let moved = state.handle_raw_events(vec![mouse(
        MouseEventKind::Drag(MouseButton::Left),
        press.0,
        press.1 + 3,
    )]);
    assert!(
        matches!(state.chrome_drag, Some(ClientChromeDrag::PaneSplit { .. })),
        "the press on B's bottom border grabs the root divider"
    );
    assert!(
        sent_methods(&moved)
            .iter()
            .any(|method| matches!(method, Method::LayoutSetSplitRatio(_))),
        "the divider drag asks for a ratio: {:?}",
        sent_methods(&moved)
    );

    apply_reflow(&mut state, three_pane_rects_at(w, h, top + 3), top + 3);
    let now = Instant::now();
    assert!(
        state.pane_motion.is_empty(),
        "no settle while a divider is dragged"
    );
    assert!(!outline_drawn(&frame_at(&mut state, now), surface));
    assert_idle_timer(&state, now, "divider drag");

    state.handle_raw_events(vec![mouse(
        MouseEventKind::Up(MouseButton::Left),
        press.0,
        press.1 + 4,
    )]);
    assert!(state.chrome_drag.is_none());
    apply_reflow(&mut state, three_pane_rects_at(w, h, top + 4), top + 4);
    assert!(
        state.pane_motion.is_empty(),
        "the final ratio after the release is still a resize, not a move"
    );
    assert!(!outline_drawn(&frame_at(&mut state, now), surface));
}

/// A drag swap: the ghost holds on B's rect after the release; when the
/// server's snapshot lands, A's outline settles from the ghost (it is already
/// where A lands) and B's outline flies from its old rect to A's.
#[test]
fn drop_settles_from_where_the_ghost_held() {
    let mut state = three_pane_state(true);
    let surface = surface_area(&state);
    let (a, b, _) = start_rects(&state);
    lift_a_to(&mut state, centre(b));
    let s1 = single_start(&state, "zone");
    frame_at(&mut state, s1 + ms(140));
    let drop = state.handle_raw_events(vec![mouse(
        MouseEventKind::Up(MouseButton::Left),
        centre(b).0,
        centre(b).1,
    )]);
    assert!(
        matches!(&sent_methods(&drop)[..], [Method::PaneSwap(_)]),
        "{:?}",
        sent_methods(&drop)
    );
    let held = compose(&mut state);
    assert_eq!(
        ghost(&held, surface, "held"),
        b,
        "the ghost holds until the snapshot"
    );
    assert!(!chip_shown(&held));

    let top = surface.height / 2;
    apply_reflow(
        &mut state,
        swapped_rects(surface.width, surface.height),
        top,
    );
    assert_eq!(state.pane_motion.len(), 2);
    let s = state.pane_motion[0].started_at;
    let b_corner = (b.right() - 1, b.y);
    assert_eq!(
        glyph_cells(&frame_at(&mut state, s), surface, "╮"),
        vec![b_corner],
        "t0: both outlines start on B's old rect, where the ghost held"
    );
    let mid = glyph_cells(&frame_at(&mut state, s + ms(100)), surface, "╮");
    assert_eq!(mid.len(), 2, "+100 ms: {mid:?}");
    assert!(
        mid.contains(&b_corner),
        "A's outline stays where it lands: {mid:?}"
    );
    let flying = mid
        .iter()
        .find(|corner| **corner != b_corner)
        .expect("B's outline");
    assert!(
        b.y < flying.1 && flying.1 < a.y && b_corner.0 < flying.0 && flying.0 < a.right() - 1,
        "B's outline is on its way to A's old rect: {flying:?}"
    );
    state.tick_pane_motion(s + ms(200));
    assert_eq!(
        compose(&mut state),
        fresh_frame(swapped_rects(surface.width, surface.height), top),
        "after the settle the frame is the plain swapped layout"
    );
}

/// `ui.reduce_motion = true`: every rect change is instant, a cancel clears
/// at once, and a settle is one accent outline on the new rects for
/// `reducedFadeMs` (100 ms).
#[test]
fn reduce_motion_jumps_to_the_end() {
    let config: Config = toml::from_str("[ui]\nreduce_motion = true\n").expect("config parses");
    assert!(ClientShellConfig::from_config(&config).reduce_motion);
    assert!(
        !ClientShellConfig::from_config(&Config::default()).reduce_motion,
        "motion is on by default"
    );

    let mut state = three_pane_state(true);
    state.config.reduce_motion = true;
    let surface = surface_area(&state);
    let (a, b, c) = start_rects(&state);
    let idle = compose(&mut state);
    let now = Instant::now();

    lift_a_to(&mut state, centre(b));
    assert_eq!(
        ghost(&compose(&mut state), surface, "reduced first zone"),
        b,
        "the ghost is on the zone at t0"
    );
    assert!(
        state.pane_motion.is_empty(),
        "no zone motion under reduce_motion"
    );
    assert_idle_timer(&state, now, "reduced drag");
    state.handle_raw_events(vec![mouse(
        MouseEventKind::Drag(MouseButton::Left),
        centre(c).0,
        centre(c).1,
    )]);
    assert_eq!(
        ghost(&compose(&mut state), surface, "reduced zone change"),
        c
    );
    state.handle_raw_events(vec![RawInputEvent::Key(crate::input::TerminalKey::new(
        crossterm::event::KeyCode::Esc,
        KeyModifiers::empty(),
    ))]);
    assert_eq!(compose(&mut state), idle, "a reduced cancel clears at once");
    assert!(state.pane_drag.is_none() && state.pane_motion.is_empty());

    let top = surface.height / 2;
    apply_reflow(
        &mut state,
        swapped_rects(surface.width, surface.height),
        top,
    );
    assert!(
        !state.pane_motion.is_empty(),
        "a reduced settle still shows"
    );
    let s = state.pane_motion[0].started_at;
    let accent = crate::protocol::color_to_u32(state.config.palette.accent);
    let mut new_corners = vec![(b.right() - 1, b.y), (a.right() - 1, a.y)];
    new_corners.sort_unstable();
    for at in [0, 50] {
        let frame = frame_at(&mut state, s + ms(at));
        let mut corners = glyph_cells(&frame, surface, "╮");
        corners.sort_unstable();
        assert_eq!(
            corners, new_corners,
            "+{at} ms: outlines on the new rects only"
        );
        for (x, y) in corners {
            assert_eq!(cell(&frame, x, y).fg, accent, "+{at} ms: accent outline");
        }
    }
    state.tick_pane_motion(s + ms(100));
    assert_eq!(
        compose(&mut state),
        fresh_frame(swapped_rects(surface.width, surface.height), top),
        "after reducedFadeMs nothing is drawn"
    );
}
