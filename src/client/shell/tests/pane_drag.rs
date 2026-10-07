//! Owner-written check for pane drag slice S3, TUI drag and drop (spec
//! `pane-drag-rearrange-2026-10-07`, section "S3"). Implementers may not edit
//! this file or `pane_drag_fixture.rs`.
//!
//! Everything runs through the real input path (`handle_raw_events`), the real
//! endpoint answer path (`handle_endpoint_result`) and the real composer
//! (`compose`). The only S3 name it relies on is `ClientChromeDrag::Pane`.
//!
//! Geometry (surface-relative, W x H = the fixture's pane surface): `B | C` on
//! top, each `W/2 x H/2`; `A` below, full width. A's grip sits on its top
//! border, inside the root split's resize hit, so the grip must win there.

use super::pane_drag_fixture::{
    grid_state, mouse, three_pane_rects, three_pane_state, A, A_LABEL, B, C, COLS, ROWS,
};
use super::*;
use crate::api::schema::{
    Method, PaneDirection, PaneLayoutPane, PaneLayoutRect, PaneLayoutSnapshot, PaneMoveDestination,
    PanePlaceResult, PanePlaceTarget, ResponseResult,
};

/// Absolute frame rects of the fixture's panes, by id.
struct Geometry {
    surface: Rect,
    a: Rect,
    b: Rect,
}

fn geometry(state: &ClientShellState) -> Geometry {
    let surface = state.layout(COLS, ROWS).pane_surface;
    let rect = |id: &str| {
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
    };
    Geometry {
        surface,
        a: rect(A),
        b: rect(B),
    }
}

/// The grip's two cells: centred on the pane's top border.
fn grip(rect: Rect) -> (u16, u16) {
    (rect.x + (rect.width - 2) / 2, rect.y)
}

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

/// No pane program saw any of these mouse events.
fn assert_no_pane_input(outcome: &ClientShellInput, step: &str) {
    assert!(
        !outcome.requests.iter().any(|request| matches!(
            request,
            ClientMessage::ClientShellPaneInput { .. }
                | ClientMessage::ClientShellPopupInput { .. }
        )),
        "{step}: a pane program received input during a pane drag: {:?}",
        outcome.requests
    );
}

fn cell(frame: &FrameData, x: u16, y: u16) -> &crate::protocol::CellData {
    &frame.cells[usize::from(y) * usize::from(frame.width) + usize::from(x)]
}

fn symbol(frame: &FrameData, x: u16, y: u16) -> &str {
    cell(frame, x, y).symbol.as_str()
}

/// The symbols of `width` cells from `(x, y)`, cell by cell.
fn text_at(frame: &FrameData, x: u16, y: u16, width: u16) -> String {
    (x..x + width).map(|x| symbol(frame, x, y)).collect()
}

fn surface_contains(frame: &FrameData, surface: Rect, needle: &str) -> bool {
    (surface.y..surface.bottom())
        .any(|y| (surface.x..surface.right()).any(|x| symbol(frame, x, y) == needle))
}

/// Press on A's grip and move straight to `to` in one motion event.
fn lift_a_to(state: &mut ClientShellState, to: (u16, u16)) -> (ClientShellInput, ClientShellInput) {
    let (gx, gy) = grip(geometry(state).a);
    let press =
        state.handle_raw_events(vec![mouse(MouseEventKind::Down(MouseButton::Left), gx, gy)]);
    let moved = state.handle_raw_events(vec![mouse(
        MouseEventKind::Drag(MouseButton::Left),
        to.0,
        to.1,
    )]);
    (press, moved)
}

