//! Slice R2 scenario, server path: a card dir on disk reaches an attached
//! client's snapshot and the JSON API through the headless loop's scheduled
//! poll (spec agents-hide-and-home-glyph-2026-10-07, "Server resolution of
//! home location"). The resolver's own rules live in `agent_home/tests.rs`;
//! this guards the glue a resolver test cannot see: the poll runs on the
//! scheduler, the map lands in `AppState.agent_homes`, a tab takes the first
//! pane in layout order that has a location, and both `TabInfo` and
//! `ClientShellTab` carry it.

use super::*;

/// Runs one scheduled tick with the agent-home poll due, as the headless loop
/// does every 15 s. A tick that changed the map must ask the loop to render.
fn home_tick(server: &mut HeadlessServer) {
    server.next_agent_home_poll = None;
    assert!(
        server.handle_scheduled_tasks_headless(Instant::now(), false),
        "a changed home map asks the loop to render"
    );
    server.render_and_stream();
}

/// The next client shell snapshot on the control lane, skipping the other
/// endpoint messages that precede it.
fn next_snapshot(rx: &std::sync::mpsc::Receiver<Vec<u8>>) -> Box<protocol::ClientShellSnapshot> {
    loop {
        let bytes = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("a client shell snapshot after the home change");
        if let ServerMessage::EndpointControl { kind, data } = read_server_message(bytes) {
            if kind == protocol::endpoint::ENDPOINT_SNAPSHOT_KIND {
                return serde_json::from_str(&data).expect("decode snapshot");
            }
        }
    }
}

fn listed_home(server: &mut HeadlessServer, tab_id: &str) -> Option<api::schema::HomeLocation> {
    let (respond_to, response_rx) = std::sync::mpsc::channel();
    server.handle_api_request_with_shutdown_check(api::ApiRequestMessage {
        request: api::schema::Request {
            id: "list-tabs".into(),
            method: api::schema::Method::TabList(api::schema::TabListParams::default()),
        },
        respond_to,
        response_write_complete: None,
    });
    let response: api::schema::SuccessResponse =
        serde_json::from_str(&response_rx.recv().unwrap()).unwrap();
    let api::schema::ResponseResult::TabList { tabs } = response.result else {
        panic!("expected tab list");
    };
    tabs.into_iter()
        .find(|tab| tab.tab_id == tab_id)
        .expect("listed tab")
        .home_location
}

fn write(path: &std::path::Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

/// Fails if the poll never runs on the scheduler, if the map never reaches the
/// tab, if the snapshot or the API drops the field, or if a later pane in the
/// layout outranks the first one.
#[tokio::test]
async fn headless_poll_carries_home_location_to_the_client_snapshot_and_api() {
    let cards = std::env::temp_dir().join(format!(
        "herdr-home-location-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&cards).unwrap();
    // nextest runs each test in its own process; only the agent-home poll
    // reads this variable.
    std::env::set_var("HERDR_AGENTS_DIR", &cards);

    let mut server = test_headless_server();
    let mut workspace = crate::workspace::Workspace::test_new("homes");
    workspace.test_split(ratatui::layout::Direction::Horizontal);
    server.app.state.workspaces = vec![workspace];
    server.app.state.ensure_test_terminals();
    server.app.state.active = Some(0);
    server.app.state.selected = 0;
    let order = server.app.state.workspaces[0].tabs[0].layout.pane_ids();
    let first = server.app.public_pane_id(0, order[0]).unwrap();
    let second = server.app.public_pane_id(0, order[1]).unwrap();
    let tab_id = server.app.public_tab_id(0, 0).unwrap();

    let (writer, control_rx, _render_rx) = test_client_writer();
    assert!(
        server.handle_server_event(ServerEvent::ClientShellConnected {
            surface_reuse: false,
            surface_delta: false,
            client_id: 91,
            surface_cols: 80,
            surface_rows: 23,
            cell_width_px: 0,
            cell_height_px: 0,
            pixel_mouse: false,
            direct_graphics: false,
            endpoint_keybindings: false,
            mouse_capture: false,
            surface_active: false,
            writer,
        })
    );
    let initial = client_shell_snapshot(&control_rx);
    let tab = |snapshot: &protocol::ClientShellSnapshot| {
        snapshot
            .tabs
            .iter()
            .find(|tab| tab.tab_id == tab_id)
            .expect("snapshot tab")
            .home_location
    };
    assert_eq!(tab(&initial), None, "no card names a pane yet");

    // A local-only card on the second pane: the tab is local.
    write(
        &cards.join("content/agent.json"),
        &serde_json::json!({ "name": "content", "pane": second }).to_string(),
    );
    home_tick(&mut server);
    assert_eq!(
        tab(&next_snapshot(&control_rx)),
        Some(api::schema::HomeLocation::Local)
    );
    assert_eq!(
        listed_home(&mut server, &tab_id),
        Some(api::schema::HomeLocation::Local)
    );

    // A cloud card on the first pane: the first pane in layout order wins.
    write(
        &cards.join("frank/agent.json"),
        &serde_json::json!({ "name": "frank", "pane": first }).to_string(),
    );
    write(
        &cards.join("frank/.rails/home.json"),
        r#"{"origin":"https://rails.so","agent_id":"agt_1","revision":7,"head":"4b825dc"}"#,
    );
    write(
        &cards.join("frank/.rails/status.json"),
        r#"{"schema":1,"location":"cloud","unsaved":0,"unsent":0,"revision":7,"rails_revision":7,"error":null,"checked_at":"2026-10-07T18:00:00Z"}"#,
    );
    home_tick(&mut server);
    assert_eq!(
        tab(&next_snapshot(&control_rx)),
        Some(api::schema::HomeLocation::Cloud)
    );
    assert_eq!(
        listed_home(&mut server, &tab_id),
        Some(api::schema::HomeLocation::Cloud)
    );

    shutdown_test_runtimes(&mut server);
    let _ = fs::remove_dir_all(&cards);
}
