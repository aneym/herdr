use super::*;
use crate::client::shell::tree::ClientTreeChrome;
use crate::factory_overlay::{Attention, FactoryOverlay, HostRow, RunTag, SpaceTag, TabKind, TabMode, TabTag};

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
    snapshot.tabs.iter_mut().find(|tab| tab.tab_id == "lane-b").unwrap().agent_status = AgentStatus::Idle;
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
    overlay.tabs.get_mut("orch").unwrap().name = Some("orchestrator".into());
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
    overlay.tabs.get_mut("wf-a").unwrap().name = Some("issues 3".into());
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

    let (snapshot, mut overlay) = lab_fixture();
    overlay.tabs.get_mut("wf-a").unwrap().name = Some("wf issues 3".into());
    overlay.tabs.get_mut("wf-a").unwrap().phase = Some("decide 99/101".into());
    let (rows, _, _) = rendered_factory_rows_at_width(&snapshot, &overlay, &tree, 23);
    let workflow = rows.iter().position(|row| row.contains("◐ issues 3")).unwrap();
    assert!(rows[workflow + 1].contains("99/101 · 8h57m"), "{}", rows[workflow + 1]);
}

#[test]
fn factory_compact_frame_w25() {
    let (snapshot, overlay) = lab_fixture();
    let (rows, _, _) = rendered_factory_rows_with_gap(&snapshot, &overlay, &ClientTreeChrome::default(), 25, 0);
    assert_golden(rows, include_str!("golden/factory_compact_w25.txt"));
}

#[test]
fn compact_factory_spaces_have_one_boundary_row_without_section_gaps() {
    let (snapshot, overlay) = lab_fixture();
    let (rows, hits, _) = rendered_factory_rows_with_gap(
        &snapshot, &overlay, &ClientTreeChrome::default(), 25, 0,
    );
    let header = |key: &str| hits.tree_headers.iter().find(|hit| hit.key == key).unwrap().rect.y as usize;
    let first = header("ws_1");
    let orchestrator = rows.iter().position(|row| row.contains("ORCHESTRATOR")).unwrap();
    let lanes = rows.iter().position(|row| row.contains("LANES")).unwrap();
    let background = header("factory-background:ws_1");
    let second = header("ws_2");
    assert_eq!(orchestrator, first + 1, "no gap after space header");
    assert!(rows[lanes - 1].contains("orchestrator"), "no gap between sections");
    assert!(rows[background - 1].trim().is_empty(), "gap before background");
    assert!(rows[background - 2].contains("plain-b"), "one gap before background");
    assert!(rows[second - 1].trim().is_empty(), "gap between spaces");
    assert_eq!(second, background + 2, "exactly one gap between spaces");
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
    let orchestrator = item("orch");
    assert!(!orchestrator.chevron.is_empty());
    assert!(orchestrator.collapsed);
    assert!(!rows.iter().any(|row| row.contains("orphan")), "{rows:?}");
    let workflow = item("wf-a");
    let untagged = item("plain-b");
    let point = |hit: &crate::client::shell::state::TreeHeaderHit| (hit.rect.x + 9, hit.rect.y);
    let lane_name = point(lane);
    let workflow_name = point(workflow);
    let untagged_name = point(untagged);
    let chevron = (lane.chevron.x, lane.chevron.y);
    let orchestrator_chevron = (orchestrator.chevron.x, orchestrator.chevron.y);
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

    let mut state = factory_state(snapshot.clone(), overlay.clone());
    state.config.agent_panel_sort = crate::config::AgentPanelSortConfig::Tree;
    state.hits = hits;
    state.last_composed_size = Some((120, 60));
    for (tab, at, visible) in [
        ("orch", orchestrator_chevron, true),
        ("orch", orchestrator_chevron, false),
    ] {
        factory_click(&mut state, MouseEventKind::Down(MouseButton::Left), at.0, at.1);
        factory_click(&mut state, MouseEventKind::Up(MouseButton::Left), at.0, at.1);
        let (rendered, new_hits, _) = rendered_factory_rows_with_tree(&snapshot, &overlay, state.tree_chrome_mut());
        assert_eq!(rendered.iter().any(|row| row.contains("orphan")), visible, "{tab} after chevron click");
        assert_eq!(new_hits.tree_headers.iter().find(|hit| hit.tab_id.as_deref() == Some(tab)).unwrap().collapsed, !visible);
        state.hits = new_hits;
    }

    let mut focused_snapshot = snapshot.clone();
    focused_snapshot.focused_tab_id = Some("wf-a".into());
    let mut focused_state = factory_state(focused_snapshot.clone(), overlay.clone());
    focused_state.config.agent_panel_sort = crate::config::AgentPanelSortConfig::Tree;
    let (visible, hits, _) = rendered_factory_rows(&focused_snapshot, &overlay);
    assert!(visible.iter().any(|row| row.contains("issues 3")));
    let hit = hits.tree_headers.iter().find(|hit| hit.tab_id.as_deref() == Some("lane-a")).unwrap();
    let at = (hit.chevron.x, hit.chevron.y);
    focused_state.hits = hits;
    focused_state.last_composed_size = Some((120, 60));
    factory_click(&mut focused_state, MouseEventKind::Down(MouseButton::Left), at.0, at.1);
    factory_click(&mut focused_state, MouseEventKind::Up(MouseButton::Left), at.0, at.1);
    for child in ["wf-a", "wf-b"] {
        focused_snapshot.focused_tab_id = Some(child.into());
        let (rendered, _, _) = rendered_factory_rows_with_tree(&focused_snapshot, &overlay, focused_state.tree_chrome_mut());
        assert!(!rendered.iter().any(|row| row.contains("issues 3") || row.contains("factory-infra")),
            "explicit fold must survive focus on {child}");
    }

    let mut alert_overlay = overlay.clone();
    alert_overlay.tabs.get_mut("wf-a").unwrap().attention = Attention::Act;
    let (visible, hits, _) = rendered_factory_rows(&snapshot, &alert_overlay);
    assert!(visible.iter().any(|row| row.contains("issues 3")), "Act child auto-opens lane");
    let hit = hits.tree_headers.iter().find(|hit| hit.tab_id.as_deref() == Some("lane-a")).unwrap();
    let at = (hit.chevron.x, hit.chevron.y);
    let mut alert_state = factory_state(snapshot.clone(), alert_overlay.clone());
    alert_state.config.agent_panel_sort = crate::config::AgentPanelSortConfig::Tree;
    alert_state.hits = hits;
    alert_state.last_composed_size = Some((120, 60));
    factory_click(&mut alert_state, MouseEventKind::Down(MouseButton::Left), at.0, at.1);
    factory_click(&mut alert_state, MouseEventKind::Up(MouseButton::Left), at.0, at.1);
    let (folded, hits, buffer) = rendered_factory_rows_with_tree(&snapshot, &alert_overlay, alert_state.tree_chrome_mut());
    assert!(!folded.iter().any(|row| row.contains("issues 3")), "explicit fold must beat Act");
    let y = hits.tree_headers.iter().find(|hit| hit.key == "lane-a").unwrap().rect.y;
    let lane = &folded[y as usize];
    assert!(lane.ends_with('!'), "Act workflow must mark folded lane: {lane}");
    assert_eq!(buffer[(24, y)].fg, alert_state.config.palette.red);
}

