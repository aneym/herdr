//! Owner-written check for pane drag slice S5, TUI keyboard move mode (spec
//! `pane-drag-rearrange-2026-10-07`, sections "Keyboard" and "S5").
//! Implementers may not edit this file.
//!
//! Every step runs through the real key path (`handle_input_bytes`, or
//! `handle_raw_events` for Esc and Alt chords), the real endpoint answer path
//! (`handle_endpoint_result`) and the real composer (`compose`). Which pane
//! the target sits on is read from what a drop sends (a centre drop is
//! `pane.swap { source, target }`), never from client internals. The only S5
//! name used here is `ClientShellMode::Move`.
//!
//! Fixtures come from `pane_drag_fixture.rs` (S3): `B | C` on top with `A`
//! full width below; and a 5 x 2 grid of `A, pane_g1 .. pane_g9` in reading
//! order. The server's focused pane is the source. Tests that need another
//! source move focus with a snapshot, as the server would after `pane.focus`.

use super::pane_drag_fixture::{
    grid_state, methods, mouse, three_pane_rects, three_pane_state, A, B, C, COLS, ROWS,
};
use super::*;
use crate::api::schema::{
    Method, PaneDirection, PaneLayoutPane, PaneLayoutRect, PaneLayoutSnapshot, PanePlaceResult,
    PanePlaceTarget, ResponseResult,
};

/// The status hint, verbatim from the spec.
const HINT: &str = "move: ←↑↓→ target · ⇧ edge · [ ] tab · ⏎ drop · esc";
/// The transient hint for prefix+m in a tab with one pane.
const NOTHING_TO_MOVE: &str = "nothing to move";

/// The default prefix, ctrl+b.
const PREFIX: &[u8] = &[0x02];
const ENTER: &[u8] = b"\r";
const SPACE: &[u8] = b" ";
const UP: &[u8] = b"\x1b[A";
const RIGHT: &[u8] = b"\x1b[C";
const LEFT: &[u8] = b"\x1b[D";
const SHIFT_DOWN: &[u8] = b"\x1b[1;2B";
const SHIFT_LEFT: &[u8] = b"\x1b[1;2D";

fn sent_methods(outcome: &ClientShellInput) -> Vec<Method> {
    outcome
        .actions
        .iter()
        .filter_map(|action| match action {
            ClientShellAction::Endpoint { request, .. } => Some(request.method.clone()),
            _ => None,
        })
        .collect()
}

fn request_id(outcome: &ClientShellInput) -> String {
    outcome
        .actions
        .iter()
        .find_map(|action| match action {
            ClientShellAction::Endpoint { request, .. } => Some(request.id.clone()),
            _ => None,
        })
        .expect("an endpoint request")
}

/// No pane program saw this input.
fn assert_no_pane_input(outcome: &ClientShellInput, step: &str) {
    assert!(
        !outcome.requests.iter().any(|request| matches!(
            request,
            ClientMessage::ClientShellPaneInput { .. }
                | ClientMessage::ClientShellPopupInput { .. }
        )),
        "{step}: a pane program received input during move mode: {:?}",
        outcome.requests
    );
}

/// Sent nothing to the server and nothing to any pane.
fn assert_quiet(outcome: &ClientShellInput, step: &str) {
    assert!(
        sent_methods(outcome).is_empty(),
        "{step}: nothing is sent yet: {:?}",
        sent_methods(outcome)
    );
    assert_no_pane_input(outcome, step);
}

fn esc(state: &mut ClientShellState) -> ClientShellInput {
    state.handle_raw_events(vec![RawInputEvent::Key(crate::input::TerminalKey::new(
        KeyCode::Esc,
        KeyModifiers::empty(),
    ))])
}

/// prefix, then `m`. Neither key sends anything or reaches a pane.
fn open_move(state: &mut ClientShellState) {
    let prefix = state.handle_input_bytes(PREFIX);
    assert_quiet(&prefix, "prefix");
    let m = state.handle_input_bytes(b"m");
    assert_quiet(&m, "prefix+m");
}

