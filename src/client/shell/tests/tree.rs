//! The unified space/tab/agent tree and attention-aware agent cycling.
//! Ported from `docs/fork/port-0.9/orig/src/ui/sidebar.rs` and
//! `orig/src/app/actions.rs` onto the client shell.

use super::*;
use crate::client::shell::tree::{tree_list_entries, AgentPanelListEntry, ClientTreeChrome};

fn tab(tab_id: &str, workspace_id: &str, number: usize, label: &str) -> ClientShellTab {
    ClientShellTab {
        sort_rank: 0,
        desk_count: 0,
        tab_id: tab_id.into(),
        workspace_id: workspace_id.into(),
        number,
        label: label.into(),
        custom_label: true,
        zoomed: false,
        focused: false,
        agent_status: AgentStatus::Idle,
        work_status: None,
    }
}

fn agent(
    pane_id: &str,
    workspace_id: &str,
    tab_id: &str,
    status: AgentStatus,
    state_change_seq: u64,
) -> ClientShellAgent {
    ClientShellAgent {
        pane_id: pane_id.into(),
        workspace_id: workspace_id.into(),
        tab_id: tab_id.into(),
        name: Some(pane_id.into()),
        display_agent: Some(pane_id.into()),
        agent: None,
        title: None,
        terminal_title: None,
        terminal_title_stripped: None,
        agent_status: status,
        state_change_seq,
        state_labels: Vec::new(),
        tokens: Vec::new(),
        focused: false,
        visible_in_profile: true,
        owner_pane_id: None,
        orphaned: false,
        group: Default::default(),
    }
}

/// Two spaces; the first has two tabs with one agent each, the second one tab
/// with one agent.
pub(super) fn tree_snapshot() -> ClientShellSnapshot {
    let mut snapshot = snapshot();
    snapshot.workspaces.push(ClientShellWorkspace {
        sort_rank: 0,
        parked: false,
        visible_in_profile: true,
        workspace_id: "ws_2".into(),
        active_tab_id: "tab_3".into(),
        new_workspace_cwd: "/other".into(),
        number: 2,
        label: "beta".into(),
        custom_label: true,
        branch: None,
        git_ahead_behind: None,
        tokens: Vec::new(),
        worktree: None,
        focused: false,
        agent_status: AgentStatus::Idle,
        orchestrator_mode: false,
        tab_count: 1,
    });
    snapshot.workspaces[0].label = "alpha".into();
    snapshot.workspaces[0].custom_label = true;
    snapshot.tabs = vec![
        tab("tab_1", "ws_1", 1, "one"),
        tab("tab_2", "ws_1", 2, "two"),
        tab("tab_3", "ws_2", 1, "three"),
    ];
    snapshot.tabs[0].focused = true;
    snapshot.panes = vec![
        ClientShellPane {
            pane_id: "pane_1".into(),
            workspace_id: "ws_1".into(),
            tab_id: "tab_1".into(),
            label: None,
            cwd: None,
            foreground_cwd: None,
            focused: true,
            right_click_passthrough: false,
            machine: None,
        },
        ClientShellPane {
            pane_id: "pane_2".into(),
            workspace_id: "ws_1".into(),
            tab_id: "tab_2".into(),
            label: None,
            cwd: None,
            foreground_cwd: None,
            focused: false,
            right_click_passthrough: false,
            machine: None,
        },
        ClientShellPane {
            pane_id: "pane_3".into(),
            workspace_id: "ws_2".into(),
            tab_id: "tab_3".into(),
            label: None,
            cwd: None,
            foreground_cwd: None,
            focused: false,
            right_click_passthrough: false,
            machine: None,
        },
    ];
    snapshot.agents = vec![
        agent("pane_1", "ws_1", "tab_1", AgentStatus::Working, 1),
        agent("pane_2", "ws_1", "tab_2", AgentStatus::Blocked, 2),
        agent("pane_3", "ws_2", "tab_3", AgentStatus::Done, 3),
    ];
    snapshot.agents[0].focused = true;
    snapshot
}

fn tree_state(tree: ClientTreeChrome) -> ClientShellState {
    let mut config = ClientShellConfig::from_config(&Config::default());
    config.agent_panel_sort = crate::config::AgentPanelSortConfig::Tree;
    let mut state = ClientShellState::new(config);
    state
        .tree_chrome
        .insert(crate::client::endpoint::ClientEndpointId::Local, tree);
    state.set_snapshot(Box::new(tree_snapshot()));
    state
}

fn shape(state: &ClientShellState, tree: &ClientTreeChrome) -> Vec<String> {
    panel_entries(state, tree)
        .iter()
        .map(|entry| match entry {
            AgentPanelListEntry::SpaceHeader(header) => {
                format!("space:{}", header.label)
            }
            AgentPanelListEntry::TabHeader(header) => format!("tab:{}", header.label),
            AgentPanelListEntry::Agent(row) => format!("agent:{}", row.pane_id),
            AgentPanelListEntry::HiddenSpacesHeader { count, collapsed } => {
                format!(
                    "hidden:{count}:{}",
                    if *collapsed { "closed" } else { "open" }
                )
            }
            AgentPanelListEntry::AutomationsHeader(summary) => {
                format!("automations:{}", summary.label())
            }
            AgentPanelListEntry::QuietSections { hidden, automations } => format!("quiet:{hidden}:{}", automations.label()),
            AgentPanelListEntry::Automation(row) => format!("automation:{}", row.pane_id),
            AgentPanelListEntry::FactorySection { label, .. } => format!("section:{label}"),
            AgentPanelListEntry::FactoryGoalPicker { filter, .. } => format!("goal:{filter:?}"),
            AgentPanelListEntry::FactoryShowAll { count, .. } => format!("show-all:{count}"),
            AgentPanelListEntry::FactoryTab(row) => format!("factory:{}", row.header.label),
            AgentPanelListEntry::FactoryHost { name, .. } => format!("host:{name}"),
            AgentPanelListEntry::FactoryBackground { count, .. } => format!("background:{count}"),
            AgentPanelListEntry::PinnedChatsHeader => "pinned".to_owned(),
            AgentPanelListEntry::AgentChatsHeader => "agents".to_owned(),
            AgentPanelListEntry::PinnedTab(row) => {
                format!("pin:{}:{}:{}", row.shortcut, row.label, row.space_label)
            }
            AgentPanelListEntry::SpaceGroupHeader { name } => format!("group:{name}"),
        })
        .collect()
}

fn panel_entries(state: &ClientShellState, tree: &ClientTreeChrome) -> Vec<AgentPanelListEntry> {
    let snapshot = state.snapshot.as_deref().expect("snapshot");
    let rows = crate::client::shell::agent_sidebar::agent_rows(snapshot, &state.config, None);
    let (rows, automations) =
        crate::client::shell::tree::partition_automations(snapshot, &state.config, rows);
    let rows = crate::client::shell::tree::arrange_agent_hierarchy_with(
        snapshot, tree, rows, !tree.show_tabs,
    );
    let mut entries = if crate::client::shell::tree::tree_view_active(&state.config) {
        tree_list_entries(snapshot, tree, rows)
    } else {
        rows.into_iter().map(AgentPanelListEntry::Agent).collect()
    };
    crate::client::shell::tree::append_automations(&mut entries, tree, automations);
    entries
}

