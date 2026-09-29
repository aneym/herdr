use super::*;
use crate::client::shell::tree::ClientTreeChrome;
use crate::factory_overlay::{Attention, FactoryOverlay, HostRow, SpaceTag, TabKind, TabTag};

fn fixture() -> (ClientShellSnapshot, FactoryOverlay) {
    let mut snapshot = snapshot();
    snapshot.tabs.clear();
    snapshot.agents.clear();
    snapshot.panes.clear();
    snapshot.focused_tab_id = Some("other".into());
    for (index, label) in [
        "orch", "lane-a", "wf-a", "wf-b", "lane-b", "orphan", "advisor", "done", "plain-a",
        "plain-b",
    ]
    .iter()
    .enumerate()
    {
        snapshot.tabs.push(ClientShellTab {
            tab_id: label.to_string(),
            workspace_id: "ws_1".into(),
            number: index + 1,
            label: label.to_string(),
            custom_label: true,
            zoomed: false,
            focused: false,
            agent_status: AgentStatus::Working,
        });
    }
    snapshot.agents.push(ClientShellAgent {
        pane_id: "plain-pane".into(),
        workspace_id: "ws_1".into(),
        tab_id: "plain-a".into(),
        name: Some("plain agent".into()),
        display_agent: Some("plain agent".into()),
        agent: None,
        title: None,
        terminal_title: None,
        terminal_title_stripped: None,
        agent_status: AgentStatus::Idle,
        state_change_seq: 1,
        state_labels: Vec::new(),
        tokens: Vec::new(),
        focused: false,
        owner_pane_id: None,
        orphaned: false,
        group: Default::default(),
        visible_in_profile: true,
    });
    let mut overlay = FactoryOverlay::default();
    for (id, kind, parent) in [
        ("orch", TabKind::Orchestrator, None),
        ("lane-a", TabKind::Lane, None),
        ("wf-a", TabKind::Workflow, Some("lane-a")),
        ("wf-b", TabKind::Workflow, Some("lane-a")),
        ("lane-b", TabKind::Lane, None),
        ("orphan", TabKind::Workflow, Some("missing")),
        ("advisor", TabKind::Advisor, None),
        ("done", TabKind::Workflow, None),
    ] {
        overlay.tabs.insert(
            id.into(),
            TabTag {
                kind,
                parent: parent.map(str::to_string),
                ..TabTag::default()
            },
        );
    }
    overlay.tabs.get_mut("orch").unwrap().summary = Some("inbox 3".into());
    overlay.tabs.get_mut("lane-a").unwrap().summary = Some("pending".into());
    overlay.tabs.get_mut("lane-b").unwrap().idle = true;
    overlay.tabs.get_mut("wf-a").unwrap().badge = Some("PC".into());
    overlay.tabs.get_mut("wf-a").unwrap().phase = Some("review 3/5".into());
    overlay.tabs.get_mut("done").unwrap().done = true;
    (snapshot, overlay)
}


fn factory_state(snapshot: ClientShellSnapshot, overlay: FactoryOverlay) -> ClientShellState {
    let mut config = ClientShellConfig::from_config(&Config::default());
    config.factory.enabled = true;
    let mut state = ClientShellState::new(config);
    state.set_snapshot(Box::new(snapshot));
    state.factory_overlay = Some(std::sync::Arc::new(overlay));
    state
}


fn rendered_factory_rows(snapshot: &ClientShellSnapshot, overlay: &FactoryOverlay) -> (Vec<String>, ShellHitMap, Buffer) {
    rendered_factory_rows_with_tree(snapshot, overlay, &ClientTreeChrome::default())
}

fn rendered_factory_rows_with_tree(snapshot: &ClientShellSnapshot, overlay: &FactoryOverlay, tree: &ClientTreeChrome) -> (Vec<String>, ShellHitMap, Buffer) {
    rendered_factory_rows_at_width(snapshot, overlay, tree, 25)
}

fn rendered_factory_rows_at_width(snapshot: &ClientShellSnapshot, overlay: &FactoryOverlay, tree: &ClientTreeChrome, width: u16) -> (Vec<String>, ShellHitMap, Buffer) {
    rendered_factory_rows_with_gap(snapshot, overlay, tree, width, 1)
}