/// One navigation key in move mode: nothing is sent and the mode stays.
fn step(state: &mut ClientShellState, bytes: &[u8], name: &str) {
    let outcome = state.handle_input_bytes(bytes);
    assert_quiet(&outcome, name);
    assert_eq!(state.mode, ClientShellMode::Move, "{name} keeps move mode");
}

fn frame(state: &mut ClientShellState) -> FrameData {
    state.compose(COLS, ROWS).expect("frame").frame.clone()
}

fn cell(frame: &FrameData, x: u16, y: u16) -> &crate::protocol::CellData {
    &frame.cells[usize::from(y) * usize::from(frame.width) + usize::from(x)]
}

fn symbol(frame: &FrameData, x: u16, y: u16) -> &str {
    cell(frame, x, y).symbol.as_str()
}

fn frame_text(frame: &FrameData) -> String {
    frame_rows(frame).join("\n")
}

/// Absolute frame rect of a pane of the three-pane fixture.
fn rect_of(state: &ClientShellState, id: &str) -> Rect {
    let surface = state.layout(COLS, ROWS).pane_surface;
    let (_, rect) = three_pane_rects(surface.width, surface.height)
        .into_iter()
        .find(|(pane_id, _)| *pane_id == id)
        .expect("fixture pane");
    Rect::new(
        surface.x + rect.x,
        surface.y + rect.y,
        rect.width,
        rect.height,
    )
}

/// The server moved focus to `pane_id`: same revision, new focus.
fn focus(state: &mut ClientShellState, pane_id: &str) {
    let mut snapshot = state.snapshot.as_deref().cloned().expect("snapshot");
    snapshot.focused_pane_id = Some(pane_id.into());
    for pane in &mut snapshot.panes {
        pane.focused = pane.pane_id == pane_id;
    }
    state.set_snapshot(Box::new(snapshot));
    frame(state);
}

/// The tab's zoom flag as the server reports it.
fn set_zoomed(state: &mut ClientShellState, zoomed: bool) {
    let mut snapshot = state.snapshot.as_deref().cloned().expect("snapshot");
    for tab in &mut snapshot.tabs {
        if tab.tab_id == "tab_1" {
            tab.zoomed = zoomed;
        }
    }
    state.set_snapshot(Box::new(snapshot));
    frame(state);
}

/// Enter drops on a centre zone, which sends one `pane.swap` from `source`.
/// Returns the pane it swaps with: the pane the target was on.
fn centre_drop_target(state: &mut ClientShellState, source: &str) -> String {
    let drop = state.handle_input_bytes(ENTER);
    assert_no_pane_input(&drop, "centre drop");
    let target = match &sent_methods(&drop)[..] {
        [Method::PaneSwap(params)] if params.source_pane_id.as_deref() == Some(source) => {
            params.target_pane_id.clone().expect("swap target")
        }
        other => panic!("a centre drop sends one pane.swap from {source}: {other:?}"),
    };
    assert_eq!(
        state.mode,
        ClientShellMode::Terminal,
        "a drop ends move mode"
    );
    target
}

/// The server's dry run for "B below C". With B gone, C spans the top row and
/// B would take its lower half. The layout area equals the surface size, so
/// the on-screen rect is the layout rect moved by the surface origin.
fn below_c_answer(surface: Rect) -> (ResponseResult, Rect) {
    let (w, h) = (surface.width, surface.height);
    let top = h / 2;
    let quarter = top / 2;
    let rect = |x: u16, y: u16, width: u16, height: u16| PaneLayoutRect {
        x,
        y,
        width,
        height,
    };
    let placed = rect(0, quarter, w, top - quarter);
    let result = ResponseResult::PanePlace {
        place: PanePlaceResult {
            changed: true,
            dry_run: true,
            reason: None,
            pane_id: B.into(),
            previous_pane_id: B.into(),
            placed_rect: placed,
            target_layout: PaneLayoutSnapshot {
                workspace_id: "ws_1".into(),
                tab_id: "tab_1".into(),
                zoomed: false,
                area: rect(0, 0, w, h),
                focused_pane_id: B.into(),
                panes: vec![
                    PaneLayoutPane {
                        pane_id: C.into(),
                        focused: false,
                        rect: rect(0, 0, w, quarter),
                    },
                    PaneLayoutPane {
                        pane_id: B.into(),
                        focused: true,
                        rect: placed,
                    },
                    PaneLayoutPane {
                        pane_id: A.into(),
                        focused: false,
                        rect: rect(0, top, w, h - top),
                    },
                ],
                splits: Vec::new(),
            },
            source_layout: None,
            closed_tab_id: None,
            closed_workspace_id: None,
            focused_pane_id: B.into(),
        },
    };
    let on_screen = Rect::new(surface.x, surface.y + quarter, w, top - quarter);
    (result, on_screen)
}

