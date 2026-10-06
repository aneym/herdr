//! Saved machines as sections (`[ui.sidebar] machines = "sections"`): Local
//! renders as if no machine were saved, and each machine adds its own section.

use super::*;
use crate::client::endpoint::{
    ClientEndpointId, ClientEndpointStatus, ProfileId, SavedSshEndpoint,
};
use crate::config::SidebarMachinesConfig;
use crossterm::event::{KeyModifiers, MouseButton, MouseEventKind};

const COLS: u16 = 100;
const ROWS: u16 = 28;

fn ax42() -> SavedSshEndpoint {
    SavedSshEndpoint {
        id: ProfileId::parse("0123456789abcdef0123456789abcdef").unwrap(),
        label: "ax42".into(),
        target: "jobs@ax42.example".into(),
        session: "default".into(),
        enabled: true,
    }
}

fn config(tree: bool, machines: SidebarMachinesConfig) -> Config {
    let mut config = Config::default();
    if tree {
        config.ui.agent_panel_sort = crate::config::AgentPanelSortConfig::Tree;
    }
    config.ui.sidebar.machines = machines;
    config
}

fn local_only(config: &Config) -> ClientShellState {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(config));
    state.set_snapshot(Box::new(snapshot()));
    state.set_pane_surface(surface());
    state
}

fn remote_snapshot() -> ClientShellSnapshot {
    let mut remote = snapshot();
    remote.boot_id = "remote-boot".into();
    remote.workspaces[0].label = "lanes".into();
    remote.tabs[0].label = "machines-build".into();
    remote.tabs[0].custom_label = true;
    remote.tabs.push(ClientShellTab {
        tab_id: "tab_2".into(),
        workspace_id: "ws_1".into(),
        number: 2,
        label: "engine-build".into(),
        custom_label: true,
        zoomed: false,
        focused: false,
        agent_status: AgentStatus::Working,
    });
    remote
}

fn with_machine(config: &Config) -> (ClientShellState, ClientEndpointId) {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(config));
    let profile = ax42();
    let endpoint_id = ClientEndpointId::Ssh(profile.id.clone());
    state.set_endpoint_catalog(&[profile]);
    state.set_endpoint_status(&endpoint_id, ClientEndpointStatus::Online);
    state.set_snapshot(Box::new(snapshot()));
    state.set_pane_surface(surface());
    state.set_endpoint_snapshot(&endpoint_id, Box::new(remote_snapshot()));
    (state, endpoint_id)
}

fn text(frame: &FrameData) -> String {
    frame_rows(frame).join("\n")
}

fn header(state: &ClientShellState, endpoint_id: &ClientEndpointId) -> Rect {
    state
        .hits
        .machines
        .iter()
        .find(|hit| &hit.endpoint_id == endpoint_id)
        .expect("machine header hit")
        .rect
}

/// The cells of one row inside `rect`'s columns.
fn sidebar_cells(rows: &[String], rect: Rect) -> String {
    rows[usize::from(rect.y)]
        .chars()
        .skip(usize::from(rect.x))
        .take(usize::from(rect.width))
        .collect()
}

fn click(state: &mut ClientShellState, column: u16, row: u16) -> ClientShellInput {
    state.handle_raw_events(vec![RawInputEvent::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column,
        row,
        modifiers: KeyModifiers::empty(),
    })])
}

#[test]
fn machines_off_keeps_every_local_cell_identical() {
    for tree in [false, true] {
        let plain = local_only(&config(tree, SidebarMachinesConfig::Off))
            .compose(COLS, ROWS)
            .expect("local frame");
        let (mut state, _) = with_machine(&config(tree, SidebarMachinesConfig::Off));
        let federated = state
            .compose(COLS, ROWS)
            .expect("frame with a saved machine");
        assert_eq!(federated.cells, plain.cells, "tree={tree}");
        assert!(state.hits.machines.is_empty());
    }
}

#[test]
fn machine_section_only_takes_free_rows_above_the_tree_footer() {
    let plain = local_only(&config(true, SidebarMachinesConfig::Sections))
        .compose(COLS, ROWS)
        .expect("local frame");
    let (mut state, endpoint_id) = with_machine(&config(true, SidebarMachinesConfig::Sections));
    let frame = state
        .compose(COLS, ROWS)
        .expect("frame with a saved machine");
    let header = header(&state, &endpoint_id);
    // Folded by default: a rule row, then the header, right above the footer row.
    assert_eq!(
        header.y + 1,
        state.hits.sidebar_toggle.y,
        "header sits right above the footer"
    );
    let strip = (header.y - 1)..=header.y;
    let plain_rows = frame_rows(&plain);
    let rows = frame_rows(&frame);
    for y in 0..ROWS {
        let start = usize::from(y) * usize::from(COLS);
        let end = start + usize::from(COLS);
        if strip.contains(&y) {
            // The strip only replaces rows Local left empty.
            let sidebar = plain_rows[usize::from(y)]
                .chars()
                .take(usize::from(header.width))
                .collect::<String>();
            assert!(
                sidebar.trim().is_empty(),
                "row {y} was not free: {sidebar:?}"
            );
            continue;
        }
        assert_eq!(
            frame.cells[start..end],
            plain.cells[start..end],
            "row {y}: {:?} != {:?}",
            rows[usize::from(y)],
            plain_rows[usize::from(y)]
        );
    }
    assert!(rows[usize::from(header.y)].contains("▸ ax42"));
}

