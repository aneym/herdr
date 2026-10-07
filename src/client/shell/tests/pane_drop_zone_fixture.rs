//! Owner-written check for pane drag slice S2 (spec
//! `pane-drag-rearrange-2026-10-07`). Implementers may not edit this file or
//! the fixture it reads.
//!
//! `shell/fixtures/pane-drop-zones.json` is the one drop-zone table the TUI,
//! the Mac and Windows all run; its `rules` spell out the geometry. This test
//! runs every case through the TUI's pure `drop_zone_at` and
//! `zone_estimate_rect`, and ties the TUI metrics to the fixture and to the
//! generated motion tokens.

use ratatui::layout::Rect;
use serde_json::Value;

use crate::client::shell::motion_tokens;
use crate::client::shell::pane_drop::{
    drop_zone_at, zone_estimate_rect, DropMetrics, DropSide, DropZone,
};

const FIXTURE: &str = include_str!("../../../../shell/fixtures/pane-drop-zones.json");

fn fixture() -> Value {
    serde_json::from_str(FIXTURE).expect("fixture is json")
}

fn number(value: &Value) -> f32 {
    value
        .as_f64()
        .unwrap_or_else(|| panic!("expected a number, got {value}")) as f32
}

fn rect(value: &Value) -> Rect {
    let cell = |index: usize| {
        let raw = value[index]
            .as_u64()
            .unwrap_or_else(|| panic!("expected a rect, got {value}"));
        u16::try_from(raw).expect("rect fits in u16")
    };
    Rect::new(cell(0), cell(1), cell(2), cell(3))
}

fn metrics(value: &Value) -> DropMetrics {
    DropMetrics {
        tab_edge: number(&value["tabEdge"]),
        band_min: number(&value["bandMin"]),
        band_fraction: number(&value["bandFraction"]),
        band_max_fraction: number(&value["bandMaxFraction"]),
    }
}

fn side(value: &Value) -> DropSide {
    match value.as_str() {
        Some("left") => DropSide::Left,
        Some("right") => DropSide::Right,
        Some("up") => DropSide::Up,
        Some("down") => DropSide::Down,
        _ => panic!("unknown side {value}"),
    }
}

fn pane_index(ids: &[&str], value: &Value) -> usize {
    let id = value
        .as_str()
        .unwrap_or_else(|| panic!("expected a pane id, got {value}"));
    ids.iter()
        .position(|candidate| *candidate == id)
        .unwrap_or_else(|| panic!("pane {id} is not in the layout"))
}

fn expected_zone(value: &Value, ids: &[&str]) -> Option<DropZone> {
    if value.is_null() {
        return None;
    }
    Some(match value["kind"].as_str() {
        Some("centre") => DropZone::Centre {
            target: pane_index(ids, &value["target"]),
        },
        Some("pane_edge") => DropZone::PaneEdge {
            target: pane_index(ids, &value["target"]),
            side: side(&value["side"]),
        },
        Some("tab_edge") => DropZone::TabEdge {
            side: side(&value["side"]),
        },
        _ => panic!("unknown zone {value}"),
    })
}

#[test]
fn every_fixture_case_gives_its_zone_and_estimate() {
    let fixture = fixture();
    let metrics = metrics(&fixture["metrics"]);
    let cases = fixture["cases"].as_array().expect("cases");
    assert!(cases.len() >= 18, "the fixture keeps at least 18 cases");

    let mut failures = Vec::new();
    for case in cases {
        let name = case["name"].as_str().expect("case name");
        let layout = &fixture["layouts"][case["layout"].as_str().expect("layout name")];
        let area = rect(&layout["area"]);
        let layout_panes = layout["panes"].as_array().expect("layout panes");
        let ids: Vec<&str> = layout_panes
            .iter()
            .map(|pane| pane["id"].as_str().expect("pane id"))
            .collect();
        let panes: Vec<Rect> = layout_panes.iter().map(|pane| rect(&pane["rect"])).collect();
        let source = (!case["source"].is_null()).then(|| pane_index(&ids, &case["source"]));
        let point = (number(&case["point"][0]), number(&case["point"][1]));

        let expected = expected_zone(&case["zone"], &ids);
        let zone = drop_zone_at(area, &panes, source, point, &metrics);
        if zone != expected {
            failures.push(format!("{name}: zone {zone:?}, want {expected:?}"));
            continue;
        }
        if let Some(zone) = zone {
            let estimate = zone_estimate_rect(area, &panes, zone);
            let want = rect(&case["estimate"]);
            if estimate != want {
                failures.push(format!("{name}: estimate {estimate:?}, want {want:?}"));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn tui_metrics_are_the_fixture_metrics_and_read_the_motion_tokens() {
    assert_eq!(DropMetrics::TUI, metrics(&fixture()["metrics"]));
    assert_eq!(
        DropMetrics::TUI.band_fraction,
        motion_tokens::EDGE_BAND_FRACTION
    );
    assert_eq!(
        DropMetrics::TUI.band_max_fraction,
        motion_tokens::EDGE_BAND_MAX_FRACTION
    );
}