#[test]
fn pinned_chats_top_the_tree_and_own_cmd_digits_in_pin_order() {
    let tree = ClientTreeChrome::default();
    let mut config = ClientShellConfig::from_config(&Config::default());
    config.agent_panel_sort = crate::config::AgentPanelSortConfig::Tree;
    // Alex's config blanks the unknown icon; a pinned chat must still show
    // its agent's state, as its row in the space does.
    config.agents.state_icons.insert("unknown".into(), String::new());
    let mut state = ClientShellState::new(config);
    state
        .tree_chrome
        .insert(crate::client::endpoint::ClientEndpointId::Local, tree.clone());
    let mut snapshot = tree_snapshot();
    // A cross-space pin first, a stale pin (closed tab) in the middle, then a
    // pin from the focused space.
    snapshot.pinned_tabs = vec![
        crate::protocol::ClientShellPinnedTab {
            role: None,
            tab_id: "tab_3".into(), workspace_id: "ws_2".into(),
        },
        crate::protocol::ClientShellPinnedTab {
            role: None,
            tab_id: "gone".into(), workspace_id: "ws_2".into(),
        },
        crate::protocol::ClientShellPinnedTab {
            role: None,
            tab_id: "tab_2".into(), workspace_id: "ws_1".into(),
        },
    ];
    // The tab-level status lags (unknown) while its agent works.
    snapshot.tabs[2].agent_status = AgentStatus::Unknown;
    snapshot.agents[2].agent_status = AgentStatus::Working;
    state.set_snapshot(Box::new(snapshot));

    // The pinned section sits above every space and each row names its space.
    let rows = shape(&state, &tree);
    assert_eq!(rows[..4], ["pinned", "pin:1:three:beta", "pin:2:two:alpha", "space:alpha"]);
    // Cmd+1..9 resolve in the same order the digits are drawn: pins first,
    // then the focused space's remaining tabs.
    let snapshot = state.snapshot.as_deref().expect("snapshot");
    assert_eq!(state.numbered_tab_ids(snapshot), ["tab_3", "tab_2", "tab_1"]);

    // The muted space label and the Cmd digit stay apart ("beta 1", not "beta1").
    let area = ratatui::layout::Rect::new(0, 0, 25, 30);
    let mut buffer = ratatui::buffer::Buffer::empty(area);
    let mut hits = ShellHitMap::default();
    let mut scroll = 0;
    crate::client::shell::agent_sidebar::render_agent_panel_with_overlay(
        &mut buffer, area, snapshot, &state.config, &tree, None, &mut scroll, &mut hits,
    );
    let hit = hits
        .tree_headers
        .iter()
        .find(|hit| hit.pinned && hit.key == "tab_3")
        .expect("pinned row hit");
    for header in hits.tree_headers.iter().filter(|hit| hit.tab_id.is_some()) {
        assert!(header.pin.intersection(header.chevron).is_empty(), "pin and disclosure overlap");
    }
    let line: String = (hit.rect.x..hit.rect.right())
        .map(|x| buffer[(x, hit.rect.y)].symbol().to_owned())
        .collect();
    // State glyph at the left, in the working color.
    let glyph = &buffer[(hit.rect.x + 1, hit.rect.y)];
    assert_eq!(glyph.symbol(), "\u{25cf}", "pinned row reads {line:?}");
    assert_eq!(glyph.fg, state.config.palette.working);
    // No pin glyph and no pin hit: the section name says it.
    assert!(!line.contains('\u{26b2}'), "pinned row reads {line:?}");
    assert!(hit.pin.is_empty());
    // The Cmd digit owns the last cell, two blanks clear of the space label,
    // so it reads as a key column rather than a count after "beta".
    assert!(line.ends_with("beta  1"), "pinned row reads {line:?}");

    // At narrow widths even a long source-space name must leave the title
    // visible; this exercises the actual rendered cells, not a width helper.
    let mut narrow_snapshot = state.snapshot.as_deref().expect("snapshot").clone();
    narrow_snapshot.workspaces[1].label = "s".repeat(40);
    let mut narrow_hits = ShellHitMap::default();
    let mut narrow_buffer = ratatui::buffer::Buffer::empty(area);
    crate::client::shell::agent_sidebar::render_agent_panel_with_overlay(
        &mut narrow_buffer, area, &narrow_snapshot, &state.config, &tree, None, &mut scroll, &mut narrow_hits,
    );
    let row = narrow_hits.tree_headers.iter().find(|hit| hit.key == "tab_3" && hit.pinned).expect("pin");
    let text: String = (row.rect.x..row.rect.right()).map(|x| narrow_buffer[(x, row.rect.y)].symbol()).collect();
    assert!(text.contains("three"), "chat title missing: {text:?}");

    // Unpinning lives in the row's context menu: right-click, Unpin. It
    // unpins without focusing the chat.
    state.compose(80, 24).expect("frame");
    let row = state.hits.tree_headers.iter()
        .find(|hit| hit.pinned && hit.key == "tab_3").expect("pinned row").rect;
    state.handle_raw_events(vec![RawInputEvent::Mouse(MouseEvent {
        kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Right),
        column: row.x + 3,
        row: row.y,
        modifiers: crossterm::event::KeyModifiers::empty(),
    })]);
    let Some(ClientShellOverlay::ContextMenu(menu)) = state.overlay.as_ref() else {
        panic!("context menu open");
    };
    let labels = menu.items().into_iter().map(|item| item.label).collect::<Vec<_>>();
    let unpin = labels.iter().position(|label| label == "Unpin").expect("Unpin in {labels:?}");
    let mut outcome = ClientShellInput::default();
    state.activate_context_menu_item(unpin, &mut outcome);
    let methods = outcome.actions.iter().filter_map(|action| match action {
        ClientShellAction::Endpoint { request, .. } => Some(&request.method),
        _ => None,
    }).collect::<Vec<_>>();
    assert!(matches!(methods[..], [crate::api::schema::Method::TabSetPinned(ref params)]
        if params.tab_id == "tab_3" && !params.pinned), "{methods:?}");

    // Regression at the input boundary: Navigate's plain digits must use the
    // same pinned order as Cmd digits, rather than selecting the first space.
    state.mode = ClientShellMode::Navigate;
    let outcome = state.handle_input_bytes(b"1");
    assert!(outcome.actions.iter().any(|action| matches!(action,
        ClientShellAction::Endpoint { request, .. }
            if matches!(&request.method, crate::api::schema::Method::TabFocus(target)
                if target.tab_id == "tab_3")
    )));

}

fn pin_mouse(kind: crossterm::event::MouseEventKind, column: u16, row: u16) -> RawInputEvent {
    RawInputEvent::Mouse(MouseEvent {
        kind,
        column,
        row,
        modifiers: crossterm::event::KeyModifiers::empty(),
    })
}

fn drawn_pins(state: &ClientShellState) -> Vec<String> {
    state
        .hits
        .pinned_rows
        .iter()
        .map(|hit| hit.tab_id.clone())
        .collect()
}

fn sent_methods(outcome: &ClientShellInput) -> Vec<crate::api::schema::Method> {
    outcome
        .actions
        .iter()
        .filter_map(|action| match action {
            ClientShellAction::Endpoint { request, .. } => Some(request.method.clone()),
            _ => None,
        })
        .collect()
}