fn rendered_factory_rows_with_gap(snapshot: &ClientShellSnapshot, overlay: &FactoryOverlay, tree: &ClientTreeChrome, width: u16, gap: u16) -> (Vec<String>, ShellHitMap, Buffer) {
    let area = Rect::new(0, 0, width, 60);
    let mut buffer = Buffer::empty(area);
    let mut config = ClientShellConfig::from_config(&Config::default());
    config.agent_panel_sort = crate::config::AgentPanelSortConfig::Tree;
    config.agents.row_gap = gap;
    config.factory.enabled = true;
    let mut hits = ShellHitMap::default();
    let mut scroll = 0;
    crate::client::shell::agent_sidebar::render_agent_panel_with_overlay(
        &mut buffer, area, snapshot, &config, tree,
        Some(overlay), &mut scroll, &mut hits,
    );
    let rows = (0..area.height)
        .map(|y| (0..area.width).map(|x| buffer[(x, y)].symbol().to_owned()).collect())
        .collect();
    (rows, hits, buffer)
}


fn factory_click(state: &mut ClientShellState, kind: MouseEventKind, x: u16, y: u16) -> ClientShellInput {
    state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
        kind, column: x, row: y, modifiers: KeyModifiers::NONE,
    })])
}

fn focused_tab(input: &ClientShellInput) -> Vec<String> {
    input.actions.iter().filter_map(|action| match action {
        ClientShellAction::Endpoint { request, .. } => match &request.method {
            crate::api::schema::Method::TabFocus(target) => Some(target.tab_id.clone()),
            _ => None,
        },
        _ => None,
    }).collect()
}


#[test]
fn focused_agent_half_pad_does_not_overlap_next_factory_space() {
    let (mut snapshot, mut overlay) = fixture();
    snapshot.tabs.retain(|tab| tab.tab_id == "plain-a");
    overlay.tabs.clear();
    snapshot.focused_tab_id = Some("plain-a".into());
    snapshot.tabs.iter_mut().find(|tab| tab.tab_id == "plain-a").unwrap().focused = true;
    let mut extra = snapshot.agents[0].clone();
    extra.pane_id = "focused-pane".into();
    extra.focused = true;
    snapshot.agents.push(extra);
    snapshot.workspaces.push(ClientShellWorkspace {
        workspace_id: "ws_2".into(), active_tab_id: "other-tab".into(),
        new_workspace_cwd: String::new(), number: 2, label: "poker".into(),
        custom_label: true, branch: None, git_ahead_behind: None, tokens: Vec::new(),
        worktree: None, focused: false, agent_status: AgentStatus::Idle,
        orchestrator_mode: false, tab_count: 1, visible_in_profile: true,
    });
    snapshot.tabs.push(ClientShellTab {
        tab_id: "other-tab".into(), workspace_id: "ws_2".into(), number: 1,
        label: "other".into(), custom_label: true, zoomed: false, focused: false,
        agent_status: AgentStatus::Idle,
    });
    overlay.tabs.insert("other-tab".into(), TabTag { kind: TabKind::Lane, ..TabTag::default() });
    let (rows, hits, buffer) = rendered_factory_rows(&snapshot, &overlay);
    let palette = ClientShellConfig::from_config(&Config::default()).palette;
    let focused = hits.agents.iter().find(|(_, pane)| pane == "focused-pane").unwrap().0;
    let next = rows.iter().position(|row| row.contains("poker")).unwrap() as u16;
    assert_eq!(next, focused.bottom(), "focused Agent row must directly precede factory space header: focused={focused:?}, next={next}, rows={rows:?}");

    for y in focused.y..focused.bottom() {
        if y == focused.bottom() - 1 {
            assert!((0..25).all(|x| buffer[(x, y)].bg != palette.active_row_bg),
                "ordinary Agent spacer must retain its default background");
        } else {
            assert!((0..25).all(|x| buffer[(x, y)].bg == palette.active_row_bg), "focused Agent row {y} must be highlighted");
        }
    }
    assert!((0..25).all(|x| {
        let cell = &buffer[(x, next)];
        !matches!(cell.symbol(), "▀" | "▄")
            && cell.fg != palette.active_row_bg && cell.bg != palette.active_row_bg
    }), "next factory space header {next} must not inherit half-pad or active highlight");
}

