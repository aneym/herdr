//! Herdr Shell's AGENTS rows say where each chat runs, from `TabInfo.runs_on`. This drives
//! the headless loop's scheduled poll against real files: the factory machine registry names
//! this machine, and the move runner's rails-host/adopted.json moves a pane onto the box.
//! Fails if the poll never runs, if a tab never takes its pane's place, or if tab.list drops
//! the field.

use super::*;

fn tick(server: &mut HeadlessServer) {
    server.next_agent_home_poll = None;
    server.handle_scheduled_tasks_headless(Instant::now(), false);
}

fn listed_runs_on(server: &mut HeadlessServer, tab_id: &str) -> Option<String> {
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
        .runs_on
}

#[tokio::test]
async fn tab_list_says_studio_for_a_local_pane_and_box_for_an_adopted_one() {
    let root = std::env::temp_dir().join(format!(
        "herdr-runs-on-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let rails_host = root.join("rails-host");
    fs::create_dir_all(&rails_host).unwrap();
    // The registry entry for this machine, as factory fleet/config/machines.json writes it:
    // the hostname is one of its aliases and the capitalised alias is its name.
    let host = crate::platform::hostname().expect("hostname");
    let host = host.trim_end_matches(".local").to_ascii_lowercase();
    let registry = root.join("machines.json");
    fs::write(
        &registry,
        serde_json::json!([{ "name": "studio", "aliases": ["Studio", host], "ssh": null }])
            .to_string(),
    )
    .unwrap();
    // nextest runs each test in its own process; only the scheduled poll reads these.
    std::env::set_var("HERDR_RAILS_HOST_DIR", &rails_host);
    std::env::set_var("HERDR_MACHINES_REGISTRY", &registry);

    let mut server = test_headless_server();
    let mut workspace = crate::workspace::Workspace::test_new("runs-on");
    workspace.test_split(ratatui::layout::Direction::Horizontal);
    server.app.state.workspaces = vec![workspace];
    server.app.state.ensure_test_terminals();
    server.app.state.active = Some(0);
    server.app.state.selected = 0;
    let first = server.app.state.workspaces[0].tabs[0].layout.pane_ids()[0];
    let first = server.app.public_pane_id(0, first).unwrap();
    let tab_id = server.app.public_tab_id(0, 0).unwrap();

    tick(&mut server);
    assert_eq!(
        listed_runs_on(&mut server, &tab_id).as_deref(),
        Some("Studio")
    );

    // The move runner adopts the pane's session onto the box.
    fs::write(
        rails_host.join("adopted.json"),
        serde_json::json!({ "panes": { first: {
            "session_id": "as_1", "host_id": "hst_box", "target": "box"
        } } })
        .to_string(),
    )
    .unwrap();
    tick(&mut server);
    assert_eq!(listed_runs_on(&mut server, &tab_id).as_deref(), Some("box"));

    shutdown_test_runtimes(&mut server);
    let _ = fs::remove_dir_all(&root);
}
