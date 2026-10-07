//! Slice R2 scenario, client side (spec agents-hide-and-home-glyph-2026-10-07,
//! "Home glyph"): agent rows trade the space name for one home-location glyph
//! at the right end of the label column; plain pinned rows keep their space
//! name. Driven through the real snapshot and composed frame, plus the wire
//! contracts the Mac and Windows Shells decode.

use super::*;
use crate::api::schema::HomeLocation;
use crate::protocol::{CellData, ClientShellPinnedTab};

const CLOUD: &str = "\u{2601}";
const LOCAL: &str = "\u{2302}";
const UNSYNCED: &str = "\u{21E1}";
const GLYPHS: [&str; 3] = [CLOUD, LOCAL, UNSYNCED];

/// A pin as the endpoint snapshot carries it. Decoded from JSON so the
/// scenario names only `tab_id`, `workspace_id` and `role`.
fn pin(tab_id: &str, workspace_id: &str, agent: bool) -> ClientShellPinnedTab {
    let mut pin = serde_json::json!({ "tab_id": tab_id, "workspace_id": workspace_id });
    if agent {
        pin["role"] = "agent".into();
    }
    serde_json::from_value(pin).expect("pin")
}

/// `tree_snapshot` (spaces alpha and beta, chats one, two, three) plus chats
/// four (beta) and five (alpha). Agents one, two, four and five lead the pin
/// order; three is a plain pin. One is cloud, two local, four unsynced, five
/// has no card. Three holds a card's pane too, but plain rows never draw it.
fn home_snapshot() -> ClientShellSnapshot {
    let mut snapshot = super::tree::tree_snapshot();
    for (tab_id, workspace_id, number, label) in
        [("tab_4", "ws_2", 2, "four"), ("tab_5", "ws_1", 3, "five")]
    {
        let mut tab = snapshot.tabs[1].clone();
        tab.tab_id = tab_id.into();
        tab.workspace_id = workspace_id.into();
        tab.number = number;
        tab.label = label.into();
        tab.focused = false;
        snapshot.tabs.push(tab);
    }
    for (tab_id, home) in [
        ("tab_1", Some(HomeLocation::Cloud)),
        ("tab_2", Some(HomeLocation::Local)),
        ("tab_3", Some(HomeLocation::Local)),
        ("tab_4", Some(HomeLocation::Unsynced)),
        ("tab_5", None),
    ] {
        let tab = snapshot
            .tabs
            .iter_mut()
            .find(|tab| tab.tab_id == tab_id)
            .unwrap();
        tab.home_location = home;
    }
    snapshot.pinned_tabs = vec![
        pin("tab_1", "ws_1", true),
        pin("tab_2", "ws_1", true),
        pin("tab_4", "ws_2", true),
        pin("tab_5", "ws_1", true),
        pin("tab_3", "ws_2", false),
    ];
    snapshot
}

/// The composed cells of each drawn pinned row, by tab id.
fn pinned_row_cells(
    state: &ClientShellState,
    frame: &FrameData,
) -> Vec<(String, u16, Vec<CellData>)> {
    state
        .hits
        .pinned_rows
        .iter()
        .map(|hit| {
            let start = usize::from(hit.rect.y) * usize::from(frame.width);
            let cells = frame.cells
                [start + usize::from(hit.rect.x)..start + usize::from(hit.rect.right())]
                .to_vec();
            (hit.tab_id.clone(), hit.rect.x, cells)
        })
        .collect()
}

fn row_text(cells: &[CellData]) -> String {
    cells.iter().map(|cell| cell.symbol.as_str()).collect()
}

/// Indices of cells drawing a home glyph (bare, or with a text selector).
fn glyph_cells(cells: &[CellData]) -> Vec<usize> {
    cells
        .iter()
        .enumerate()
        .filter(|(_, cell)| GLYPHS.iter().any(|glyph| cell.symbol.starts_with(glyph)))
        .map(|(index, _)| index)
        .collect()
}