#[test]
fn factory_explicit_folds_survive_preferences_roundtrip() {
    let (mut snapshot, mut overlay) = fixture();
    snapshot.focused_tab_id = Some("wf-a".into());
    overlay.tabs.get_mut("orphan").unwrap().attention = Attention::Act;
    let mut tree = ClientTreeChrome::default();
    tree.factory_collapsed_lanes.extend(["lane-a".into(), "orch".into()]);
    let before = rendered_factory_rows_with_tree(&snapshot, &overlay, &tree).0;
    assert!(!before.iter().any(|row| row.contains("orphan") || row.contains("wf-a")));
    let saved = tree.to_preferences();
    assert_eq!(saved.factory_collapsed_lanes, ["lane-a", "orch"]);
    let restored = ClientTreeChrome::from_preferences(saved);
    assert_eq!(rendered_factory_rows_with_tree(&snapshot, &overlay, &restored).0, before);
}

#[test]
fn factory_parent_with_running_workflow_or_busy_tag_shows_working() {
    let (mut snapshot, mut overlay) = fixture();
    let palette = ClientShellConfig::from_config(&Config::default()).palette;
    for tab in &mut snapshot.tabs {
        if tab.tab_id == "lane-a" || tab.tab_id == "orch" {
            tab.agent_status = AgentStatus::Idle;
        }
    }
    overlay.tabs.get_mut("lane-a").unwrap().idle = true;
    overlay.tabs.get_mut("orch").unwrap().idle = true;
    let glyph_color = |snapshot: &ClientShellSnapshot, overlay: &FactoryOverlay, id: &str| {
        let (rows, hits, buffer) = rendered_factory_rows(snapshot, overlay);
        let y = hits.tree_headers.iter().find(|hit| hit.tab_id.as_deref() == Some(id)).unwrap().rect.y;
        let x = rows[y as usize].chars().position(|ch| ch == '●' || ch == '○').unwrap() as u16;
        (buffer[(x, y)].fg, rows[y as usize].clone())
    };
    for id in ["lane-a", "orch"] {
        let (color, row) = glyph_color(&snapshot, &overlay, id);
        assert_eq!(color, palette.working, "{id} with running workflow: {row}");
        assert!(!row.contains("idle"), "{row}");
    }
    overlay.tabs.get_mut("wf-b").unwrap().parent = Some("orch".into());
    overlay.tabs.get_mut("orch").unwrap().name = Some("orchestrator".into());
    let mut folded = ClientTreeChrome::default();
    folded.factory_collapsed_lanes.insert("orch".into());
    let (wide, _, _) = rendered_factory_rows_at_width(&snapshot, &overlay, &folded, 35);
    assert!(wide.iter().any(|row| row.contains("orchestrator") && row.contains("2 · inbox 3")), "{wide:?}");
    let (narrow, _, _) = rendered_factory_rows_with_tree(&snapshot, &overlay, &folded);
    assert!(narrow.iter().any(|row| row.contains("orchestrator") && row.trim_end().ends_with('2')), "{narrow:?}");
    snapshot.tabs.retain(|tab| !matches!(tab.tab_id.as_str(), "wf-a" | "wf-b" | "orphan"));
    overlay.tabs.get_mut("lane-a").unwrap().summary = None;
    let (empty, _, _) = rendered_factory_rows_with_tree(&snapshot, &overlay, &folded);
    assert!(empty.iter().any(|row| row.contains("inbox 3")), "{empty:?}");
    for id in ["lane-a", "orch"] {
        let (color, row) = glyph_color(&snapshot, &overlay, id);
        assert_eq!(color, palette.overlay0, "{id} with no running workflow: {row}");
        assert!(row.contains(if id == "orch" { "inbox 3" } else { "idle" }), "{row}");
        overlay.tabs.get_mut(id).unwrap().busy = true;
        let (color, row) = glyph_color(&snapshot, &overlay, id);
        assert_eq!(color, palette.working, "{id} tagged busy: {row}");
        assert!(!row.contains("idle"), "{row}");
    }
}