/// B is the source; the user lifts it, picks C's bottom edge with Shift+Down,
/// sees the dry run's answer and drops with Enter.
#[test]
fn pane_move_mode_lifts_and_drops_on_an_edge() {
    let mut state = three_pane_state(true);
    focus(&mut state, B);
    let idle = frame(&mut state);
    let (b, c) = (rect_of(&state, B), rect_of(&state, C));
    assert!(!frame_text(&idle).contains(HINT), "no hint while idle");

    // prefix+m: move mode, B lifted, the target on C (B's right neighbour,
    // centre zone, so the ghost is C's rect and nothing is asked yet).
    open_move(&mut state);
    assert_eq!(
        state.mode,
        ClientShellMode::Move,
        "prefix+m opens move mode"
    );
    let lifted = frame(&mut state);
    let text = frame_text(&lifted);
    assert!(
        text.contains(HINT),
        "move mode shows the hint `{HINT}`:\n{text}"
    );
    let dim = Modifier::DIM.bits();
    let b_content = (b.x + 3, b.y + 3);
    assert_eq!(cell(&idle, b_content.0, b_content.1).modifier & dim, 0);
    assert_ne!(
        cell(&lifted, b_content.0, b_content.1).modifier & dim,
        0,
        "the lifted pane's content is DIM, as in a mouse drag"
    );
    assert_eq!(symbol(&lifted, c.x, c.y), "╭", "ghost on C's rect");
    assert_eq!(
        symbol(&lifted, c.right() - 1, c.y),
        "╮",
        "ghost on C's rect"
    );

    // Shift+Down: C's bottom edge, one dry run, still in move mode.
    let edge = state.handle_input_bytes(SHIFT_DOWN);
    assert_no_pane_input(&edge, "Shift+Down");
    assert!(
        matches!(&sent_methods(&edge)[..], [Method::PanePlace(params)]
            if params.pane_id == B
                && params.target == PanePlaceTarget::Pane { pane_id: C.into() }
                && params.side == PaneDirection::Down
                && params.dry_run),
        "Shift+Down asks a dry run for C's bottom edge: {:?}",
        sent_methods(&edge)
    );
    assert_eq!(state.mode, ClientShellMode::Move);

    // The answer moves the ghost to placed_rect, as for the mouse.
    let surface = state.layout(COLS, ROWS).pane_surface;
    let (answer, placed) = below_c_answer(surface);
    let (_, follow_up) = state.handle_endpoint_result("boot-1", &request_id(&edge), Ok(answer));
    assert!(follow_up.is_empty(), "{follow_up:?}");
    let shown = frame(&mut state);
    assert_eq!(
        symbol(&shown, placed.x, placed.y),
        "╭",
        "ghost top-left at placed_rect"
    );
    assert_eq!(
        symbol(&shown, placed.right() - 1, placed.y),
        "╮",
        "ghost top-right at placed_rect"
    );
    assert_ne!(
        symbol(&shown, c.x, c.y),
        "╭",
        "the centre ghost on C is gone"
    );

    // Enter: the real placement, focused, and move mode ends.
    let drop = state.handle_input_bytes(ENTER);
    assert_no_pane_input(&drop, "Enter");
    assert!(
        matches!(&sent_methods(&drop)[..], [Method::PanePlace(params)]
            if params.pane_id == B
                && params.target == PanePlaceTarget::Pane { pane_id: C.into() }
                && params.side == PaneDirection::Down
                && !params.dry_run
                && params.focus),
        "Enter places B below C: {:?}",
        sent_methods(&drop)
    );
    assert_eq!(
        state.mode,
        ClientShellMode::Terminal,
        "a drop ends move mode"
    );
}

