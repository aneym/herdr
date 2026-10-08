//! Owner-written fixture and base characterization for pane drag slice S3
//! (spec `pane-drag-rearrange-2026-10-07`). Implementers may not edit this
//! file, `pane_drag.rs` beside it, or the golden it reads.
//!
//! This module uses only interfaces that exist on the S3 base (782f26a1), so
//! it compiles there on its own. The golden it checks is captured from the
//! base, before any S3 product code exists:
//!
//! ```text
//! HERDR_BLESS_PANE_DRAG_IDLE=1 just test-one pane_drag_fixture
//! ```
//!
//! run on the base with only this module registered. After that, the S3 head
//! must reproduce the base idle frame byte for byte whenever the endpoint does
//! not offer `pane.place`.
//!
//! S4 (`pane_motion.rs`) reuses the builders below to stage server reflows;
//! the refactor that exposed them leaves every fixture byte unchanged.

use super::*;
use crossterm::event::MouseEvent;

pub(super) const COLS: u16 = 106;
pub(super) const ROWS: u16 = 40;

/// The dragged pane: the full-width shell along the bottom.
pub(super) const A: &str = "pane_1";
/// Top left; its program has mouse reporting on.
pub(super) const B: &str = "pane_b";
/// Top right.
pub(super) const C: &str = "pane_c";
/// A's pane label, which the drag chip shows.
pub(super) const A_LABEL: &str = "build";

const GOLDEN: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/src/client/shell/tests/golden/pane_drag_idle_base.txt"
);
const BLESS_ENV: &str = "HERDR_BLESS_PANE_DRAG_IDLE";

/// Every method the scenario's endpoint offers, plus `pane.place` when the
/// endpoint supports pane drag.
pub(super) fn methods(pane_place: bool) -> Vec<String> {
    let mut methods = [
        "pane.focus",
        "pane.swap",
        "pane.move",
        "tab.focus",
        "layout.set_split_ratio",
    ]
    .map(String::from)
    .to_vec();
    if pane_place {
        methods.push("pane.place".into());
    }
    methods
}

/// One pane of a fixture surface, in surface-relative cells.
pub(super) struct FixturePane {
    pub(super) pane_id: String,
    pub(super) rect: SurfaceRect,
    pub(super) fill: char,
    pub(super) mouse_reporting: bool,
}

fn inner(rect: SurfaceRect) -> SurfaceRect {
    SurfaceRect {
        x: rect.x + 1,
        y: rect.y + 1,
        width: rect.width.saturating_sub(2),
        height: rect.height.saturating_sub(2),
    }
}

fn client_pane(pane_id: &str, label: Option<&str>) -> crate::protocol::ClientShellPane {
    crate::protocol::ClientShellPane {
        tokens: Default::default(),
        pane_id: pane_id.into(),
        workspace_id: "ws_1".into(),
        tab_id: "tab_1".into(),
        label: label.map(str::to_owned),
        cwd: Some("/repo".into()),
        foreground_cwd: Some("/repo".into()),
        focused: pane_id == A,
        right_click_passthrough: false,
        machine: None,
    }
}

/// A server-drawn surface: each pane has its own square border and is filled
/// with one letter. It carries no rounded corner and no grip glyph, so any
/// `╭`, `╯` or `⠿` in a composed frame came from the client.
pub(super) fn fixture_surface(
    width: u16,
    height: u16,
    panes: &[FixturePane],
    splits: Vec<PaneSurfaceSplit>,
) -> PaneSurfaceFrame {
    let mut buffer = Buffer::empty(Rect::new(0, 0, width, height));
    for pane in panes {
        let rect = Rect::new(pane.rect.x, pane.rect.y, pane.rect.width, pane.rect.height);
        for y in rect.y..rect.bottom() {
            for x in rect.x..rect.right() {
                let symbol = match (
                    x == rect.x,
                    x + 1 == rect.right(),
                    y == rect.y,
                    y + 1 == rect.bottom(),
                ) {
                    (true, _, true, _) => "┌".to_string(),
                    (_, true, true, _) => "┐".to_string(),
                    (true, _, _, true) => "└".to_string(),
                    (_, true, _, true) => "┘".to_string(),
                    (_, _, true, _) | (_, _, _, true) => "─".to_string(),
                    (true, _, _, _) | (_, true, _, _) => "│".to_string(),
                    _ => pane.fill.to_string(),
                };
                buffer[(x, y)].set_symbol(&symbol);
            }
        }
    }
    PaneSurfaceFrame {
        boot_id: "boot-1".into(),
        projection_revision: 1,
        surface_revision: 1,
        frame: FrameData::from_ratatui_buffer_with_hyperlinks(&buffer, None, &[]),
        panes: panes
            .iter()
            .map(|pane| PaneSurfacePane {
                pane_id: pane.pane_id.clone(),
                content_revision: 0,
                rect: pane.rect,
                inner_rect: inner(pane.rect),
                scrollbar_rect: None,
                scroll: None,
                focused: pane.pane_id == A,
                mouse_reporting: pane.mouse_reporting,
                sgr_pixel_mouse: false,
                alternate_screen_active: false,
                pixel_width: 0,
                pixel_height: 0,
            })
            .collect(),
        splits,
        popup: None,
        graphics: crate::protocol::SurfaceGraphicsScene::default(),
    }
}

