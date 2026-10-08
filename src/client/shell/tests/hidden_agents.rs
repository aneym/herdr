//! Hiding an agent pin (spec agents-hide-and-home-glyph-2026-10-07, slice R1).
//!
//! One story across the real boundaries, with no PTYs and no mocks of our own
//! code: the TUI row menu emits `tab.set_hidden`, the server's client-method
//! gate admits it, `App::handle_api_request` applies it, the server builds the
//! client snapshot, the snapshot crosses the wire as JSON, and the client
//! draws, numbers, folds and clicks the result. Session and preference
//! persistence go through their real files.

use super::*;
use crate::api::schema::{EventKind, Method};
use crate::client::shell::tree::{tree_list_entries, AgentPanelListEntry};
use crossterm::event::{MouseButton, MouseEventKind};
use serde_json::{json, Value};

const LABELS: [&str; 4] = ["agent-a", "agent-b", "agent-c", "plain-p"];

/// One space `w1`: an unpinned shell tab, then agent pins A, B, C and the
/// plain pin P, in that pin order. A, B and C run an idle detected agent.
fn hidden_agent_app() -> (crate::app::App, crate::api::EventHub, [String; 4]) {
    let events = crate::api::EventHub::default();
    let (_api_tx, api_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut app = crate::app::App::new(
        &Config::default(),
        crate::app::AppPolicy::TEST,
        None,
        api_rx,
        events.clone(),
    );
    let mut space = crate::workspace::Workspace::test_new("home");
    space.id = "w1".into();
    for label in LABELS {
        space.test_add_tab(Some(label));
    }
    app.state.workspaces = vec![space];
    app.state.active = Some(0);
    app.state.selected = 0;
    app.state.ensure_test_terminals();
    let tabs = [1, 2, 3, 4].map(|index| app.public_tab_id(0, index).expect("tab id"));
    for (index, tab) in tabs.iter().take(3).enumerate() {
        set_agent_state(&mut app, index + 1, crate::detect::AgentState::Idle);
        let response = call(
            &mut app,
            "tab.set_role",
            json!({"tab_id": tab, "role": "agent"}),
        );
        assert_eq!(response["result"]["tab"]["role"], "agent", "{response}");
    }
    let response = call(
        &mut app,
        "tab.set_pinned",
        json!({"tab_id": tabs[3], "pinned": true}),
    );
    assert!(response.get("error").is_none(), "{response}");
    assert_eq!(pin_order(&app), tabs);
    check_state(&app);
    (app, events, tabs)
}

fn set_agent_state(app: &mut crate::app::App, tab_index: usize, state: crate::detect::AgentState) {
    let tab = &app.state.workspaces[0].tabs[tab_index];
    let terminal_id = tab.panes[&tab.root_pane].attached_terminal_id.clone();
    let terminal = app
        .state
        .terminals
        .get_mut(&terminal_id)
        .expect("test terminal");
    terminal.set_agent_name("worker".into());
    terminal.set_detected_state(Some(crate::detect::Agent::Pi), state);
}

/// A JSON request through the public API dispatch, as a socket client sends it.
fn call(app: &mut crate::app::App, method: &str, params: Value) -> Value {
    let request: crate::api::schema::Request =
        serde_json::from_value(json!({"id": method, "method": method, "params": params}))
            .unwrap_or_else(|error| panic!("{method} request decodes: {error}"));
    serde_json::from_str(&app.handle_api_request(request)).expect("response is json")
}

fn pin_order(app: &crate::app::App) -> Vec<String> {
    app.state
        .pinned_tabs
        .iter()
        .map(|pin| pin.tab_id.clone())
        .collect()
}

fn hidden_flags(app: &crate::app::App) -> Vec<bool> {
    app.state.pinned_tabs.iter().map(|pin| pin.hidden).collect()
}

fn check_state(app: &crate::app::App) {
    app.state.assert_invariants_for_test();
    assert!(
        app.state
            .pinned_tabs
            .iter()
            .all(|pin| !pin.hidden || pin.role.is_some()),
        "only agent pins may be hidden: {:?}",
        app.state.pinned_tabs
    );
}

/// Every method the server admits from a client shell, as it advertises them.
fn advertised_methods() -> Vec<String> {
    crate::server::client_commands::supported_client_shell_method_names()
        .iter()
        .map(|name| (*name).to_owned())
        .collect()
}

fn hidden_agent_client(preferences: Option<&std::path::Path>) -> ClientShellState {
    let mut config = ClientShellConfig::from_config(&Config::default());
    if let Some(path) = preferences {
        config = config.with_preferences_path(path.to_path_buf());
    }
    config.agent_panel_sort = crate::config::AgentPanelSortConfig::Tree;
    ClientShellState::new(config)
}

/// Build the server's client snapshot, send it over the JSON wire, and hand
/// the decoded copy to the client. Returns the wire JSON.
fn refresh(client: &mut ClientShellState, app: &crate::app::App, revision: &mut u64) -> Value {
    *revision += 1;
    let snapshot = crate::server::client_shell::snapshot(app, "boot", *revision, None, None);
    let wire = serde_json::to_value(&snapshot).expect("snapshot encodes");
    let decoded: ClientShellSnapshot =
        serde_json::from_value(wire.clone()).expect("snapshot decodes");
    client.set_snapshot(Box::new(decoded));
    wire
}

fn sidebar_entries(client: &mut ClientShellState) -> Vec<AgentPanelListEntry> {
    let tree = client.tree_chrome_mut().clone();
    let snapshot = client.snapshot.as_deref().expect("snapshot");
    let rows = crate::client::shell::agent_sidebar::agent_rows(snapshot, &client.config, None);
    let (rows, _) =
        crate::client::shell::tree::partition_automations(snapshot, &client.config, rows);
    let rows = crate::client::shell::tree::arrange_agent_hierarchy_with(
        snapshot,
        &tree,
        rows,
        !tree.show_tabs,
    );
    tree_list_entries(snapshot, &tree, rows)
}

/// The pin sections at the top of the sidebar, up to the first space row.
/// Visible pins read `pin:<label>:<digit>`, hidden ones `hidden:<label>:<digit>`.
fn pin_shape(client: &mut ClientShellState) -> Vec<String> {
    sidebar_entries(client)
        .iter()
        .map_while(|entry| match entry {
            AgentPanelListEntry::AgentChatsHeader => Some("agents".to_owned()),
            AgentPanelListEntry::PinnedChatsHeader => Some("pinned".to_owned()),
            AgentPanelListEntry::HiddenAgentsHeader {
                count, collapsed, ..
            } => Some(format!(
                "hidden-agents:{count}:{}",
                if *collapsed { "shut" } else { "open" }
            )),
            AgentPanelListEntry::PinnedTab(row) => Some(format!(
                "{}:{}:{}",
                if row.hidden { "hidden" } else { "pin" },
                row.label,
                row.shortcut
            )),
            _ => None,
        })
        .collect()
}

/// `(count, collapsed, alert)` of the hidden-agents header, if drawn.
fn hidden_header(client: &mut ClientShellState) -> Option<(usize, bool, bool)> {
    sidebar_entries(client)
        .iter()
        .find_map(|entry| match entry {
            AgentPanelListEntry::HiddenAgentsHeader {
                count,
                collapsed,
                alert,
            } => Some((*count, *collapsed, *alert)),
            _ => None,
        })
}

/// Compose a frame and find `needle` (case-insensitive) inside the agent
/// panel body. Returns its cell and the panel's text on that row.
fn sidebar_text(client: &mut ClientShellState, needle: &str) -> Option<((u16, u16), String)> {
    let frame = client.compose(106, 40).expect("frame");
    let body = client.hits.agent_body;
    let rows = frame_rows(&frame);
    (body.y..body.bottom().min(frame.height)).find_map(|y| {
        let text = rows[y as usize]
            .chars()
            .skip(body.x as usize)
            .take(body.width as usize)
            .collect::<String>();
        let lower = text.to_lowercase();
        let byte = lower.find(needle)?;
        Some(((body.x + lower[..byte].chars().count() as u16, y), text))
    })
}

fn sidebar_cell(client: &mut ClientShellState, needle: &str) -> (u16, u16) {
    sidebar_text(client, needle)
        .unwrap_or_else(|| panic!("{needle:?} drawn in the agent panel"))
        .0
}

fn mouse(kind: MouseEventKind, (column, row): (u16, u16)) -> RawInputEvent {
    RawInputEvent::Mouse(MouseEvent {
        kind,
        column,
        row,
        modifiers: crossterm::event::KeyModifiers::empty(),
    })
}

fn click(client: &mut ClientShellState, at: (u16, u16)) -> ClientShellInput {
    client.handle_raw_events(vec![
        mouse(MouseEventKind::Down(MouseButton::Left), at),
        mouse(MouseEventKind::Up(MouseButton::Left), at),
    ])
}

fn sent(outcome: &ClientShellInput) -> Vec<Method> {
    outcome
        .actions
        .iter()
        .filter_map(|action| match action {
            ClientShellAction::Endpoint { request, .. } => Some(request.method.clone()),
            _ => None,
        })
        .collect()
}

/// Right-click the row drawn with `label` and read its menu, then close it.
fn row_menu(client: &mut ClientShellState, label: &str) -> Vec<String> {
    let at = sidebar_cell(client, label);
    client.handle_raw_events(vec![mouse(MouseEventKind::Down(MouseButton::Right), at)]);
    let Some(ClientShellOverlay::ContextMenu(menu)) = client.overlay.as_ref() else {
        panic!("right-click on {label} opens its menu");
    };
    let labels = menu.items().into_iter().map(|item| item.label).collect();
    client.overlay = None;
    labels
}

/// Right-click the row drawn with `label` and choose `item` from its menu.
fn choose(client: &mut ClientShellState, label: &str, item: &str) -> ClientShellInput {
    let at = sidebar_cell(client, label);
    client.handle_raw_events(vec![mouse(MouseEventKind::Down(MouseButton::Right), at)]);
    let Some(ClientShellOverlay::ContextMenu(menu)) = client.overlay.as_ref() else {
        panic!("right-click on {label} opens its menu");
    };
    let labels = menu
        .items()
        .into_iter()
        .map(|item| item.label)
        .collect::<Vec<_>>();
    let index = labels
        .iter()
        .position(|candidate| candidate == item)
        .unwrap_or_else(|| panic!("{item:?} in the {label} menu: {labels:?}"));
    let mut outcome = ClientShellInput::default();
    client.activate_context_menu_item(index, &mut outcome);
    outcome
}

/// Deliver the client's endpoint requests to the server the way the client
/// socket does: through the server's client-method gate, then the API.
fn forward(app: &mut crate::app::App, outcome: &ClientShellInput) -> Vec<Value> {
    outcome
        .actions
        .iter()
        .filter_map(|action| match action {
            ClientShellAction::Endpoint { request, .. } => Some(request),
            _ => None,
        })
        .map(|request| {
            assert!(
                crate::server::client_commands::supports_client_shell_method(&request.method),
                "the server admits {:?} from a client shell",
                request.method
            );
            serde_json::from_str::<Value>(&app.handle_api_request((**request).clone()))
                .expect("response is json")
        })
        .collect()
}

fn wire_pin<'a>(wire: &'a Value, tab_id: &str) -> &'a Value {
    wire["pinned_tabs"]
        .as_array()
        .expect("pinned_tabs")
        .iter()
        .find(|pin| pin["tab_id"] == tab_id)
        .unwrap_or_else(|| panic!("{tab_id} in the wire snapshot"))
}