/// Mouse-first reorder: a pinned row dragged through the real input path
/// previews its slot, sends `tab.pin_move` on release and never focuses the
/// chat; Esc or a release off the section leaves the order alone; a press and
/// release in place is still the row's click.
#[test]
fn dragging_a_pinned_chat_moves_its_pin_and_never_clicks() {
    use crossterm::event::{MouseButton, MouseEventKind};
    let mut config = ClientShellConfig::from_config(&Config::default());
    config.agent_panel_sort = crate::config::AgentPanelSortConfig::Tree;
    let mut state = ClientShellState::new(config);
    let mut snapshot = tree_snapshot();
    snapshot.pinned_tabs = ["tab_1", "tab_2", "tab_3"]
        .into_iter()
        .map(|tab_id| crate::protocol::ClientShellPinnedTab {
            role: None,
            tab_id: tab_id.into(),
            workspace_id: if tab_id == "tab_3" { "ws_2" } else { "ws_1" }.into(),
        })
        .collect();
    state.set_snapshot(Box::new(snapshot));
    state.compose(80, 24).expect("frame");
    assert_eq!(drawn_pins(&state), ["tab_1", "tab_2", "tab_3"]);
    let slot = |state: &ClientShellState, index: usize| state.hits.pinned_rows[index].rect;
    let (top, bottom) = (slot(&state, 0), slot(&state, 2));

    // Drag the last pin to the top: the rows and Cmd digits follow at once.
    state.handle_raw_events(vec![pin_mouse(
        MouseEventKind::Down(MouseButton::Left),
        bottom.x + 4,
        bottom.y,
    )]);
    state.handle_raw_events(vec![pin_mouse(
        MouseEventKind::Drag(MouseButton::Left),
        bottom.x + 4,
        top.y + 1,
    )]);
    let drag = state.handle_raw_events(vec![pin_mouse(
        MouseEventKind::Drag(MouseButton::Left),
        top.x + 4,
        top.y,
    )]);
    assert!(
        drag.actions.is_empty(),
        "a drag sends nothing until the drop"
    );
    state.compose(80, 24).expect("frame");
    assert_eq!(drawn_pins(&state), ["tab_3", "tab_1", "tab_2"]);
    let snapshot = state.snapshot.as_deref().expect("snapshot");
    assert_eq!(
        state.numbered_tab_ids(snapshot)[..3],
        ["tab_3", "tab_1", "tab_2"]
    );
    let drop = state.handle_raw_events(vec![pin_mouse(
        MouseEventKind::Up(MouseButton::Left),
        top.x + 4,
        top.y,
    )]);
    assert!(
        matches!(sent_methods(&drop)[..], [crate::api::schema::Method::TabPinMove(ref params)]
            if params.tab_id == "tab_3" && params.pin_index == 0),
        "{:?}",
        sent_methods(&drop)
    );
    state.compose(80, 24).expect("frame");
    assert_eq!(
        drawn_pins(&state),
        ["tab_3", "tab_1", "tab_2"],
        "the drop stands until the server answers"
    );

    // Esc mid-drag puts the order back, and the release then does nothing.
    let (first, last) = (slot(&state, 0), slot(&state, 2));
    state.handle_raw_events(vec![pin_mouse(
        MouseEventKind::Down(MouseButton::Left),
        first.x + 4,
        first.y,
    )]);
    state.handle_raw_events(vec![pin_mouse(
        MouseEventKind::Drag(MouseButton::Left),
        last.x + 4,
        last.y,
    )]);
    state.compose(80, 24).expect("frame");
    assert_eq!(drawn_pins(&state), ["tab_1", "tab_2", "tab_3"]);
    let esc = state.handle_raw_events(vec![RawInputEvent::Key(crate::input::TerminalKey::new(
        crossterm::event::KeyCode::Esc,
        crossterm::event::KeyModifiers::empty(),
    ))]);
    assert!(esc.actions.is_empty(), "{:?}", sent_methods(&esc));
    let release = state.handle_raw_events(vec![pin_mouse(
        MouseEventKind::Up(MouseButton::Left),
        last.x + 4,
        last.y,
    )]);
    assert!(
        sent_methods(&release).is_empty(),
        "{:?}",
        sent_methods(&release)
    );
    state.compose(80, 24).expect("frame");
    assert_eq!(drawn_pins(&state), ["tab_3", "tab_1", "tab_2"]);

    // A release well below the section is a cancel.
    state.handle_raw_events(vec![pin_mouse(
        MouseEventKind::Down(MouseButton::Left),
        first.x + 4,
        first.y,
    )]);
    state.handle_raw_events(vec![pin_mouse(
        MouseEventKind::Drag(MouseButton::Left),
        first.x + 4,
        20,
    )]);
    let release = state.handle_raw_events(vec![pin_mouse(
        MouseEventKind::Up(MouseButton::Left),
        first.x + 4,
        20,
    )]);
    assert!(
        sent_methods(&release).is_empty(),
        "{:?}",
        sent_methods(&release)
    );
    state.compose(80, 24).expect("frame");
    assert_eq!(drawn_pins(&state), ["tab_3", "tab_1", "tab_2"]);

    // A press and release in place is the row's click.
    let middle = slot(&state, 1);
    state.handle_raw_events(vec![pin_mouse(
        MouseEventKind::Down(MouseButton::Left),
        middle.x + 4,
        middle.y,
    )]);
    let click = state.handle_raw_events(vec![pin_mouse(
        MouseEventKind::Up(MouseButton::Left),
        middle.x + 4,
        middle.y,
    )]);
    assert!(
        matches!(sent_methods(&click)[..], [crate::api::schema::Method::TabFocus(ref target)] if target.tab_id == "tab_1"),
        "{:?}",
        sent_methods(&click)
    );
}

/// Release coordinates, not the last motion event, decide whether a pin drop
/// stays in PINNED. Exercises the raw mouse boundary without calling drag helpers.
#[test]
fn pin_drag_release_outside_without_motion_cancels() {
    let mut state = pin_drag_state();
    let (top, bottom) = (
        state.hits.pinned_rows[0].rect,
        state.hits.pinned_rows[2].rect,
    );
    state.handle_raw_events(vec![pin_mouse(
        MouseEventKind::Down(MouseButton::Left),
        bottom.x + 4,
        bottom.y,
    )]);
    state.handle_raw_events(vec![pin_mouse(
        MouseEventKind::Drag(MouseButton::Left),
        top.x + 4,
        top.y,
    )]);
    state.compose(80, 24).expect("frame");
    assert_eq!(drawn_pins(&state), ["tab_3", "tab_1", "tab_2"]);

    let release = state.handle_raw_events(vec![pin_mouse(
        MouseEventKind::Up(MouseButton::Left),
        top.x + 4,
        20,
    )]);
    assert!(
        sent_methods(&release).is_empty(),
        "{:?}",
        sent_methods(&release)
    );
    state.compose(80, 24).expect("frame");
    assert_eq!(drawn_pins(&state), ["tab_1", "tab_2", "tab_3"]);
}

fn pin_drag_state() -> ClientShellState {
    let mut config = ClientShellConfig::from_config(&Config::default());
    config.agent_panel_sort = crate::config::AgentPanelSortConfig::Tree;
    let mut state = ClientShellState::new(config);
    let mut snapshot = tree_snapshot();
    snapshot.pinned_tabs = snapshot
        .tabs
        .iter()
        .map(|tab| crate::protocol::ClientShellPinnedTab {
            role: None,
            tab_id: tab.tab_id.clone(),
            workspace_id: tab.workspace_id.clone(),
        })
        .collect();
    state.set_snapshot(Box::new(snapshot));
    state.compose(80, 24).expect("frame");
    state
}

/// A live snapshot during a mouse drag owns the cancel order and subsequent
/// moves; a removed dragged pin cancels instead of being resurrected.
#[test]
fn pin_drag_snapshot_rebases_preview_and_cancel_order() {
    for finish in ["escape", "drop", "unpin"] {
        let mut state = pin_drag_state();
        let mut live = state.snapshot.as_deref().expect("snapshot").clone();
        let mut fourth = live.tabs[0].clone();
        fourth.tab_id = "tab_4".into();
        fourth.number = 4;
        live.pinned_tabs
            .push(crate::protocol::ClientShellPinnedTab {
                role: None,
                tab_id: fourth.tab_id.clone(),
                workspace_id: fourth.workspace_id.clone(),
            });
        live.tabs.push(fourth);
        let (top, bottom) = (
            state.hits.pinned_rows[0].rect,
            state.hits.pinned_rows[2].rect,
        );
        state.handle_raw_events(vec![pin_mouse(
            MouseEventKind::Down(MouseButton::Left),
            bottom.x + 4,
            bottom.y,
        )]);
        state.handle_raw_events(vec![pin_mouse(
            MouseEventKind::Drag(MouseButton::Left),
            top.x + 4,
            top.y,
        )]);
        if finish == "unpin" {
            live.pinned_tabs.retain(|pin| pin.tab_id != "tab_3");
        }
        state.set_snapshot(Box::new(live));
        state.compose(80, 24).expect("frame");
        if finish == "unpin" {
            assert!(state.chrome_drag.is_none());
            assert_eq!(drawn_pins(&state), ["tab_1", "tab_2", "tab_4"]);
            continue;
        }
        assert_eq!(drawn_pins(&state), ["tab_3", "tab_1", "tab_2", "tab_4"]);
        let middle = state.hits.pinned_rows[1].rect;
        state.handle_raw_events(vec![pin_mouse(
            MouseEventKind::Drag(MouseButton::Left),
            middle.x + 4,
            middle.y,
        )]);
        state.compose(80, 24).expect("frame");
        assert_eq!(drawn_pins(&state), ["tab_1", "tab_3", "tab_2", "tab_4"]);
        let outcome = if finish == "escape" {
            state.handle_raw_events(vec![RawInputEvent::Key(crate::input::TerminalKey::new(
                crossterm::event::KeyCode::Esc,
                KeyModifiers::empty(),
            ))])
        } else {
            state.handle_raw_events(vec![pin_mouse(
                MouseEventKind::Up(MouseButton::Left),
                middle.x + 4,
                middle.y,
            )])
        };
        state.compose(80, 24).expect("frame");
        let expected = if finish == "escape" {
            assert!(sent_methods(&outcome).is_empty());
            ["tab_1", "tab_2", "tab_3", "tab_4"]
        } else {
            assert!(
                matches!(sent_methods(&outcome)[..], [crate::api::schema::Method::TabPinMove(ref params)]
                if params.tab_id == "tab_3" && params.pin_index == 1)
            );
            ["tab_1", "tab_3", "tab_2", "tab_4"]
        };
        assert_eq!(drawn_pins(&state), expected);
        let snapshot = state.snapshot.as_deref().expect("snapshot");
        assert_eq!(state.numbered_tab_ids(snapshot)[..4], expected);
    }
}