/// A shell state on the tree sidebar (two spaces; `ws_1` holds `tab_1` and
/// `tab_2`, `ws_2` holds `tab_3`) whose focused tab `tab_1` shows `panes`.
pub(super) fn state_with_panes(
    pane_place: bool,
    panes: &[FixturePane],
    splits: impl FnOnce(u16, u16) -> Vec<PaneSurfaceSplit>,
) -> ClientShellState {
    let mut config = ClientShellConfig::from_config(&Config::default());
    config.agent_panel_sort = crate::config::AgentPanelSortConfig::Tree;
    let mut state = ClientShellState::new(config);
    let mut snapshot = super::tree::tree_snapshot();
    snapshot.panes.retain(|pane| pane.pane_id != A);
    for pane in panes {
        let label = (pane.pane_id == A).then_some(A_LABEL);
        snapshot.panes.push(client_pane(&pane.pane_id, label));
    }
    state.set_snapshot(Box::new(snapshot));
    state.set_endpoint_methods(Some(methods(pane_place)));
    let area = state.layout(COLS, ROWS).pane_surface;
    state.set_pane_surface(fixture_surface(
        area.width,
        area.height,
        panes,
        splits(area.width, area.height),
    ));
    state.compose(COLS, ROWS).expect("fixture frame");
    state
}