#[test]
fn machine_section_unfolds_to_labelled_tabs_without_switching() {
    let (mut state, endpoint_id) = with_machine(&config(true, SidebarMachinesConfig::Sections));
    let folded = state.compose(COLS, ROWS).expect("folded");
    assert!(text(&folded).contains("▸ ax42"));
    assert!(!text(&folded).contains("engine-build"));
    assert!(state.hits.endpoint_tabs.is_empty());

    let rect = header(&state, &endpoint_id);
    let outcome = click(&mut state, rect.x + 3, rect.y);
    assert!(
        outcome.actions.is_empty(),
        "unfolding never activates the machine"
    );
    assert_eq!(state.active_endpoint_id, ClientEndpointId::Local);

    let open = state.compose(COLS, ROWS).expect("unfolded");
    let open_text = text(&open);
    assert!(open_text.contains("▾ ax42"));
    assert!(open_text.contains("lanes"));
    let rows = frame_rows(&open);
    for (row, label) in [
        (&state.hits.endpoint_tabs[0], "machines-"),
        (&state.hits.endpoint_tabs[1], "engine-build"),
    ] {
        let line = sidebar_cells(&rows, row.0);
        assert!(line.contains(label), "{line:?}");
        assert!(line.trim_end().ends_with("· ax42"), "{line:?}");
        assert_eq!(row.1, endpoint_id);
    }
    // Local's own rows still start at the top, unchanged.
    let plain = local_only(&config(true, SidebarMachinesConfig::Sections))
        .compose(COLS, ROWS)
        .expect("local frame");
    assert_eq!(rows[..3], frame_rows(&plain)[..3]);
}

#[test]
fn remote_tab_row_activates_that_machine_on_that_tab() {
    let (mut state, endpoint_id) = with_machine(&config(true, SidebarMachinesConfig::Sections));
    state.compose(COLS, ROWS).unwrap();
    let rect = header(&state, &endpoint_id);
    click(&mut state, rect.x + 3, rect.y);
    state.compose(COLS, ROWS).unwrap();
    let (row, _, _) = state
        .hits
        .endpoint_tabs
        .iter()
        .find(|(_, _, tab_id)| tab_id == "tab_2")
        .cloned()
        .expect("engine-build row");
    let outcome = click(&mut state, row.x + 6, row.y);
    assert!(matches!(
        outcome.actions.as_slice(),
        [ClientShellAction::ActivateEndpoint {
            endpoint_id: activated,
            target: Some(ClientEndpointFocusTarget::Tab(tab_id)),
        }] if activated == &endpoint_id && tab_id == "tab_2"
    ));
}

#[test]
fn local_rows_stay_local_and_return_home_while_a_machine_is_active() {
    // Spaces view, so the Local workspace label is on screen.
    let (mut state, endpoint_id) = with_machine(&config(false, SidebarMachinesConfig::Sections));
    assert!(state.activate_endpoint_projection(&endpoint_id));
    let mut remote_surface = surface();
    remote_surface.boot_id = "remote-boot".into();
    state.set_pane_surface(remote_surface);

    let frame = state.compose(COLS, ROWS).expect("machine active");
    let sidebar = state.hits.local_sidebar;
    assert!(
        !sidebar.is_empty(),
        "Local rows are marked while a machine is active"
    );
    let local_rows = frame_rows(&frame)[usize::from(sidebar.y)..usize::from(sidebar.bottom())]
        .iter()
        .map(|row| {
            row.chars()
                .take(usize::from(sidebar.width))
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(local_rows.contains("client-shell"), "{local_rows}");
    assert!(!local_rows.contains("lanes"), "{local_rows}");

    let outcome = click(&mut state, sidebar.x + 2, sidebar.y + 1);
    assert!(
        matches!(
            outcome.actions.as_slice(),
            [ClientShellAction::ActivateEndpoint { endpoint_id, .. }] if endpoint_id.is_local()
        ),
        "{:?}",
        outcome.actions
    );
}

#[test]
fn removing_the_last_machine_restores_the_plain_frame() {
    let cfg = config(true, SidebarMachinesConfig::Sections);
    let plain = local_only(&cfg).compose(COLS, ROWS).expect("local frame");
    let (mut state, _) = with_machine(&cfg);
    let with = state.compose(COLS, ROWS).expect("with machine");
    assert_ne!(with.cells, plain.cells);
    state.set_endpoint_catalog(&[]);
    let without = state.compose(COLS, ROWS).expect("machine removed");
    assert_eq!(without.cells, plain.cells);
}

#[test]
fn disabled_machine_gets_no_section() {
    let cfg = config(true, SidebarMachinesConfig::Sections);
    let plain = local_only(&cfg).compose(COLS, ROWS).expect("local frame");
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&cfg));
    let mut profile = ax42();
    profile.enabled = false;
    state.set_endpoint_catalog(&[profile]);
    state.set_snapshot(Box::new(snapshot()));
    state.set_pane_surface(surface());
    let frame = state.compose(COLS, ROWS).expect("disabled machine");
    assert_eq!(frame.cells, plain.cells);
}