#[test]
fn space_groups_keep_pinned_chats_above_the_groups() {
    let tree = ClientTreeChrome::default();
    let mut config = ClientShellConfig::from_config(&Config::default());
    config.agent_panel_sort = crate::config::AgentPanelSortConfig::Tree;
    let mut snapshot = tree_snapshot();
    snapshot.pinned_tabs = vec![crate::protocol::ClientShellPinnedTab {
        role: None,
        tab_id: "tab_3".into(),
        workspace_id: "ws_2".into(),
    }];
    let mut overlay = crate::factory_overlay::FactoryOverlay::default();
    overlay
        .apply_areas_file(br#"{"space_groups":[{"name":"Rails","spaces":["beta"]}]}"#)
        .expect("areas file");
    let rows = crate::client::shell::agent_sidebar::agent_rows(&snapshot, &config, None);
    let rows = crate::client::shell::tree::arrange_agent_hierarchy_with(
        &snapshot, &tree, rows, !tree.show_tabs,
    );
    let entries = crate::client::shell::tree::tree_list_entries_with_overlay(
        &snapshot,
        &tree,
        rows,
        Some(&overlay),
    );
    let shape: Vec<String> = entries
        .iter()
        .filter_map(|entry| match entry {
            AgentPanelListEntry::PinnedChatsHeader => Some("pinned".to_owned()),
            AgentPanelListEntry::PinnedTab(row) => Some(format!("pin:{}", row.label)),
            AgentPanelListEntry::SpaceGroupHeader { name } => Some(format!("group:{name}")),
            AgentPanelListEntry::SpaceHeader(header) => Some(format!("space:{}", header.label)),
            _ => None,
        })
        .collect();
    // The pinned section owns the top and the Cmd digits; groups follow it.
    assert_eq!(shape, ["pinned", "pin:three", "group:Rails", "space:beta", "space:alpha"]);
}

#[test]
fn tree_nests_spaces_then_tabs_then_agents_in_workspace_order() {
    let tree = ClientTreeChrome::default();
    let state = tree_state(tree.clone());

    assert_eq!(
        shape(&state, &tree),
        [
            "space:alpha",
            "tab:one",
            "tab:two",
            "space:beta",
            "tab:three",
        ]
    );
    // The merged header retains each chat's status in place of a duplicate row.
    // set_snapshot marks the non-focused workspace's done state as viewed (idle).
    let snapshot = state.snapshot.as_deref().expect("snapshot");
    let rows = crate::client::shell::agent_sidebar::agent_rows(snapshot, &state.config, None);
    let rows = crate::client::shell::tree::arrange_agent_hierarchy_with(
        snapshot, &tree, rows, false,
    );
    let headers = tree_list_entries(snapshot, &tree, rows)
        .into_iter()
        .filter_map(|entry| match entry {
            AgentPanelListEntry::TabHeader(header) => Some((header.label, header.child_states)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        headers,
        [
            ("one".to_owned(), vec![AgentStatus::Working]),
            ("two".to_owned(), vec![AgentStatus::Blocked]),
            ("three".to_owned(), vec![AgentStatus::Idle]),
        ]
    );
}

#[test]
fn collapsed_space_moves_to_the_hidden_section_and_hides_its_agents() {
    let mut tree = ClientTreeChrome::default();
    tree.collapsed_spaces.insert("ws_1".into());
    let state = tree_state(tree.clone());

    assert_eq!(
        shape(&state, &tree),
        ["space:beta", "tab:three", "hidden:1:closed"]
    );
}

#[test]
fn collapsed_tab_hides_only_that_tabs_agents() {
    let mut tree = ClientTreeChrome::default();
    tree.collapsed_tabs.insert("ws_1#1".into());
    let mut state = tree_state(tree.clone());
    let mut snapshot = tree_snapshot();
    snapshot.agents.push(agent("pane_4", "ws_1", "tab_1", AgentStatus::Idle, 4));
    state.set_snapshot(Box::new(snapshot));

    let mut open = tree.clone();
    open.collapsed_tabs.clear();
    assert!(shape(&state, &open).contains(&"agent:pane_4".to_owned()));
    assert_eq!(
        shape(&state, &tree),
        [
            "space:alpha",
            "tab:one",
            "tab:two",
            "space:beta",
            "tab:three",
        ]
    );
}

#[test]
fn collapsed_tab_header_carries_one_dot_per_hidden_agent() {
    let mut tree = ClientTreeChrome::default();
    tree.collapsed_tabs.insert("ws_1#1".into());
    let state = tree_state(tree.clone());
    let snapshot = state.snapshot.as_deref().expect("snapshot");
    let rows = crate::client::shell::agent_sidebar::agent_rows(snapshot, &state.config, None);

    let dots = tree_list_entries(snapshot, &tree, rows)
        .into_iter()
        .find_map(|entry| match entry {
            AgentPanelListEntry::TabHeader(header) if header.label == "one" => {
                Some(header.child_states)
            }
            _ => None,
        })
        .expect("collapsed tab header");

    assert_eq!(dots, [AgentStatus::Working]);
}

#[test]
fn tree_layer_toggles_drop_their_header_rows() {
    let mut tree = ClientTreeChrome {
        show_tabs: false,
        ..ClientTreeChrome::default()
    };
    let state = tree_state(tree.clone());
    assert_eq!(
        shape(&state, &tree),
        [
            "space:alpha",
            "agent:pane_1",
            "agent:pane_2",
            "space:beta",
            "agent:pane_3",
        ]
    );

    tree.show_tabs = true;
    tree.show_spaces = false;
    assert_eq!(
        shape(&state, &tree),
        [
            "tab:one",
            "tab:two",
            "tab:three",
        ]
    );
}

#[test]
fn hidden_agents_leave_only_space_and_tab_headers() {
    let tree = ClientTreeChrome {
        show_agents: false,
        ..ClientTreeChrome::default()
    };
    let state = tree_state(tree.clone());

    assert_eq!(
        shape(&state, &tree),
        [
            "space:alpha",
            "tab:one",
            "tab:two",
            "space:beta",
            "tab:three"
        ]
    );
}

#[test]
fn hiding_all_tree_layers_yields_an_empty_list() {
    let tree = ClientTreeChrome {
        show_spaces: false,
        show_tabs: false,
        show_agents: false,
        ..ClientTreeChrome::default()
    };
    let state = tree_state(tree.clone());

    assert!(shape(&state, &tree).is_empty());
}

#[test]
fn tree_rows_drop_the_labels_their_headers_already_carry() {
    let tree = ClientTreeChrome::default();
    let mut state = tree_state(tree.clone());
    let mut snapshot = tree_snapshot();
    snapshot.agents.push(agent("pane_4", "ws_1", "tab_1", AgentStatus::Idle, 4));
    state.set_snapshot(Box::new(snapshot));
    let snapshot = state.snapshot.as_deref().expect("snapshot");
    let rows = crate::client::shell::agent_sidebar::agent_rows(snapshot, &state.config, None);

    let rows = crate::client::shell::tree::arrange_agent_hierarchy_with(
        snapshot, &tree, rows, false,
    );
    let row = tree_list_entries(snapshot, &tree, rows)
        .into_iter()
        .find_map(|entry| match entry {
            AgentPanelListEntry::Agent(row) if row.pane_id == "pane_4" => Some(row),
            _ => None,
        })
        .expect("agent row");

    assert!(row.rows.iter().flatten().all(|token| !matches!(
        token.kind,
        crate::ui::ResolvedTokenKind::Workspace(_) | crate::ui::ResolvedTokenKind::Tab(_)
    )));
    // Two layers of headers above the peer row, so it indents twice.
    assert_eq!(row.indent, 2);
}

#[test]
fn agent_cycle_skips_agents_inside_a_collapsed_space() {
    let mut tree = ClientTreeChrome::default();
    tree.collapsed_spaces.insert("ws_2".into());
    let state = tree_state(tree);
    let snapshot = state.snapshot.as_deref().expect("snapshot");

    let panes = state
        .agent_cycle_candidates(snapshot)
        .into_iter()
        .map(|entry| entry.pane_id)
        .collect::<Vec<_>>();

    assert_eq!(panes, ["pane_1", "pane_2"]);
}

#[test]
fn agent_cycle_skips_owned_workflow_tabs_even_when_the_workflow_needs_attention() {
    for expanded in [false, true] {
        let mut tree = ClientTreeChrome::default();
        if expanded {
            tree.collapsed_agent_groups.insert("pane_1".into());
        }
        let mut state = tree_state(tree);
        let mut snapshot = tree_snapshot();
        // The blocked workflow has its own tab, but belongs beneath lane A.
        snapshot.agents[1].owner_pane_id = Some("pane_1".into());
        state.set_snapshot(Box::new(snapshot));
        let snapshot = state.snapshot.as_deref().expect("snapshot");

        let panes = state
            .agent_cycle_candidates(snapshot)
            .into_iter()
            .map(|entry| entry.pane_id)
            .collect::<Vec<_>>();
        assert_eq!(panes, ["pane_1", "pane_3"]);

        let mut next = ClientShellInput::default();
        state.record_binding(
            crate::input::KeybindMatch::Action(crate::input::KeybindAction::NextAgent),
            &mut next,
        );
        assert!(matches!(
            &next.actions[..],
            [ClientShellAction::Endpoint { request, .. }]
                if matches!(
                    &request.method,
                    crate::api::schema::Method::PaneFocus(target) if target.pane_id == "pane_3"
                )
        ));
    }
}

#[test]
fn agent_cycle_prefers_the_most_demanding_peer_over_the_next_in_order() {
    // Spaces order keeps a stable list, so the key has to rank for itself:
    // blocked outranks an unread completion, which outranks a working agent.
    let mut config = ClientShellConfig::from_config(&Config::default());
    config.agent_panel_sort = crate::config::AgentPanelSortConfig::Spaces;
    let mut state = ClientShellState::new(config);
    state.set_snapshot(Box::new(tree_snapshot()));
    let snapshot = state.snapshot.as_deref().expect("snapshot");
    let candidates = state.agent_cycle_candidates(snapshot);

    // Focused: pane_1 (working). pane_2 is blocked, pane_3 is done.
    let next = state
        .agent_cycle_target(snapshot, &candidates, true)
        .expect("cycle target");
    assert_eq!(candidates[next].pane_id, "pane_2");
}

#[test]
fn agent_cycle_never_returns_the_focused_agent() {
    let mut config = ClientShellConfig::from_config(&Config::default());
    config.agent_panel_sort = crate::config::AgentPanelSortConfig::Spaces;
    let mut state = ClientShellState::new(config);
    let mut snapshot = tree_snapshot();
    // The focused agent is the only blocked one, so ranking it alongside the
    // rest would wrap the search straight back onto it.
    snapshot.agents[0].agent_status = AgentStatus::Blocked;
    state.set_snapshot(Box::new(snapshot));
    let snapshot = state.snapshot.as_deref().expect("snapshot");
    let candidates = state.agent_cycle_candidates(snapshot);

    let next = state
        .agent_cycle_target(snapshot, &candidates, true)
        .expect("cycle target");
    assert_ne!(candidates[next].pane_id, "pane_1");
}

#[test]
fn triage_sort_orders_tiers_and_oldest_state_change_first() {
    let mut snapshot = tree_snapshot();
    snapshot.agents = vec![
        agent("p1", "ws_1", "tab_1", AgentStatus::Working, 2),
        agent("p2", "ws_1", "tab_1", AgentStatus::Done, 8),
        agent("p3", "ws_1", "tab_1", AgentStatus::Unknown, 0),
        agent("p4", "ws_1", "tab_1", AgentStatus::Idle, 3),
        agent("p5", "ws_1", "tab_1", AgentStatus::Blocked, 5),
        agent("p6", "ws_1", "tab_1", AgentStatus::Done, 1),
    ];

    let ordered = crate::client::shell::agent_sidebar::ordered_agent_pane_ids(
        &snapshot,
        crate::config::AgentPanelSortConfig::Triage,
    );

    assert_eq!(ordered, ["p5", "p6", "p2", "p4", "p1", "p3"]);
}

#[test]
fn tree_chevron_click_collapses_the_space_and_persists_it() {
    let tree = ClientTreeChrome::default();
    let mut state = tree_state(tree);
    state.set_pane_surface(surface());
    state.compose(106, 40).expect("composed frame");

    let header = state
        .hits
        .tree_headers
        .iter()
        .find(|hit| hit.key == "ws_1" && hit.tab_id.is_none())
        .expect("space header hit");
    let chevron = header.chevron;
    assert!(chevron.width > 0);

    state.handle_raw_events(vec![RawInputEvent::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: chevron.x,
        row: chevron.y,
        modifiers: KeyModifiers::empty(),
    })]);

    assert!(state
        .tree_chrome
        .get(&crate::client::endpoint::ClientEndpointId::Local)
        .expect("local tree chrome")
        .collapsed_spaces
        .contains("ws_1"));
}

#[test]
fn tree_tab_header_click_focuses_that_tab() {
    let tree = ClientTreeChrome::default();
    let mut state = tree_state(tree);
    state.set_pane_surface(surface());
    state.compose(106, 40).expect("composed frame");

    let header = state
        .hits
        .tree_headers
        .iter()
        .find(|hit| hit.tab_id.as_deref() == Some("tab_2"))
        .expect("tab header hit");
    let rect = header.rect;

    let outcome = state.handle_raw_events(vec![RawInputEvent::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: rect.x + 1,
        row: rect.y,
        modifiers: KeyModifiers::empty(),
    })]);

    assert!(matches!(
        &outcome.actions[..],
        [ClientShellAction::Endpoint { request, .. }]
            if matches!(
                &request.method,
                crate::api::schema::Method::TabFocus(target) if target.tab_id == "tab_2"
            )
    ));
}