#[test]
fn ordinary_focused_agent_keeps_half_pad_in_unfilled_gap() {
    let (mut snapshot, _) = fixture();
    snapshot.tabs.retain(|tab| matches!(tab.tab_id.as_str(), "plain-a" | "plain-b"));
    let mut extra = snapshot.agents[0].clone();
    extra.pane_id = "focused-pane".into();
    extra.focused = true;
    snapshot.agents.push(extra.clone());
    extra.pane_id = "following-pane".into();
    extra.focused = false;
    snapshot.agents.push(extra);
    let mut config = ClientShellConfig::from_config(&Config::default());
    config.agent_panel_sort = crate::config::AgentPanelSortConfig::Tree;
    config.agents.row_gap = 1;
    let area = Rect::new(0, 0, 25, 35);
    let mut buffer = Buffer::empty(area);
    let mut hits = ShellHitMap::default();
    let mut scroll = 0;
    crate::client::shell::agent_sidebar::render_agent_panel_with_overlay(
        &mut buffer, area, &snapshot, &config, &ClientTreeChrome::default(),
        None, &mut scroll, &mut hits,
    );
    let rect = hits.agents.iter().find(|(_, pane)| pane == "focused-pane").unwrap().0;
    let gap_y = rect.bottom() - 1;
    assert!(rect.height > 1 && gap_y < area.bottom());
    assert!(rect.y > area.y && (0..area.width).any(|x| buffer[(x, rect.y - 1)].symbol() == "▄"),
        "ordinary focused row keeps its upper half-pad");
    assert!((0..area.width).all(|x| buffer[(x, gap_y)].bg != config.palette.active_row_bg),
        "ordinary gap must not receive a full-row highlight");
    assert!((0..area.width).any(|x| buffer[(x, gap_y)].symbol() == "▀"),
        "ordinary focused row keeps its lower half-pad");
    let next_y = gap_y + 1;
    assert!((0..area.width).all(|x| buffer[(x, next_y)].bg != config.palette.active_row_bg),
        "the following ordinary row must not inherit a full-row highlight");
}


#[test]
fn factory_tab_strip_omits_workflows_but_preserves_other_tabs_and_click_targets() {
    let (mut snapshot, overlay) = fixture();
    snapshot.tabs.retain(|tab| matches!(tab.tab_id.as_str(), "orch" | "wf-a" | "plain-a"));
    let mut state = factory_state(snapshot.clone(), overlay.clone());
    state.set_pane_surface(surface());
    let frame = state.compose(120, 40).expect("composed factory strip");
    let strip = frame_rows(&frame)[0].clone();
    let visible = state.hits.tabs.iter().map(|(_, id)| id.as_str()).collect::<Vec<_>>();
    assert_eq!(visible, ["orch", "plain-a"]);
    assert!(strip.contains("orch") && strip.contains("plain-a"), "{strip:?}");
    assert!(!strip.contains("wf-a"), "{strip:?}");
    let shortcut = |state: &mut ClientShellState, index| {
        let mut outcome = ClientShellInput::default();
        state.record_binding(crate::input::KeybindMatch::Action(
            crate::input::KeybindAction::SwitchTab(index)), &mut outcome);
        focused_tab(&outcome)
    };
    assert_eq!(shortcut(&mut state, 1), ["plain-a"]);
    assert!(shortcut(&mut state, 2).is_empty());
    let target = state.hits.tabs.iter().find(|(_, id)| id == "plain-a").unwrap().0;
    let x = target.x + 1;
    let y = target.y;
    factory_click(&mut state, MouseEventKind::Down(MouseButton::Left), x, y);
    let focus = factory_click(&mut state, MouseEventKind::Up(MouseButton::Left), x, y);
    assert_eq!(focused_tab(&focus), ["plain-a"]);

    // Off: the identical snapshot shows workflow tabs in their original order.
    let mut config = ClientShellConfig::from_config(&Config::default());
    config.factory.enabled = false;
    let mut off = ClientShellState::new(config);
    off.set_snapshot(Box::new(snapshot));
    off.factory_overlay = Some(std::sync::Arc::new(overlay));
    off.set_pane_surface(surface());
    off.compose(120, 40).expect("composed ordinary strip");
    assert!(off.hits.tabs.iter().any(|(_, id)| id == "wf-a"));
}