fn preferences_path(name: &str) -> std::path::PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "herdr-hidden-agent-{name}-{}-{stamp}.json",
        std::process::id()
    ))
}

/// The whole user story: hide B from its row menu, see it leave the agents
/// and the digits while keeping its pin slot, open and shut the Hidden fold
/// (which persists per client), get a quiet alert while shut, and bring B
/// back to slot 2 with Show in Agents.
#[test]
fn hidden_agent_hides_renumbers_folds_alerts_and_returns_to_its_slot() {
    let (mut app, events, tabs) = hidden_agent_app();
    let [a, b, c, p] = tabs.clone();
    let preferences = preferences_path("story");
    let _ = std::fs::remove_file(&preferences);
    let mut client = hidden_agent_client(Some(&preferences));
    let mut revision = 0;
    refresh(&mut client, &app, &mut revision);
    assert_eq!(
        pin_shape(&mut client),
        [
            "agents",
            "pin:agent-a:1",
            "pin:agent-b:2",
            "pin:agent-c:3",
            "pinned",
            "pin:plain-p:4"
        ]
    );
    assert_eq!(
        hidden_header(&mut client),
        None,
        "no header with nothing hidden"
    );

    // A server that does not advertise tab.set_hidden gets no Hide item.
    client.set_endpoint_methods(Some(
        advertised_methods()
            .into_iter()
            .filter(|method| method != "tab.set_hidden")
            .collect(),
    ));
    assert!(!row_menu(&mut client, "agent-b").contains(&"Hide".to_owned()));
    client.set_endpoint_methods(Some(advertised_methods()));
    let plain_menu = row_menu(&mut client, "plain-p");
    assert!(
        !plain_menu.contains(&"Hide".to_owned()),
        "a plain pin offers no Hide: {plain_menu:?}"
    );

    // Hide B from its row menu; the server applies it in place.
    let outcome = choose(&mut client, "agent-b", "Hide");
    assert!(
        matches!(sent(&outcome).as_slice(), [Method::TabSetHidden(params)]
            if params.tab_id == b && params.hidden),
        "{:?}",
        sent(&outcome)
    );
    let _ = app.drain_all_internal_events();
    let seen = events.current_sequence();
    let responses = forward(&mut app, &outcome);
    assert_eq!(
        responses[0]["result"]["tab"]["hidden"], true,
        "{responses:?}"
    );
    assert_eq!(responses[0]["result"]["tab"]["pin_index"], 1);
    let emitted = events.events_after(seen);
    let kinds = emitted
        .iter()
        .map(|(_, envelope)| envelope.event)
        .collect::<Vec<_>>();
    assert!(
        kinds.contains(&EventKind::WorkspaceUpdated) && kinds.contains(&EventKind::TabPinMoved),
        "tab.set_hidden emits what tab.set_role emits: {kinds:?}"
    );
    assert_eq!(pin_order(&app), tabs, "hiding never moves the pin");
    assert_eq!(hidden_flags(&app), [false, true, false, false]);
    check_state(&app);
    let listed = call(&mut app, "agents.list", json!({}));
    let listed = listed["result"]["agents"].as_array().expect("agents");
    let listed_hidden = |tab: &str| {
        listed
            .iter()
            .find(|agent| agent["tab_id"] == tab)
            .map(|agent| agent.get("hidden").cloned())
    };
    assert_eq!(listed_hidden(b.as_str()), Some(Some(json!(true))));
    assert_eq!(
        listed_hidden(a.as_str()),
        Some(None),
        "visible agents omit the flag"
    );

    // The snapshot pin carries the flag over the wire.
    let wire = refresh(&mut client, &app, &mut revision);
    assert_eq!(wire_pin(&wire, &b)["hidden"], true);
    assert_ne!(wire_pin(&wire, &a)["hidden"], true);
    assert!(client.snapshot.as_deref().unwrap().pinned_tabs[1].hidden);

    // Shut: B leaves the agents and the sidebar; its slot's digit goes to C.
    assert_eq!(
        pin_shape(&mut client),
        [
            "agents",
            "pin:agent-a:1",
            "pin:agent-c:2",
            "hidden-agents:1:shut",
            "pinned",
            "pin:plain-p:3"
        ]
    );
    assert!(
        sidebar_text(&mut client, "agent-b").is_none(),
        "a shut hidden agent is drawn nowhere in the panel, not even in its space"
    );
    let (_, header_text) = sidebar_text(&mut client, "hidden").expect("header drawn");
    assert!(
        header_text.contains('1') && header_text.contains('\u{25b8}'),
        "shut header shows its count and a closed chevron: {header_text:?}"
    );
    let snapshot = client.snapshot.as_deref().unwrap();
    let numbered = client.numbered_tab_ids(snapshot);
    assert_eq!(numbered[..3], [a.clone(), c.clone(), p.clone()]);
    assert!(!numbered.contains(&b), "a hidden agent owns no digit");
    let mut outcome = ClientShellInput::default();
    client.record_binding(
        crate::input::KeybindMatch::Action(crate::input::KeybindAction::SwitchTab(1)),
        &mut outcome,
    );
    let focused = sent(&outcome)
        .into_iter()
        .filter_map(|method| match method {
            Method::TabFocus(target) => Some(target.tab_id),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(focused, std::slice::from_ref(&c), "Cmd+2 opens C");

    // Open the fold: B shows under the header with no digit.
    let header = sidebar_cell(&mut client, "hidden");
    click(&mut client, header);
    assert!(client.tree_chrome_mut().hidden_agents_expanded);
    assert_eq!(
        pin_shape(&mut client),
        [
            "agents",
            "pin:agent-a:1",
            "pin:agent-c:2",
            "hidden-agents:1:open",
            "hidden:agent-b:0",
            "pinned",
            "pin:plain-p:3"
        ]
    );
    let (_, header_text) = sidebar_text(&mut client, "hidden").expect("header drawn");
    assert!(header_text.contains('\u{25be}'), "{header_text:?}");
    assert_eq!(
        client.numbered_tab_ids(client.snapshot.as_deref().unwrap())[..3],
        [a.clone(), c.clone(), p.clone()],
        "opening the fold does not give B a digit"
    );

    // The fold is this client's preference and survives a client restart.
    let mut reopened = hidden_agent_client(Some(&preferences));
    refresh(&mut reopened, &app, &mut revision);
    assert_eq!(hidden_header(&mut reopened), Some((1, false, false)));
    let mut fresh = hidden_agent_client(None);
    refresh(&mut fresh, &app, &mut revision);
    assert_eq!(
        hidden_header(&mut fresh),
        Some((1, true, false)),
        "another client starts shut"
    );

    // A hidden row still opens its chat on click, but never drags.
    let row = sidebar_cell(&mut client, "agent-b");
    let outcome = click(&mut client, row);
    assert!(
        sent(&outcome)
            .iter()
            .any(|method| matches!(method, Method::TabFocus(target) if target.tab_id == b)),
        "{:?}",
        sent(&outcome)
    );
    let from = sidebar_cell(&mut client, "agent-b");
    let to = sidebar_cell(&mut client, "agent-a");
    let mut gesture = Vec::new();
    for event in [
        mouse(MouseEventKind::Down(MouseButton::Left), from),
        mouse(MouseEventKind::Drag(MouseButton::Left), to),
        mouse(MouseEventKind::Up(MouseButton::Left), to),
    ] {
        gesture.extend(sent(&client.handle_raw_events(vec![event])));
    }
    assert!(
        !gesture
            .iter()
            .any(|method| matches!(method, Method::TabPinMove(_))),
        "{gesture:?}"
    );
    let hidden_menu = row_menu(&mut client, "agent-b");
    assert!(
        hidden_menu.contains(&"Show in Agents".to_owned())
            && !hidden_menu.contains(&"Hide".to_owned()),
        "{hidden_menu:?}"
    );

    // Shut it again; B turning blocked lights the header's quiet alert.
    let header = sidebar_cell(&mut client, "hidden");
    click(&mut client, header);
    assert_eq!(hidden_header(&mut client), Some((1, true, false)));
    set_agent_state(&mut app, 2, crate::detect::AgentState::Blocked);
    refresh(&mut client, &app, &mut revision);
    assert_eq!(hidden_header(&mut client), Some((1, true, true)));
    // Cmd+E skips the hidden agent even though it is the one blocked.
    let b_pane = client
        .snapshot
        .as_deref()
        .unwrap()
        .panes
        .iter()
        .find(|pane| pane.tab_id == b)
        .expect("B's pane")
        .pane_id
        .clone();
    let mut outcome = ClientShellInput::default();
    client.record_binding(
        crate::input::KeybindMatch::Action(crate::input::KeybindAction::NextAttention),
        &mut outcome,
    );
    assert!(
        !sent(&outcome).iter().any(|method| matches!(method,
            Method::PaneFocus(target) if target.pane_id == b_pane)
            || matches!(method, Method::TabFocus(target) if target.tab_id == b)),
        "{:?}",
        sent(&outcome)
    );

    // Show in Agents puts B back in slot 2 with its digit.
    let header = sidebar_cell(&mut client, "hidden");
    click(&mut client, header);
    let outcome = choose(&mut client, "agent-b", "Show in Agents");
    assert!(
        matches!(sent(&outcome).as_slice(), [Method::TabSetHidden(params)]
            if params.tab_id == b && !params.hidden),
        "{:?}",
        sent(&outcome)
    );
    let responses = forward(&mut app, &outcome);
    assert!(
        responses[0]["result"]["tab"].get("hidden").is_none(),
        "a visible tab omits the flag: {responses:?}"
    );
    assert_eq!(responses[0]["result"]["tab"]["pin_index"], 1);
    assert_eq!(hidden_flags(&app), [false; 4]);
    check_state(&app);
    refresh(&mut client, &app, &mut revision);
    assert_eq!(
        pin_shape(&mut client),
        [
            "agents",
            "pin:agent-a:1",
            "pin:agent-b:2",
            "pin:agent-c:3",
            "pinned",
            "pin:plain-p:4"
        ]
    );
    assert_eq!(hidden_header(&mut client), None);
    assert_eq!(
        client.numbered_tab_ids(client.snapshot.as_deref().unwrap())[..4],
        [a, b, c, p]
    );
    let _ = std::fs::remove_file(&preferences);
}

/// Only agent pins hide, and the flag never outlives the agent role or the pin.
#[test]
fn hidden_agent_flag_refuses_plain_tabs_and_drops_with_role_or_pin() {
    let (mut app, _events, tabs) = hidden_agent_app();
    let [a, b, c, p] = tabs.clone();
    let shell = app.public_tab_id(0, 0).expect("shell tab");

    for tab in [&p, &shell] {
        let response = call(
            &mut app,
            "tab.set_hidden",
            json!({"tab_id": tab, "hidden": true}),
        );
        assert_eq!(response["error"]["code"], "tab_not_agent", "{response}");
    }
    let response = call(
        &mut app,
        "tab.set_hidden",
        json!({"tab_id": "w1:t99", "hidden": true}),
    );
    assert_eq!(response["error"]["code"], "tab_not_found", "{response}");
    assert_eq!(pin_order(&app), tabs);
    assert_eq!(hidden_flags(&app), [false; 4]);
    check_state(&app);

    // Clearing the role drops hidden; making it an agent again shows it.
    for tab in [&b, &c] {
        let response = call(
            &mut app,
            "tab.set_hidden",
            json!({"tab_id": tab, "hidden": true}),
        );
        assert_eq!(response["result"]["tab"]["hidden"], true, "{response}");
    }
    let response = call(&mut app, "tab.set_role", json!({"tab_id": b}));
    assert!(
        response["result"]["tab"].get("role").is_none(),
        "{response}"
    );
    assert!(
        response["result"]["tab"].get("hidden").is_none(),
        "{response}"
    );
    check_state(&app);
    call(
        &mut app,
        "tab.set_role",
        json!({"tab_id": b, "role": "agent"}),
    );
    check_state(&app);

    // Unpinning drops hidden too.
    let response = call(
        &mut app,
        "tab.set_pinned",
        json!({"tab_id": c, "pinned": false}),
    );
    assert!(response.get("error").is_none(), "{response}");
    check_state(&app);
    call(
        &mut app,
        "tab.set_role",
        json!({"tab_id": c, "role": "agent"}),
    );
    check_state(&app);
    assert_eq!(pin_order(&app), [a, b, c, p]);
    assert_eq!(hidden_flags(&app), [false; 4]);

    let mut client = hidden_agent_client(None);
    refresh(&mut client, &app, &mut 0);
    assert_eq!(
        pin_shape(&mut client),
        [
            "agents",
            "pin:agent-a:1",
            "pin:agent-b:2",
            "pin:agent-c:3",
            "pinned",
            "pin:plain-p:4"
        ]
    );
}

/// A saved session restores B hidden in its slot and the server's copy of the
/// fold, through the real session.json and a cold `App::new`.
#[cfg(unix)]
#[tokio::test]
async fn hidden_agent_and_server_fold_survive_session_restore() {
    let _guard = crate::config::test_config_env_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let config_home = preferences_path("config-home").with_extension("d");
    let original_config_home = std::env::var_os("XDG_CONFIG_HOME");
    std::env::set_var("XDG_CONFIG_HOME", &config_home);

    let (mut app, _events, tabs) = hidden_agent_app();
    let response = call(
        &mut app,
        "tab.set_hidden",
        json!({"tab_id": tabs[1], "hidden": true}),
    );
    assert_eq!(response["result"]["tab"]["hidden"], true, "{response}");
    app.state.hidden_agents_expanded = true;
    let snapshot = crate::persist::capture(
        &app.state.workspaces,
        &app.state.terminals,
        &app.terminal_runtimes,
        app.state.active,
        app.state.active_profile.clone(),
        app.state.selected,
        app.state.snapshot_ui_prefs(),
    );
    let session = crate::session::data_dir().join("session.json");
    std::fs::create_dir_all(session.parent().expect("session dir")).expect("session dir");
    std::fs::write(
        &session,
        serde_json::to_string(&snapshot).expect("session encodes"),
    )
    .expect("session written");

    let mut config = Config::default();
    config.terminal.default_shell = "/usr/bin/true".into();
    config.session.resume_agents_on_restore = false;
    let (_api_tx, api_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut restored = crate::app::App::new(
        &config,
        crate::app::AppPolicy {
            restore_session: true,
            ..crate::app::AppPolicy::TEST
        },
        None,
        api_rx,
        crate::api::EventHub::default(),
    );
    assert_eq!(pin_order(&restored), tabs);
    assert_eq!(hidden_flags(&restored), [false, true, false, false]);
    assert!(restored.state.hidden_agents_expanded);
    check_state(&restored);
    let mut client = hidden_agent_client(None);
    refresh(&mut client, &restored, &mut 0);
    assert_eq!(hidden_header(&mut client).map(|(count, ..)| count), Some(1));
    assert_eq!(
        client.numbered_tab_ids(client.snapshot.as_deref().unwrap())[..3],
        [tabs[0].clone(), tabs[2].clone(), tabs[3].clone()]
    );

    for (_, runtime) in restored.terminal_runtimes.drain().collect::<Vec<_>>() {
        runtime.shutdown();
    }
    match original_config_home {
        Some(value) => std::env::set_var("XDG_CONFIG_HOME", value),
        None => std::env::remove_var("XDG_CONFIG_HOME"),
    }
    let _ = std::fs::remove_dir_all(&config_home);
}

/// Pins written before the flag existed, in session.json and on the client
/// wire, decode as visible; a visible pin writes no key, so older readers see
/// the JSON they always did.
#[test]
fn hidden_agent_flag_absent_in_older_pin_json_decodes_visible() {
    let saved: crate::app::state::PinnedTab =
        serde_json::from_str(r#"{"tab_id":"w1:t2","priority":0,"role":"agent"}"#)
            .expect("older session pin decodes");
    assert!(!saved.hidden);
    assert!(serde_json::to_value(&saved)
        .expect("pin encodes")
        .get("hidden")
        .is_none());
    let hidden = crate::app::state::PinnedTab {
        hidden: true,
        ..saved
    };
    let encoded = serde_json::to_value(&hidden).expect("pin encodes");
    assert_eq!(encoded["hidden"], true);
    let decoded: crate::app::state::PinnedTab =
        serde_json::from_value(encoded).expect("hidden pin decodes");
    assert!(decoded.hidden);

    let wire: crate::protocol::ClientShellPinnedTab =
        serde_json::from_str(r#"{"tab_id":"w1:t2","workspace_id":"w1","role":"agent"}"#)
            .expect("older wire pin decodes");
    assert!(!wire.hidden);
}