#[test]
fn lane_idle_follows_live_pane_instead_of_delayed_overlay_flag() {
    let (mut snapshot, mut overlay) = fixture();
    let palette = ClientShellConfig::from_config(&Config::default()).palette;
    let agent = snapshot.agents[0].clone();
    let check = |snapshot: &ClientShellSnapshot, overlay: &FactoryOverlay, id: &str,
                 glyph: char, color: ratatui::style::Color, idle: bool| {
        let (rows, hits, buffer) = rendered_factory_rows(snapshot, overlay);
        let y = hits.tree_headers.iter().find(|hit| hit.tab_id.as_deref() == Some(id)).unwrap().rect.y;
        let row = &rows[y as usize];
        let x = row.chars().position(|ch| ch == '●' || ch == '○').unwrap() as u16;
        assert_eq!(row.chars().nth(x as usize), Some(glyph), "{id}: {row}");
        assert_eq!(buffer[(x, y)].fg, color, "{id}: {row}");
        assert_eq!(row.contains("idle"), idle, "{id}: {row}");
    };
    snapshot.tabs.retain(|tab| tab.tab_id == "lane-b");
    snapshot.agents = vec![agent];
    snapshot.agents[0].tab_id = "lane-b".into();
    overlay.tabs.get_mut("lane-b").unwrap().idle = false;
    snapshot.agents[0].agent_status = AgentStatus::Done;
    check(&snapshot, &overlay, "lane-b", '●', palette.green, false);
    // Only the live pane changes when the client acknowledges the unread result.
    snapshot.agents[0].agent_status = AgentStatus::Idle;
    check(&snapshot, &overlay, "lane-b", '○', palette.overlay0, true);

    overlay.tabs.get_mut("lane-b").unwrap().idle = true;
    snapshot.agents[0].agent_status = AgentStatus::Working;
    check(&snapshot, &overlay, "lane-b", '●', palette.working, false);
    snapshot.agents[0].agent_status = AgentStatus::Idle;
    overlay.tabs.get_mut("lane-b").unwrap().busy = true;
    check(&snapshot, &overlay, "lane-b", '●', palette.working, false);
    overlay.tabs.get_mut("lane-b").unwrap().busy = false;
    overlay.tabs.get_mut("lane-b").unwrap().summary = Some("1 wf".into());
    check(&snapshot, &overlay, "lane-b", '●', palette.overlay0, false);

    snapshot.tabs[0].tab_id = "orch".into();
    snapshot.agents[0].tab_id = "orch".into();
    overlay.tabs.get_mut("orch").unwrap().summary = None;
    overlay.tabs.get_mut("orch").unwrap().idle = true;
    check(&snapshot, &overlay, "orch", '●', palette.overlay0, false);
}

#[test]
fn busy_lane_syncs_tab_and_sidebar_and_preserves_shell_status() {
    let (mut snapshot, mut overlay) = fixture();
    snapshot.tabs.retain(|tab| tab.tab_id == "lane-b");
    snapshot.tabs[0].agent_status = AgentStatus::Idle;
    let mut idle_agent = snapshot.agents[0].clone();
    idle_agent.tab_id = "lane-b".into();
    idle_agent.agent_status = AgentStatus::Idle;
    snapshot.agents = vec![idle_agent.clone()];
    snapshot.panes = vec![ClientShellPane {
        pane_id: idle_agent.pane_id.clone(), workspace_id: "ws_1".into(), tab_id: "lane-b".into(),
        label: None, cwd: None, foreground_cwd: None, focused: false, right_click_passthrough: false,
    }];
    let palette = ClientShellConfig::from_config(&Config::default()).palette;
    let check = |snapshot: &ClientShellSnapshot, overlay: &FactoryOverlay, expected, glyph| {
        let mut state = factory_state(snapshot.clone(), overlay.clone());
        state.config.show_tab_status = crate::config::ShowTabStatusConfig::All;
        state.set_pane_surface(surface());
        let frame = state.compose(110, 35).unwrap();
        let tab_rect = state.hits.tabs.iter().find(|(_, id)| id == "lane-b").unwrap().0;
        let tab_cells = (tab_rect.x..tab_rect.right())
            .map(|x| &frame.cells[usize::from(tab_rect.y) * usize::from(frame.width) + usize::from(x)])
            .collect::<Vec<_>>();
        assert!(tab_cells.iter().any(|cell| cell.symbol == glyph && cell.fg == crate::protocol::color_to_u32(expected)),
            "tab status should match sidebar: {:?}", tab_cells.iter().map(|cell| (cell.symbol.as_str(), cell.fg)).collect::<Vec<_>>());
        let (rows, hits, buffer) = rendered_factory_rows(snapshot, overlay);
        let y = hits.tree_headers.iter().find(|hit| hit.tab_id.as_deref() == Some("lane-b")).unwrap().rect.y;
        let x = rows[y as usize].chars().position(|ch| ch == '●' || ch == '○').unwrap() as u16;
        assert_eq!(buffer[(x, y)].fg, expected, "sidebar: {}", rows[y as usize]);
        (rows, hits)
    };
    overlay.tabs.get_mut("lane-b").unwrap().busy = true;
    let (rows, _) = check(&snapshot, &overlay, palette.working, "●");
    assert!(!rows.iter().find(|row| row.contains("lane-b")).unwrap().contains("idle"));
    overlay.tabs.get_mut("lane-b").unwrap().busy = false;
    check(&snapshot, &overlay, palette.overlay0, "○");

    snapshot.agents[0].agent_status = AgentStatus::Done;
    snapshot.tabs[0].agent_status = AgentStatus::Done;
    overlay.tabs.get_mut("lane-b").unwrap().busy = true;
    check(&snapshot, &overlay, palette.working, "●");
    snapshot.agents.clear();
    snapshot.tabs[0].agent_status = AgentStatus::Unknown;
    let mut config = ClientShellConfig::from_config(&Config::default());
    config.show_tab_status = crate::config::ShowTabStatusConfig::All;
    let glyphs = crate::client::shell::render::tab_status_glyphs(&snapshot, &snapshot.tabs[0], &config, true);
    assert_eq!(glyphs[0].0, "·", "shell pane stays Unknown while tagged busy");
}