// The three frames share a lab-like fixture. Keep the ages a half-minute away
// from a boundary so a slow CI machine does not change the displayed minute.
fn lab_fixture() -> (ClientShellSnapshot, FactoryOverlay) {
    let (mut snapshot, mut overlay) = fixture();
    snapshot.workspaces[0].label = "agent-rails".into();
    snapshot.workspaces.push(ClientShellWorkspace {
        workspace_id: "ws_2".into(), active_tab_id: "poker".into(),
        new_workspace_cwd: String::new(), number: 2, label: "poker".into(),
        custom_label: true, branch: None, git_ahead_behind: None, tokens: Vec::new(),
        worktree: None, focused: false, agent_status: AgentStatus::Idle,
        orchestrator_mode: false, tab_count: 1, visible_in_profile: true,
    });
    snapshot.tabs.push(ClientShellTab {
        tab_id: "poker".into(), workspace_id: "ws_2".into(), number: 1,
        label: "poker coach".into(), custom_label: true, zoomed: false,
        focused: false, agent_status: AgentStatus::Working,
    });
    overlay.tabs.insert("poker".into(), TabTag { kind: TabKind::Lane, ..TabTag::default() });
    overlay.hosts.push(HostRow { name: "Studio".into(), summary: Some("3/28 live".into()), attention: Attention::None });
    overlay.spaces.insert("ws_1".into(), SpaceTag {
        attention: Attention::Act, target_tab: Some("wf-a".into()), summary: Some("3".into()),
    });
    overlay.tabs.get_mut("wf-a").unwrap().name = Some("wf issues 3".into());
    overlay.tabs.get_mut("wf-a").unwrap().badge = None;
    overlay.tabs.get_mut("wf-a").unwrap().phase = Some("decide 59/62".into());
    overlay.tabs.get_mut("wf-b").unwrap().name = Some("factory-infra".into());
    overlay.tabs.get_mut("wf-b").unwrap().badge = Some("Studio".into());
    overlay.tabs.get_mut("wf-b").unwrap().phase = Some("build 1/2".into());
    overlay.tabs.get_mut("orphan").unwrap().phase = None; // no stage, no progress line
    overlay.tabs.get_mut("orphan").unwrap().started = Some(1); // no bare age
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    overlay.tabs.get_mut("wf-a").unwrap().started = Some((now - (8 * 60 + 57) * 60 - 30) as i64);
    (snapshot, overlay)
}

fn assert_golden(actual: Vec<String>, expected: &str) {
    // Rows after the visible content are empty buffer capacity, not part of the frame.
    let actual = actual.join("\n");
    let expected = expected.trim_end_matches('\n');
    if actual != expected {
        let got = actual.lines().collect::<Vec<_>>();
        let want = expected.lines().collect::<Vec<_>>();
        let differences = (0..got.len().max(want.len()))
            .filter(|&i| got.get(i) != want.get(i))
            .map(|i| format!("row {i}: expected {:?}\n          actual {:?}", want.get(i), got.get(i)))
            .collect::<Vec<_>>().join("\n");
        panic!("golden frame mismatch:\n{differences}\nACTUAL FRAME: {actual:?}");
    }
}

#[test]
fn factory_default_frame_w25() {
    let (snapshot, overlay) = lab_fixture();
    let (rows, _, _) = rendered_factory_rows(&snapshot, &overlay);
    assert_golden(rows, include_str!("golden/factory_default_w25.txt"));
}

#[test]
fn factory_expanded_frame_w27() {
    let (snapshot, overlay) = lab_fixture();
    let mut tree = ClientTreeChrome::default();
    tree.factory_expanded_lanes.insert("lane-a".into());
    let (rows, _, _) = rendered_factory_rows_at_width(&snapshot, &overlay, &tree, 27);
    assert_golden(rows, include_str!("golden/factory_expanded_w27.txt"));
}

#[test]
fn factory_compact_frame_w25() {
    let (snapshot, overlay) = lab_fixture();
    let (rows, _, _) = rendered_factory_rows_with_gap(&snapshot, &overlay, &ClientTreeChrome::default(), 25, 0);
    assert_golden(rows, include_str!("golden/factory_compact_w25.txt"));
}