#[test]
fn pinned_space_keeps_its_header_without_any_agents() {
    let mut tree = ClientTreeChrome::default();
    tree.pinned_spaces.insert("ws_2".into());
    let mut state = tree_state(tree.clone());
    let mut snapshot = tree_snapshot();
    // Nothing runs in beta any more.
    snapshot.agents.retain(|agent| agent.workspace_id != "ws_2");
    state.set_snapshot(Box::new(snapshot));

    assert_eq!(
        shape(&state, &tree),
        [
            "space:alpha",
            "tab:one",
            "tab:two",
            "space:beta",
        ]
    );
}

#[test]
fn unpinned_agentless_space_drops_out_of_the_tree() {
    let tree = ClientTreeChrome::default();
    let mut state = tree_state(tree.clone());
    let mut snapshot = tree_snapshot();
    snapshot.agents.retain(|agent| agent.workspace_id != "ws_2");
    state.set_snapshot(Box::new(snapshot));

    assert!(!shape(&state, &tree).contains(&"space:beta".to_owned()));
}

#[test]
fn collapsed_spaces_move_into_the_hidden_section() {
    let mut tree = ClientTreeChrome {
        show_hidden_spaces: true,
        ..ClientTreeChrome::default()
    };
    tree.collapsed_spaces.insert("ws_1".into());
    let state = tree_state(tree.clone());

    assert_eq!(
        shape(&state, &tree),
        ["space:beta", "tab:three", "hidden:1:closed"]
    );
}