#[test]
fn registered_runs_count_and_expand_under_parent() {
    let (mut snapshot, mut overlay) = fixture();
    snapshot.tabs.retain(|tab| tab.tab_id == "lane-b" || tab.tab_id == "orch");
    overlay.tabs.get_mut("lane-b").unwrap().runs = vec![
        RunTag { id: "r1".into(), name: Some("review".into()), phase: Some("review 2/3".into()), agents: 2 },
        RunTag { id: "r2".into(), name: None, phase: Some("build 1/2".into()), agents: 1 },
    ];
    let (folded, hits, _) = rendered_factory_rows(&snapshot, &overlay);
    let lane = hits.tree_headers.iter().find(|hit| hit.key == "lane-b").unwrap();
    assert!(!lane.chevron.is_empty() && lane.collapsed);
    assert!(folded[lane.rect.y as usize].contains('2'));
    let mut tree = ClientTreeChrome::default();
    tree.factory_expanded_lanes.insert("lane-b".into());
    let (expanded, hits, _) = rendered_factory_rows_with_tree(&snapshot, &overlay, &tree);
    for (id, name, phase) in [("r1", "review", "review 2/3"), ("r2", "r2", "build 1/2")] {
        let run = hits.tree_headers.iter().find(|hit| hit.key == format!("lane-b#run:{id}")).unwrap();
        assert_eq!(run.tab_id.as_deref(), Some("lane-b"));
        assert!(expanded[run.rect.y as usize].contains(&format!("◐ {name}")));
        assert!(expanded[run.rect.y as usize + 1].contains(phase));
    }
}

#[test]
fn parked_and_automated_lanes_fold_separately_and_keep_asks_visible() {
    let (mut snapshot, mut overlay) = fixture();
    snapshot.tabs.retain(|tab| matches!(tab.tab_id.as_str(), "lane-a" | "lane-b" | "advisor"));
    let mut parked = snapshot.tabs.iter().find(|tab| tab.tab_id == "lane-b").unwrap().clone();
    parked.tab_id = "parked-lane".into();
    parked.label = "noah sdr".into();
    parked.agent_status = AgentStatus::Blocked;
    snapshot.tabs.push(parked);
    overlay.tabs.insert("parked-lane".into(), TabTag {
        kind: TabKind::Lane, mode: TabMode::Parked, attention: Attention::Act, ..Default::default()
    });
    overlay.tabs.get_mut("lane-b").unwrap().mode = TabMode::Auto;
    snapshot.tabs.iter_mut().find(|tab| tab.tab_id == "lane-b").unwrap().agent_status = AgentStatus::Working;
    let (rows, hits, buffer) = rendered_factory_rows(&snapshot, &overlay);
    let auto = hits.tree_headers.iter().find(|hit| hit.key == "factory-background:automations:ws_1").unwrap();
    let parked = hits.tree_headers.iter().find(|hit| hit.key == "factory-background:parked:ws_1").unwrap();
    let lane = hits.tree_headers.iter().find(|hit| hit.key == "lane-a").unwrap();
    assert!(lane.rect.y < auto.rect.y && auto.rect.y < parked.rect.y);
    assert!(rows[lane.rect.y as usize].contains("lane-a"));
    assert!(rows[auto.rect.y as usize].contains("automations 1"));
    assert!(rows[parked.rect.y as usize].contains("parked 1"));
    assert!(!rows.iter().any(|row| row.contains("noah sdr") || row.contains("lane-b")));
    assert!(rows[parked.rect.y as usize].ends_with('!'));
    let palette = ClientShellConfig::from_config(&Config::default()).palette;
    assert_eq!(buffer[(24, parked.rect.y)].fg, palette.red);
    let mut state = factory_state(snapshot.clone(), overlay.clone());
    state.config.agent_panel_sort = crate::config::AgentPanelSortConfig::Tree;
    state.hits = hits;
    state.last_composed_size = Some((120, 60));
    for (key, name) in [("automations", "lane-b"), ("parked", "noah sdr")] {
        let (rows, hits, _) = rendered_factory_rows_with_tree(&snapshot, &overlay, state.tree_chrome_mut());
        let rect = hits.tree_headers.iter().find(|hit| hit.key == format!("factory-background:{key}:ws_1")).unwrap().rect;
        assert!(!rows.iter().any(|row| row.contains(name)));
        state.hits = hits;
        factory_click(&mut state, MouseEventKind::Down(MouseButton::Left), rect.x + 3, rect.y);
        factory_click(&mut state, MouseEventKind::Up(MouseButton::Left), rect.x + 3, rect.y);
        let (rows, _, _) = rendered_factory_rows_with_tree(&snapshot, &overlay, state.tree_chrome_mut());
        assert!(rows.iter().any(|row| row.contains(name)), "{rows:?}");
    }
    let restored = ClientTreeChrome::from_preferences(state.tree_chrome_mut().to_preferences());
    assert!(restored.factory_auto_expanded.contains("ws_1"));
    assert!(restored.factory_parked_expanded.contains("ws_1"));
}