/// Shift+l is the letter form of Shift+Right; Space drops like Enter.
#[test]
fn pane_move_mode_shift_letter_edge_and_space_drop() {
    let mut state = three_pane_state(true);
    focus(&mut state, B);
    open_move(&mut state);
    assert_eq!(state.mode, ClientShellMode::Move);

    let edge = state.handle_input_bytes(b"L");
    assert_no_pane_input(&edge, "Shift+l");
    assert!(
        matches!(&sent_methods(&edge)[..], [Method::PanePlace(params)]
            if params.pane_id == B
                && params.target == PanePlaceTarget::Pane { pane_id: C.into() }
                && params.side == PaneDirection::Right
                && params.dry_run),
        "Shift+l asks a dry run for C's right edge: {:?}",
        sent_methods(&edge)
    );
    assert_eq!(state.mode, ClientShellMode::Move);

    let drop = state.handle_input_bytes(SPACE);
    assert_no_pane_input(&drop, "Space");
    assert!(
        matches!(&sent_methods(&drop)[..], [Method::PanePlace(params)]
            if params.pane_id == B
                && params.target == PanePlaceTarget::Pane { pane_id: C.into() }
                && params.side == PaneDirection::Right
                && !params.dry_run
                && params.focus),
        "Space places B right of C: {:?}",
        sent_methods(&drop)
    );
    assert_eq!(state.mode, ClientShellMode::Terminal);
}

/// Esc and a right click both cancel: nothing is sent, the lift clears, and a
/// late dry-run answer does not bring the ghost back.
#[test]
fn pane_move_mode_cancel_sends_nothing() {
    let mut state = three_pane_state(true);
    focus(&mut state, B);
    let idle = frame(&mut state);

    open_move(&mut state);
    let edge = state.handle_input_bytes(b"L");
    let stale = request_id(&edge);
    frame(&mut state);
    let cancelled = esc(&mut state);
    assert!(
        sent_methods(&cancelled).is_empty(),
        "{:?}",
        sent_methods(&cancelled)
    );
    assert!(
        cancelled.requests.is_empty(),
        "Esc reaches no pane: {:?}",
        cancelled.requests
    );
    assert_eq!(state.mode, ClientShellMode::Terminal, "Esc ends move mode");
    assert_eq!(frame(&mut state), idle, "Esc leaves the idle frame");
    let (answer, _) = below_c_answer(state.layout(COLS, ROWS).pane_surface);
    let (_, late) = state.handle_endpoint_result("boot-1", &stale, Ok(answer));
    assert!(late.is_empty(), "{late:?}");
    assert_eq!(
        frame(&mut state),
        idle,
        "a late dry-run answer does not bring the ghost back"
    );

    // A right click on B (whose program reports the mouse) cancels too.
    open_move(&mut state);
    assert_eq!(state.mode, ClientShellMode::Move);
    let b = rect_of(&state, B);
    let click = state.handle_raw_events(vec![mouse(
        MouseEventKind::Down(MouseButton::Right),
        b.x + b.width / 2,
        b.y + b.height / 2,
    )]);
    assert_quiet(&click, "right click");
    assert_eq!(
        state.mode,
        ClientShellMode::Terminal,
        "a right click ends move mode"
    );
    assert_eq!(
        frame(&mut state),
        idle,
        "a right click leaves the idle frame"
    );
}

