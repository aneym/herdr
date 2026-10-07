//! Owner-written scenario for `pane.place` (pane drag slice S1, spec
//! `pane-drag-rearrange-2026-10-07`). Implementers may not edit this file.
//!
//! The run drives the JSON API boundary with wire-shaped requests: tab 1 holds
//! `A | (B / C)`, tab 2 holds `X` and tab 3 holds `Y`, laid out in a 120x40
//! terminal area. Every step checks the app state invariants. The same run
//! repeats on the adversarial identity workspace, where public pane and tab
//! numbers differ from raw ids and tab positions.

use ratatui::layout::{Direction, Rect};
use serde_json::{json, Value};

use crate::{
    api::schema::EventKind, app::App, config::Config, layout::PaneId, workspace::Workspace,
};

const AREA: Rect = Rect {
    x: 0,
    y: 0,
    width: 120,
    height: 40,
};

struct PlaceIds {
    a: String,
    b: String,
    c: String,
    x: String,
    y: String,
    tab1: String,
    tab2: String,
    tab3: String,
}

fn app_with(workspace: Workspace) -> App {
    let (_api_tx, api_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut app = App::new(
        &Config::default(),
        crate::app::AppPolicy::TEST,
        None,
        api_rx,
        crate::api::EventHub::default(),
    );
    app.state.workspaces = vec![workspace];
    app.state.active = Some(0);
    app.state.selected = 0;
    app
}

/// Adds tab 1 `A | (B / C)`, tab 2 `X` and tab 3 `Y` after any tabs the
/// workspace already has, so the adversarial run keeps its identity noise.
fn build_place_tabs(app: &mut App) -> PlaceIds {
    let ws = &mut app.state.workspaces[0];
    let tab1 = ws.test_add_tab(Some("place"));
    ws.switch_tab(tab1);
    let a = ws.tabs[tab1].root_pane;
    let b = ws.test_split(Direction::Horizontal);
    let c = ws.test_split(Direction::Vertical);
    let tab2 = ws.test_add_tab(Some("beside"));
    let x = ws.tabs[tab2].root_pane;
    let tab3 = ws.test_add_tab(Some("lone"));
    let y = ws.tabs[tab3].root_pane;
    app.state.ensure_test_terminals();
    app.state.view.terminal_area = AREA;

    PlaceIds {
        a: public_pane(app, a),
        b: public_pane(app, b),
        c: public_pane(app, c),
        x: public_pane(app, x),
        y: public_pane(app, y),
        tab1: public_tab(app, tab1),
        tab2: public_tab(app, tab2),
        tab3: public_tab(app, tab3),
    }
}

fn public_pane(app: &App, pane_id: PaneId) -> String {
    app.public_pane_id(0, pane_id).expect("public pane id")
}

fn public_tab(app: &App, tab_idx: usize) -> String {
    app.public_tab_id(0, tab_idx).expect("public tab id")
}

fn call(app: &mut App, method: &str, params: Value) -> Value {
    let request = serde_json::from_value(json!({
        "id": "req",
        "method": method,
        "params": params,
    }))
    .unwrap_or_else(|err| panic!("{method} request must parse: {err}"));
    serde_json::from_str(&app.handle_api_request(request)).expect("response is json")
}

fn place(app: &mut App, params: Value) -> Value {
    let response = call(app, "pane.place", params);
    assert_eq!(
        response["result"]["type"], "pane_place",
        "pane.place must succeed: {response}"
    );
    response["result"]["place"].clone()
}

fn place_error_code(app: &mut App, params: Value) -> Value {
    let response = call(app, "pane.place", params);
    assert!(
        response["result"].is_null(),
        "pane.place must fail: {response}"
    );
    response["error"]["code"].clone()
}

fn layout_of(app: &mut App, pane_id: &str) -> Value {
    let response = call(app, "pane.layout", json!({ "pane_id": pane_id }));
    response["result"]["layout"].clone()
}

fn pane_tab(app: &mut App, pane_id: &str) -> Value {
    let response = call(app, "pane.get", json!({ "pane_id": pane_id }));
    response["result"]["pane"]["tab_id"].clone()
}

fn rect(x: u16, y: u16, width: u16, height: u16) -> Value {
    json!({ "x": x, "y": y, "width": width, "height": height })
}

fn rect_of(layout: &Value, pane_id: &str) -> Value {
    layout["panes"]
        .as_array()
        .unwrap_or_else(|| panic!("layout has no panes: {layout}"))
        .iter()
        .find(|pane| pane["pane_id"] == pane_id)
        .map(|pane| pane["rect"].clone())
        .unwrap_or_else(|| panic!("{pane_id} missing from layout {layout}"))
}

fn pane_count(layout: &Value) -> usize {
    layout["panes"].as_array().map_or(0, Vec::len)
}

fn event_mark(app: &App) -> u64 {
    app.event_hub
        .events_after(0)
        .last()
        .map_or(0, |(sequence, _)| *sequence)
}

fn events_since(app: &App, mark: u64) -> Vec<EventKind> {
    app.event_hub
        .events_after(mark)
        .into_iter()
        .map(|(_, envelope)| envelope.event)
        .collect()
}

fn pane_moved_count(events: &[EventKind]) -> usize {
    events
        .iter()
        .filter(|event| **event == EventKind::PaneMoved)
        .count()
}

fn run_pane_place_scenario(mut app: App) {
    let ids = build_place_tabs(&mut app);
    app.state.assert_invariants_for_test();

    let before = layout_of(&mut app, &ids.a);
    assert_eq!(before["area"], rect(0, 0, 120, 40));
    assert_eq!(rect_of(&before, &ids.a), rect(0, 0, 60, 40));
    assert_eq!(rect_of(&before, &ids.b), rect(60, 0, 60, 20));
    assert_eq!(rect_of(&before, &ids.c), rect(60, 20, 60, 20));
    let tab2_before = layout_of(&mut app, &ids.x);

    // 1. A dry run reports the reflowed result and changes nothing. C leaves
    //    first, so A is full height again and C takes the left half of it.
    let mark = event_mark(&app);
    let dry = place(
        &mut app,
        json!({
            "pane_id": ids.c,
            "target": { "type": "pane", "pane_id": ids.a },
            "side": "left",
            "focus": true,
            "dry_run": true,
        }),
    );
    assert_eq!(dry["changed"], true);
    assert_eq!(dry["dry_run"], true);
    assert!(dry["reason"].is_null(), "dry run reason: {dry}");
    assert_eq!(dry["placed_rect"], rect(0, 0, 30, 40));
    assert_eq!(dry["target_layout"]["tab_id"], ids.tab1);
    assert_eq!(rect_of(&dry["target_layout"], &ids.c), rect(0, 0, 30, 40));
    assert_eq!(rect_of(&dry["target_layout"], &ids.a), rect(30, 0, 30, 40));
    assert_eq!(rect_of(&dry["target_layout"], &ids.b), rect(60, 0, 60, 40));
    assert_eq!(
        layout_of(&mut app, &ids.a).to_string(),
        before.to_string(),
        "a dry run must leave the layout byte-equal"
    );
    assert_eq!(layout_of(&mut app, &ids.x), tab2_before);
    assert!(
        events_since(&app, mark).is_empty(),
        "a dry run emits nothing"
    );
    app.state.assert_invariants_for_test();

    // 2. The same call applied gives `(C | A) | B`, focuses C and lands
    //    exactly where the dry run said.
    let mark = event_mark(&app);
    let applied = place(
        &mut app,
        json!({
            "pane_id": ids.c,
            "target": { "type": "pane", "pane_id": ids.a },
            "side": "left",
            "focus": true,
        }),
    );
    assert_eq!(applied["changed"], true);
    assert_eq!(applied["dry_run"], false);
    assert!(applied["reason"].is_null(), "applied reason: {applied}");
    assert_eq!(applied["placed_rect"], dry["placed_rect"]);
    assert_eq!(applied["pane_id"], ids.c);
    assert_eq!(applied["previous_pane_id"], ids.c);
    assert_eq!(applied["focused_pane_id"], ids.c);
    assert!(applied["closed_tab_id"].is_null());
    let after = layout_of(&mut app, &ids.a);
    assert_eq!(pane_count(&after), 3);
    assert_eq!(rect_of(&after, &ids.c), rect(0, 0, 30, 40));
    assert_eq!(rect_of(&after, &ids.a), rect(30, 0, 30, 40));
    assert_eq!(rect_of(&after, &ids.b), rect(60, 0, 60, 40));
    assert!(
        after["splits"].as_array().is_some_and(|splits| splits
            .iter()
            .any(|split| split["rect"] == rect(0, 0, 60, 40))),
        "C and A must share one split inside the left half: {after}"
    );
    assert_eq!(after["focused_pane_id"], ids.c);
    assert_eq!(applied["target_layout"], after);
    assert_eq!(pane_moved_count(&events_since(&app, mark)), 1);
    app.state.assert_invariants_for_test();

    // 3. The tab's right edge makes C a full-height right third.
    let mark = event_mark(&app);
    let edge_params = json!({
        "pane_id": ids.c,
        "target": { "type": "tab", "tab_id": ids.tab1 },
        "side": "right",
    });
    let edge = place(&mut app, edge_params.clone());
    assert_eq!(edge["changed"], true);
    assert_eq!(edge["placed_rect"], rect(80, 0, 40, 40));
    let after = layout_of(&mut app, &ids.a);
    assert_eq!(rect_of(&after, &ids.a), rect(0, 0, 40, 40));
    assert_eq!(rect_of(&after, &ids.b), rect(40, 0, 40, 40));
    assert_eq!(rect_of(&after, &ids.c), rect(80, 0, 40, 40));
    assert_eq!(pane_moved_count(&events_since(&app, mark)), 1);
    app.state.assert_invariants_for_test();

    // 4. Repeating it changes nothing and emits nothing.
    let mark = event_mark(&app);
    let repeat = place(&mut app, edge_params);
    assert_eq!(repeat["changed"], false);
    assert_eq!(repeat["reason"], "no_change");
    assert_eq!(repeat["placed_rect"], rect(80, 0, 40, 40));
    assert_eq!(layout_of(&mut app, &ids.a), after);
    assert!(events_since(&app, mark).is_empty());
    app.state.assert_invariants_for_test();

    // 5. A pane cannot be placed beside itself.
    let mark = event_mark(&app);
    let same = place(
        &mut app,
        json!({
            "pane_id": ids.b,
            "target": { "type": "pane", "pane_id": ids.b },
            "side": "left",
        }),
    );
    assert_eq!(same["changed"], false);
    assert_eq!(same["reason"], "same_pane");
    assert_eq!(layout_of(&mut app, &ids.a), after);
    assert!(events_since(&app, mark).is_empty());
    app.state.assert_invariants_for_test();

    // 6. A zoomed tab refuses the move.
    let zoom = call(
        &mut app,
        "pane.zoom",
        json!({ "pane_id": ids.a, "mode": "on" })
    );
    assert_eq!(zoom["result"]["zoom"]["zoomed"], true, "zoom: {zoom}");
    let mark = event_mark(&app);
    let zoomed = place(
        &mut app,
        json!({
            "pane_id": ids.b,
            "target": { "type": "pane", "pane_id": ids.a },
            "side": "left",
        }),
    );
    assert_eq!(zoomed["changed"], false);
    assert_eq!(zoomed["reason"], "zoomed_tab");
    assert!(events_since(&app, mark).is_empty());
    let unzoom = call(
        &mut app,
        "pane.zoom",
        json!({ "pane_id": ids.a, "mode": "off" })
    );
    assert_eq!(
        unzoom["result"]["zoom"]["zoomed"], false,
        "unzoom: {unzoom}"
    );
    app.state.assert_invariants_for_test();

    // 7. Across tabs: B goes above X in tab 2 and tab 1 reflows to `A | C`.
    let mark = event_mark(&app);
    let cross = place(
        &mut app,
        json!({
            "pane_id": ids.b,
            "target": { "type": "pane", "pane_id": ids.x },
            "side": "up",
            "focus": true,
        }),
    );
    assert_eq!(cross["changed"], true);
    assert_eq!(cross["previous_pane_id"], ids.b);
    assert_eq!(
        cross["pane_id"], ids.b,
        "a move inside one workspace keeps the public pane id"
    );
    assert_eq!(cross["placed_rect"], rect(0, 0, 120, 20));
    assert_eq!(cross["target_layout"]["tab_id"], ids.tab2);
    assert_eq!(
        rect_of(&cross["target_layout"], &ids.b),
        rect(0, 0, 120, 20)
    );
    assert_eq!(
        rect_of(&cross["target_layout"], &ids.x),
        rect(0, 20, 120, 20)
    );
    assert_eq!(cross["source_layout"]["tab_id"], ids.tab1);
    assert_eq!(pane_count(&cross["source_layout"]), 2);
    assert_eq!(rect_of(&cross["source_layout"], &ids.a), rect(0, 0, 80, 40));
    assert_eq!(
        rect_of(&cross["source_layout"], &ids.c),
        rect(80, 0, 40, 40)
    );
    assert!(cross["closed_tab_id"].is_null());
    assert_eq!(pane_tab(&mut app, &ids.b), ids.tab2);
    assert_eq!(pane_moved_count(&events_since(&app, mark)), 1);
    app.state.assert_invariants_for_test();

    // 8. Moving the only pane out of tab 3 closes that tab.
    let tabs_before = app.state.workspaces[0].tabs.len();
    let mark = event_mark(&app);
    let lone = place(
        &mut app,
        json!({
            "pane_id": ids.y,
            "target": { "type": "pane", "pane_id": ids.a },
            "side": "right",
        }),
    );
    assert_eq!(lone["changed"], true);
    assert_eq!(lone["closed_tab_id"], ids.tab3);
    assert!(lone["closed_workspace_id"].is_null());
    assert_eq!(lone["placed_rect"], rect(40, 0, 40, 40));
    assert_eq!(app.state.workspaces[0].tabs.len(), tabs_before - 1);
    assert_eq!(pane_tab(&mut app, &ids.y), ids.tab1);
    let after = layout_of(&mut app, &ids.a);
    assert_eq!(rect_of(&after, &ids.a), rect(0, 0, 40, 40));
    assert_eq!(rect_of(&after, &ids.y), rect(40, 0, 40, 40));
    assert_eq!(rect_of(&after, &ids.c), rect(80, 0, 40, 40));
    assert_eq!(pane_moved_count(&events_since(&app, mark)), 1);
    app.state.assert_invariants_for_test();
}

#[test]
fn plain_workspace() {
    run_pane_place_scenario(app_with(Workspace::test_new("place")));
}

#[test]
fn adversarial_identity_workspace() {
    run_pane_place_scenario(app_with(Workspace::test_adversarial_identity_state()));
}

#[test]
fn unknown_pane_or_tab_is_an_error_and_changes_nothing() {
    let mut app = app_with(Workspace::test_new("place"));
    let ids = build_place_tabs(&mut app);
    let before = layout_of(&mut app, &ids.a);
    let mark = event_mark(&app);

    let missing_pane = place_error_code(
        &mut app,
        json!({
            "pane_id": "missing-pane",
            "target": { "type": "pane", "pane_id": ids.a },
            "side": "left",
        }),
    );
    assert_eq!(missing_pane, "pane_not_found");
    let missing_target = place_error_code(
        &mut app,
        json!({
            "pane_id": ids.c,
            "target": { "type": "pane", "pane_id": "missing-pane" },
            "side": "left",
        }),
    );
    assert_eq!(missing_target, "pane_not_found");
    let missing_tab = place_error_code(
        &mut app,
        json!({
            "pane_id": ids.c,
            "target": { "type": "tab", "tab_id": "missing-tab" },
            "side": "right",
        }),
    );
    assert_eq!(missing_tab, "tab_not_found");

    assert_eq!(layout_of(&mut app, &ids.a), before);
    assert!(events_since(&app, mark).is_empty());
    app.state.assert_invariants_for_test();
}