#[test]
fn factory_grouped_lanes_fold_with_parent_and_roll_up_state() {
    let (mut snapshot, mut overlay) = fixture();
    snapshot.tabs.retain(|tab| matches!(tab.tab_id.as_str(), "lane-a" | "lane-b" | "orch" | "plain-a"));
    snapshot.tabs.iter_mut().find(|tab| tab.tab_id == "lane-a").unwrap().agent_status = AgentStatus::Idle;
    snapshot.tabs.iter_mut().find(|tab| tab.tab_id == "lane-b").unwrap().agent_status = AgentStatus::Working;
    snapshot.agents.clear();
    let template = fixture().0.agents[0].clone();
    for (id, parent) in [("lane-a", None), ("lane-b", Some("lane-a-pane")),
                         ("plain-a", None)] {
        let mut agent = template.clone();
        agent.tab_id = id.into();
        agent.pane_id = format!("{id}-pane");
        agent.group.parent_pane_id = parent.map(str::to_string);
        agent.agent_status = if id == "lane-b" { AgentStatus::Working } else { AgentStatus::Idle };
        snapshot.agents.push(agent);
    }
    // A second child follows the first, in snapshot tab order.
    let mut child = snapshot.tabs.iter().find(|tab| tab.tab_id == "lane-b").unwrap().clone();
    child.tab_id = "child-two".into();
    child.label = "child-two".into();
    snapshot.tabs.push(child);
    overlay.tabs.insert("child-two".into(), TabTag { kind: TabKind::Lane, attention: Attention::Act, ..TabTag::default() });
    let mut second = snapshot.agents[1].clone();
    second.tab_id = "child-two".into();
    second.pane_id = "child-two-pane".into();
    snapshot.agents.push(second);
    let mut tree = ClientTreeChrome::default();
    let (rows, hits, buffer) = rendered_factory_rows_at_width(&snapshot, &overlay, &tree, 35);
    let position = |id: &str| hits.tree_headers.iter().find(|hit| hit.key == id).unwrap();
    let parent = position("lane-a");
    let first = position("lane-b");
    let next = position("child-two");
    assert!(parent.chevron.width > 0 && !parent.collapsed);
    assert!(parent.rect.y < first.rect.y && first.rect.y < next.rect.y, "{rows:?}");
    let x = |id: &str, y: u16| rows[y as usize].find(id).unwrap();
    let sibling = position("plain-a");
    assert_eq!(x("lane-b", first.rect.y), x("plain-a", sibling.rect.y) + 1);
    assert_eq!(x("child-two", next.rect.y), x("lane-b", first.rect.y));
    assert!(rows[parent.rect.y as usize].contains("2"), "{rows:?}");
    assert_eq!(buffer[(parent.rect.x + 4, parent.rect.y)].fg,
        ClientShellConfig::from_config(&Config::default()).palette.working);
    assert!(rows[parent.rect.y as usize].ends_with('!'), "child Act rolls up: {rows:?}");
    tree.factory_collapsed_lanes.insert("lane-a".into());
    let (folded, hits, _) = rendered_factory_rows_with_tree(&snapshot, &overlay, &tree);
    assert!(hits.tree_headers.iter().find(|hit| hit.key == "lane-a").unwrap().collapsed);
    assert!(!folded.iter().any(|line| line.contains("lane-b") || line.contains("child-two")), "{folded:?}");
}

fn grouped_workflow_fixture() -> (ClientShellSnapshot, FactoryOverlay) {
    let (mut snapshot, mut overlay) = fixture();
    snapshot.tabs.retain(|tab| matches!(tab.tab_id.as_str(), "lane-a" | "lane-b" | "wf-a" | "wf-b"));
    snapshot.agents.clear();
    let template = fixture().0.agents[0].clone();
    for (id, parent) in [("lane-a", None), ("lane-b", Some("lane-a-pane"))] {
        let mut agent = template.clone();
        agent.tab_id = id.into();
        agent.pane_id = format!("{id}-pane");
        agent.group.parent_pane_id = parent.map(str::to_string);
        agent.agent_status = AgentStatus::Idle;
        snapshot.agents.push(agent);
        snapshot.tabs.iter_mut().find(|tab| tab.tab_id == id).unwrap().agent_status = AgentStatus::Idle;
    }
    for id in ["wf-a", "wf-b"] {
        overlay.tabs.get_mut(id).unwrap().parent = Some("lane-b".into());
    }
    overlay.tabs.get_mut("wf-b").unwrap().done = true;
    overlay.tabs.get_mut("lane-b").unwrap().runs = vec![RunTag {
        id: "fold".into(), name: Some("fold run".into()), phase: None, agents: 1,
    }];
    (snapshot, overlay)
}

#[test]
fn grouped_lane_draws_live_workflows_and_runs_and_rolls_up_state() {
    let (snapshot, overlay) = grouped_workflow_fixture();
    let mut tree = ClientTreeChrome::default();
    tree.factory_expanded_lanes.insert("lane-a".into());
    tree.factory_background_expanded.insert("ws_1".into());
    let (rows, hits, buffer) = rendered_factory_rows_at_width(&snapshot, &overlay, &tree, 35);
    let hit = |id: &str| hits.tree_headers.iter().find(|hit| hit.key == id).unwrap();
    let parent = hit("lane-a");
    let child = hit("lane-b");
    let workflow = hit("wf-a");
    let run = hit("lane-b#run:fold");
    let done = hit("wf-b");
    assert_eq!(workflow.rect.y, child.rect.y + 2, "{rows:?}");
    assert_eq!(run.rect.y, workflow.rect.y + 3, "{rows:?}");
    assert_eq!(rows[child.rect.y as usize].find("lane-b"), rows[workflow.rect.y as usize].find("wf-a"));
    assert_eq!(rows[workflow.rect.y as usize].find("wf-a"), rows[run.rect.y as usize].find("fold run"));
    assert!(done.rect.y > run.rect.y && rows.iter().any(|row| row.contains("background 1")), "{rows:?}");
    assert!(rows[parent.rect.y as usize].contains('3'), "{rows:?}");
    let palette = ClientShellConfig::from_config(&Config::default()).palette;
    assert_eq!(buffer[(parent.rect.x + 4, parent.rect.y)].fg, palette.working);
}