#[test]
fn offline_machine_header_says_reconnecting_and_dims() {
    let (mut state, endpoint_id) = with_machine(&config(true, SidebarMachinesConfig::Sections));
    state.set_endpoint_status(&endpoint_id, ClientEndpointStatus::Reconnecting);
    let frame = state.compose(COLS, ROWS).expect("offline machine");
    let rect = header(&state, &endpoint_id);
    let line = sidebar_cells(&frame_rows(&frame), rect);
    assert!(line.contains("▸ ax42"), "{line:?}");
    assert!(line.trim_end().ends_with("reconnecting"), "{line:?}");
}

#[test]
fn navigator_gives_only_saved_machines_a_parent_row() {
    let (mut state, endpoint_id) = with_machine(&config(true, SidebarMachinesConfig::Sections));
    state.open_navigator_overlay();
    let ClientShellOverlay::Navigator(navigator) = state.overlay.as_ref().expect("navigator")
    else {
        panic!("expected navigator");
    };
    let rows =
        render::client_navigator_rows(&state.endpoints, &state.active_endpoint_id, navigator);
    let machines = rows
        .iter()
        .filter_map(|row| match &row.target {
            ClientNavigatorTarget::Machine { endpoint_id } => Some(endpoint_id.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(machines, vec![endpoint_id.clone()]);
    for row in &rows {
        match &row.target {
            ClientNavigatorTarget::Workspace { endpoint_id, .. } if endpoint_id.is_local() => {
                assert_eq!(row.depth, 0)
            }
            ClientNavigatorTarget::Workspace { .. } => assert_eq!(row.depth, 1),
            _ => {}
        }
    }
}

#[test]
fn workspace_and_agent_stepping_stay_inside_the_active_endpoint() {
    let (mut state, _) = with_machine(&config(true, SidebarMachinesConfig::Sections));
    for action in [
        crate::input::KeybindAction::NextWorkspace,
        crate::input::KeybindAction::PreviousWorkspace,
        crate::input::KeybindAction::NextAgent,
    ] {
        let mut outcome = ClientShellInput::default();
        assert!(!state.handle_endpoint_navigation(action, &mut outcome));
        assert!(outcome.actions.is_empty());
    }
}

#[test]
fn sections_fit_half_the_sidebar_and_keep_every_header() {
    let (mut state, endpoint_id) = with_machine(&config(true, SidebarMachinesConfig::Sections));
    let mut busy = remote_snapshot();
    busy.tabs = (0..30)
        .map(|index| ClientShellTab {
            tab_id: format!("tab_{index}"),
            workspace_id: "ws_1".into(),
            number: index + 1,
            label: format!("lead-{index}"),
            custom_label: true,
            zoomed: false,
            focused: false,
            agent_status: AgentStatus::Idle,
        })
        .collect();
    state.set_endpoint_snapshot(&endpoint_id, Box::new(busy));
    state.collapsed_endpoints.remove(&endpoint_id);
    let rows = super::super::machine_sections::fit_rows(
        super::super::machine_sections::section_rows(&state.endpoints, &state.collapsed_endpoints),
        20,
    );
    assert_eq!(rows.len(), 10);
    assert!(matches!(
        rows[0],
        super::super::machine_sections::MachineRow::Rule
    ));
    assert!(matches!(
        rows[1],
        super::super::machine_sections::MachineRow::Header { .. }
    ));
    assert!(matches!(
        rows.last(),
        Some(super::super::machine_sections::MachineRow::More(hidden)) if *hidden == 31 - 7
    ));
}