/// The server's dry-run answer: with A gone, `B | C` fill the tab and A would
/// be B's right half, full height. Rects are in the server's layout space,
/// whose origin is not the client's surface origin; the client maps them
/// through `target_layout.area`.
fn dry_run_answer(surface: Rect, origin: (u16, u16)) -> (ResponseResult, Rect) {
    let (w, h) = (surface.width, surface.height);
    let half = w / 2;
    let quarter = half / 2;
    let layout_rect = |x: u16, y: u16, width: u16, height: u16| PaneLayoutRect {
        x: origin.0 + x,
        y: origin.1 + y,
        width,
        height,
    };
    let placed = layout_rect(quarter, 0, half - quarter, h);
    let result = ResponseResult::PanePlace {
        place: PanePlaceResult {
            changed: true,
            dry_run: true,
            reason: None,
            pane_id: A.into(),
            previous_pane_id: A.into(),
            placed_rect: placed,
            target_layout: PaneLayoutSnapshot {
                workspace_id: "ws_1".into(),
                tab_id: "tab_1".into(),
                zoomed: false,
                area: layout_rect(0, 0, w, h),
                focused_pane_id: A.into(),
                panes: vec![
                    PaneLayoutPane {
                        pane_id: B.into(),
                        focused: false,
                        rect: layout_rect(0, 0, quarter, h),
                    },
                    PaneLayoutPane {
                        pane_id: A.into(),
                        focused: true,
                        rect: placed,
                    },
                    PaneLayoutPane {
                        pane_id: C.into(),
                        focused: false,
                        rect: layout_rect(half, 0, w - half, h),
                    },
                ],
                splits: Vec::new(),
            },
            source_layout: None,
            closed_tab_id: None,
            closed_workspace_id: None,
            focused_pane_id: A.into(),
        },
    };
    let on_screen = Rect::new(surface.x + quarter, surface.y, half - quarter, h);
    (result, on_screen)
}