#[test]
fn grouped_workflow_focus_or_act_auto_opens_parent_but_explicit_fold_hides_children() {
    let (mut snapshot, mut overlay) = grouped_workflow_fixture();
    let mut tree = ClientTreeChrome::default();
    let (folded, hits, _) = rendered_factory_rows_with_tree(&snapshot, &overlay, &tree);
    assert!(hits.tree_headers.iter().find(|hit| hit.key == "lane-a").unwrap().collapsed);
    assert!(!folded.iter().any(|row| row.contains("lane-b") || row.contains("wf-a") || row.contains("fold run")));
    snapshot.focused_tab_id = Some("wf-a".into());
    let (focused, hits, _) = rendered_factory_rows_with_tree(&snapshot, &overlay, &tree);
    assert!(!hits.tree_headers.iter().find(|hit| hit.key == "lane-a").unwrap().collapsed);
    assert!(focused.iter().any(|row| row.contains("wf-a")));
    snapshot.focused_tab_id = None;
    overlay.tabs.get_mut("wf-a").unwrap().attention = Attention::Act;
    let (alert, _, _) = rendered_factory_rows_with_tree(&snapshot, &overlay, &tree);
    assert!(alert.iter().any(|row| row.contains("wf-a")));
    tree.factory_collapsed_lanes.insert("lane-a".into());
    let (folded, _, _) = rendered_factory_rows_with_tree(&snapshot, &overlay, &tree);
    assert!(!folded.iter().any(|row| row.contains("lane-b") || row.contains("wf-a") || row.contains("fold run")));
}

#[test]
fn factory_grouping_ignores_cycles_and_cross_space_parents() {
    let (mut snapshot, mut overlay) = fixture();
    snapshot.tabs.retain(|tab| matches!(tab.tab_id.as_str(), "lane-a" | "lane-b" | "plain-a"));
    let template = snapshot.agents[0].clone();
    snapshot.agents.clear();
    for (id, parent) in [("lane-a", "lane-b-pane"), ("lane-b", "lane-a-pane"),
                         ("plain-a", "remote-pane")] {
        let mut agent = template.clone();
        agent.tab_id = id.into();
        agent.pane_id = format!("{id}-pane");
        agent.group.parent_pane_id = Some(parent.into());
        snapshot.agents.push(agent);
    }
    let mut remote = template;
    remote.workspace_id = "ws_2".into();
    remote.tab_id = "remote".into();
    remote.pane_id = "remote-pane".into();
    snapshot.agents.push(remote);
    snapshot.tabs.push(ClientShellTab {
        tab_id: "remote".into(), workspace_id: "ws_2".into(), number: 1,
        label: "remote".into(), custom_label: true, zoomed: false, focused: false,
        agent_status: AgentStatus::Idle,
    });
    snapshot.workspaces.push(ClientShellWorkspace {
        workspace_id: "ws_2".into(), active_tab_id: "remote".into(), new_workspace_cwd: String::new(),
        number: 2, label: "elsewhere".into(), custom_label: true, branch: None,
        git_ahead_behind: None, tokens: Vec::new(), worktree: None, focused: false,
        agent_status: AgentStatus::Idle, orchestrator_mode: false, tab_count: 1, visible_in_profile: true,
    });
    overlay.tabs.insert("remote".into(), TabTag { kind: TabKind::Lane, ..TabTag::default() });
    overlay.tabs.insert("plain-a".into(), TabTag { kind: TabKind::Lane, ..TabTag::default() });
    let (rows, hits, _) = rendered_factory_rows(&snapshot, &overlay);
    let a = hits.tree_headers.iter().find(|hit| hit.key == "lane-a").unwrap();
    let b = hits.tree_headers.iter().find(|hit| hit.key == "lane-b").unwrap();
    assert_eq!(rows[a.rect.y as usize].find("lane-a"), rows[b.rect.y as usize].find("lane-b"),
        "cycle should stay flat: {rows:?}");
    assert_eq!(a.chevron.width, 0, "{rows:?}");
    assert_eq!(b.chevron.width, 0, "{rows:?}");
    assert!(a.rect.y < b.rect.y, "{rows:?}");
    let cross = hits.tree_headers.iter().find(|hit| hit.key == "plain-a").unwrap();
    assert_eq!(cross.chevron.width, 0, "cross-space parent cannot fold: {rows:?}");
    assert_eq!(rows[cross.rect.y as usize].find("plain-a"), rows[a.rect.y as usize].find("lane-a"));
}