#[test]
fn expanding_the_hidden_section_lists_the_folded_spaces() {
    let mut tree = ClientTreeChrome {
        show_hidden_spaces: true,
        hidden_spaces_expanded: true,
        ..ClientTreeChrome::default()
    };
    tree.collapsed_spaces.insert("ws_1".into());
    let state = tree_state(tree.clone());

    assert_eq!(
        shape(&state, &tree),
        [
            "space:beta",
            "tab:three",
            "hidden:1:open",
            "space:alpha",
        ]
    );
}

#[test]
fn no_hidden_section_without_the_reveal() {
    let mut tree = ClientTreeChrome {
        show_hidden_spaces: false,
        ..ClientTreeChrome::default()
    };
    tree.collapsed_spaces.insert("ws_1".into());
    let state = tree_state(tree.clone());

    assert!(!shape(&state, &tree)
        .iter()
        .any(|row| row.starts_with("hidden:")));
    assert!(shape(&state, &tree).contains(&"space:alpha".to_owned()));
}

#[test]
fn hidden_section_counts_spaces_not_agents() {
    let mut tree = ClientTreeChrome {
        show_hidden_spaces: true,
        ..ClientTreeChrome::default()
    };
    tree.collapsed_spaces.insert("ws_1".into());
    tree.collapsed_spaces.insert("ws_2".into());
    let state = tree_state(tree.clone());

    assert_eq!(shape(&state, &tree), ["hidden:2:closed"]);
}

#[test]
fn collapsed_space_header_hides_its_status_dots() {
    let mut tree = ClientTreeChrome {
        show_tabs: false,
        show_agents: false,
        hidden_spaces_expanded: true,
        ..ClientTreeChrome::default()
    };
    tree.collapsed_spaces.insert("ws_1".into());
    let state = tree_state(tree.clone());
    let snapshot = state.snapshot.as_deref().expect("snapshot");
    let rows = crate::client::shell::agent_sidebar::agent_rows(snapshot, &state.config, None);
    let headers = tree_list_entries(snapshot, &tree, rows)
        .into_iter()
        .filter_map(|entry| match entry {
            AgentPanelListEntry::SpaceHeader(header) => Some((header.label, header.child_states)),
            _ => None,
        })
        .collect::<Vec<_>>();

    let beta_status = snapshot
        .agents
        .iter()
        .find(|agent| agent.pane_id == "pane_3")
        .expect("beta agent")
        .agent_status;
    assert_eq!(
        headers,
        [
            ("beta".to_owned(), vec![beta_status]),
            ("alpha".to_owned(), Vec::new()),
        ]
    );
}

#[test]
fn space_order_moves_whole_space_blocks() {
    let tree = ClientTreeChrome {
        space_order: vec!["ws_2".into(), "ws_1".into()],
        ..ClientTreeChrome::default()
    };
    let state = tree_state(tree.clone());

    assert_eq!(
        shape(&state, &tree),
        [
            "space:beta",
            "tab:three",
            "space:alpha",
            "tab:one",
            "tab:two",
        ]
    );
}

#[test]
fn pin_click_toggles_the_pin_and_tells_the_endpoint() {
    let tree = ClientTreeChrome::default();
    let mut state = tree_state(tree);
    state.set_pane_surface(surface());
    state.compose(106, 40).expect("composed frame");

    let pin = state
        .hits
        .tree_headers
        .iter()
        .find(|hit| hit.key == "ws_1" && hit.tab_id.is_none())
        .expect("space header hit")
        .pin;
    assert!(pin.width > 0);

    let outcome = state.handle_raw_events(vec![RawInputEvent::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: pin.x,
        row: pin.y,
        modifiers: KeyModifiers::empty(),
    })]);

    assert!(state
        .tree_chrome
        .get(&crate::client::endpoint::ClientEndpointId::Local)
        .expect("local tree chrome")
        .pinned_spaces
        .contains("ws_1"));
    assert!(matches!(
        &outcome.actions[..],
        [ClientShellAction::Endpoint { request, .. }]
            if matches!(
                &request.method,
                crate::api::schema::Method::WorkspaceSetPinned(params)
                    if params.workspace_id == "ws_1" && params.pinned
            )
    ));
}

#[test]
fn dragging_a_space_header_records_the_new_order() {
    let tree = ClientTreeChrome::default();
    let mut state = tree_state(tree);
    state.set_pane_surface(surface());
    state.compose(106, 40).expect("composed frame");

    let (alpha, beta) = {
        let headers = state
            .hits
            .tree_headers
            .iter()
            .filter(|hit| hit.tab_id.is_none())
            .collect::<Vec<_>>();
        (headers[0].rect, headers[1].rect)
    };

    state.handle_raw_events(vec![RawInputEvent::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: alpha.x + 1,
        row: alpha.y,
        modifiers: KeyModifiers::empty(),
    })]);
    assert!(state.tree_space_press.is_some());
    state.handle_raw_events(vec![RawInputEvent::Mouse(MouseEvent {
        kind: MouseEventKind::Drag(MouseButton::Left),
        column: beta.x + 1,
        row: beta.y + 1,
        modifiers: KeyModifiers::empty(),
    })]);
    state.handle_raw_events(vec![RawInputEvent::Mouse(MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column: beta.x + 1,
        row: beta.y + 1,
        modifiers: KeyModifiers::empty(),
    })]);

    assert_eq!(
        state
            .tree_chrome
            .get(&crate::client::endpoint::ClientEndpointId::Local)
            .expect("local tree chrome")
            .space_order,
        ["ws_2", "ws_1"]
    );
}

#[test]
fn left_click_on_the_sort_label_opens_the_view_picker() {
    let tree = ClientTreeChrome::default();
    let mut state = tree_state(tree);
    state.set_pane_surface(surface());
    state.compose(106, 40).expect("composed frame");
    let toggle = state.hits.agent_sort_toggle;
    assert!(toggle.width > 0);

    state.handle_raw_events(vec![RawInputEvent::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: toggle.x,
        row: toggle.y,
        modifiers: KeyModifiers::empty(),
    })]);

    let Some(crate::client::shell::ClientShellOverlay::ContextMenu(menu)) = state.overlay.as_ref()
    else {
        panic!("left-click on the sort label should open the view menu");
    };
    assert_eq!(
        menu.items()
            .iter()
            .map(|item| item.label.as_str())
            .collect::<Vec<_>>(),
        [
            "● tree",
            "  grouped",
            "  priority",
            "  triage",
            "──────────",
            "✓ spaces",
            "✓ tabs",
            "✓ agents",
            "✓ hidden"
        ]
    );
    assert_eq!(
        state.config.agent_panel_sort,
        crate::config::AgentPanelSortConfig::Tree
    );

    let mut outcome = ClientShellInput::default();
    state.activate_context_menu_item(2, &mut outcome);
    state.open_sidebar_view_context_menu(toggle.x, toggle.y);
    let Some(crate::client::shell::ClientShellOverlay::ContextMenu(menu)) = state.overlay.as_ref()
    else {
        panic!("priority view picker did not open");
    };
    assert_eq!(
        menu.items()
            .iter()
            .map(|item| item.label.as_str())
            .collect::<Vec<_>>(),
        ["  tree", "  grouped", "● priority", "  triage"]
    );
}

#[test]
fn revealing_folded_spaces_starts_the_section_compact() {
    let tree = ClientTreeChrome {
        hidden_spaces_expanded: true,
        show_hidden_spaces: true,
        ..ClientTreeChrome::default()
    };
    let mut state = tree_state(tree);
    state.open_sidebar_view_context_menu(0, 0);
    let Some(crate::client::shell::ClientShellOverlay::ContextMenu(menu)) = state.overlay.as_ref()
    else {
        panic!("view picker did not open");
    };
    let hidden = menu
        .items()
        .iter()
        .position(|item| item.label == "✓ hidden")
        .expect("hidden spaces toggle");
    let mut outcome = ClientShellInput::default();
    state.activate_context_menu_item(hidden, &mut outcome);

    let tree = state
        .tree_chrome
        .get(&crate::client::endpoint::ClientEndpointId::Local)
        .expect("local tree chrome");
    assert!(!tree.show_hidden_spaces);
    assert!(!tree.hidden_spaces_expanded);
}