/// S3 steps 1-8, in the spec's order.
#[test]
fn pane_drag_end_to_end() {
    // Idle: every pane of the 3-pane tab shows a grip; nothing is lifted.
    let mut state = three_pane_state(true);
    let idle = state.compose(COLS, ROWS).expect("idle frame").frame.clone();
    let g = geometry(&state);
    let (gx, gy) = grip(g.a);
    assert_eq!(
        text_at(&idle, gx, gy, 2),
        "⠿⠿",
        "A's grip on its top border"
    );
    assert_eq!(
        text_at(&idle, grip(g.b).0, grip(g.b).1, 2),
        "⠿⠿",
        "B's grip on its top border"
    );
    assert!(!surface_contains(&idle, g.surface, "╭"), "no ghost at idle");

    // 1. Press A's grip, move into B's right band: exactly one dry run for
    //    (B, right), and no pane program sees the drag.
    let right_band = (g.b.right() - 4, g.b.y + g.b.height / 2);
    let (press, moved) = lift_a_to(&mut state, right_band);
    assert!(
        sent_methods(&press).is_empty(),
        "a grip press sends nothing until it moves or releases: {:?}",
        sent_methods(&press)
    );
    assert_no_pane_input(&press, "press");
    assert!(
        matches!(state.chrome_drag, Some(ClientChromeDrag::Pane { .. })),
        "leaving the press cell starts a pane drag, even on the split's resize row"
    );
    assert!(
        matches!(&sent_methods(&moved)[..], [Method::PanePlace(params)]
            if params.pane_id == A
                && params.target == PanePlaceTarget::Pane { pane_id: B.into() }
                && params.side == PaneDirection::Right
                && params.dry_run),
        "one dry run for B's right edge: {:?}",
        sent_methods(&moved)
    );
    assert_no_pane_input(&moved, "move into B");
    let dry_run_id = request_id(&moved);
    let pointer = (right_band.0 + 1, right_band.1);
    let same_zone = state.handle_raw_events(vec![mouse(
        MouseEventKind::Drag(MouseButton::Left),
        pointer.0,
        pointer.1,
    )]);
    assert!(
        sent_methods(&same_zone).is_empty(),
        "moving inside the same zone asks nothing new: {:?}",
        sent_methods(&same_zone)
    );
    assert_no_pane_input(&same_zone, "move inside B's right band");

    // Before the answer: lifted look, and the ghost on the local estimate
    // (the right half of B's current rect).
    let lifted = state
        .compose(COLS, ROWS)
        .expect("lifted frame")
        .frame
        .clone();
    let accent = crate::protocol::color_to_u32(state.config.palette.accent);
    let surface1 = crate::protocol::color_to_u32(state.config.palette.surface1);
    let dim = Modifier::DIM.bits();
    for x in [gx, gx + 1] {
        assert_eq!(symbol(&lifted, x, gy), "⠿");
        assert_eq!(cell(&lifted, x, gy).fg, accent, "the lifted grip is accent");
    }
    let a_content = (g.a.right() - 10, g.a.y + 3);
    assert_eq!(cell(&idle, a_content.0, a_content.1).modifier & dim, 0);
    assert_ne!(
        cell(&lifted, a_content.0, a_content.1).modifier & dim,
        0,
        "the lifted pane's content is DIM"
    );
    // `zone_estimate_rect` rounds the first half the way `layout::split_rect` does.
    let first_half = (f32::from(g.b.width) * 0.5).round() as u16;
    let estimate = Rect::new(
        g.b.x + first_half,
        g.b.y,
        g.b.width - first_half,
        g.b.height,
    );
    assert_eq!(symbol(&lifted, estimate.x, estimate.y), "╭");
    assert_eq!(
        symbol(&lifted, estimate.right() - 1, estimate.bottom() - 1),
        "╯",
        "until the dry run answers, the ghost sits on the local estimate"
    );

    // 2. Feed the dry-run answer: the ghost moves to `placed_rect`, and the
    //    chip ` ⠿ <label> ` sits one row below, one column right of the pointer.
    let (answer, placed) = dry_run_answer(g.surface, (3, 1));
    let (_, follow_up) = state.handle_endpoint_result("boot-1", &dry_run_id, Ok(answer));
    assert!(follow_up.is_empty(), "{follow_up:?}");
    let shown = state
        .compose(COLS, ROWS)
        .expect("ghost frame")
        .frame
        .clone();
    assert_eq!(
        symbol(&shown, placed.x, placed.y),
        "╭",
        "ghost top-left at placed_rect"
    );
    assert_eq!(
        symbol(&shown, placed.right() - 1, placed.bottom() - 1),
        "╯",
        "ghost bottom-right at placed_rect"
    );
    let chip = format!(" ⠿ {A_LABEL} ");
    assert_eq!(
        text_at(
            &shown,
            pointer.0 + 1,
            pointer.1 + 1,
            chip.chars().count() as u16
        ),
        chip,
        "chip beside the pointer"
    );
    assert_eq!(
        cell(&shown, pointer.0 + 2, pointer.1 + 1).bg,
        surface1,
        "chip on surface1"
    );
    let inside = (placed.x + 5, placed.y + 5);
    assert_eq!(
        symbol(&shown, inside.0, inside.1),
        symbol(&idle, inside.0, inside.1),
        "the ghost tints; it never replaces glyphs"
    );
    assert_eq!(
        cell(&shown, inside.0, inside.1).fg,
        cell(&idle, inside.0, inside.1).fg,
        "the ghost keeps foregrounds"
    );
    assert_eq!(
        cell(&shown, inside.0, inside.1).bg,
        surface1,
        "ghost interior on surface1"
    );

    // 3. Release: the real placement, focused.
    let drop = state.handle_raw_events(vec![mouse(
        MouseEventKind::Up(MouseButton::Left),
        pointer.0,
        pointer.1,
    )]);
    assert!(
        matches!(&sent_methods(&drop)[..], [Method::PanePlace(params)]
            if params.pane_id == A
                && params.target == PanePlaceTarget::Pane { pane_id: B.into() }
                && params.side == PaneDirection::Right
                && !params.dry_run
                && params.focus),
        "release places A right of B: {:?}",
        sent_methods(&drop)
    );
    assert_no_pane_input(&drop, "release");

    // 4. Esc before the release: nothing is sent, and the next frame is the
    //    idle frame, even when the stale dry run answers afterwards.
    let mut state = three_pane_state(true);
    let idle = state.compose(COLS, ROWS).expect("idle frame").frame.clone();
    let (_, moved) = lift_a_to(&mut state, right_band);
    let stale_id = request_id(&moved);
    state.compose(COLS, ROWS).expect("lifted frame");
    let esc = state.handle_raw_events(vec![RawInputEvent::Key(crate::input::TerminalKey::new(
        crossterm::event::KeyCode::Esc,
        KeyModifiers::empty(),
    ))]);
    assert!(sent_methods(&esc).is_empty(), "{:?}", sent_methods(&esc));
    assert!(
        esc.requests.is_empty(),
        "Esc reaches no pane: {:?}",
        esc.requests
    );
    let release = state.handle_raw_events(vec![mouse(
        MouseEventKind::Up(MouseButton::Left),
        right_band.0,
        right_band.1,
    )]);
    assert!(
        sent_methods(&release).is_empty(),
        "{:?}",
        sent_methods(&release)
    );
    assert!(release.requests.is_empty(), "{:?}", release.requests);
    assert_eq!(
        state.compose(COLS, ROWS).expect("cancelled frame").frame,
        idle,
        "a cancelled drag leaves the idle frame"
    );
    let (answer, _) = dry_run_answer(g.surface, (3, 1));
    let (_, late) = state.handle_endpoint_result("boot-1", &stale_id, Ok(answer));
    assert!(late.is_empty(), "{late:?}");
    assert_eq!(
        state
            .compose(COLS, ROWS)
            .expect("frame after a late answer")
            .frame,
        idle,
        "a late dry-run answer does not bring the ghost back"
    );

    // 5. Centre of B: no dry run, the ghost is B's rect, release swaps.
    let mut state = three_pane_state(true);
    let centre = (g.b.x + g.b.width / 2, g.b.y + g.b.height / 2);
    let (_, moved) = lift_a_to(&mut state, centre);
    assert!(
        sent_methods(&moved).is_empty(),
        "a centre zone needs no dry run: {:?}",
        sent_methods(&moved)
    );
    let swap_frame = state
        .compose(COLS, ROWS)
        .expect("centre frame")
        .frame
        .clone();
    assert_eq!(
        symbol(&swap_frame, g.b.x, g.b.y),
        "╭",
        "centre ghost is B's rect"
    );
    assert_eq!(symbol(&swap_frame, g.b.right() - 1, g.b.bottom() - 1), "╯");
    let drop = state.handle_raw_events(vec![mouse(
        MouseEventKind::Up(MouseButton::Left),
        centre.0,
        centre.1,
    )]);
    assert!(
        matches!(&sent_methods(&drop)[..], [Method::PaneSwap(params)]
            if params.source_pane_id.as_deref() == Some(A)
                && params.target_pane_id.as_deref() == Some(B)),
        "centre release swaps A and B: {:?}",
        sent_methods(&drop)
    );
    assert_no_pane_input(&drop, "centre release");

    // 6. A tab-bar tab of the same workspace: into that tab, right side.
    let mut state = three_pane_state(true);
    let tab = state
        .hits
        .tabs
        .iter()
        .find(|(_, tab_id)| tab_id == "tab_2")
        .map(|(rect, _)| *rect)
        .expect("tab_2 in the tab bar");
    let over_tab = (tab.x + tab.width / 2, tab.y);
    let (_, moved) = lift_a_to(&mut state, over_tab);
    assert!(
        sent_methods(&moved).is_empty(),
        "{:?}",
        sent_methods(&moved)
    );
    state.compose(COLS, ROWS).expect("tab hover frame");
    let drop = state.handle_raw_events(vec![mouse(
        MouseEventKind::Up(MouseButton::Left),
        over_tab.0,
        over_tab.1,
    )]);
    assert!(
        matches!(&sent_methods(&drop)[..], [Method::PanePlace(params)]
            if params.pane_id == A
                && params.target == PanePlaceTarget::Tab { tab_id: "tab_2".into() }
                && params.side == PaneDirection::Right
                && !params.dry_run
                && params.focus),
        "tab release places A into tab_2: {:?}",
        sent_methods(&drop)
    );

    // 7. A space header in the sidebar: a new tab in that space.
    let mut state = three_pane_state(true);
    let header = state
        .hits
        .tree_headers
        .iter()
        .find(|hit| hit.tab_id.is_none() && hit.workspace_id == "ws_2")
        .expect("ws_2 space header");
    let column = (header.rect.x..header.rect.right())
        .find(|x| {
            ![header.chevron, header.plus, header.pin]
                .iter()
                .any(|control| contains(*control, (*x, header.rect.y)))
        })
        .expect("a plain cell on the space header");
    let over_header = (column, header.rect.y);
    let (_, moved) = lift_a_to(&mut state, over_header);
    assert!(
        sent_methods(&moved).is_empty(),
        "{:?}",
        sent_methods(&moved)
    );
    state.compose(COLS, ROWS).expect("header hover frame");
    let drop = state.handle_raw_events(vec![mouse(
        MouseEventKind::Up(MouseButton::Left),
        over_header.0,
        over_header.1,
    )]);
    assert!(
        matches!(&sent_methods(&drop)[..], [Method::PaneMove(params)]
            if params.pane_id == A
                && params.focus
                && matches!(&params.destination, PaneMoveDestination::NewTab { workspace_id, .. }
                    if workspace_id.as_deref() == Some("ws_2"))),
        "space header release opens A as a new tab in ws_2: {:?}",
        sent_methods(&drop)
    );

    // 8. An endpoint without `pane.place`: no grip, and the same gesture on
    //    B's top border never becomes a pane drag.
    let mut state = three_pane_state(false);
    let frame = state
        .compose(COLS, ROWS)
        .expect("frame without pane.place")
        .frame
        .clone();
    assert!(
        !surface_contains(&frame, g.surface, "⠿"),
        "no grip without pane.place"
    );
    let (bx, by) = grip(g.b);
    let mut outcomes =
        vec![state.handle_raw_events(vec![mouse(MouseEventKind::Down(MouseButton::Left), bx, by)])];
    outcomes.push(state.handle_raw_events(vec![mouse(
        MouseEventKind::Drag(MouseButton::Left),
        g.a.x + g.a.width / 2,
        g.a.y + g.a.height / 2,
    )]));
    assert!(!matches!(
        state.chrome_drag,
        Some(ClientChromeDrag::Pane { .. })
    ));
    let frame = state
        .compose(COLS, ROWS)
        .expect("frame mid-gesture")
        .frame
        .clone();
    assert!(
        !surface_contains(&frame, g.surface, "╭"),
        "no ghost without pane.place"
    );
    outcomes.push(state.handle_raw_events(vec![mouse(
        MouseEventKind::Up(MouseButton::Left),
        g.a.x + g.a.width / 2,
        g.a.y + g.a.height / 2,
    )]));
    for outcome in &outcomes {
        assert!(
            !sent_methods(outcome).iter().any(|method| matches!(
                method,
                Method::PanePlace(_) | Method::PaneSwap(_) | Method::PaneMove(_)
            )),
            "no pane rearrangement without pane.place: {:?}",
            sent_methods(outcome)
        );
    }
}