/// Surface-relative rects of the scenario's three panes: `B | C` on top, `A`
/// full width below. Two agents side by side, a shell below.
pub(super) fn three_pane_rects(width: u16, height: u16) -> [(&'static str, SurfaceRect); 3] {
    three_pane_rects_at(width, height, height / 2)
}

/// The same three panes with the horizontal divider at row `top`.
pub(super) fn three_pane_rects_at(
    width: u16,
    height: u16,
    top: u16,
) -> [(&'static str, SurfaceRect); 3] {
    let half = width / 2;
    [
        (
            A,
            SurfaceRect {
                x: 0,
                y: top,
                width,
                height: height - top,
            },
        ),
        (
            B,
            SurfaceRect {
                x: 0,
                y: 0,
                width: half,
                height: top,
            },
        ),
        (
            C,
            SurfaceRect {
                x: half,
                y: 0,
                width: width - half,
                height: top,
            },
        ),
    ]
}

/// The fixture panes at `rects`: one fill letter per pane id, and pane B with
/// mouse reporting on.
pub(super) fn three_pane_panes(rects: [(&'static str, SurfaceRect); 3]) -> Vec<FixturePane> {
    rects
        .into_iter()
        .map(|(pane_id, rect)| FixturePane {
            pane_id: pane_id.into(),
            rect,
            fill: match pane_id {
                A => 'a',
                B => 'b',
                _ => 'c',
            },
            mouse_reporting: pane_id == B,
        })
        .collect()
}

/// The splits of `three_pane_rects_at(width, height, top)`: the root
/// divider at `top` (its resize hit covers the rows either side of it) and
/// the `B | C` divider above it.
pub(super) fn three_pane_splits(width: u16, height: u16, top: u16) -> Vec<PaneSurfaceSplit> {
    let half = width / 2;
    vec![
        PaneSurfaceSplit {
            direction: PaneSurfaceSplitDirection::Vertical,
            pos: top,
            area: SurfaceRect {
                x: 0,
                y: 0,
                width,
                height,
            },
            hit_rect: SurfaceRect {
                x: 0,
                y: top - 1,
                width,
                height: 2,
            },
            path: Vec::new(),
        },
        PaneSurfaceSplit {
            direction: PaneSurfaceSplitDirection::Horizontal,
            pos: half,
            area: SurfaceRect {
                x: 0,
                y: 0,
                width,
                height: top,
            },
            hit_rect: SurfaceRect {
                x: half - 1,
                y: 0,
                width: 2,
                height: top,
            },
            path: vec![false],
        },
    ]
}

/// The scenario fixture. Pane B has mouse reporting on. The root split's
/// resize hit covers the row A's top border sits on, so A's grip has to win
/// over the split there.
pub(super) fn three_pane_state(pane_place: bool) -> ClientShellState {
    let mut config = ClientShellConfig::from_config(&Config::default());
    config.agent_panel_sort = crate::config::AgentPanelSortConfig::Tree;
    let mut sizing = ClientShellState::new(config);
    sizing.set_snapshot(Box::new(super::tree::tree_snapshot()));
    let area = sizing.layout(COLS, ROWS).pane_surface;
    assert!(
        area.width >= 60 && area.height >= 24,
        "fixture needs a roomy pane surface, got {area:?}"
    );
    let panes = three_pane_panes(three_pane_rects(area.width, area.height));
    state_with_panes(pane_place, &panes, |width, height| {
        three_pane_splits(width, height, height / 2)
    })
}

/// `count` panes tiled in a grid over the same fixed geometry, for the
/// scaling profile. Pane 0 is `A`.
pub(super) fn grid_state(count: u16, pane_place: bool) -> ClientShellState {
    let columns = count.min(5);
    let rows = count.div_ceil(columns);
    assert_eq!(columns * rows, count, "grid fixture tiles exactly");
    let mut config = ClientShellConfig::from_config(&Config::default());
    config.agent_panel_sort = crate::config::AgentPanelSortConfig::Tree;
    let mut sizing = ClientShellState::new(config);
    sizing.set_snapshot(Box::new(super::tree::tree_snapshot()));
    let area = sizing.layout(COLS, ROWS).pane_surface;
    let (cell_w, cell_h) = (area.width / columns, area.height / rows);
    let panes = (0..count)
        .map(|index| {
            let (column, row) = (index % columns, index / columns);
            let width = if column + 1 == columns {
                area.width - cell_w * column
            } else {
                cell_w
            };
            let height = if row + 1 == rows {
                area.height - cell_h * row
            } else {
                cell_h
            };
            FixturePane {
                pane_id: if index == 0 {
                    A.into()
                } else {
                    format!("pane_g{index}")
                },
                rect: SurfaceRect {
                    x: cell_w * column,
                    y: cell_h * row,
                    width,
                    height,
                },
                fill: char::from(b'a' + (index % 26) as u8),
                mouse_reporting: index % 2 == 1,
            }
        })
        .collect::<Vec<_>>();
    state_with_panes(pane_place, &panes, |_, _| Vec::new())
}

pub(super) fn mouse(kind: MouseEventKind, column: u16, row: u16) -> RawInputEvent {
    RawInputEvent::Mouse(MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::empty(),
    })
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// The golden text of a frame: a hash over every cell field (symbol, colours,
/// modifiers, skip, hyperlink) and the cursor, then the symbol rows between
/// bars so a mismatch reads as a picture.
pub(super) fn golden_text(frame: &FrameData) -> String {
    let bytes = serde_json::to_vec(frame).expect("frame serializes");
    let mut text = format!(
        "# pane drag S3 idle frame, captured on the S3 base; regenerate only there\nfnv1a64 {:016x}\n",
        fnv1a64(&bytes)
    );
    for row in frame_rows(frame) {
        text.push('|');
        text.push_str(&row);
        text.push_str("|\n");
    }
    text
}

/// Today's idle frame, byte for byte, for an endpoint without `pane.place`.
/// The S3 head must not change a single cell of it.
#[test]
fn pane_drag_fixture_idle_frame_matches_base() {
    let mut state = three_pane_state(false);
    let frame = state.compose(COLS, ROWS).expect("idle frame");
    let actual = golden_text(&frame);
    if std::env::var_os(BLESS_ENV).is_some() {
        std::fs::write(GOLDEN, &actual).expect("write golden");
        return;
    }
    let expected = std::fs::read_to_string(GOLDEN).unwrap_or_else(|_| {
        panic!("{GOLDEN} is missing: capture it on the S3 base with {BLESS_ENV}=1")
    });
    let rows = |text: &str| -> Vec<String> {
        text.lines()
            .filter(|line| line.starts_with('|'))
            .map(str::to_owned)
            .collect()
    };
    assert_eq!(
        rows(&actual),
        rows(&expected),
        "idle frame glyphs drifted from the base"
    );
    assert_eq!(
        actual, expected,
        "idle frame styles drifted from the base (same glyphs, different bytes)"
    );
}