#[test]
fn configured_factory_shapes_follow_state_but_idle_stays_hollow() {
    let (mut snapshot, mut overlay) = fixture();
    snapshot.tabs.retain(|tab| matches!(tab.tab_id.as_str(), "lane-a" | "lane-b" | "orch"));
    let mut config = ClientShellConfig::from_config(&Config::default());
    config.agent_panel_sort = crate::config::AgentPanelSortConfig::Tree;
    config.factory.enabled = true;
    config.agents.state_icons.extend([
        ("working".into(), "●".into()), ("idle_unseen".into(), "■".into()),
        ("blocked".into(), "■".into()), ("idle".into(), "■".into()),
        ("unknown".into(), "".into()),
    ]);
    let symbol = |snapshot: &ClientShellSnapshot, overlay: &FactoryOverlay, id: &str| {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 25, 50));
        let mut hits = ShellHitMap::default();
        let mut scroll = 0;
        crate::client::shell::agent_sidebar::render_agent_panel_with_overlay(
            &mut buffer, Rect::new(0, 0, 25, 50), snapshot, &config,
            &ClientTreeChrome::default(), Some(overlay), &mut scroll, &mut hits,
        );
        let hit = hits.tree_headers.iter().find(|hit| hit.key == id).unwrap();
        buffer[(hit.rect.x + 4, hit.rect.y)].symbol().to_owned()
    };
    assert_eq!(symbol(&snapshot, &overlay, "lane-a"), "●");
    snapshot.tabs.iter_mut().find(|tab| tab.tab_id == "lane-b").unwrap().agent_status = AgentStatus::Done;
    overlay.tabs.get_mut("lane-b").unwrap().idle = false;
    assert_eq!(symbol(&snapshot, &overlay, "lane-b"), "■");
    snapshot.tabs.iter_mut().find(|tab| tab.tab_id == "lane-b").unwrap().agent_status = AgentStatus::Blocked;
    assert_eq!(symbol(&snapshot, &overlay, "lane-b"), "■");
    snapshot.tabs.iter_mut().find(|tab| tab.tab_id == "lane-b").unwrap().agent_status = AgentStatus::Idle;
    overlay.tabs.get_mut("lane-b").unwrap().idle = true;
    assert_eq!(symbol(&snapshot, &overlay, "lane-b"), "○");
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
    let palette = ClientShellConfig::from_config(&Config::default()).palette;
    let check_row = |snapshot: &ClientShellSnapshot, overlay: &FactoryOverlay, id: &str,
                     dot: ratatui::style::Color, mark: Option<ratatui::style::Color>| {
        let (rows, hits, buffer) = rendered_factory_rows(snapshot, overlay);
        let y = hits.tree_headers.iter().find(|hit| hit.tab_id.as_deref() == Some(id)).unwrap().rect.y;
        let x = rows[y as usize].chars().position(|ch| ch == '●' || ch == '○').unwrap() as u16;
        assert_eq!(buffer[(x, y)].fg, dot, "{id}: {}", rows[y as usize]);
        assert_eq!(mark.is_some(), rows[y as usize].ends_with('!'), "{id}: {}", rows[y as usize]);
        if let Some(mark) = mark {
            assert_eq!(buffer[(24, y)].fg, mark, "{id}: {}", rows[y as usize]);
        }
        rows[y as usize].clone()
    };
    assert!(check_row(&snapshot, &overlay, "lane-a", palette.working, None).contains("lane-a"),
        "working pane must outrank done pane");
    overlay.tabs.get_mut("lane-a").unwrap().attention = Attention::Act;
    check_row(&snapshot, &overlay, "lane-a", palette.working, Some(palette.red));

    snapshot.tabs.iter_mut().find(|tab| tab.tab_id == "lane-b").unwrap().agent_status = AgentStatus::Blocked;
    overlay.tabs.get_mut("lane-b").unwrap().idle = false;
    check_row(&snapshot, &overlay, "lane-b", palette.red, None);
    let mut done_pane = agent_template.clone();
    done_pane.tab_id = "lane-b".into();
    done_pane.agent_status = AgentStatus::Done;
    snapshot.agents.push(done_pane);
    check_row(&snapshot, &overlay, "lane-b", palette.green, None);
    let (done_rows, _, done_buffer) = rendered_factory_rows(&snapshot, &overlay);
    let done_y = done_rows.iter().position(|row| row.contains("lane-b")).unwrap() as u16;
    let name_x = done_rows[done_y as usize].find("lane-b").unwrap() as u16;
    assert_eq!(done_buffer[(name_x, done_y)].fg, palette.subtext0, "unread Done name stays full strength");
    snapshot.agents.last_mut().unwrap().agent_status = AgentStatus::Idle;
    check_row(&snapshot, &overlay, "lane-b", palette.overlay0, None);

    overlay.tabs.get_mut("orch").unwrap().name = Some("orchestrator".into());
    let mut folded = ClientTreeChrome::default();
    folded.factory_collapsed_lanes.insert("orch".into());
    let (plain_rows, hits, _) = rendered_factory_rows_with_tree(&snapshot, &overlay, &folded);
    let y = hits.tree_headers.iter().find(|hit| hit.tab_id.as_deref() == Some("orch")).unwrap().rect.y;
    let plain = &plain_rows[y as usize];
    for (attention, expected) in [(Attention::Warn, palette.peach), (Attention::Act, palette.red)] {
        overlay.tabs.get_mut("orch").unwrap().attention = attention;
        let (rows, hits, buffer) = rendered_factory_rows_with_tree(&snapshot, &overlay, &folded);
        let hit = hits.tree_headers.iter().find(|hit| hit.tab_id.as_deref() == Some("orch")).unwrap();
        assert!(hit.collapsed, "{attention:?} child must stay folded");
        let y = hit.rect.y;
        let x = rows[y as usize].chars().position(|ch| ch == '●').unwrap() as u16;
        assert_eq!(buffer[(x, y)].fg, palette.working, "folded {attention:?} child");
        assert_eq!(rows[y as usize].chars().last(), Some('!'));
        assert_eq!(buffer[(24, y)].fg, expected);
        let count_x = rows[y as usize].chars().collect::<Vec<_>>().iter().rposition(|ch| *ch == '1').unwrap();
        assert_eq!(plain.chars().collect::<Vec<_>>().iter().rposition(|ch| *ch == '1'), Some(count_x + 2),
            "count moves left by two");
    }
}