/// Idle-frame characterization: with `pane.place` on offer, the idle frame is
/// the base frame (`pane_drag_fixture_idle_frame_matches_base`) plus exactly
/// the grips, two `⠿` cells per pane of the 3-pane tab, and nothing else.
#[test]
fn pane_drag_idle_frame_adds_only_grips() {
    let mut without = three_pane_state(false);
    let base = without
        .compose(COLS, ROWS)
        .expect("base frame")
        .frame
        .clone();
    let mut with = three_pane_state(true);
    let idle = with.compose(COLS, ROWS).expect("idle frame").frame.clone();
    assert_eq!((idle.width, idle.height), (base.width, base.height));
    assert_eq!(idle.cursor, base.cursor);
    let surface = with.layout(COLS, ROWS).pane_surface;
    let mut expected = three_pane_rects(surface.width, surface.height)
        .into_iter()
        .flat_map(|(_, rect)| {
            let rect = Rect::new(
                surface.x + rect.x,
                surface.y + rect.y,
                rect.width,
                rect.height,
            );
            let (x, y) = grip(rect);
            [(x, y), (x + 1, y)]
        })
        .collect::<Vec<_>>();
    expected.sort_unstable();
    let mut changed = Vec::new();
    for y in 0..idle.height {
        for x in 0..idle.width {
            if cell(&idle, x, y) != cell(&base, x, y) {
                changed.push((x, y));
            }
        }
    }
    changed.sort_unstable();
    assert_eq!(
        changed, expected,
        "only the grip cells differ from the base"
    );
    for (x, y) in expected {
        assert_eq!(symbol(&idle, x, y), "⠿");
    }
    // A lone pane never has a grip: the tab is the unit.
    let mut lone = grid_state(1, true);
    let frame = lone.compose(COLS, ROWS).expect("lone pane frame");
    assert!(
        !surface_contains(&frame, surface, "⠿"),
        "a lone pane has no grip"
    );
}

