//! Owner-written cases for `TileLayout::place_pane` (pane drag slice S1, spec
//! `pane-drag-rearrange-2026-10-07`). Implementers may not edit this file.
//!
//! These are unit tests by design: `place_pane` is the pure tree algorithm
//! shared by `pane.place` apply and dry run, and its edge cases (side order,
//! root wrap, size clamp, no-op detection inputs, refusals) are cheaper to
//! pin here than through the API scenario, which covers the wiring.
//!
//! Base layout in a 120x40 area: `P1 | (P2 / P3)`, focus on P2.

use super::*;

const AREA: Rect = Rect {
    x: 0,
    y: 0,
    width: 120,
    height: 40,
};

fn pane(id: u32) -> PaneId {
    PaneId::from_raw(id)
}

fn base() -> TileLayout {
    TileLayout::from_saved(
        Node::Split {
            direction: Direction::Horizontal,
            ratio: 0.5,
            first: Box::new(Node::Pane(pane(1))),
            second: Box::new(Node::Split {
                direction: Direction::Vertical,
                ratio: 0.5,
                first: Box::new(Node::Pane(pane(2))),
                second: Box::new(Node::Pane(pane(3))),
            }),
        },
        pane(2),
    )
}

fn single() -> TileLayout {
    TileLayout::from_saved(Node::Pane(pane(1)), pane(1))
}

fn rects(layout: &TileLayout) -> Vec<(u32, Rect)> {
    let mut rects: Vec<_> = layout
        .panes(AREA)
        .into_iter()
        .map(|info| (info.id.raw(), info.rect))
        .collect();
    rects.sort_by_key(|(id, _)| *id);
    rects
}

fn splits(layout: &TileLayout) -> Vec<(u16, f32)> {
    layout
        .splits(AREA)
        .into_iter()
        .map(|split| (split.pos, split.ratio))
        .collect()
}

#[test]
fn left_puts_the_moved_pane_first_after_its_siblings_reflow() {
    let layout = base();
    let placed = layout
        .place_pane(pane(3), Some(pane(1)), NavDirection::Left, 0.5)
        .expect("placement");

    assert_eq!(
        rects(&placed),
        vec![
            (1, Rect::new(30, 0, 30, 40)),
            (2, Rect::new(60, 0, 60, 40)),
            (3, Rect::new(0, 0, 30, 40)),
        ]
    );
    assert_eq!(placed.focused(), pane(2), "placement leaves focus alone");
    assert_eq!(
        rects(&layout),
        vec![
            (1, Rect::new(0, 0, 60, 40)),
            (2, Rect::new(60, 0, 60, 20)),
            (3, Rect::new(60, 20, 60, 20)),
        ],
        "the source layout is not mutated"
    );
}

#[test]
fn up_puts_the_moved_pane_first() {
    let placed = base()
        .place_pane(pane(1), Some(pane(2)), NavDirection::Up, 0.5)
        .expect("placement");

    assert_eq!(
        rects(&placed),
        vec![
            (1, Rect::new(0, 0, 120, 10)),
            (2, Rect::new(0, 10, 120, 10)),
            (3, Rect::new(0, 20, 120, 20)),
        ]
    );
}

#[test]
fn right_and_down_put_the_moved_pane_second() {
    let down = base()
        .place_pane(pane(1), Some(pane(3)), NavDirection::Down, 0.5)
        .expect("placement");
    assert_eq!(
        rects(&down),
        vec![
            (1, Rect::new(0, 30, 120, 10)),
            (2, Rect::new(0, 0, 120, 20)),
            (3, Rect::new(0, 20, 120, 10)),
        ]
    );

    let right = base()
        .place_pane(pane(2), Some(pane(1)), NavDirection::Right, 0.5)
        .expect("placement");
    assert_eq!(
        rects(&right),
        vec![
            (1, Rect::new(0, 0, 30, 40)),
            (2, Rect::new(30, 0, 30, 40)),
            (3, Rect::new(60, 0, 60, 40)),
        ]
    );
}

#[test]
fn no_target_wraps_the_root_on_that_side() {
    let right_third = base()
        .place_pane(pane(2), None, NavDirection::Right, 1.0 / 3.0)
        .expect("placement");
    assert_eq!(
        rects(&right_third),
        vec![
            (1, Rect::new(0, 0, 40, 40)),
            (2, Rect::new(80, 0, 40, 40)),
            (3, Rect::new(40, 0, 40, 40)),
        ]
    );

    let top_quarter = base()
        .place_pane(pane(3), None, NavDirection::Up, 0.25)
        .expect("placement");
    assert_eq!(
        rects(&top_quarter),
        vec![
            (1, Rect::new(0, 10, 60, 30)),
            (2, Rect::new(60, 10, 60, 30)),
            (3, Rect::new(0, 0, 120, 10)),
        ]
    );
}

#[test]
fn size_is_the_moved_panes_share_clamped_to_a_tenth_and_nine_tenths() {
    let tiny = base()
        .place_pane(pane(3), Some(pane(1)), NavDirection::Left, 0.0)
        .expect("placement");
    assert_eq!(rects(&tiny)[2], (3, Rect::new(0, 0, 6, 40)));
    assert_eq!(rects(&tiny)[0], (1, Rect::new(6, 0, 54, 40)));

    let huge = base()
        .place_pane(pane(3), Some(pane(1)), NavDirection::Left, 5.0)
        .expect("placement");
    assert_eq!(rects(&huge)[2], (3, Rect::new(0, 0, 54, 40)));

    let tiny_right = base()
        .place_pane(pane(3), Some(pane(1)), NavDirection::Right, 0.0)
        .expect("placement");
    assert_eq!(rects(&tiny_right)[0], (1, Rect::new(0, 0, 54, 40)));
    assert_eq!(rects(&tiny_right)[2], (3, Rect::new(54, 0, 6, 40)));
}

#[test]
fn placing_again_where_the_pane_already_is_yields_the_same_tree() {
    let once = base()
        .place_pane(pane(2), None, NavDirection::Right, 1.0 / 3.0)
        .expect("placement");
    let twice = once
        .place_pane(pane(2), None, NavDirection::Right, 1.0 / 3.0)
        .expect("placement");

    assert_eq!(rects(&twice), rects(&once));
    assert_eq!(splits(&twice), splits(&once));
}

#[test]
fn a_pane_from_another_layout_is_inserted_without_a_removal() {
    let beside = single()
        .place_pane(pane(9), Some(pane(1)), NavDirection::Up, 0.5)
        .expect("placement");
    assert_eq!(
        rects(&beside),
        vec![
            (1, Rect::new(0, 20, 120, 20)),
            (9, Rect::new(0, 0, 120, 20))
        ]
    );

    let edge = single()
        .place_pane(pane(9), None, NavDirection::Left, 1.0 / 3.0)
        .expect("placement");
    assert_eq!(
        rects(&edge),
        vec![(1, Rect::new(40, 0, 80, 40)), (9, Rect::new(0, 0, 40, 40))]
    );
}

#[test]
fn refuses_itself_a_missing_target_and_emptying_the_layout() {
    assert!(base()
        .place_pane(pane(1), Some(pane(1)), NavDirection::Left, 0.5)
        .is_none());
    assert!(base()
        .place_pane(pane(1), Some(pane(9)), NavDirection::Left, 0.5)
        .is_none());
    assert!(single()
        .place_pane(pane(1), None, NavDirection::Right, 1.0 / 3.0)
        .is_none());
}