fn automation_config() -> ClientShellConfig {
    let mut config = Config::default();
    config.ui.sidebar.automations.workspaces = vec!["beta".into()];
    ClientShellConfig::from_config(&config)
}

#[test]
fn automation_entries_are_partitioned_and_expand_after_the_header() {
    let mut config = automation_config();
    config.agent_panel_sort = crate::config::AgentPanelSortConfig::Tree;
    let mut state = ClientShellState::new(config);
    state.set_snapshot(Box::new(tree_snapshot()));
    let mut tree = ClientTreeChrome::default();

    assert_eq!(
        shape(&state, &tree),
        [
            "space:alpha",
            "tab:one",
            "tab:two",
            "automations:1",
        ]
    );

    tree.automations_expanded = true;
    assert_eq!(
        shape(&state, &tree),
        [
            "space:alpha",
            "tab:one",
            "tab:two",
            "automations:1",
            "automation:pane_3",
        ]
    );
}

#[test]
fn empty_automation_config_preserves_the_stock_render() {
    let tree = ClientTreeChrome::default();
    let state = tree_state(tree.clone());

    assert!(!shape(&state, &tree)
        .iter()
        .any(|row| row.starts_with("automations:")));
}

#[test]
fn automation_summary_reports_blocked_before_working() {
    let mut config = automation_config();
    config.agent_panel_sort = crate::config::AgentPanelSortConfig::Tree;
    let mut state = ClientShellState::new(config);
    let mut snapshot = tree_snapshot();
    snapshot
        .agents
        .push(agent("pane_4", "ws_2", "tab_3", AgentStatus::Blocked, 4));
    snapshot
        .agents
        .push(agent("pane_5", "ws_2", "tab_3", AgentStatus::Working, 5));
    state.set_snapshot(Box::new(snapshot));
    let tree = ClientTreeChrome::default();

    let header = shape(&state, &tree)
        .into_iter()
        .find(|row| row.starts_with("automations:"))
        .expect("automations header");
    assert!(header.starts_with("automations:1 blocked"));
    assert!(header.contains("1 working"));
}

#[test]
fn clicking_the_automations_header_expands_the_section() {
    let mut config = automation_config();
    config.agent_panel_sort = crate::config::AgentPanelSortConfig::Tree;
    let mut state = ClientShellState::new(config);
    state.set_snapshot(Box::new(tree_snapshot()));
    state.set_pane_surface(surface());
    state.compose(106, 40).expect("composed frame");
    let header = state.hits.automations_header;
    assert!(header.width > 0);

    state.handle_raw_events(vec![RawInputEvent::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: header.x + 1,
        row: header.y,
        modifiers: KeyModifiers::empty(),
    })]);

    assert!(
        state
            .tree_chrome
            .get(&crate::client::endpoint::ClientEndpointId::Local)
            .expect("local tree chrome")
            .automations_expanded
    );
}

#[test]
fn tree_view_gives_the_agents_panel_the_whole_sidebar() {
    let tree = ClientTreeChrome::default();
    let mut state = tree_state(tree);
    state.set_pane_surface(surface());
    state.compose(106, 40).expect("composed frame");

    // Only the one-row footer strip is left for the spaces section, so no
    // workspace rows are drawn and the section divider is gone.
    assert!(state.hits.workspaces.is_empty());
    assert_eq!(state.hits.sidebar_section_divider, Rect::default());
    assert!(state.hits.global_launcher.width > 0);
    assert!(state.hits.agent_body.height > 30);
}

#[test]
fn content_fit_spaces_hug_their_rows_and_leave_the_rest_to_agents() {
    let mut config = Config::default();
    config.ui.sidebar.spaces.max_visible = 1;
    let mut ratio = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    let mut fitted = ClientShellState::new(ClientShellConfig::from_config(&config));
    for state in [&mut ratio, &mut fitted] {
        state.set_snapshot(Box::new(tree_snapshot()));
        state.set_pane_surface(surface());
        state.compose(106, 40).expect("composed frame");
    }

    assert!(fitted.hits.agent_body.height > ratio.hits.agent_body.height);
    // The fitted list still shows its one entry.
    assert_eq!(fitted.hits.workspaces.len(), 1);
    // A content-fit sidebar has no draggable split.
    assert_eq!(fitted.hits.sidebar_section_divider, Rect::default());
}

#[test]
fn agent_titles_use_tab_label_ink_in_dark_and_light_themes() {
    let mut config = Config::default();
    config.ui.sidebar.agents.rows = vec![vec![
        crate::config::AgentSidebarToken::TerminalTitleStripped,
    ]];
    let mut snapshot = tree_snapshot();
    snapshot.agents[1].terminal_title_stripped = Some("UNFOCUSED_TITLE".into());
    for appearance in [
        crate::terminal_theme::HostAppearance::Dark,
        crate::terminal_theme::HostAppearance::Light,
    ] {
        let mut shell_config = ClientShellConfig::from_config(&config);
        shell_config.palette =
            crate::app::client_palette_for_appearance(&shell_config.theme_runtime, appearance);
        let expected = crate::protocol::color_to_u32(shell_config.palette.subtext0);
        let muted = crate::protocol::color_to_u32(shell_config.palette.overlay0);
        assert_ne!(expected, muted);
        let mut state = ClientShellState::new(shell_config);
        state.set_snapshot(Box::new(snapshot.clone()));
        state.set_pane_surface(surface());
        let frame = state.compose(106, 40).expect("agent title frame");
        let (x, y) = cell_symbol_position(
            &frame,
            Rect::new(0, 0, frame.width, frame.height),
            "UNFOCUSED_TITLE",
        );
        let cell = &frame.cells[usize::from(y) * usize::from(frame.width) + usize::from(x)];
        assert_eq!(cell.fg, expected, "appearance: {appearance:?}");
    }
}

#[test]
fn agents_first_section_order_puts_the_spaces_list_at_the_bottom() {
    let mut config = Config::default();
    config.ui.sidebar.section_order = [
        crate::config::SidebarSection::Agents,
        crate::config::SidebarSection::Spaces,
    ];
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&config));
    state.set_snapshot(Box::new(tree_snapshot()));
    state.set_pane_surface(surface());
    state.compose(106, 40).expect("composed frame");

    assert!(state.hits.workspaces[0].rect.y > state.hits.agent_body.y);
}

#[test]
fn header_new_button_moves_out_of_the_footer() {
    let mut config = Config::default();
    config.ui.sidebar.new_button = crate::config::SidebarNewButtonConfig::Header;
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&config));
    state.set_snapshot(Box::new(tree_snapshot()));
    state.set_pane_surface(surface());
    state.compose(106, 40).expect("composed frame");

    let new_button = state.hits.new_workspace;
    assert_eq!(new_button.y, 0);
    assert!(new_button.x > state.hits.global_launcher.x.saturating_sub(40));
    assert_ne!(new_button.y, state.hits.global_launcher.y);
}

#[test]
fn left_menu_position_moves_the_launcher_to_the_footer_start() {
    let mut config = Config::default();
    config.ui.sidebar.menu_position = crate::config::SidebarMenuPositionConfig::Left;
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&config));
    state.set_snapshot(Box::new(tree_snapshot()));
    state.set_pane_surface(surface());
    state.compose(106, 40).expect("composed frame");

    assert_eq!(state.hits.global_launcher.x, 0);
}