#[test]
fn factory_sections_render_and_idle_click_persists() {
    use crate::factory_overlay::TabSection;
    let (mut snapshot, mut overlay) = fixture();
    snapshot.tabs.retain(|tab| matches!(tab.tab_id.as_str(), "orch" | "lane-a" | "lane-b" | "plain-a" | "plain-b"));
    let template = snapshot.tabs.iter().find(|tab| tab.tab_id == "lane-a").unwrap().clone();
    for (id, section) in [("waiting-lane", Some(TabSection::Waiting)), ("idle-lane", Some(TabSection::Idle)),
        ("orchestrator-lane", Some(TabSection::Orchestrator)), ("untagged-lane", None)] {
        let mut tab = template.clone();
        tab.tab_id = id.into();
        tab.label = id.into();
        snapshot.tabs.push(tab);
        overlay.tabs.insert(id.into(), TabTag { kind: TabKind::Lane, section, ..Default::default() });
    }
    overlay.tabs.get_mut("lane-a").unwrap().section = Some(TabSection::Scoping);
    overlay.tabs.get_mut("lane-a").unwrap().name = Some("[Scoping] shared connections".into());
    overlay.tabs.get_mut("lane-b").unwrap().section = Some(TabSection::Inflight);
    let (rows, hits, _) = rendered_factory_rows_with_gap(&snapshot, &overlay, &ClientTreeChrome::default(), 25, 0);
    let find = |name: &str| rows.iter().position(|row| row.contains(name)).unwrap();
    assert!(find("ORCHESTRATOR") < find("orchestrator-lane"));
    assert!(find("orchestrator-lane") < find("SCOPING"));
    assert!(find("SCOPING") < find("shared connections"));
    assert!(find("shared connections") < find("IN FLIGHT"));
    assert!(find("IN FLIGHT") < find("untagged-lane"));
    assert!(find("untagged-lane") < find("plain-a"));
    assert!(find("plain-a") < find("WAITING"));
    assert!(find("WAITING") < find("waiting-lane"));
    assert!(find("waiting-lane") < find("idle 1"));
    assert!(rows[find("SCOPING")].contains("⌘1..9"));
    assert!(!rows.iter().any(|row| row.contains("idle-lane") || row.contains("[Scoping]")));
    let hit = hits.tree_headers.iter().find(|hit| hit.key == "factory-background:idle:ws_1").unwrap();
    assert!(hit.collapsed);
    let rect = hit.rect;
    let mut state = factory_state(snapshot.clone(), overlay.clone());
    state.config.agent_panel_sort = crate::config::AgentPanelSortConfig::Tree;
    state.hits = hits;
    state.last_composed_size = Some((120, 60));
    factory_click(&mut state, MouseEventKind::Down(MouseButton::Left), rect.x + 3, rect.y);
    factory_click(&mut state, MouseEventKind::Up(MouseButton::Left), rect.x + 3, rect.y);
    let restored = ClientTreeChrome::from_preferences(state.tree_chrome_mut().to_preferences());
    assert!(restored.factory_idle_expanded.contains("ws_1"));
    let (expanded, _, _) = rendered_factory_rows_with_tree(&snapshot, &overlay, &restored);
    assert!(expanded.iter().any(|row| row.contains("idle-lane")));

    // A folded idle lane must not hide an actionable child workflow.
    let mut child = snapshot.tabs.iter().find(|tab| tab.tab_id == "lane-a").unwrap().clone();
    child.tab_id = "idle-child".into();
    child.label = "idle-child".into();
    snapshot.tabs.push(child);
    overlay.tabs.insert("idle-child".into(), TabTag {
        kind: TabKind::Workflow, parent: Some("idle-lane".into()), attention: Attention::Act,
        ..Default::default()
    });
    for blocked in [false, true] {
        snapshot.tabs.iter_mut().find(|tab| tab.tab_id == "idle-child").unwrap().agent_status =
            if blocked { AgentStatus::Blocked } else { AgentStatus::Idle };
        overlay.tabs.get_mut("idle-child").unwrap().attention =
            if blocked { Attention::None } else { Attention::Act };
        let (rows, hits, _) = rendered_factory_rows(&snapshot, &overlay);
        let idle = hits.tree_headers.iter().find(|hit| hit.key == "factory-background:idle:ws_1").unwrap();
        assert!(idle.collapsed);
        assert!(rows[idle.rect.y as usize].ends_with('!'), "blocked={blocked}: {rows:?}");
        assert!(!rows.iter().any(|row| row.contains("idle-child")));
    }
}

#[test]
fn sectioned_grouped_child_and_parked_lane_keep_their_parent_groups() {
    use crate::factory_overlay::TabSection;
    let (mut snapshot, mut overlay) = fixture();
    snapshot.tabs.retain(|tab| matches!(tab.tab_id.as_str(), "lane-a" | "lane-b"));
    let template = fixture().0.agents[0].clone();
    snapshot.agents.clear();
    for (id, parent) in [("lane-a", None), ("lane-b", Some("lane-a-pane"))] {
        let mut agent = template.clone();
        agent.tab_id = id.into();
        agent.pane_id = format!("{id}-pane");
        agent.group.parent_pane_id = parent.map(str::to_string);
        snapshot.agents.push(agent);
    }
    overlay.tabs.get_mut("lane-a").unwrap().section = Some(TabSection::Inflight);
    overlay.tabs.get_mut("lane-b").unwrap().section = Some(TabSection::Idle);
    let mut parked = snapshot.tabs[0].clone();
    parked.tab_id = "parked-lane".into();
    parked.label = "parked-lane".into();
    snapshot.tabs.push(parked);
    overlay.tabs.insert("parked-lane".into(), TabTag {
        kind: TabKind::Lane, mode: TabMode::Parked, section: Some(TabSection::Scoping), ..Default::default()
    });
    let mut tree = ClientTreeChrome::default();
    tree.factory_expanded_lanes.insert("lane-a".into());
    let (rows, hits, _) = rendered_factory_rows_with_tree(&snapshot, &overlay, &tree);
    let parent = hits.tree_headers.iter().find(|hit| hit.key == "lane-a").unwrap();
    let child = hits.tree_headers.iter().find(|hit| hit.key == "lane-b").unwrap();
    assert!(parent.rect.y < child.rect.y);
    assert!(rows.iter().any(|row| row.contains("IN FLIGHT")));
    assert!(!rows.iter().any(|row| row.contains("SCOPING") || row.contains("idle 1") || row.contains("parked-lane")));
    assert!(rows.iter().any(|row| row.contains("parked 1")));
}

#[test]
fn unknown_section_does_not_break_overlay_parse() {
    let parsed = crate::factory_overlay::parse(br#"{"version":1,"tabs":{"lane":{"kind":"lane","section":"bogus"}}}"#).unwrap();
    assert_eq!(parsed.tabs["lane"].section, None);
}

#[test]
fn factory_sectioned_compact_frame_w25() {
    use crate::factory_overlay::TabSection;
    let (snapshot, mut overlay) = lab_fixture();
    overlay.tabs.get_mut("lane-a").unwrap().section = Some(TabSection::Scoping);
    overlay.tabs.get_mut("lane-a").unwrap().name = Some("[scoping] shared connections".into());
    overlay.tabs.get_mut("lane-b").unwrap().section = Some(TabSection::Idle);
    let (rows, _, _) = rendered_factory_rows_with_gap(&snapshot, &overlay, &ClientTreeChrome::default(), 25, 0);
    assert_golden(rows, include_str!("golden/factory_sectioned_compact_w25.txt"));
}