fn median_and_p95(mut samples: Vec<u128>) -> (u128, u128) {
    samples.sort_unstable();
    (
        samples[samples.len() / 2],
        samples[samples.len() * 95 / 100],
    )
}

fn profile(mut run: impl FnMut()) -> (u128, u128) {
    let mut samples = Vec::new();
    for sample in 0..35 {
        let start = std::time::Instant::now();
        for _ in 0..20 {
            run();
        }
        if sample >= 5 {
            samples.push(start.elapsed().as_nanos() / 20);
        }
    }
    median_and_p95(samples)
}

/// Compose cost at fixed geometry with 1 and 15 populated panes, idle and
/// mid-drag (mid-drag needs two panes, so its small case is 2), plus the
/// pointer-move cost that carries the O(panes) zone lookup. Report the deltas
/// against the base: idle within noise (2% or less); mid-drag compose must not
/// grow with pane count beyond that lookup.
///
/// `cargo test --release --locked --bin herdr client_compose_scale_profile -- --ignored --nocapture --test-threads=1`
#[test]
#[ignore = "manual client compose scaling profile"]
fn client_compose_scale_profile() {
    for count in [1u16, 2, 15] {
        let mut state = grid_state(count, true);
        let (median, p95) = profile(|| {
            std::hint::black_box(state.compose(COLS, ROWS));
        });
        eprintln!("client_compose_scale panes={count} state=idle median_ns={median} p95_ns={p95}");
        if count < 2 {
            continue;
        }
        let first = state.hits.panes[0].rect;
        let last = state.hits.panes[usize::from(count) - 1].rect;
        let (gx, gy) = grip(first);
        let centre = (last.x + last.width / 2, last.y + last.height / 2);
        state.handle_raw_events(vec![mouse(MouseEventKind::Down(MouseButton::Left), gx, gy)]);
        state.handle_raw_events(vec![mouse(
            MouseEventKind::Drag(MouseButton::Left),
            centre.0,
            centre.1,
        )]);
        assert!(matches!(
            state.chrome_drag,
            Some(ClientChromeDrag::Pane { .. })
        ));
        let (median, p95) = profile(|| {
            std::hint::black_box(state.compose(COLS, ROWS));
        });
        eprintln!("client_compose_scale panes={count} state=drag median_ns={median} p95_ns={p95}");
        let mut flip = false;
        let (median, p95) = profile(|| {
            flip = !flip;
            let x = if flip { centre.0 } else { centre.0 + 1 };
            std::hint::black_box(state.handle_raw_events(vec![mouse(
                MouseEventKind::Drag(MouseButton::Left),
                x,
                centre.1,
            )]));
        });
        eprintln!(
            "client_compose_scale panes={count} state=pointer_move median_ns={median} p95_ns={p95}"
        );
    }
}