/// With the target on the source, there is no pane zone: Enter puts it back,
/// and Shift+arrow means the tab edge on that side.
#[test]
fn pane_move_mode_target_on_source_means_tab_edge() {
    let mut state = three_pane_state(true);
    focus(&mut state, B);
    let idle = frame(&mut state);

    // C, then `h` back onto B itself; Enter sends nothing and ends the mode.
    open_move(&mut state);
    step(&mut state, b"h", "h onto the source");
    let drop = state.handle_input_bytes(ENTER);
    assert_quiet(&drop, "Enter on the source");
    assert_eq!(state.mode, ClientShellMode::Terminal);
    assert_eq!(
        frame(&mut state),
        idle,
        "dropping on the source changes nothing"
    );

    // Shift+Left on the source: the tab's left edge.
    open_move(&mut state);
    step(&mut state, b"h", "h onto the source");
    let edge = state.handle_input_bytes(SHIFT_LEFT);
    assert_no_pane_input(&edge, "Shift+Left on the source");
    assert!(
        matches!(&sent_methods(&edge)[..], [Method::PanePlace(params)]
            if params.pane_id == B
                && params.target == PanePlaceTarget::Tab { tab_id: "tab_1".into() }
                && params.side == PaneDirection::Left
                && params.dry_run),
        "Shift+Left on the source asks a dry run for the tab's left edge: {:?}",
        sent_methods(&edge)
    );
    let drop = state.handle_input_bytes(ENTER);
    assert_no_pane_input(&drop, "Enter on the tab edge");
    assert!(
        matches!(&sent_methods(&drop)[..], [Method::PanePlace(params)]
            if params.pane_id == B
                && params.target == PanePlaceTarget::Tab { tab_id: "tab_1".into() }
                && params.side == PaneDirection::Left
                && !params.dry_run
                && params.focus),
        "Enter places B along the tab's left edge: {:?}",
        sent_methods(&drop)
    );
    assert_eq!(state.mode, ClientShellMode::Terminal);
}

/// The mode opens with the target on the source's nearest neighbour, looking
/// right, then down, then left, then up. A centre drop shows where it was.
#[test]
fn pane_move_mode_opens_on_nearest_neighbour_right_down_left_up() {
    // (source, expected first target) on the 5 x 2 grid.
    for (source, expected, why) in [
        (A, "pane_g1", "right before down"),
        ("pane_g4", "pane_g9", "down before left"),
        ("pane_g9", "pane_g8", "left before up"),
    ] {
        let mut state = grid_state(10, true);
        focus(&mut state, source);
        open_move(&mut state);
        assert_eq!(state.mode, ClientShellMode::Move, "{source}: move mode");
        assert_eq!(
            centre_drop_target(&mut state, source),
            expected,
            "{source}: {why}"
        );
    }
    // A has only panes above it: the target opens on one of them.
    let mut state = three_pane_state(true);
    open_move(&mut state);
    assert_eq!(state.mode, ClientShellMode::Move);
    let target = centre_drop_target(&mut state, A);
    assert!(
        target == B || target == C,
        "up when nothing is right, below or left: {target}"
    );
}

/// hjkl and the arrows move the target to the neighbour of the current
/// target, zone centre, sending nothing; a key with no neighbour keeps it.
#[test]
fn pane_move_mode_arrows_and_hjkl_move_the_target() {
    let mut state = grid_state(10, true);
    open_move(&mut state);
    assert_eq!(state.mode, ClientShellMode::Move);
    // Opens on pane_g1 (row 0, column 1).
    step(&mut state, b"l", "l"); // pane_g2
    step(&mut state, b"j", "j"); // pane_g7
    step(&mut state, LEFT, "Left"); // pane_g6
    step(&mut state, UP, "Up"); // pane_g1
    step(&mut state, RIGHT, "Right"); // pane_g2
    step(&mut state, b"k", "k with nothing above"); // stays on pane_g2
    assert_eq!(centre_drop_target(&mut state, A), "pane_g2");
}