#[test]
fn space_header_plus_creates_the_next_tab_without_prompting() {
    let mut config = ClientShellConfig::from_config(&Config::default());
    config.agent_panel_sort = crate::config::AgentPanelSortConfig::Tree;
    // Even with the name prompt on, the space plus makes the next tab directly.
    config.prompt_new_tab_name = true;
    let mut state = ClientShellState::new(config);
    state.set_snapshot(Box::new(tree_snapshot()));
    state.set_pane_surface(surface());
    state.compose(106, 40).expect("composed frame");

    let header = state
        .hits
        .tree_headers
        .iter()
        .find(|hit| hit.key == "ws_2" && hit.tab_id.is_none())
        .expect("space header hit");
    let plus = header.plus;
    assert!(plus.width > 0);
    assert!(plus.right() <= header.chevron.x || header.chevron.width == 0);
    assert!(plus.x >= header.pin.right());

    let outcome = state.handle_raw_events(vec![RawInputEvent::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: plus.x,
        row: plus.y,
        modifiers: KeyModifiers::empty(),
    })]);

    assert!(state.overlay.is_none(), "the plus must not open a prompt");
    assert!(matches!(
        &outcome.actions[..],
        [ClientShellAction::Endpoint { request, .. }]
            if matches!(
                &request.method,
                crate::api::schema::Method::TabCreate(params)
                    if params.workspace_id.as_deref() == Some("ws_2")
                        && params.label.is_none()
                        && params.focus
            )
    ));
}

#[test]
fn tab_headers_carry_a_chat_pin_but_no_plus() {
    let tree = ClientTreeChrome::default();
    let mut state = tree_state(tree);
    state.set_pane_surface(surface());
    state.compose(106, 40).expect("composed frame");

    let header = state
        .hits
        .tree_headers
        .iter()
        .find(|hit| hit.tab_id.is_some())
        .expect("tab header hit");

    assert_eq!(header.plus, Rect::default());
    // Every chat row carries its pin toggle in the row's last cell pair.
    assert_eq!(header.pin, Rect::new(header.rect.right() - 2, header.rect.y, 2, 1));
}

/// Pure sidebar projection plus real mouse input: headers, shortcut ordering,
/// drag bounds, and capability-gated role changes are observable client behavior.
#[test]
fn agent_pin_section_digits_drag_boundary_and_role_menu() {
    use crossterm::event::{MouseButton, MouseEventKind};
    let mut config = ClientShellConfig::from_config(&Config::default());
    config.agent_panel_sort = crate::config::AgentPanelSortConfig::Tree;
    let mut state = ClientShellState::new(config);
    let mut snapshot = tree_snapshot();
    snapshot.pinned_tabs = vec![
        crate::protocol::ClientShellPinnedTab {
            tab_id: "tab_1".into(),
            workspace_id: "ws_1".into(),
            role: Some(crate::api::schema::TabRole::Agent),
        },
        crate::protocol::ClientShellPinnedTab {
            tab_id: "tab_2".into(),
            workspace_id: "ws_1".into(),
            role: None,
        },
    ];
    state.set_snapshot(Box::new(snapshot));
    let tree = ClientTreeChrome::default();
    assert_eq!(
        &shape(&state, &tree)[..4],
        ["agents", "pin:1:one:alpha", "pinned", "pin:2:two:alpha"]
    );
    assert_eq!(
        &state.numbered_tab_ids(state.snapshot.as_deref().unwrap())[..2],
        ["tab_1", "tab_2"]
    );
    state.compose(80, 24).unwrap();
    let (first, last) = (
        state.hits.pinned_rows[0].rect,
        state.hits.pinned_rows[1].rect,
    );
    state.handle_raw_events(vec![pin_mouse(
        MouseEventKind::Down(MouseButton::Left),
        first.x + 4,
        first.y,
    )]);
    state.handle_raw_events(vec![pin_mouse(
        MouseEventKind::Drag(MouseButton::Left),
        last.x + 4,
        last.y,
    )]);
    assert_eq!(drawn_pins(&state), ["tab_1", "tab_2"]);
    assert_eq!(
        state.endpoints[0].snapshot.as_deref().unwrap().pinned_tabs[0].tab_id,
        "tab_1"
    );
    let drop = state.handle_raw_events(vec![pin_mouse(
        MouseEventKind::Up(MouseButton::Left),
        last.x + 4,
        last.y,
    )]);
    assert!(sent_methods(&drop).is_empty());
    state.open_tab_context_menu("tab_1".into(), first.x + 4, first.y);
    let Some(ClientShellOverlay::ContextMenu(menu)) = state.overlay.as_ref() else {
        panic!("menu");
    };
    assert!(!menu
        .items()
        .iter()
        .any(|item| item.label == "Remove from agents"));
    state.endpoints[0].methods = Some(["tab.set_role".to_string()].into_iter().collect());
    state.open_tab_context_menu("tab_1".into(), first.x + 4, first.y);
    let Some(ClientShellOverlay::ContextMenu(menu)) = state.overlay.as_ref() else {
        panic!("menu");
    };
    let index = menu
        .items()
        .iter()
        .position(|item| item.label == "Remove from agents")
        .unwrap();
    let mut outcome = ClientShellInput::default();
    state.activate_context_menu_item(index, &mut outcome);
    assert!(
        matches!(sent_methods(&outcome).as_slice(), [crate::api::schema::Method::TabSetRole(params)] if params.tab_id == "tab_1" && params.role.is_none())
    );
    let mut snapshot = state.snapshot.as_deref().unwrap().clone();
    snapshot.pinned_tabs.truncate(1);
    state.set_snapshot(Box::new(snapshot));
    assert!(!shape(&state, &tree).iter().any(|entry| entry == "pinned"));
    let mut snapshot = state.snapshot.as_deref().unwrap().clone();
    snapshot.pinned_tabs[0].role = None;
    state.set_snapshot(Box::new(snapshot));
    assert!(!shape(&state, &tree).iter().any(|entry| entry == "agents"));
}

/// Pure sidebar projection guards role filtering, empty-space retention, and
/// layer-dependent rollups. The existing pin interaction test only checks the
/// leading sections, so a duplicate chat under its space would go unnoticed.
#[test]
fn agent_pin_appears_only_in_agents_while_plain_pin_stays_in_space() {
    let tree = ClientTreeChrome::default();
    let mut state = tree_state(tree.clone());
    let mut snapshot = tree_snapshot();
    snapshot.pinned_tabs = vec![
        crate::protocol::ClientShellPinnedTab {
            tab_id: "tab_1".into(),
            workspace_id: "ws_1".into(),
            role: Some(crate::api::schema::TabRole::Agent),
        },
        crate::protocol::ClientShellPinnedTab {
            tab_id: "tab_2".into(),
            workspace_id: "ws_1".into(),
            role: None,
        },
    ];
    snapshot.agents[0].agent_status = AgentStatus::Working;
    snapshot.agents[1].agent_status = AgentStatus::Idle;
    state.set_snapshot(Box::new(snapshot));
    assert_eq!(
        shape(&state, &tree),
        [
            "agents",
            "pin:1:one:alpha",
            "pinned",
            "pin:2:two:alpha",
            "space:alpha",
            "tab:two",
            "space:beta",
            "tab:three",
        ]
    );
    let snapshot = state.snapshot.as_deref().expect("snapshot");
    assert!(state.focused_space_numbered_tab_ids(snapshot).is_empty());
    assert_eq!(state.numbered_tab_ids(snapshot), ["tab_1", "tab_2"]);

    let mut hidden_layers = tree.clone();
    hidden_layers.show_tabs = false;
    hidden_layers.show_agents = false;
    let entries = panel_entries(&state, &hidden_layers);
    let header = entries
        .iter()
        .find_map(|entry| match entry {
            AgentPanelListEntry::SpaceHeader(header) if header.workspace_id == "ws_1" => {
                Some(header)
            }
            _ => None,
        })
        .expect("space header");
    assert_eq!(header.child_states, [AgentStatus::Idle]);
    assert!(entries.iter().any(|entry| matches!(entry,
        AgentPanelListEntry::PinnedTab(row) if row.tab_id == "tab_1" && row.status == AgentStatus::Working
    )));

    // With no ordinary chats left, even an agentless agent pin keeps its space.
    let mut snapshot = snapshot.clone();
    snapshot.tabs.retain(|tab| tab.tab_id != "tab_2");
    snapshot.agents.retain(|agent| agent.workspace_id != "ws_1");
    snapshot.pinned_tabs.truncate(1);
    state.set_snapshot(Box::new(snapshot));
    assert_eq!(
        shape(&state, &tree),
        [
            "agents",
            "pin:1:one:alpha",
            "space:alpha",
            "space:beta",
            "tab:three",
        ]
    );
}