#[test]
fn factory_click_table() {
    let (snapshot, overlay) = lab_fixture();
    let mut tree = ClientTreeChrome::default();
    tree.factory_expanded_lanes.insert("lane-a".into());
    let (rows, hits, _) = rendered_factory_rows_with_tree(&snapshot, &overlay, &tree);
    let item = |id: &str| hits.tree_headers.iter()
        .find(|hit| hit.tab_id.as_deref() == Some(id)).expect("click target");
    let lane = item("lane-a");
    let workflow = item("wf-a");
    let untagged = item("plain-b");
    let point = |hit: &crate::client::shell::state::TreeHeaderHit| (hit.rect.x + 9, hit.rect.y);
    let lane_name = point(lane);
    let workflow_name = point(workflow);
    let untagged_name = point(untagged);
    let chevron = (lane.chevron.x, lane.chevron.y);
    let chevron_end = (lane.chevron.x + 1, lane.chevron.y);
    let glyph = (lane.chevron.x + 2, lane.chevron.y);
    let gap = (lane_name.0, lane_name.1 + 1);
    let progress = (workflow_name.0, workflow_name.1 + 1);
    assert!(rows[progress.1 as usize].contains("59/62"), "{rows:?}");
    // (description, press cell, release cell, modifiers, focused tab, toggle, panel)
    let cases = [
        ("lane name", lane_name, lane_name, KeyModifiers::NONE, Some("lane-a"), false, false),
        ("status glyph", glyph, glyph, KeyModifiers::NONE, Some("lane-a"), false, false),
        ("nested workflow", workflow_name, workflow_name, KeyModifiers::NONE, Some("wf-a"), false, false),
        ("progress line", progress, progress, KeyModifiers::NONE, Some("wf-a"), false, false),
        ("chevron first cell", chevron, chevron, KeyModifiers::NONE, None, true, false),
        ("chevron second cell", chevron_end, chevron_end, KeyModifiers::NONE, None, true, false),
        ("after chevron", glyph, glyph, KeyModifiers::NONE, Some("lane-a"), false, false),
        ("gap row", gap, gap, KeyModifiers::NONE, Some("lane-a"), false, false),
        ("release on own gap", lane_name, gap, KeyModifiers::NONE, Some("lane-a"), false, false),
        ("ten column drift", lane_name, (lane_name.0 + 10, lane_name.1), KeyModifiers::NONE, Some("lane-a"), false, false),
        ("adjacent item", lane_name, workflow_name, KeyModifiers::NONE, None, false, false),
        ("alt click", lane_name, lane_name, KeyModifiers::ALT, None, false, true),
        ("untagged lane", untagged_name, untagged_name, KeyModifiers::NONE, Some("plain-b"), false, false),
    ];
    for (description, press, release, modifiers, focus, toggle, panel) in cases {
        let mut state = factory_state(snapshot.clone(), overlay.clone());
        state.config.agent_panel_sort = crate::config::AgentPanelSortConfig::Tree;
        state.config.agents.row_gap = 1;
        state.tree_chrome_mut().factory_expanded_lanes.insert("lane-a".into());
        state.hits = rendered_factory_rows_with_tree(&snapshot, &overlay, &tree).1;
        state.last_composed_size = Some((120, 60));
        let click = |kind, (column, row)| RawInputEvent::Mouse(crossterm::event::MouseEvent {
            kind, column, row, modifiers,
        });
        let outcome = state.handle_raw_events(vec![
            click(MouseEventKind::Down(MouseButton::Left), press),
            click(MouseEventKind::Up(MouseButton::Left), release),
        ]);
        let focused = focused_tab(&outcome);
        assert_eq!(focused.as_slice(), focus.into_iter().collect::<Vec<_>>(), "{description}");
        assert_eq!(!state.tree_chrome_mut().factory_expanded_lanes.contains("lane-a"), toggle, "{description}");
        assert_eq!(state.detail_panel.is_some(), panel, "{description}");
        if panel {
            let detail = state.detail_panel.as_ref().unwrap();
            assert_eq!(detail.key, "tab:lane-a", "{description}");
            assert!(!detail.focused, "{description}: detail must not steal focus");
        }
    }
}

#[test]
fn factory_status_glyph_colors() {
    let (mut snapshot, mut overlay) = fixture();
    let agent_template = snapshot.agents[0].clone();
    snapshot.agents.clear();
    for (name, status) in [("done-pane", AgentStatus::Done), ("working-pane", AgentStatus::Working)] {
        let mut agent = agent_template.clone();
        agent.pane_id = name.into();
        agent.tab_id = "lane-a".into();
        agent.agent_status = status;
        snapshot.agents.push(agent);
    }
    overlay.tabs.get_mut("orch").unwrap().attention = Attention::Act;
    snapshot.tabs.iter_mut().find(|tab| tab.tab_id == "lane-b").unwrap().agent_status = AgentStatus::Blocked;
    overlay.tabs.get_mut("lane-b").unwrap().idle = false;
    let (rows, _, buffer) = rendered_factory_rows(&snapshot, &overlay);
    let palette = ClientShellConfig::from_config(&Config::default()).palette;
    let glyph_color = |name: &str| {
        let y = rows.iter().position(|row| row.contains(name)).unwrap() as u16;
        let x = rows[y as usize].chars().position(|ch| ch == '●').unwrap() as u16;
        buffer[(x, y)].fg
    };
    assert_eq!(glyph_color("lane-a"), palette.working, "working pane must outrank done pane");
    assert_eq!(glyph_color("lane-b"), palette.red, "blocked lane");
    assert_eq!(glyph_color("orch"), palette.red, "Attention::Act must show on the row");
}