/// `[` and `]` belong to move mode (S8 makes them spring-load tabs). They never
/// reach a pane, never start copy mode, and never rearrange anything.
#[test]
fn pane_move_mode_brackets_stay_in_move_mode() {
    let mut state = three_pane_state(true);
    focus(&mut state, B);
    open_move(&mut state);
    for key in [b"[", b"]"] {
        let outcome = state.handle_input_bytes(key);
        assert_no_pane_input(&outcome, "bracket");
        assert!(
            !sent_methods(&outcome).iter().any(|method| matches!(
                method,
                Method::PanePlace(_) | Method::PaneSwap(_) | Method::PaneMove(_)
            )),
            "a bracket rearranges nothing: {:?}",
            sent_methods(&outcome)
        );
        assert_eq!(
            state.mode,
            ClientShellMode::Move,
            "a bracket keeps move mode"
        );
    }
    esc(&mut state);
    assert_eq!(state.mode, ClientShellMode::Terminal);
}

/// prefix+m does not open the mode when there is nothing it could do. Each
/// refusal is followed by the same state with the one blocking fact removed,
/// so the refusal is about that fact.
#[test]
fn pane_move_mode_refuses_when_nothing_can_move() {
    // A lone pane: no mode, and the hint `nothing to move`.
    let mut state = grid_state(1, true);
    assert!(!frame_text(&frame(&mut state)).contains(NOTHING_TO_MOVE));
    open_move(&mut state);
    assert_eq!(
        state.mode,
        ClientShellMode::Terminal,
        "a lone pane stays put"
    );
    let text = frame_text(&frame(&mut state));
    assert!(
        text.contains(NOTHING_TO_MOVE),
        "a lone pane shows `{NOTHING_TO_MOVE}`:\n{text}"
    );
    assert!(!text.contains(HINT), "no move hint for a lone pane");

    // A zoomed tab: no mode until it is unzoomed.
    let mut state = three_pane_state(true);
    set_zoomed(&mut state, true);
    open_move(&mut state);
    assert_ne!(
        state.mode,
        ClientShellMode::Move,
        "a zoomed tab has no move"
    );
    set_zoomed(&mut state, false);
    open_move(&mut state);
    assert_eq!(state.mode, ClientShellMode::Move, "unzoomed, it opens");
    esc(&mut state);

    // An endpoint without pane.place: no mode until it offers the method.
    let mut state = three_pane_state(false);
    open_move(&mut state);
    assert_ne!(
        state.mode,
        ClientShellMode::Move,
        "no move mode without pane.place"
    );
    state.set_endpoint_methods(Some(methods(true)));
    frame(&mut state);
    open_move(&mut state);
    assert_eq!(
        state.mode,
        ClientShellMode::Move,
        "with pane.place, it opens"
    );
    esc(&mut state);
}

/// `keys.move_pane_mode` defaults to prefix+m (every test above) and can be
/// rebound. The mode is keyboard-only, so it works with mouse capture off.
#[test]
fn pane_move_mode_key_is_configurable() {
    let rebound: Config =
        toml::from_str("[keys]\nmove_pane_mode = \"alt+m\"").expect("rebound config");
    let mut state = three_pane_state(true);
    state.config.keybinds = ClientShellConfig::from_config(&rebound).keybinds;
    open_move(&mut state);
    assert_ne!(
        state.mode,
        ClientShellMode::Move,
        "prefix+m no longer opens move mode once rebound"
    );
    let alt_m = state.handle_raw_events(vec![RawInputEvent::Key(crate::input::TerminalKey::new(
        KeyCode::Char('m'),
        KeyModifiers::ALT,
    ))]);
    assert_quiet(&alt_m, "alt+m");
    assert_eq!(
        state.mode,
        ClientShellMode::Move,
        "the rebound key opens move mode"
    );
    esc(&mut state);

    let mut state = three_pane_state(true);
    state.config.mouse_capture = false;
    open_move(&mut state);
    assert_eq!(
        state.mode,
        ClientShellMode::Move,
        "move mode does not need mouse capture"
    );
    let target = centre_drop_target(&mut state, A);
    assert!(target == B || target == C, "{target}");
}