/// Protects the user-visible swap: an agent row shows where its memory lives
/// instead of its space, a plain pin keeps its space, an agent with no
/// location shows neither. Fails if agent rows keep the space name, if the
/// glyph lands anywhere but the column the space name ended in, if plain pins
/// lose their space or draw a glyph, if a glyph is double width, or if
/// unsynced is drawn in the same muted color as cloud and local.
#[test]
fn home_location_glyph_replaces_the_space_name_on_agent_rows_only() {
    for glyph in GLYPHS {
        assert_eq!(
            unicode_width::UnicodeWidthStr::width(glyph),
            1,
            "{glyph:?} must be one terminal cell"
        );
    }
    let mut config = ClientShellConfig::from_config(&Config::default());
    config.agent_panel_sort = crate::config::AgentPanelSortConfig::Tree;
    let mut state = ClientShellState::new(config);
    state.set_snapshot(Box::new(home_snapshot()));
    let frame = state.compose(100, 30).expect("composed frame");
    let rows = pinned_row_cells(&state, &frame);
    assert_eq!(
        rows.iter()
            .map(|(tab, _, _)| tab.as_str())
            .collect::<Vec<_>>(),
        ["tab_1", "tab_2", "tab_4", "tab_5", "tab_3"]
    );
    let row = |tab_id: &str| {
        rows.iter()
            .find(|(tab, _, _)| tab == tab_id)
            .map(|(_, x, cells)| (usize::from(*x), cells.as_slice()))
            .unwrap()
    };

    // The plain pin still names its space, muted, at the right of its label
    // column, and draws no glyph although its tab holds a card's pane.
    let (plain_x, plain) = row("tab_3");
    let text = row_text(plain);
    assert!(text.contains("three"), "{text:?}");
    let space_start = plain
        .windows(4)
        .position(|cells| row_text(cells) == "beta")
        .unwrap_or_else(|| panic!("plain pin keeps its space name: {text:?}"));
    let muted = plain[space_start + 3].fg;
    let space_end = plain_x + space_start + 3;
    assert!(glyph_cells(plain).is_empty(), "{text:?}");

    for (tab_id, label, space, glyph) in [
        ("tab_1", "one", "alpha", CLOUD),
        ("tab_2", "two", "alpha", LOCAL),
        ("tab_4", "four", "beta", UNSYNCED),
    ] {
        let (x, cells) = row(tab_id);
        let text = row_text(cells);
        assert!(text.contains(label), "{tab_id}: {text:?}");
        assert!(!text.contains(space), "{tab_id} drops its space: {text:?}");
        let found = glyph_cells(cells);
        assert_eq!(found.len(), 1, "{tab_id} draws one glyph: {text:?}");
        let at = found[0];
        assert!(cells[at].symbol.starts_with(glyph), "{tab_id}: {text:?}");
        assert_eq!(
            x + at,
            space_end,
            "{tab_id}: the glyph sits where the space name ended: {text:?}"
        );
        if glyph == UNSYNCED {
            assert_ne!(cells[at].fg, muted, "unsynced reads as a warning");
        } else {
            assert_eq!(cells[at].fg, muted, "{tab_id}: muted like the space name");
        }
    }

    // An agent with no card draws no glyph and no space name.
    let (_, bare) = row("tab_5");
    let text = row_text(bare);
    assert!(text.contains("five"), "{text:?}");
    assert!(!text.contains("alpha"), "{text:?}");
    assert!(glyph_cells(bare).is_empty(), "{text:?}");
}

/// Wire compatibility: a tab from a server that predates home location (this
/// is the ClientShellTab JSON on origin/main b1736d29) decodes with no
/// location, and a location survives the snapshot round trip under the key
/// and value the Mac and Windows Shells read.
#[test]
fn home_location_absent_from_an_older_client_shell_tab_decodes_as_none() {
    let old = r#"{"desk_count":0,"sort_rank":0,"tab_id":"w1:t1","workspace_id":"w1","number":1,"label":"frank","custom_label":true,"zoomed":false,"focused":false,"agent_status":"working","work_status":"working"}"#;
    let mut tab: crate::protocol::ClientShellTab =
        serde_json::from_str(old).expect("older client shell tab");
    assert_eq!(tab.home_location, None);

    tab.home_location = Some(HomeLocation::Unsynced);
    let encoded = serde_json::to_value(&tab).unwrap();
    assert_eq!(encoded["home_location"], "unsynced");
    let decoded: crate::protocol::ClientShellTab = serde_json::from_value(encoded).unwrap();
    assert_eq!(decoded, tab);
}

/// Snapshot decoding tolerates a location a newer server invents, as pin
/// roles do, so one new value never drops a client's whole snapshot. Fails if
/// an unknown value is an error or if a known value decodes wrong.
#[test]
fn home_location_snapshot_decoding_tolerates_unknown_values() {
    let decode = |value: serde_json::Value| {
        let mut tab = serde_json::json!({
            "tab_id": "w1:t1", "workspace_id": "w1", "number": 1, "label": "frank",
            "custom_label": true, "zoomed": false, "focused": false, "agent_status": "idle",
        });
        tab["home_location"] = value;
        serde_json::from_value::<crate::protocol::ClientShellTab>(tab)
            .expect("snapshot tab decodes")
            .home_location
    };
    assert_eq!(decode("cloud".into()), Some(HomeLocation::Cloud));
    assert_eq!(decode("local".into()), Some(HomeLocation::Local));
    assert_eq!(decode("unsynced".into()), Some(HomeLocation::Unsynced));
    assert_eq!(decode("orbit".into()), None);
    assert_eq!(decode(serde_json::Value::Null), None);
}

/// API output (`tab.list`, `tab.get`, `agents.list` consumers on every
/// platform): snake_case values, and no key at all when the tab holds no
/// card's pane, so older readers see the payload they always saw.
#[test]
fn home_location_api_output_is_snake_case_and_omitted_when_absent() {
    let mut tab: crate::api::schema::TabInfo = serde_json::from_value(serde_json::json!({
        "tab_id": "w1:t1", "workspace_id": "w1", "number": 1, "label": "frank",
        "focused": false, "pane_count": 1, "agent_status": "idle",
    }))
    .expect("tab info");
    assert_eq!(tab.home_location, None);
    let absent = serde_json::to_value(&tab).unwrap();
    assert!(absent.get("home_location").is_none(), "{absent}");

    for (location, wire) in [
        (HomeLocation::Cloud, "cloud"),
        (HomeLocation::Local, "local"),
        (HomeLocation::Unsynced, "unsynced"),
    ] {
        tab.home_location = Some(location);
        let encoded = serde_json::to_value(&tab).unwrap();
        assert_eq!(encoded["home_location"], wire);
        let decoded: crate::api::schema::TabInfo = serde_json::from_value(encoded).unwrap();
        assert_eq!(decoded.home_location, Some(location));
    }
}
