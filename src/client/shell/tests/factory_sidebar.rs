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
    overlay.hosts.push(HostRow { name: "Studio".into(), summary: Some("3/28 live".into()), attention: Attention::None, ..HostRow::default() });
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

#[test]
fn factory_usage_footer_renders_and_opens_only_usage_urls() {
    let (snapshot, mut overlay) = lab_fixture();
    let url = "https://studio.tailf266ac.ts.net:2455/";
    overlay.usage = vec![
        HostRow { name: "claude".into(), summary: Some("3/8 · 66%".into()),
            url: Some(url.into()), ..HostRow::default() },
        HostRow { name: "codex".into(), summary: Some("4/5 · 60%".into()),
            url: Some(format!("{url}codex")), ..HostRow::default() },
    ];
    overlay.hosts = vec![
        HostRow { name: "Studio".into(), summary: Some("load 198/16".into()),
            attention: Attention::Warn, url: Some(format!("{url}hosts")) },
        HostRow { name: "PC".into(), summary: Some("1/12 live".into()), ..HostRow::default() },
        HostRow { name: "forge".into(), summary: Some("3/12 live".into()), ..HostRow::default() },
    ];
    let (rows, hits, buffer) = rendered_factory_rows_at_width(&snapshot, &overlay, &ClientTreeChrome::default(), 38);
    assert_eq!(rows[58].trim(), "claude 3/8 66% · codex 4/5 60%");
    assert_eq!(rows[59].trim(), "Studio 198/16 · PC 1/12 · forge 3/12");
    assert!(!rows.iter().any(|row| row.contains("USAGE") || row.contains("HOSTS")));
    let mut state = factory_state(snapshot.clone(), overlay.clone());
    for x in 8..14 {
        assert_eq!(buffer[(x, 59)].fg, state.config.palette.peach);
    }
    state.hits = hits;
    state.last_composed_size = Some((120, 60));
    state.config.mouse_capture = true;
    for (x, y, expected) in [(18, 58, Some(format!("{url}codex"))), (16, 58, None), (8, 59, None), (5, 58, Some(url.to_string()))] {
        let input = factory_click(&mut state, MouseEventKind::Down(MouseButton::Left), x, y);
        let urls = input.actions.iter().filter_map(|action| match action {
            ClientShellAction::OpenSafeWebUrl(url) => Some(url.as_str()), _ => None,
        }).collect::<Vec<_>>();
        assert_eq!(urls, expected.as_deref().into_iter().collect::<Vec<_>>());
        factory_click(&mut state, MouseEventKind::Up(MouseButton::Left), x, y);
    }
    // The default expanded sidebar clamp has an 18-column minimum.
    let (rows, hits, _) = rendered_factory_rows_at_width(&snapshot, &overlay, &ClientTreeChrome::default(), 18);
    for (row, value) in rows[55..].iter().zip(["3/8 66%", "4/5 60%", "198/16", "1/12", "3/12"]) {
        assert!(row.contains(value), "{row}");
    }
    assert_eq!(hits.factory_usage_urls.len(), 2);
    overlay.usage.clear();
    overlay.hosts = vec![
        HostRow { name: "forge".into(), summary: Some("down".into()), attention: Attention::Act, ..HostRow::default() },
        HostRow { name: "PC".into(), summary: None, ..HostRow::default() },
        HostRow { name: "idle".into(), summary: Some(String::new()), ..HostRow::default() },
        HostRow { name: "old".into(), summary: Some("3 live · drained".into()), ..HostRow::default() },
    ];
    let (rows, _, buffer) = rendered_factory_rows_at_width(&snapshot, &overlay, &ClientTreeChrome::default(), 40);
    assert_eq!(rows[59].trim(), "forge down · PC · idle · old 3 drained");
    for x in 7..11 {
        assert_eq!(buffer[(x, 59)].fg, state.config.palette.red);
    }
}

#[test]
fn factory_footer_uses_spaces_then_first_words_before_falling_back() {
    let (snapshot, mut overlay) = lab_fixture();
    overlay.hosts = vec![
        HostRow { name: "Studio".into(), summary: Some("load 150/16".into()),
            attention: Attention::Warn, ..HostRow::default() },
        HostRow { name: "PC".into(), summary: Some("6/12 live".into()), ..HostRow::default() },
        HostRow { name: "forge".into(), summary: Some("6/12 live".into()), ..HostRow::default() },
    ];
    let (rows, _, buffer) = rendered_factory_rows_at_width(&snapshot, &overlay, &ClientTreeChrome::default(), 36);
    assert_eq!(rows[59].trim(), "Studio 150/16  PC 6/12  forge 6/12");
    for x in 8..14 {
        assert_eq!(buffer[(x, 59)].fg, ClientShellConfig::from_config(&Config::default()).palette.peach);
    }
    let (rows, _, _) = rendered_factory_rows_at_width(&snapshot, &overlay, &ClientTreeChrome::default(), 30);
    for (row, expected) in rows[57..].iter().zip(["studio 150/16", "pc 6/12", "forge 6/12"]) {
        assert_eq!(row.split_whitespace().collect::<Vec<_>>().join(" "), expected);
    }
    overlay.hosts[0].name = "Studio workstation".into();
    let (rows, _, _) = rendered_factory_rows_at_width(&snapshot, &overlay, &ClientTreeChrome::default(), 36);
    assert_eq!(rows[59].trim(), "Studio 150/16  PC 6/12  forge 6/12");

    overlay.hosts.clear();
    overlay.usage = vec![
        HostRow { name: "claude".into(), summary: Some("3/8 · 66%".into()),
            url: Some("https://example.com/claude".into()), ..HostRow::default() },
        HostRow { name: "codex".into(), summary: Some("4/5 · 60%".into()),
            url: Some("https://example.com/codex".into()), ..HostRow::default() },
    ];
    let (rows, hits, _) = rendered_factory_rows_at_width(&snapshot, &overlay, &ClientTreeChrome::default(), 30);
    assert_eq!(rows[59].trim(), "claude 3/8 66%  codex 4/5 60%");
    let mut state = factory_state(snapshot, overlay);
    state.hits = hits;
    state.last_composed_size = Some((120, 60));
    state.config.mouse_capture = true;
    for (range, expected) in [(1..15, Some("https://example.com/claude")),
        (15..17, None), (17..30, Some("https://example.com/codex"))] {
        for x in range {
            let input = factory_click(&mut state, MouseEventKind::Down(MouseButton::Left), x, 59);
            let urls = input.actions.iter().filter_map(|action| match action {
                ClientShellAction::OpenSafeWebUrl(url) => Some(url.as_str()), _ => None,
            }).collect::<Vec<_>>();
            assert_eq!(urls, expected.into_iter().collect::<Vec<_>>(), "x={x}");
            factory_click(&mut state, MouseEventKind::Up(MouseButton::Left), x, 59);
        }
    }
}

#[test]
fn collapsed_hidden_and_automations_share_independent_click_spans() {
    let (mut snapshot, overlay) = lab_fixture();
    snapshot.workspaces[1].label = "automated".into();
    let mut automation = snapshot.agents[0].clone();
    automation.workspace_id = "ws_2".into();
    automation.tab_id = "poker".into();
    automation.pane_id = "auto-1".into();
    automation.agent_status = AgentStatus::Blocked;
    snapshot.agents.push(automation.clone());
    automation.pane_id = "auto-2".into();
    snapshot.agents.push(automation);
    let mut hidden = snapshot.workspaces[0].clone();
    hidden.workspace_id = "ws_3".into();
    hidden.label = "hidden-two".into();
    snapshot.workspaces.push(hidden);
    let mut hidden_tab = snapshot.tabs[0].clone();
    hidden_tab.tab_id = "hidden-tab".into();
    hidden_tab.workspace_id = "ws_3".into();
    snapshot.tabs.push(hidden_tab);
    let mut state = factory_state(snapshot.clone(), overlay.clone());
    state.config.automations.workspaces = vec!["automated".into()];
    let mut tree = ClientTreeChrome::default();
    tree.collapsed_spaces.extend(["ws_1".into(), "ws_3".into()]);
    let draw = |tree: &ClientTreeChrome| {
        let area = Rect::new(0, 0, 38, 60);
        let mut buffer = Buffer::empty(area);
        let mut hits = ShellHitMap::default();
        crate::client::shell::agent_sidebar::render_agent_panel_with_overlay(
            &mut buffer, area, &snapshot, &state.config, tree, Some(&overlay), &mut 0, &mut hits,
        );
        (buffer, hits)
    };
    let (buffer, hits) = draw(&tree);
    let row = hits.tree_hidden_header.y;
    let text: String = (0..38).map(|x| buffer[(x, row)].symbol()).collect();
    assert!(text.contains("2 hidden · 2 automations ▸"), "{text}");
    assert_eq!(hits.automations_header.y, row);
    assert_eq!(buffer[(hits.automations_header.x, row)].fg, state.config.palette.red);
    let mut expanded = tree.clone();
    expanded.hidden_spaces_expanded = true;
    let (_, expanded_hits) = draw(&expanded);
    assert_ne!(expanded_hits.tree_hidden_header.y, expanded_hits.automations_header.y);
    let mut expanded = tree.clone();
    expanded.automations_expanded = true;
    let (_, expanded_hits) = draw(&expanded);
    assert_ne!(expanded_hits.tree_hidden_header.y, expanded_hits.automations_header.y);
    state.tree_chrome.insert(crate::client::endpoint::ClientEndpointId::Local, tree);
    state.last_composed_size = Some((120, 60));
    state.hits = hits;
    state.config.mouse_capture = true;
    let hidden = state.hits.tree_hidden_header;
    let automation = state.hits.automations_header;
    factory_click(&mut state, MouseEventKind::Down(MouseButton::Left), hidden.x, hidden.y);
    factory_click(&mut state, MouseEventKind::Up(MouseButton::Left), hidden.x, hidden.y);
    let tree = &state.tree_chrome[&crate::client::endpoint::ClientEndpointId::Local];
    assert!(tree.hidden_spaces_expanded);
    assert!(!tree.automations_expanded);
    factory_click(&mut state, MouseEventKind::Down(MouseButton::Left), automation.x, automation.y);
    let tree = &state.tree_chrome[&crate::client::endpoint::ClientEndpointId::Local];
    assert!(tree.hidden_spaces_expanded);
    assert!(tree.automations_expanded);
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
fn factory_collapsed_frame_w25() {
    let (snapshot, overlay) = lab_fixture();
    let mut tree = ClientTreeChrome::default();
    tree.factory_collapsed_lanes.insert("lane-a".into());
    let (rows, _, _) = rendered_factory_rows_with_tree(&snapshot, &overlay, &tree);
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
fn factory_collapsed_compact_frame_w25() {
    let (snapshot, overlay) = lab_fixture();
    let mut tree = ClientTreeChrome::default();
    tree.factory_collapsed_lanes.insert("lane-a".into());
    let (rows, _, _) = rendered_factory_rows_with_gap(&snapshot, &overlay, &tree, 25, 0);
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
    assert_eq!(header("orch"), lanes - 1, "no gap between sections");
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
    let (visible, hits, buffer) = rendered_factory_rows(&snapshot, &alert_overlay);
    assert!(!visible.iter().any(|row| row.contains("issues 3")), "Act child leaves lane folded");
    let hit = hits.tree_headers.iter().find(|hit| hit.tab_id.as_deref() == Some("lane-a")).unwrap();
    assert!(hit.collapsed);
    assert!(visible[hit.rect.y as usize].ends_with('!'));
    assert_eq!(buffer[(24, hit.rect.y)].fg, ClientShellConfig::from_config(&Config::default()).palette.red);
    let at = (hit.chevron.x, hit.chevron.y);
    let mut alert_state = factory_state(snapshot.clone(), alert_overlay.clone());
    alert_state.config.agent_panel_sort = crate::config::AgentPanelSortConfig::Tree;
    alert_state.hits = hits;
    alert_state.last_composed_size = Some((120, 60));
    factory_click(&mut alert_state, MouseEventKind::Down(MouseButton::Left), at.0, at.1);
    factory_click(&mut alert_state, MouseEventKind::Up(MouseButton::Left), at.0, at.1);
    let (opened, hits, _) = rendered_factory_rows_with_tree(&snapshot, &alert_overlay, alert_state.tree_chrome_mut());
    assert!(opened.iter().any(|row| row.contains("issues 3")), "user expand shows Act child");
    alert_state.hits = hits;
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
    let (wide, _, _) = rendered_factory_rows_at_width(&snapshot, &overlay, &folded, 48);
    assert!(wide.iter().any(|row| row.contains("orchestrator") && row.contains("2 workflows · inbox 3")), "{wide:?}");
    let (narrow, hits, _) = rendered_factory_rows_with_tree(&snapshot, &overlay, &folded);
    let orch = hits.tree_headers.iter().find(|hit| hit.key == "orch").unwrap();
    assert!(narrow[orch.rect.y as usize].trim_end().ends_with(" 2"), "{narrow:?}");
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
        machine: None,
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
        RunTag { id: "r1".into(), name: Some("review".into()), phase: Some("review 2/3".into()), agents: 2, ..RunTag::default() },
        RunTag { id: "r2".into(), name: None, phase: Some("build 1/2".into()), agents: 1, ..RunTag::default() },
    ];
    let mut tree = ClientTreeChrome::default();
    tree.factory_collapsed_lanes.insert("lane-b".into());
    let (folded, hits, _) = rendered_factory_rows_with_tree(&snapshot, &overlay, &tree);
    let lane = hits.tree_headers.iter().find(|hit| hit.key == "lane-b").unwrap();
    assert!(!lane.chevron.is_empty() && lane.collapsed);
    assert!(folded[lane.rect.y as usize].contains('2'));
    let (_, hits, _) = rendered_factory_rows(&snapshot, &overlay);
    assert!(hits.tree_headers.iter().find(|hit| hit.key == "lane-b").unwrap().collapsed,
        "running runs leave their lane folded by default");
    let mut tree = ClientTreeChrome::default();
    tree.factory_expanded_lanes.insert("lane-b".into());
    let (expanded, hits, _) = rendered_factory_rows_with_tree(&snapshot, &overlay, &tree);
    assert!(!hits.tree_headers.iter().find(|hit| hit.key == "lane-b").unwrap().collapsed);
    for (id, name, phase) in [("r1", "review", "review 2/3"), ("r2", "r2", "build 1/2")] {
        let run = hits.tree_headers.iter().find(|hit| hit.key == format!("lane-b#run:{id}")).unwrap();
        assert_eq!(run.tab_id.as_deref(), Some("lane-b"));
        assert!(expanded[run.rect.y as usize].contains(&format!("◐ {name}")));
        assert!(expanded[run.rect.y as usize + 1].contains(phase));
    }
}

#[test]
fn registered_run_progress_completion_and_failure_render_under_lane() {
    let (mut snapshot, mut overlay) = fixture();
    snapshot.tabs.retain(|tab| tab.tab_id == "lane-b");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
    let palette = ClientShellConfig::from_config(&Config::default()).palette;
    for (done, attention, icon) in [
        (false, Attention::None, "◐"),
        (true, Attention::None, "✓"),
        (true, Attention::Act, "✗"),
    ] {
        overlay.tabs.get_mut("lane-b").unwrap().runs = vec![RunTag {
            id: "build".into(), name: Some("build run".into()),
            phase: Some("Build 2/3".into()), started: Some(now - 90),
            done, attention, badge: Some("PC".into()), ..RunTag::default()
        }];
        let (folded, hits, buffer) = rendered_factory_rows_at_width(
            &snapshot, &overlay, &ClientTreeChrome::default(), 48);
        let lane = hits.tree_headers.iter().find(|hit| hit.key == "lane-b").unwrap();
        let lane_line = &folded[lane.rect.y as usize];
        if done {
            assert!(!lane_line.contains("1 workflow"), "{lane_line}");
            assert_eq!(buffer[(lane_line.chars().position(|ch| ch == '○').unwrap() as u16, lane.rect.y)].fg,
                palette.overlay0, "done run must not make its lane working");
        } else {
            assert!(lane_line.contains("1 workflow"), "{lane_line}");
        }
        assert!(!hits.tree_headers.iter().any(|hit| hit.key == "lane-b#run:build"),
            "runs stay folded even when they need action");
        assert!(!folded.iter().any(|row| row.contains("build run")));
        if attention == Attention::Act {
            let x = lane_line.chars().position(|ch| ch == '!').unwrap() as u16;
            assert_eq!(buffer[(x, lane.rect.y)].fg, palette.red);
        }
        let mut tree = ClientTreeChrome::default();
        tree.factory_expanded_lanes.insert("lane-b".into());
        let (rows, hits, buffer) = rendered_factory_rows_at_width(&snapshot, &overlay, &tree, 48);
        let run = hits.tree_headers.iter().find(|hit| hit.key == "lane-b#run:build").unwrap();
        let line = &rows[run.rect.y as usize];
        assert!(line.contains(&format!("{icon} build run")), "{rows:?}");
        assert!(line.contains("PC"), "{line}");
        if done {
            assert_eq!(run.rect.height, 1);
            assert!(!rows.iter().any(|line| line.contains("build 2/3")), "{rows:?}");
        } else {
            let progress = &rows[run.rect.y as usize + 1];
            assert!(progress.contains("build 2/3") && progress.contains("1m"), "{progress}");
        }
        if attention == Attention::Act {
            for ch in ['✗', '!'] {
                let x = line.chars().position(|symbol| symbol == ch).unwrap() as u16;
                assert_eq!(buffer[(x, run.rect.y)].fg, palette.red);
            }
        }
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
    let auto = hits.tree_headers.iter().find(|hit| hit.key == "factory-background:services:ws_1").unwrap();
    let parked = hits.tree_headers.iter().find(|hit| hit.key == "factory-background:parked:ws_1").unwrap();
    let lane = hits.tree_headers.iter().find(|hit| hit.key == "lane-a").unwrap();
    assert!(lane.rect.y < auto.rect.y && auto.rect.y < parked.rect.y);
    assert!(rows[lane.rect.y as usize].contains("lane-a"));
    assert!(rows[auto.rect.y as usize].contains("services 1"));
    assert!(rows[parked.rect.y as usize].contains("parked 1"));
    assert!(!rows.iter().any(|row| row.contains("noah sdr") || row.contains("lane-b")));
    assert!(rows[parked.rect.y as usize].ends_with('!'));
    let palette = ClientShellConfig::from_config(&Config::default()).palette;
    assert_eq!(buffer[(24, parked.rect.y)].fg, palette.red);
    let mut state = factory_state(snapshot.clone(), overlay.clone());
    state.config.agent_panel_sort = crate::config::AgentPanelSortConfig::Tree;
    state.hits = hits;
    state.last_composed_size = Some((120, 60));
    for (key, name) in [("services", "lane-b"), ("parked", "noah sdr")] {
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
    tree.factory_expanded_lanes.insert("lane-a".into());
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
    assert!(rows[parent.rect.y as usize].contains("2 agents"), "{rows:?}");
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
        id: "fold".into(), name: Some("fold run".into()), phase: None, agents: 1, ..RunTag::default()
    }];
    (snapshot, overlay)
}

#[test]
fn collapsed_factory_groups_show_grouped_workflow_alerts_and_work() {
    use crate::factory_overlay::TabSection;
    for (mode, section, label) in [
        (TabMode::Active, Some(TabSection::Closed), "closed"),
        (TabMode::Parked, None, "parked"),
        (TabMode::Auto, None, "services"),
    ] {
        let (mut snapshot, mut overlay) = grouped_workflow_fixture();
        snapshot.tabs.retain(|tab| tab.tab_id != "wf-b");
        overlay.tabs.get_mut("lane-b").unwrap().runs.clear();
        let root = overlay.tabs.get_mut("lane-a").unwrap();
        root.mode = mode;
        root.section = section;
        for (status, attention, busy, run, alert, working) in [
            (AgentStatus::Blocked, Attention::None, false, None, true, false),
            (AgentStatus::Working, Attention::None, false, None, false, true),
            (AgentStatus::Idle, Attention::Act, false, None, true, false),
            (AgentStatus::Idle, Attention::None, true, None, false, true),
            (AgentStatus::Idle, Attention::None, false, Some((false, Attention::None)), false, true),
            (AgentStatus::Idle, Attention::None, false, Some((true, Attention::None)), false, false),
            (AgentStatus::Idle, Attention::None, false, None, false, false),
        ] {
            snapshot.tabs.iter_mut().find(|tab| tab.tab_id == "wf-a").unwrap().agent_status = status;
            let workflow = overlay.tabs.get_mut("wf-a").unwrap();
            workflow.attention = attention;
            workflow.busy = busy;
            workflow.runs = run.into_iter().map(|(done, attention)| RunTag {
                id: "workflow-run".into(), done, attention, ..Default::default()
            }).collect();
            let (rows, hits, _) = rendered_factory_rows(&snapshot, &overlay);
            let key = format!("factory-background:{label}:ws_1");
            let hit = hits.tree_headers.iter().find(|hit| hit.key == key).unwrap();
            let row = &rows[hit.rect.y as usize];
            assert!(hit.collapsed && row.contains(&format!("{label} 1")), "{rows:?}");
            assert_eq!(row.contains('!'), alert, "{label}, {status:?}, {attention:?}: {row}");
            assert_eq!(row.contains('●'), working && mode == TabMode::Auto, "{label}, {status:?}: {row}");
            assert!(!rows.iter().any(|row| row.contains("wf-a")), "{rows:?}");
        }
    }
}

#[test]
fn collapsed_idle_group_shows_grouped_done_run_attention() {
    use crate::factory_overlay::TabSection;
    let (mut snapshot, mut overlay) = grouped_workflow_fixture();
    snapshot.tabs.retain(|tab| !tab.tab_id.starts_with("wf-"));
    overlay.tabs.get_mut("lane-a").unwrap().section = Some(TabSection::Closed);
    let run = &mut overlay.tabs.get_mut("lane-b").unwrap().runs[0];
    run.done = true;
    run.attention = Attention::Act;
    let (rows, hits, _) = rendered_factory_rows(&snapshot, &overlay);
    let hit = hits.tree_headers.iter().find(|hit| hit.key == "factory-background:closed:ws_1").unwrap();
    let row = &rows[hit.rect.y as usize];
    assert!(hit.collapsed && row.contains("closed 1"), "{rows:?}");
    assert!(row.contains('!'), "done run attention must remain visible: {row}");
    assert!(!row.contains('●'), "done run must not show work: {row}");
}

#[test]
fn lane_run_defaults_folded_unless_user_expanded() {
    let (mut snapshot, mut overlay) = fixture();
    snapshot.tabs.retain(|tab| tab.tab_id == "lane-a");
    snapshot.agents.clear();
    // (done, attention, user expanded, user collapsed, visible)
    for (done, attention, expanded, collapsed, visible) in [
        (false, Attention::None, false, false, false),
        (false, Attention::Act, false, false, false),
        (false, Attention::None, true, false, true),
        (false, Attention::None, false, true, false),
        (true, Attention::None, false, false, false),
    ] {
        overlay.tabs.get_mut("lane-a").unwrap().runs = vec![RunTag {
            id: "fold".into(), name: Some("fold run".into()), done, attention, ..RunTag::default()
        }];
        let mut tree = ClientTreeChrome::default();
        if expanded {
            tree.factory_expanded_lanes.insert("lane-a".into());
        }
        if collapsed {
            ClientTreeChrome::toggle(&mut tree.factory_collapsed_lanes, "lane-a".into());
        }
        let (rows, hits, buffer) = rendered_factory_rows_with_tree(&snapshot, &overlay, &tree);
        let lane = hits.tree_headers.iter().find(|hit| hit.key == "lane-a").unwrap();
        if attention == Attention::Act {
            assert!(rows[lane.rect.y as usize].ends_with('!'));
            assert_eq!(buffer[(24, lane.rect.y)].fg, ClientShellConfig::from_config(&Config::default()).palette.red);
        }
        if !done {
            assert_eq!(lane.collapsed, !visible, "done={done}, expanded={expanded}, collapsed={collapsed}: {rows:?}");
        }
        assert_eq!(rows.iter().any(|row| row.contains("fold run")), visible,
            "done={done}, expanded={expanded}, collapsed={collapsed}: {rows:?}");
        assert_eq!(hits.tree_headers.iter().any(|hit| hit.key == "lane-a#run:fold"), visible);
        if visible {
            let run = hits.tree_headers.iter().find(|hit| hit.key == "lane-a#run:fold").unwrap();
            assert!(run.rect.y > lane.rect.y, "run must render under its lane: {rows:?}");
        }
    }
    overlay.tabs.get_mut("lane-a").unwrap().runs = vec![
        RunTag { id: "agent:one".into(), ..RunTag::default() },
        RunTag { id: "wf_two".into(), ..RunTag::default() },
        RunTag { id: "agent:done".into(), done: true, ..RunTag::default() },
    ];
    for (width, summary) in [(48, "1 agent · 1 workflow"), (25, "1 agent")] {
        let (rows, hits, _) = rendered_factory_rows_at_width(&snapshot, &overlay, &ClientTreeChrome::default(), width);
        let lane = hits.tree_headers.iter().find(|hit| hit.key == "lane-a").unwrap();
        assert!(rows[lane.rect.y as usize].trim_end().ends_with(summary), "{rows:?}");
    }
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
    assert!(rows[parent.rect.y as usize].contains("1 agent · 2 workflows"), "{rows:?}");
    let palette = ClientShellConfig::from_config(&Config::default()).palette;
    assert_eq!(buffer[(parent.rect.x + 4, parent.rect.y)].fg, palette.working);
}

#[test]
fn grouped_running_workflow_stays_folded_until_expanded_or_focused() {
    let (mut snapshot, mut overlay) = grouped_workflow_fixture();
    let mut tree = ClientTreeChrome::default();
    let (running, hits, _) = rendered_factory_rows_with_tree(&snapshot, &overlay, &tree);
    assert!(hits.tree_headers.iter().find(|hit| hit.key == "lane-a").unwrap().collapsed,
        "running workflows leave their parent folded: {running:?}");
    for label in ["lane-b", "wf-a", "fold run"] {
        assert!(!running.iter().any(|row| row.contains(label)), "{running:?}");
    }
    tree.factory_expanded_lanes.insert("lane-a".into());
    let (opened, hits, _) = rendered_factory_rows_with_tree(&snapshot, &overlay, &tree);
    assert!(!hits.tree_headers.iter().find(|hit| hit.key == "lane-a").unwrap().collapsed);
    for label in ["lane-b", "wf-a", "fold run"] {
        assert!(opened.iter().any(|row| row.contains(label)), "{opened:?}");
    }
    tree.factory_expanded_lanes.clear();
    snapshot.focused_tab_id = Some("wf-a".into());
    let (focused, hits, _) = rendered_factory_rows_with_tree(&snapshot, &overlay, &tree);
    assert!(!hits.tree_headers.iter().find(|hit| hit.key == "lane-a").unwrap().collapsed);
    assert!(focused.iter().any(|row| row.contains("wf-a")));
    snapshot.focused_tab_id = None;
    overlay.tabs.get_mut("wf-a").unwrap().attention = Attention::Act;
    let (alert, hits, buffer) = rendered_factory_rows_with_tree(&snapshot, &overlay, &tree);
    assert!(!alert.iter().any(|row| row.contains("wf-a")));
    let parent = hits.tree_headers.iter().find(|hit| hit.key == "lane-a").unwrap();
    assert!(parent.collapsed && alert[parent.rect.y as usize].ends_with('!'));
    assert_eq!(buffer[(24, parent.rect.y)].fg, ClientShellConfig::from_config(&Config::default()).palette.red);
    tree.factory_collapsed_lanes.insert("lane-a".into());
    let (folded, _, _) = rendered_factory_rows_with_tree(&snapshot, &overlay, &tree);
    assert!(!folded.iter().any(|row| row.contains("lane-b") || row.contains("wf-a") || row.contains("fold run")));
}

#[test]
fn orchestrator_grouped_lane_draws_workflow_and_run_and_rolls_up_state() {
    let (mut snapshot, mut overlay) = fixture();
    snapshot.tabs.retain(|tab| matches!(tab.tab_id.as_str(), "orch" | "lane-a" | "wf-a" | "wf-b"));
    let template = snapshot.agents[0].clone();
    snapshot.agents.clear();
    for (id, parent) in [("orch", None), ("lane-a", Some("orch-pane"))] {
        let mut agent = template.clone();
        agent.tab_id = id.into();
        agent.pane_id = format!("{id}-pane");
        agent.group.parent_pane_id = parent.map(str::to_string);
        snapshot.agents.push(agent);
    }
    overlay.tabs.get_mut("wf-b").unwrap().done = true;
    overlay.tabs.get_mut("lane-a").unwrap().runs = vec![RunTag {
        id: "fold".into(), name: Some("fold run".into()), phase: None, agents: 1, ..RunTag::default()
    }];
    let mut tree = ClientTreeChrome::default();
    tree.factory_expanded_lanes.insert("orch".into());
    let (rows, hits, _) = rendered_factory_rows_at_width(&snapshot, &overlay, &tree, 42);
    let hit = |id: &str| hits.tree_headers.iter().find(|hit| hit.key == id).unwrap();
    let orch = hit("orch");
    let lane = hit("lane-a");
    let workflow = hit("wf-a");
    let run = hit("lane-a#run:fold");
    assert!(orch.rect.y < lane.rect.y && lane.rect.y < workflow.rect.y
        && workflow.rect.y < run.rect.y, "{rows:?}");
    assert_eq!(rows[workflow.rect.y as usize].find("wf-a"),
        rows[run.rect.y as usize].find("fold run"), "{rows:?}");
    assert!(rows[workflow.rect.y as usize].find("wf-a") > rows[lane.rect.y as usize].find("lane-a"), "{rows:?}");
    assert!(rows[orch.rect.y as usize].contains("1 agent · 2 workflows"), "{rows:?}");
    let (trimmed, trimmed_hits, _) = rendered_factory_rows_at_width(&snapshot, &overlay, &tree, 35);
    let header = trimmed_hits.tree_headers.iter().find(|hit| hit.key == "orch").unwrap();
    assert!(trimmed[header.rect.y as usize].trim_end().ends_with("1 agent · 2 workflows"), "{trimmed:?}");
    tree.factory_collapsed_lanes.insert("orch".into());
    let (folded, hits, _) = rendered_factory_rows_with_tree(&snapshot, &overlay, &tree);
    assert!(hits.tree_headers.iter().find(|hit| hit.key == "orch").unwrap().collapsed);
    assert!(!folded.iter().any(|row| row.contains("lane-a") || row.contains("wf-a") || row.contains("fold run")));
    tree.factory_collapsed_lanes.clear();
    tree.factory_expanded_lanes.clear();
    snapshot.focused_tab_id = Some("wf-a".into());
    let (focused, _, _) = rendered_factory_rows_with_tree(&snapshot, &overlay, &tree);
    assert!(focused.iter().any(|row| row.contains("wf-a")));
    snapshot.focused_tab_id = None;
    overlay.tabs.get_mut("wf-a").unwrap().attention = Attention::Act;
    let (alert, hits, buffer) = rendered_factory_rows_with_tree(&snapshot, &overlay, &tree);
    assert!(!alert.iter().any(|row| row.contains("wf-a")));
    let parent = hits.tree_headers.iter().find(|hit| hit.key == "orch").unwrap();
    assert!(parent.collapsed && alert[parent.rect.y as usize].ends_with('!'));
    assert_eq!(buffer[(24, parent.rect.y)].fg, ClientShellConfig::from_config(&Config::default()).palette.red);
    tree.factory_expanded_lanes.insert("orch".into());
    let (opened, _, _) = rendered_factory_rows_with_tree(&snapshot, &overlay, &tree);
    assert!(opened.iter().any(|row| row.contains("wf-a")));
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
    for (id, section) in [("waiting-lane", Some(TabSection::Reviewing)), ("idle-lane", Some(TabSection::Closed)),
        ("orchestrator-lane", Some(TabSection::Orchestrator)), ("untagged-lane", None),
        ("review-second", Some(TabSection::Reviewing)), ("monitor-lane", Some(TabSection::Monitoring))] {
        let mut tab = template.clone();
        tab.tab_id = id.into();
        tab.label = id.into();
        snapshot.tabs.push(tab);
        overlay.tabs.insert(id.into(), TabTag { kind: TabKind::Lane, section, ..Default::default() });
    }
    overlay.tabs.get_mut("lane-a").unwrap().section = Some(TabSection::Scoping);
    overlay.tabs.get_mut("lane-a").unwrap().name = Some("[Scoping] shared connections".into());
    overlay.tabs.get_mut("lane-b").unwrap().section = Some(TabSection::Implementing);
    let (rows, hits, _) = rendered_factory_rows_with_gap(&snapshot, &overlay, &ClientTreeChrome::default(), 40, 0);
    let find = |name: &str| rows.iter().position(|row| row.contains(name)).unwrap();
    assert!(find("ORCHESTRATOR") < find("orchestrator-lane"));
    assert!(find("orchestrator-lane") < find("READY FOR REVIEW"));
    assert!(find("waiting-lane") < find("SCOPING"));
    assert!(find("SCOPING") < find("shared connections"));
    assert!(find("shared connections") < find("IMPLEMENTING"));
    assert!(find("IMPLEMENTING") < find("untagged-lane"));
    assert!(find("untagged-lane") < find("plain-a"));
    assert!(find("plain-a") < find("closed 1"));
    assert!(find("READY FOR REVIEW") < find("waiting-lane"));
    assert!(find("IMPLEMENTING") < find("MONITORING"));
    assert!(find("monitor-lane") < find("closed 1"));
    assert!(rows[find("READY FOR REVIEW")].contains("2◎"));
    assert!(!rows[find("READY FOR REVIEW")].contains("⌘1..9"));
    assert!(rows[find("SCOPING")].contains("⌘1..9"));
    assert!(!rows.iter().any(|row| row.contains("idle-lane") || row.contains("[Scoping]")));
    let hit = hits.tree_headers.iter().find(|hit| hit.key == "factory-background:closed:ws_1").unwrap();
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
        let idle = hits.tree_headers.iter().find(|hit| hit.key == "factory-background:closed:ws_1").unwrap();
        assert!(idle.collapsed);
        assert!(rows[idle.rect.y as usize].ends_with('!'), "blocked={blocked}: {rows:?}");
        assert!(!rows.iter().any(|row| row.contains("idle-child")));
    }

    let (mut snapshot, mut overlay) = fixture();
    snapshot.tabs.retain(|tab| matches!(tab.tab_id.as_str(), "lane-a" | "lane-b"));
    let mut second = snapshot.tabs[1].clone();
    second.tab_id = "lane-c".into();
    snapshot.tabs.push(second);
    let mut inflight = snapshot.tabs[1].clone();
    inflight.tab_id = "lane-inflight".into();
    snapshot.tabs.push(inflight);
    let template = snapshot.agents[0].clone();
    snapshot.agents.clear();
    for (id, parent) in [("lane-a", None), ("lane-b", Some("lane-a-pane")),
                         ("lane-c", Some("lane-a-pane")), ("lane-inflight", None)] {
        let mut agent = template.clone();
        agent.tab_id = id.into();
        agent.pane_id = format!("{id}-pane");
        agent.group.parent_pane_id = parent.map(str::to_string);
        snapshot.agents.push(agent);
    }
    for (id, section, name) in [
        ("lane-a", TabSection::Scoping, "[Scoping] parent"),
        ("lane-b", TabSection::Scoping, "[scoping] first child"),
        ("lane-c", TabSection::Scoping, "[SCOPING] second child"),
        ("lane-inflight", TabSection::Implementing, "[scoping] in flight"),
    ] {
        let tag = overlay.tabs.entry(id.into()).or_insert_with(TabTag::default);
        tag.kind = TabKind::Lane;
        tag.section = Some(section);
        tag.name = Some(name.into());
    }
    let mut tree = ClientTreeChrome::default();
    tree.factory_expanded_lanes.insert("lane-a".into());
    let (rows, hits, _) = rendered_factory_rows_at_width(&snapshot, &overlay, &tree, 40);
    for (id, expected) in [("lane-a", "parent"), ("lane-b", "first child"),
                            ("lane-c", "second child"), ("lane-inflight", "in flight")] {
        let y = hits.tree_headers.iter().find(|hit| hit.key == id).unwrap().rect.y;
        assert!(rows[y as usize].contains(expected), "{id}: {rows:?}");
        {
            assert!(!rows[y as usize].contains("[scoping]")
                && !rows[y as usize].contains("[Scoping]")
                && !rows[y as usize].contains("[SCOPING]"), "{id}: {rows:?}");
        }
    }
}

#[test]
fn scoping_lane_grouped_under_other_section_draws_flat_in_scoping() {
    use crate::factory_overlay::TabSection;
    for parent_section in [TabSection::Monitoring, TabSection::Scoping] {
        let (mut snapshot, mut overlay) = fixture();
        snapshot.tabs.retain(|tab| matches!(tab.tab_id.as_str(), "lane-a" | "lane-b" | "plain-a"));
        let template = snapshot.agents[0].clone();
        snapshot.agents.clear();
        for (id, parent) in [("lane-a", None), ("lane-b", Some("lane-a-pane"))] {
            let mut agent = template.clone();
            agent.tab_id = id.into();
            agent.pane_id = format!("{id}-pane");
            agent.group.parent_pane_id = parent.map(str::to_string);
            snapshot.agents.push(agent);
        }
        overlay.tabs.get_mut("lane-a").unwrap().section = Some(parent_section);
        overlay.tabs.get_mut("lane-b").unwrap().section = Some(TabSection::Scoping);
        overlay.tabs.get_mut("lane-b").unwrap().attention = Attention::Act;
        let mut tree = ClientTreeChrome::default();
        tree.factory_collapsed_lanes.insert("lane-a".into());
        let (rows, hits, _) = rendered_factory_rows_at_width(&snapshot, &overlay, &tree, 40);
        let child = hits.tree_headers.iter().find(|hit| hit.key == "lane-b").unwrap();
        let parent = hits.tree_headers.iter().find(|hit| hit.key == "lane-a").unwrap();
        let sibling = hits.tree_headers.iter().find(|hit| hit.key == "plain-a").unwrap();
        let scoping = rows.iter().position(|row| row.contains("SCOPING")).unwrap();
        let next_section = rows.iter().position(|row| row.contains("IMPLEMENTING")).unwrap();
        assert!(scoping < child.rect.y as usize && (child.rect.y as usize) < next_section, "{rows:?}");
        assert_eq!(rows[child.rect.y as usize].find("lane-b"), rows[sibling.rect.y as usize].find("plain-a"));
        assert_eq!(parent.chevron.width, 0, "scoping child must not count in parent roll-up: {rows:?}");
        assert!(!rows[parent.rect.y as usize].ends_with('!'), "scoping attention must not roll up: {rows:?}");
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
    overlay.tabs.get_mut("lane-a").unwrap().section = Some(TabSection::Implementing);
    overlay.tabs.get_mut("lane-b").unwrap().section = Some(TabSection::Closed);
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
    assert!(rows.iter().any(|row| row.contains("IMPLEMENTING")));
    assert!(!rows.iter().any(|row| row.contains("SCOPING") || row.contains("closed 1") || row.contains("parked-lane")));
    assert!(rows.iter().any(|row| row.contains("parked 1")));
}

#[test]
fn unknown_section_does_not_break_overlay_parse() {
    let parsed = crate::factory_overlay::parse(br#"{"version":1,"tabs":{"lane":{"kind":"lane","section":"bogus"}}}"#).unwrap();
    assert_eq!(parsed.tabs["lane"].section, None);
}

#[test]
fn factory_sectioned_collapsed_compact_frame_w25() {
    use crate::factory_overlay::TabSection;
    let (snapshot, mut overlay) = lab_fixture();
    overlay.tabs.get_mut("lane-a").unwrap().section = Some(TabSection::Scoping);
    overlay.tabs.get_mut("lane-a").unwrap().name = Some("[scoping] shared connections".into());
    overlay.tabs.get_mut("lane-b").unwrap().section = Some(TabSection::Closed);
    let mut tree = ClientTreeChrome::default();
    tree.factory_collapsed_lanes.insert("lane-a".into());
    let (rows, _, _) = rendered_factory_rows_with_gap(&snapshot, &overlay, &tree, 25, 0);
    assert_golden(rows, include_str!("golden/factory_sectioned_compact_w25.txt"));
}

#[test]
fn legacy_sections_place_lanes_and_clean_implementing_labels() {
    let (mut snapshot, _) = fixture();
    snapshot.tabs.retain(|tab| matches!(tab.tab_id.as_str(), "lane-a" | "lane-b" | "plain-a"));
    let overlay = crate::factory_overlay::parse(r#"{"version":1,"tabs":{
        "lane-a":{"kind":"lane","section":"inflight","name":"[scoping] issue reporter"},
        "lane-b":{"kind":"lane","section":"idle","name":"routing · it2 · Scoping"},
        "plain-a":{"kind":"lane","section":"waiting","name":"review lane"}
    }}"#.as_bytes()).unwrap();
    let (rows, hits, _) = rendered_factory_rows(&snapshot, &overlay);
    let implementing = rows.iter().position(|row| row.contains("IMPLEMENTING")).unwrap();
    let reviewing = rows.iter().position(|row| row.contains("READY FOR REVIEW")).unwrap();
    let review = hits.tree_headers.iter().find(|hit| hit.key == "plain-a").unwrap();
    assert!(reviewing < review.rect.y as usize && (review.rect.y as usize) < implementing);
    for (id, label) in [("lane-a", "issue reporter"), ("lane-b", "routing · it2")] {
        let hit = hits.tree_headers.iter().find(|hit| hit.key == id).unwrap();
        let row = &rows[hit.rect.y as usize];
        assert!(implementing < hit.rect.y as usize && row.contains(label), "{rows:?}");
        assert!(!row.contains("[scoping]") && !row.contains("Scoping"), "{row}");
    }
}

#[test]
fn sectioned_parentless_workflow_lives_in_services_and_rolls_up_state() {
    use crate::factory_overlay::TabSection;
    let (mut snapshot, mut overlay) = fixture();
    snapshot.tabs.retain(|tab| matches!(tab.tab_id.as_str(), "lane-a" | "wf-a"));
    overlay.tabs.get_mut("lane-a").unwrap().section = Some(TabSection::Implementing);
    overlay.tabs.get_mut("wf-a").unwrap().parent = None;
    for (status, attention, busy, run, alert, working) in [
        (AgentStatus::Blocked, Attention::None, false, false, true, false),
        (AgentStatus::Idle, Attention::Act, false, false, true, false),
        (AgentStatus::Working, Attention::None, false, false, false, true),
        (AgentStatus::Idle, Attention::None, true, false, false, true),
        (AgentStatus::Idle, Attention::None, false, true, false, true),
        (AgentStatus::Idle, Attention::None, false, false, false, false),
    ] {
        snapshot.tabs.iter_mut().find(|tab| tab.tab_id == "wf-a").unwrap().agent_status = status;
        let tag = overlay.tabs.get_mut("wf-a").unwrap();
        tag.attention = attention;
        tag.busy = busy;
        tag.runs = if run { vec![RunTag { id: "run".into(), ..Default::default() }] } else { vec![] };
        let (rows, hits, _) = rendered_factory_rows(&snapshot, &overlay);
        let group = hits.tree_headers.iter().find(|hit| hit.key == "factory-background:services:ws_1").unwrap();
        let row = &rows[group.rect.y as usize];
        assert!(group.collapsed && row.contains("services 1"), "{rows:?}");
        assert_eq!(row.contains('!'), alert, "{row}");
        assert_eq!(row.contains('●'), working, "{row}");
        assert!(!hits.tree_headers.iter().any(|hit| hit.key == "wf-a"));
        let mut tree = ClientTreeChrome::default();
        tree.factory_auto_expanded.insert("ws_1".into());
        let (expanded_rows, hits, _) = rendered_factory_rows_with_tree(&snapshot, &overlay, &tree);
        let group = hits.tree_headers.iter().find(|hit| hit.key == "factory-background:services:ws_1").unwrap();
        let workflow = hits.tree_headers.iter().find(|hit| hit.key == "wf-a").unwrap();
        assert!(group.rect.y < workflow.rect.y);
        let group_x = expanded_rows[group.rect.y as usize].find("services").unwrap();
        let workflow_x = expanded_rows[workflow.rect.y as usize].find("wf-a").unwrap();
        assert!(workflow_x > group_x, "{expanded_rows:?}");
    }
}

// Owner-boundary regressions: real rendered hit regions drive mouse input; old
// headers have no controls. These cover visibility, hidden alerts, and restart
// persistence without a test-only production seam.
fn section_controls_fixture() -> (ClientShellSnapshot, FactoryOverlay) {
    use crate::factory_overlay::TabSection;
    let (mut snapshot, mut overlay) = fixture();
    snapshot.tabs.retain(|tab| matches!(tab.tab_id.as_str(), "orch" | "lane-a" | "lane-b" | "advisor"));
    snapshot.agents.clear();
    overlay.tabs.get_mut("lane-a").unwrap().section = Some(TabSection::Reviewing);
    overlay.tabs.get_mut("lane-b").unwrap().section = Some(TabSection::Scoping);
    snapshot.tabs.iter_mut().find(|tab| tab.tab_id == "lane-a").unwrap().agent_status = AgentStatus::Blocked;
    let template = snapshot.tabs[1].clone();
    for (id, section, mode) in [("service", None, TabMode::Auto), ("closed-lane", Some(TabSection::Closed), TabMode::Active)] {
        let mut tab = template.clone();
        tab.tab_id = id.into();
        tab.label = id.into();
        tab.agent_status = AgentStatus::Idle;
        snapshot.tabs.push(tab);
        overlay.tabs.insert(id.into(), TabTag { kind: TabKind::Lane, section, mode, ..Default::default() });
    }
    (snapshot, overlay)
}

fn click_section_control(state: &mut ClientShellState, label: &str, button: bool) -> (Vec<String>, Buffer) {
    let snapshot = state.snapshot.as_deref().unwrap().clone();
    let overlay = state.factory_overlay().unwrap().clone();
    let (rows, hits, _) = rendered_factory_rows_at_width(&snapshot, &overlay, state.tree_chrome_mut(), 40);
    let y = rows.iter().position(|row| row.contains(label)).unwrap() as u16;
    state.hits = hits;
    state.last_composed_size = Some((120, 60));
    let x = if button { 38 } else { 3 };
    factory_click(state, MouseEventKind::Down(MouseButton::Left), x, y);
    factory_click(state, MouseEventKind::Up(MouseButton::Left), x, y);
    let (rows, _, buffer) = rendered_factory_rows_at_width(&snapshot, &overlay, state.tree_chrome_mut(), 40);
    (rows, buffer)
}

#[test]
fn factory_section_label_collapses_restores_and_reports_hidden_blocked_member() {
    let (snapshot, overlay) = section_controls_fixture();
    let mut state = factory_state(snapshot, overlay);
    let (rows, buffer) = click_section_control(&mut state, "READY FOR REVIEW", false);
    let y = rows.iter().position(|row| row.contains("READY FOR REVIEW")).unwrap();
    assert!(rows[y].contains("▸ READY FOR REVIEW") && rows[y].contains("1!"), "{rows:?}");
    assert!(!rows.iter().any(|row| row.contains("lane-a")));
    let x = rows[y].chars().position(|c| c == '!').unwrap();
    assert_eq!(buffer[(x as u16, y as u16)].fg, state.config.palette.red);
    let (rows, _) = click_section_control(&mut state, "READY FOR REVIEW", false);
    assert!(rows.iter().any(|row| row.contains("▾ READY FOR REVIEW")));
    assert!(rows.iter().any(|row| row.contains("lane-a")));
    let (rows, _) = click_section_control(&mut state, "ORCHESTRATOR", false);
    assert!(rows.iter().any(|row| row.contains("▸ ORCHESTRATOR")));
    assert!(!rows.iter().any(|row| row.contains("inbox 3")));
}

#[test]
fn factory_section_focus_hides_other_sections_groups_and_keeps_alert_reveal() {
    let (snapshot, overlay) = section_controls_fixture();
    let mut state = factory_state(snapshot, overlay);
    let (rows, buffer) = click_section_control(&mut state, "SCOPING", true);
    assert!(rows.iter().any(|row| row.contains("SCOPING") && row.contains('✕')));
    assert!(rows.iter().any(|row| row.contains("ORCHESTRATOR")));
    assert!(rows.iter().any(|row| row.contains("lane-b")));
    assert!(!rows.iter().any(|row| row.contains("READY FOR REVIEW") || row.contains("services") || row.contains("closed") || row.contains("background")));
    let y = rows.iter().position(|row| row.contains("show all · 4 more !")).unwrap();
    assert_eq!(buffer[(1, y as u16)].fg, state.config.palette.red);
    let (rows, _) = click_section_control(&mut state, "SCOPING", true);
    assert!(rows.iter().any(|row| row.contains("READY FOR REVIEW")));
    assert!(rows.iter().any(|row| row.contains("services")));
    assert!(rows.iter().any(|row| row.contains("closed")));
    let (rows, _) = click_section_control(&mut state, "SCOPING", true);
    let y = rows.iter().position(|row| row.contains("show all")).unwrap() as u16;
    let snapshot = state.snapshot.as_deref().unwrap().clone();
    let overlay = state.factory_overlay().unwrap().clone();
    let (_, hits, _) = rendered_factory_rows_at_width(&snapshot, &overlay, state.tree_chrome_mut(), 40);
    state.hits = hits;
    factory_click(&mut state, MouseEventKind::Down(MouseButton::Left), 3, y);
    factory_click(&mut state, MouseEventKind::Up(MouseButton::Left), 3, y);
    let (rows, _, _) = rendered_factory_rows_at_width(&snapshot, &overlay, state.tree_chrome_mut(), 40);
    assert!(rows.iter().any(|row| row.contains("READY FOR REVIEW")));
    assert!(!rows.iter().any(|row| row.contains("show all")));
}

#[test]
fn factory_section_preferences_preserve_controls_and_ignore_unknown_values() {
    let (snapshot, overlay) = section_controls_fixture();
    let mut state = factory_state(snapshot.clone(), overlay.clone());
    click_section_control(&mut state, "READY FOR REVIEW", false);
    click_section_control(&mut state, "SCOPING", true);
    let saved = serde_json::to_value(state.tree_chrome_mut().to_preferences()).unwrap();
    assert_eq!(saved["factory_sections_collapsed"], serde_json::json!(["ws_1:READY FOR REVIEW"]));
    assert_eq!(saved["factory_section_focus"]["ws_1"], "SCOPING");
    let mut saved = saved;
    saved["factory_sections_collapsed"].as_array_mut().unwrap().push(serde_json::json!("ws_1:bogus"));
    saved["factory_section_focus"]["ws_2"] = serde_json::json!("bogus");
    let tree = ClientTreeChrome::from_preferences(serde_json::from_value(saved).unwrap());
    let clean = serde_json::to_value(tree.to_preferences()).unwrap();
    assert_eq!(clean["factory_sections_collapsed"], serde_json::json!(["ws_1:READY FOR REVIEW"]));
    assert!(clean["factory_section_focus"].get("ws_2").is_none());
    let (rows, _, _) = rendered_factory_rows_at_width(&snapshot, &overlay, &tree, 40);
    assert!(rows.iter().any(|row| row.contains("SCOPING") && row.contains('✕')));
    let mut state = factory_state(snapshot, overlay);
    *state.tree_chrome_mut() = tree;
    let (rows, _) = click_section_control(&mut state, "SCOPING", true);
    assert!(rows.iter().any(|row| row.contains("▸ READY FOR REVIEW")));
    assert!(!rows.iter().any(|row| row.contains("lane-a")));
}

#[test]
fn factory_legacy_reviewing_preferences_restore_ready_for_review_controls() {
    let (snapshot, overlay) = section_controls_fixture();
    let mut saved = serde_json::to_value(ClientTreeChrome::default().to_preferences()).unwrap();
    saved["factory_sections_collapsed"] = serde_json::json!(["ws_1:REVIEWING"]);
    let mut tree = ClientTreeChrome::from_preferences(serde_json::from_value(saved.clone()).unwrap());
    let (rows, _, _) = rendered_factory_rows_at_width(&snapshot, &overlay, &tree, 40);
    assert!(rows.iter().any(|row| row.contains("▸ READY FOR REVIEW")), "{rows:?}");
    assert!(!rows.iter().any(|row| row.contains("lane-a")));
    saved["factory_section_focus"] = serde_json::json!({"ws_1": "REVIEWING"});
    tree = ClientTreeChrome::from_preferences(serde_json::from_value(saved).unwrap());
    let (rows, _, _) = rendered_factory_rows_at_width(&snapshot, &overlay, &tree, 40);
    assert!(rows.iter().any(|row| row.contains("READY FOR REVIEW") && row.contains('✕')));
    assert!(rows.iter().any(|row| row.contains("lane-a")));
    assert!(!rows.iter().any(|row| row.contains("SCOPING")));
}

#[test]
fn ready_for_review_header_preserves_count_and_focus_at_narrow_width() {
    let (snapshot, overlay) = section_controls_fixture();
    for (width, label) in [(40, "READY FOR REVIEW"), (20, "READY")] {
        let (rows, hits, _) = rendered_factory_rows_at_width(&snapshot, &overlay, &ClientTreeChrome::default(), width);
        let hit = hits.factory_sections.iter().find(|hit| hit.label == "READY FOR REVIEW").unwrap();
        let row = &rows[hit.rect.y as usize];
        assert!(row.contains(&format!("▾ {label}")) && row.contains("1◎"), "{row}");
        assert!(!row.contains("⌘1..9"));
    }
}

#[test]
fn factory_section_context_menu_changes_visibility_without_endpoint_requests() {
    let (snapshot, overlay) = section_controls_fixture();
    let mut state = factory_state(snapshot.clone(), overlay.clone());
    let (rows, hits, _) = rendered_factory_rows_at_width(&snapshot, &overlay, state.tree_chrome_mut(), 40);
    let y = rows.iter().position(|row| row.contains("READY FOR REVIEW")).unwrap() as u16;
    state.hits = hits;
    state.last_composed_size = Some((120, 60));
    factory_click(&mut state, MouseEventKind::Down(MouseButton::Right), 3, y);
    let Some(ClientShellOverlay::ContextMenu(menu)) = &state.overlay else { panic!("missing section menu") };
    let items = menu.items();
    assert_eq!(items.iter().map(|item| item.label.as_str()).collect::<Vec<_>>(), ["Focus READY FOR REVIEW", "Collapse"]);
    let mut input = ClientShellInput::default();
    state.activate_context_menu_item(0, &mut input);
    assert!(input.actions.is_empty());
    let (rows, hits, _) = rendered_factory_rows_at_width(&snapshot, &overlay, state.tree_chrome_mut(), 40);
    assert!(!rows.iter().any(|row| row.contains("SCOPING")));
    let y = rows.iter().position(|row| row.contains("READY FOR REVIEW")).unwrap() as u16;
    state.hits = hits;
    factory_click(&mut state, MouseEventKind::Down(MouseButton::Right), 3, y);
    let Some(ClientShellOverlay::ContextMenu(menu)) = &state.overlay else { panic!("missing section menu") };
    assert_eq!(menu.items()[0].label, "Show all sections");
    state.activate_context_menu_item(0, &mut input);
    let (rows, _, _) = rendered_factory_rows_at_width(&snapshot, &overlay, state.tree_chrome_mut(), 40);
    assert!(rows.iter().any(|row| row.contains("SCOPING")));
}


#[test]
fn reviewing_lane_links_render_and_open_without_focusing() {
    use crate::factory_overlay::TabSection;
    for grouped in [false, true] {
        for (section, url) in [
            (TabSection::Reviewing, Some("https://studio.tailf266ac.ts.net:8799/review")),
            (TabSection::Reviewing, None),
            (TabSection::Implementing, Some("https://studio.tailf266ac.ts.net:8799/review")),
        ] {
            let (mut snapshot, mut overlay) = fixture();
            snapshot.tabs.retain(|tab| matches!(tab.tab_id.as_str(), "lane-a" | "lane-b"));
            let template = snapshot.agents[0].clone();
            snapshot.agents.clear();
            if grouped {
                for (id, parent) in [("lane-a", None), ("lane-b", Some("lane-a-pane"))] {
                    let mut agent = template.clone();
                    agent.tab_id = id.into();
                    agent.pane_id = format!("{id}-pane");
                    agent.group.parent_pane_id = parent.map(str::to_string);
                    snapshot.agents.push(agent);
                }
            }
            for id in ["lane-a", "lane-b"] {
                let mut value = serde_json::to_value(&overlay.tabs[id]).unwrap();
                value["section"] = serde_json::to_value(section).unwrap();
                value["review_url"] = serde_json::to_value(url).unwrap();
                value["attention"] = serde_json::json!("act");
                overlay.tabs.insert(id.into(), serde_json::from_value(value).unwrap());
            }
            let mut state = factory_state(snapshot.clone(), overlay.clone());
            state.tree_chrome_mut().factory_expanded_lanes.insert("lane-a".into());
            let (rows, hits, buffer) = rendered_factory_rows_at_width(&snapshot, &overlay, state.tree_chrome_mut(), 35);
            let row = hits.tree_headers.iter().find(|hit| hit.key == "lane-b").unwrap().rect;
            let text = &rows[row.y as usize];
            assert!(text.ends_with('!'), "{rows:?}");
            state.hits = hits;
            if section == TabSection::Reviewing {
                let label = if url.is_some() { "review ↗" } else { "no link" };
                assert!(text.contains(label), "grouped={grouped}: {rows:?}");
                let x = text[..text.find(label).unwrap()].chars().count() as u16;
                let palette = &state.config.palette;
                assert_eq!(buffer[(x, row.y)].fg, if url.is_some() { palette.blue } else { palette.overlay0 });
                if url.is_none() {
                    assert!(buffer[(x, row.y)].modifier.contains(Modifier::DIM));
                }
                for offset in 0..label.chars().count() as u16 {
                    let input = factory_click(&mut state, MouseEventKind::Down(MouseButton::Left), x + offset, row.y);
                    let opened: Vec<_> = input.actions.iter().filter_map(|action| match action {
                        ClientShellAction::OpenSafeWebUrl(url) => Some(url.as_str()),
                        _ => None,
                    }).collect();
                    assert_eq!(opened, url.into_iter().collect::<Vec<_>>());
                    if url.is_some() { assert!(focused_tab(&input).is_empty()); }
                    factory_click(&mut state, MouseEventKind::Up(MouseButton::Left), x + offset, row.y);
                }
            } else {
                assert!(!text.contains("review ↗") && !text.contains("no link"), "{rows:?}");
            }
            let name_x = text[..text.find("lane-b").unwrap()].chars().count() as u16;
            factory_click(&mut state, MouseEventKind::Down(MouseButton::Left), name_x, row.y);
            let input = factory_click(&mut state, MouseEventKind::Up(MouseButton::Left), name_x, row.y);
            assert_eq!(focused_tab(&input), vec!["lane-b"]);
            assert!(!input.actions.iter().any(|action| matches!(action, ClientShellAction::OpenSafeWebUrl(_))));
        }
    }
}

#[test]
fn scoping_lane_links_render_and_open_without_focusing() {
    use crate::factory_overlay::TabSection;
    for grouped in [false, true] {
        for (section, scope_url, review_url) in [
            (TabSection::Scoping, Some("https://studio.tailf266ac.ts.net:8799/scope"), None),
            (TabSection::Scoping, None, None),
            (TabSection::Implementing, Some("https://studio.tailf266ac.ts.net:8799/scope"), None),
            (TabSection::Reviewing, Some("https://studio.tailf266ac.ts.net:8799/scope"), Some("https://studio.tailf266ac.ts.net:8799/review")),
        ] {
            let (mut snapshot, mut overlay) = fixture();
            snapshot.tabs.retain(|tab| matches!(tab.tab_id.as_str(), "lane-a" | "lane-b"));
            let template = snapshot.agents[0].clone();
            snapshot.agents.clear();
            if grouped {
                for (id, parent) in [("lane-a", None), ("lane-b", Some("lane-a-pane"))] {
                    let mut agent = template.clone();
                    agent.tab_id = id.into();
                    agent.pane_id = format!("{id}-pane");
                    agent.group.parent_pane_id = parent.map(str::to_string);
                    snapshot.agents.push(agent);
                }
            }
            for id in ["lane-a", "lane-b"] {
                let mut value = serde_json::to_value(&overlay.tabs[id]).unwrap();
                value["section"] = serde_json::to_value(section).unwrap();
                value["scope_url"] = serde_json::to_value(scope_url).unwrap();
                value["review_url"] = serde_json::to_value(review_url).unwrap();
                value["attention"] = serde_json::json!("act");
                overlay.tabs.insert(id.into(), serde_json::from_value(value).unwrap());
            }
            let mut state = factory_state(snapshot.clone(), overlay.clone());
            state.tree_chrome_mut().factory_expanded_lanes.insert("lane-a".into());
            let (rows, hits, buffer) = rendered_factory_rows_at_width(&snapshot, &overlay, state.tree_chrome_mut(), 35);
            let row = hits.tree_headers.iter().find(|hit| hit.key == "lane-b").unwrap().rect;
            let text = &rows[row.y as usize];
            assert!(text.ends_with('!'), "{rows:?}");
            state.hits = hits;
            if section == TabSection::Scoping && scope_url.is_some() {
                let label = "scope ↗";
                assert!(text.contains(label), "grouped={grouped}: {rows:?}");
                let x = text[..text.find(label).unwrap()].chars().count() as u16;
                assert_eq!(buffer[(x, row.y)].fg, state.config.palette.blue);
                for offset in 0..label.chars().count() as u16 {
                    let input = factory_click(&mut state, MouseEventKind::Down(MouseButton::Left), x + offset, row.y);
                    let opened: Vec<_> = input.actions.iter().filter_map(|action| match action {
                        ClientShellAction::OpenSafeWebUrl(url) => Some(url.as_str()),
                        _ => None,
                    }).collect();
                    assert_eq!(opened, scope_url.into_iter().collect::<Vec<_>>());
                    assert!(focused_tab(&input).is_empty());
                    factory_click(&mut state, MouseEventKind::Up(MouseButton::Left), x + offset, row.y);
                }
            } else if section == TabSection::Scoping {
                assert!(!text.contains("scope ↗") && !text.contains("no link"), "{rows:?}");
            } else {
                assert!(!text.contains("scope ↗"), "{rows:?}");
                if section == TabSection::Reviewing {
                    assert!(text.contains("review ↗"), "{rows:?}");
                }
            }
            let name_x = text[..text.find("lane-b").unwrap()].chars().count() as u16;
            factory_click(&mut state, MouseEventKind::Down(MouseButton::Left), name_x, row.y);
            let input = factory_click(&mut state, MouseEventKind::Up(MouseButton::Left), name_x, row.y);
            assert_eq!(focused_tab(&input), vec!["lane-b"]);
            assert!(!input.actions.iter().any(|action| matches!(action, ClientShellAction::OpenSafeWebUrl(_))));
        }
    }
}

#[test]
fn factory_space_header_omits_child_status_but_keeps_count() {
    let (mut snapshot, mut overlay) = fixture();
    snapshot.tabs.retain(|tab| tab.tab_id == "plain-a");
    snapshot.tabs[0].agent_status = AgentStatus::Blocked;
    snapshot.agents[0].agent_status = AgentStatus::Blocked;
    overlay.tabs.clear();
    overlay.spaces.insert("ws_1".into(), SpaceTag { attention: Attention::Act, summary: Some("1".into()), ..Default::default() });
    for factory in [true, false] {
        if factory {
            overlay.tabs.insert("plain-a".into(), TabTag { kind: TabKind::Lane, ..Default::default() });
        } else {
            overlay.tabs.clear();
        }
        let tree = ClientTreeChrome { show_tabs: false, show_agents: false, ..Default::default() };
        let (rows, hits, _) = rendered_factory_rows_at_width(&snapshot, &overlay, &tree, 40);
        let header = hits.tree_headers.iter().find(|hit| hit.tab_id.is_none() && hit.key == "ws_1").unwrap();
        let text = &rows[header.rect.y as usize];
        assert!(text.contains('1'), "{rows:?}");
        assert_eq!(text.contains('●'), !factory, "factory={factory}: {rows:?}");
        if factory {
            assert!(!text.contains(['○', '■', '◐', '✓', '✗']), "{rows:?}");
        }
    }
}

// Goal filtering is exercised through the rendered sidebar and its real menu/click
// handlers; JSON preferences also let the same regressions run on the base.
fn goal_fixture() -> (ClientShellSnapshot, FactoryOverlay) {
    let (mut snapshot, mut overlay) = section_controls_fixture();
    let template = snapshot.tabs[1].clone();
    for (id, goal, area, section) in [
        ("lane-a", "recruiter", None, "reviewing"),
        ("lane-b", "rails", Some("workspace ui"), "scoping"),
        ("rail-other", "rails", Some("infra"), "implementing"),
        ("closer-lane", "closer", None, "implementing"),
    ] {
        if !snapshot.tabs.iter().any(|tab| tab.tab_id == id) {
            let mut tab = template.clone();
            tab.tab_id = id.into();
            tab.label = id.into();
            snapshot.tabs.push(tab);
        }
        overlay.tabs.insert(id.into(), serde_json::from_value(serde_json::json!({
            "kind": "lane", "goal": goal, "goal_area": area, "section": section
        })).unwrap());
    }
    (snapshot, overlay)
}

fn goal_preferences(filter: &str) -> ClientTreeChrome {
    ClientTreeChrome::from_preferences(serde_json::from_value(serde_json::json!({
        "factory_goal_filter": filter
    })).unwrap())
}

#[test]
fn factory_goal_filter_limits_sections_shortcuts_and_clear_restores_rows() {
    let (snapshot, overlay) = goal_fixture();
    let tree = goal_preferences("recruiter");
    let mut state = factory_state(snapshot.clone(), overlay.clone());
    *state.tree_chrome_mut() = tree;
    let (rows, hits, buffer) = rendered_factory_rows_at_width(&snapshot, &overlay, state.tree_chrome_mut(), 40);
    let y = rows.iter().position(|row| row.contains("goal") && row.contains("recruiter") && row.contains('✕')).unwrap() as u16;
    assert_eq!(buffer[(7, y)].fg, state.config.palette.blue);
    assert!(rows.iter().any(|row| row.contains("orch")));
    assert!(rows.iter().any(|row| row.contains("lane-a")));
    assert!(!rows.iter().any(|row| row.contains("lane-b") || row.contains("rail-other") || row.contains("closer-lane")
        || row.contains("IMPLEMENTING") || row.contains("SCOPING") || row.contains("closed") || row.contains("background")));
    // Services remain outside goal filtering.
    assert!(rows.iter().any(|row| row.contains("services")));
    let mut shortcut = ClientShellInput::default();
    state.record_binding(crate::input::KeybindMatch::Action(crate::input::KeybindAction::SwitchTab(1)), &mut shortcut);
    assert_eq!(focused_tab(&shortcut), ["lane-a"]);
    state.tree_chrome_mut().factory_section_focus.insert("ws_1".into(), "SCOPING".into());
    let (focused, _, _) = rendered_factory_rows_at_width(&snapshot, &overlay, state.tree_chrome_mut(), 40);
    assert!(focused.iter().any(|row| row.contains("show all · 2 more")));
    state.tree_chrome_mut().factory_section_focus.clear();
    state.hits = hits;
    state.last_composed_size = Some((120, 60));
    factory_click(&mut state, MouseEventKind::Down(MouseButton::Left), 39, y);
    factory_click(&mut state, MouseEventKind::Up(MouseButton::Left), 39, y);
    let (rows, _, _) = rendered_factory_rows_at_width(&snapshot, &overlay, state.tree_chrome_mut(), 40);
    assert!(rows.iter().any(|row| row.contains("goal") && row.contains("All")));
    assert!(rows.iter().any(|row| row.contains("lane-b")));
    assert!(rows.iter().any(|row| row.contains("IMPLEMENTING")));
    assert!(rows.iter().any(|row| row.contains("closed")));
}

#[test]
fn numbered_shortcuts_skip_goal_filtered_collapsed_and_unfocused_rows() {
    let (mut snapshot, mut overlay) = section_controls_fixture();
    snapshot.tabs.retain(|tab| tab.tab_id == "lane-b");
    snapshot.agents.clear();
    let template = snapshot.tabs[0].clone();
    snapshot.tabs.clear();
    overlay.tabs.clear();
    for (id, goal, section) in [
        ("rails-a", "rails", "scoping"),
        ("recruiter-b", "recruiter", "implementing"),
        ("recruiter-c", "recruiter", "implementing"),
    ] {
        let mut tab = template.clone();
        tab.tab_id = id.into();
        tab.label = id.into();
        snapshot.tabs.push(tab);
        overlay.tabs.insert(id.into(), serde_json::from_value(serde_json::json!({
            "kind": "lane", "goal": goal, "section": section
        })).unwrap());
    }
    let mut state = factory_state(snapshot.clone(), overlay.clone());
    let shortcut = |state: &mut ClientShellState, index| {
        let mut output = ClientShellInput::default();
        state.record_binding(crate::input::KeybindMatch::Action(
            crate::input::KeybindAction::SwitchTab(index)), &mut output);
        focused_tab(&output)
    };
    assert_eq!(shortcut(&mut state, 0), ["rails-a"]);
    *state.tree_chrome_mut() = goal_preferences("recruiter");
    let (rows, _, _) = rendered_factory_rows_at_width(&snapshot, &overlay, state.tree_chrome_mut(), 40);
    assert!(!rows.iter().any(|row| row.contains("rails-a")));
    assert!(rows.iter().any(|row| row.contains("recruiter-b")));
    assert!(rows.iter().any(|row| row.contains("recruiter-c")));
    assert_eq!(shortcut(&mut state, 0), ["recruiter-b"]);
    assert_eq!(shortcut(&mut state, 1), ["recruiter-c"]);
    assert!(shortcut(&mut state, 2).is_empty());
    state.tree_chrome_mut().factory_goal_filter = None;
    assert_eq!(shortcut(&mut state, 0), ["rails-a"]);
    state.tree_chrome_mut().factory_sections_collapsed.insert("ws_1:SCOPING".into());
    assert_eq!(shortcut(&mut state, 0), ["recruiter-b"]);
    assert_eq!(shortcut(&mut state, 1), ["recruiter-c"]);
    state.tree_chrome_mut().factory_sections_collapsed.clear();
    state.tree_chrome_mut().factory_section_focus.insert("ws_1".into(), "IMPLEMENTING".into());
    assert_eq!(shortcut(&mut state, 0), ["recruiter-b"]);
    assert_eq!(shortcut(&mut state, 1), ["recruiter-c"]);
}

#[test]
fn factory_goal_menu_area_selection_and_preferences_roundtrip() {
    let (snapshot, overlay) = goal_fixture();
    let mut state = factory_state(snapshot.clone(), overlay.clone());
    state.tree_chrome_mut().space_order = vec!["ws_1".into()];
    let (rows, hits, _) = rendered_factory_rows_at_width(&snapshot, &overlay, state.tree_chrome_mut(), 40);
    let y = rows.iter().position(|row| row.contains("goal") && row.contains("All")).unwrap() as u16;
    assert!(y < rows.iter().position(|row| row.contains("ORCHESTRATOR")).unwrap() as u16, "picker stays before manually ordered spaces");
    state.hits = hits;
    state.last_composed_size = Some((120, 60));
    factory_click(&mut state, MouseEventKind::Down(MouseButton::Left), 8, y);
    factory_click(&mut state, MouseEventKind::Up(MouseButton::Left), 8, y);
    let Some(ClientShellOverlay::ContextMenu(menu)) = &state.overlay else { panic!("missing goal menu") };
    assert_eq!(menu.items().iter().map(|item| item.label.as_str()).collect::<Vec<_>>(),
        ["All", "recruiter", "closer", "rails", "  infra", "  workspace ui"]);
    let mut input = ClientShellInput::default();
    state.activate_context_menu_item(5, &mut input);
    assert!(input.actions.is_empty());
    let saved = serde_json::to_value(state.tree_chrome_mut().to_preferences()).unwrap();
    assert_eq!(saved["factory_goal_filter"], "rails:workspace ui");
    let tree = ClientTreeChrome::from_preferences(serde_json::from_value(saved).unwrap());
    let (rows, _, _) = rendered_factory_rows_at_width(&snapshot, &overlay, &tree, 40);
    assert!(rows.iter().any(|row| row.contains("rails · workspace ui") && row.contains('✕')));
    assert!(rows.iter().any(|row| row.contains("lane-b")));
    assert!(rows.iter().any(|row| row.contains("orch")));
    assert!(!rows.iter().any(|row| row.contains("rail-other") || row.contains("lane-a") || row.contains("closer-lane")));
    for invalid in ["unknown", "rails:", "rails:missing"] {
        let (rows, _, _) = rendered_factory_rows_at_width(&snapshot, &overlay, &goal_preferences(invalid), 40);
        assert!(rows.iter().any(|row| row.contains("goal") && row.contains("All")));
        assert!(rows.iter().any(|row| row.contains("lane-a")));
    }
    // No tagged goals (and non-sectioned layouts) never draw the picker.
    for sectioned in [true, false] {
        let mut no_goals = overlay.clone();
        for tag in no_goals.tabs.values_mut() {
            if sectioned {
                let mut json = serde_json::to_value(&*tag).unwrap();
                json["goal"] = serde_json::Value::Null;
                *tag = serde_json::from_value(json).unwrap();
            } else { tag.section = None; }
        }
        let (rows, _, _) = rendered_factory_rows_at_width(&snapshot, &no_goals, &ClientTreeChrome::default(), 40);
        assert!(!rows.iter().any(|row| row.contains("goal")));
    }
}

fn sidebar_report_fixture(snapshot: &ClientShellSnapshot, overlay: &FactoryOverlay, tree: &ClientTreeChrome) -> serde_json::Value {
    sidebar_report_fixture_at_height(snapshot, overlay, tree, 100)
}

fn sidebar_report_fixture_at_height(snapshot: &ClientShellSnapshot, overlay: &FactoryOverlay, tree: &ClientTreeChrome, height: u16) -> serde_json::Value {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let directory = std::env::temp_dir().join(format!("herdr-sidebar-report-{}-{}", std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
    std::fs::create_dir_all(&directory).unwrap();
    let preferences = directory.join("local-test.json");
    let mut state = factory_state(snapshot.clone(), overlay.clone());
    state.config.preferences_path = Some(preferences);
    state.config.agent_panel_sort = crate::config::AgentPanelSortConfig::Tree;
    *state.tree_chrome_mut() = tree.clone();
    state.compose(160, height).expect("factory sidebar frame");
    let bytes = std::fs::read(directory.join("sidebar-test.json"))
        .expect("a frame drawing the factory sidebar must publish its placement report");
    let report = serde_json::from_slice(&bytes).unwrap();
    std::fs::remove_dir_all(directory).unwrap();
    report
}

#[test]
fn factory_sidebar_report_covers_shown_collapsed_grouped_and_scoping_tabs() {
    use crate::factory_overlay::TabSection;
    let (mut snapshot, mut overlay) = grouped_workflow_fixture();
    snapshot.tabs.retain(|tab| !tab.tab_id.starts_with("wf-"));
    for agent in &mut snapshot.agents { agent.agent_status = AgentStatus::Idle; }
    overlay.tabs.get_mut("lane-a").unwrap().section = Some(TabSection::Monitoring);
    overlay.tabs.get_mut("lane-a").unwrap().summary = None;
    overlay.tabs.get_mut("lane-b").unwrap().section = Some(TabSection::Implementing);
    overlay.tabs.get_mut("lane-b").unwrap().runs.clear();
    let template = snapshot.tabs[0].clone();
    for (id, section) in [("shown", TabSection::Implementing), ("collapsed", TabSection::Reviewing), ("scoping", TabSection::Scoping), ("idle", TabSection::Closed)] {
        let mut tab = template.clone(); tab.tab_id = id.into(); tab.label = id.into();
        snapshot.tabs.push(tab);
        overlay.tabs.insert(id.into(), TabTag { kind: TabKind::Lane, section: Some(section), ..Default::default() });
    }
    let mut scoping = snapshot.agents[1].clone();
    scoping.tab_id = "scoping".into(); scoping.pane_id = "scoping-pane".into();
    snapshot.agents.push(scoping);
    let mut tree = ClientTreeChrome::default();
    tree.factory_collapsed_lanes.insert("lane-a".into());
    tree.factory_sections_collapsed.insert("ws_1:READY FOR REVIEW".into());
    let report = sidebar_report_fixture(&snapshot, &overlay, &tree);
    let workspace = &report["workspaces"][0];
    let tabs = workspace["tabs"].as_array().unwrap();
    assert_eq!(tabs.len(), snapshot.tabs.len());
    for (id, section, shown, hidden, under) in [
        ("lane-a", "MONITORING", true, None, None),
        ("lane-b", "MONITORING", false, Some("group_folded"), Some("lane-a")),
        ("shown", "IMPLEMENTING", true, None, None),
        ("collapsed", "READY FOR REVIEW", false, Some("section_collapsed"), None),
        ("scoping", "SCOPING", true, None, None),
        ("idle", "closed", false, Some("closed_folded"), None),
    ] {
        let matches = tabs.iter().filter(|tab| tab["tab"] == id).collect::<Vec<_>>();
        assert_eq!(matches.len(), 1, "{id}: {report}");
        let tab = matches[0];
        assert_eq!(tab["kind"], "lane", "{id}: {report}");
        assert_eq!(tab["section"], section, "{id}: {report}");
        assert_eq!(tab["shown"], shown, "{id}: {report}");
        assert_eq!(tab["hidden"].as_str(), hidden, "{id}: {report}");
        assert_eq!(tab["under"].as_str(), under, "{id}: {report}");
        assert_eq!(tab["row"].is_number(), shown);
        if shown { assert!(tab["indent"].is_number()); }
    }
}

#[test]
fn factory_sidebar_report_has_no_unplaced_tagged_tabs_in_sectioned_fixtures() {
    for (snapshot, overlay) in [section_controls_fixture(), goal_fixture(), grouped_workflow_fixture()] {
        for collapsed in [false, true] {
            let mut overlay = overlay.clone();
            overlay.tabs.get_mut("lane-a").unwrap().section = Some(crate::factory_overlay::TabSection::Implementing);
            let mut tree = ClientTreeChrome::default();
            if collapsed { tree.collapsed_spaces.insert("ws_1".into()); }
            let report = sidebar_report_fixture(&snapshot, &overlay, &tree);
            let tabs = report["workspaces"][0]["tabs"].as_array().unwrap();
            let expected = snapshot.tabs.iter().filter(|tab| overlay.tab(&tab.tab_id).is_some()).count();
            assert_eq!(tabs.len(), expected, "{report}");
            let ids = tabs.iter().map(|tab| tab["tab"].as_str().unwrap()).collect::<std::collections::HashSet<_>>();
            assert_eq!(ids.len(), expected, "{report}");
            assert!(tabs.iter().all(|tab| tab["hidden"] != "unplaced"), "{report}");
        }
    }
}

#[test]
fn factory_idle_reason_draws_stalled_peach_and_fold_but_ignores_working() {
    let (mut snapshot, mut overlay) = fixture();
    snapshot.tabs.retain(|tab| tab.tab_id == "lane-b");
    snapshot.agents.clear();
    let palette = ClientShellConfig::from_config(&Config::default()).palette;
    for (status, reason, badge, expected) in [(AgentStatus::Idle, "stalled", None, Some("stalled")), (AgentStatus::Idle, "fold 2", None, Some("fold 2")), (AgentStatus::Working, "stalled", None, None), (AgentStatus::Idle, "stalled", Some("PC"), Some("PC"))] {
        snapshot.tabs[0].agent_status = status;
        let mut tag = serde_json::to_value(&overlay.tabs["lane-b"]).unwrap();
        tag["idle_reason"] = serde_json::json!(reason);
        tag["badge"] = serde_json::json!(badge);
        overlay.tabs.insert("lane-b".into(), serde_json::from_value(tag).unwrap());
        let (rows, hits, buffer) = rendered_factory_rows_at_width(&snapshot, &overlay, &ClientTreeChrome::default(), 40);
        let y = hits.tree_headers.iter().find(|hit| hit.key == "lane-b").unwrap().rect.y;
        let text = &rows[y as usize];
        if let Some(expected) = expected {
            let byte = text.find(expected).expect("idle reason must replace the idle metadata");
            if badge == Some("PC") {
                assert!(text.contains("PC"), "{text}");
                assert!(!text.contains("stalled"), "{text}");
            } else {
                let x = unicode_width::UnicodeWidthStr::width(&text[..byte]) as u16;
                assert_eq!(buffer[(x, y)].fg, if reason == "stalled" && badge.is_none() { palette.peach } else { palette.overlay0 });
            }
            assert!(!text.contains("idle"), "{text}");
        } else { assert!(!text.contains(reason), "{text}"); }
    }
}


#[test]
fn factory_sidebar_report_marks_scrolled_rows_offscreen_not_background() {
    let (mut snapshot, mut overlay) = section_controls_fixture();
    let template = snapshot.tabs[0].clone();
    for index in 0..20 {
        let id = format!("offscreen-{index}");
        let mut tab = template.clone();
        tab.tab_id = id.clone(); tab.label = id.clone();
        snapshot.tabs.push(tab);
        overlay.tabs.insert(id, TabTag { kind: TabKind::Lane,
            section: Some(crate::factory_overlay::TabSection::Implementing), ..Default::default() });
    }
    let report = sidebar_report_fixture_at_height(&snapshot, &overlay, &ClientTreeChrome::default(), 12);
    let tabs = report["workspaces"][0]["tabs"].as_array().unwrap();
    let offscreen = tabs.iter().filter(|tab| tab["tab"].as_str().unwrap().starts_with("offscreen-")
        && !tab["shown"].as_bool().unwrap()).collect::<Vec<_>>();
    assert!(!offscreen.is_empty(), "{report}");
    for tab in offscreen {
        assert_eq!(tab["hidden"], "offscreen", "{report}");
        assert_eq!(tab["section"], "IMPLEMENTING", "{report}");
        assert_eq!(tab["indent"], 1, "{report}");
        assert!(tab["row"].is_null(), "{report}");
    }
    assert!(tabs.iter().all(|tab| tab["hidden"] != "background" || tab["section"] == "background"), "{report}");
}

#[test]
fn remote_shell_pane_names_its_machine_on_lane_and_plain_agent_rows() {
    let (mut snapshot, overlay) = fixture();
    let pane = |pane_id: &str, tab_id: &str, machine: Option<&str>| ClientShellPane {
        pane_id: pane_id.into(), workspace_id: "ws_1".into(), tab_id: tab_id.into(),
        label: None, cwd: None, foreground_cwd: None, focused: false, right_click_passthrough: false,
        machine: machine.map(str::to_owned),
    };
    snapshot.panes = vec![pane("lane-b-pane", "lane-b", Some("ax42")), pane("plain-pane", "plain-a", Some("book"))];
    let (rows, _, buffer) = rendered_factory_rows_at_width(&snapshot, &overlay, &ClientTreeChrome::default(), 30);
    let (y, lane) = rows.iter().enumerate().find(|(_, row)| row.contains("lane-b")).unwrap();
    assert!(lane.trim_end().ends_with("idle · ax42"), "lane row: {lane}");
    let x = lane.find("ax42").map(|byte| lane[..byte].chars().count()).unwrap() as u16;
    let palette = ClientShellConfig::from_config(&Config::default()).palette;
    assert_eq!(buffer[(x, y as u16)].fg, palette.overlay0, "machine label stays quiet");
    assert!(rows.iter().any(|row| row.contains("book")), "plain agent row names its machine: {rows:#?}");
    assert!(!rows.iter().any(|row| row.contains("lane-a") && row.contains("ax42")), "local tabs stay unlabeled");

    // A folded parent's counts leave room for its machine before the name truncates.
    snapshot.panes.push(pane("lane-a-pane", "lane-a", Some("book")));
    for width in [30, 26] {
        let (rows, hits, _) = rendered_factory_rows_at_width(&snapshot, &overlay, &ClientTreeChrome::default(), width);
        let lane = hits.tree_headers.iter().find(|hit| hit.key == "lane-a").unwrap();
        let line = rows[lane.rect.y as usize].trim_end();
        assert!(line.contains("lane-a ") && line.ends_with(" · book"), "width {width}: {line}");
    }
    snapshot.panes.pop();

    snapshot.panes = vec![pane("lane-b-pane", "lane-b", None), pane("plain-pane", "plain-a", None)];
    let (rows, _, _) = rendered_factory_rows_at_width(&snapshot, &overlay, &ClientTreeChrome::default(), 30);
    assert!(!rows.iter().any(|row| row.contains("ax42") || row.contains("book")), "no machine for local work: {rows:#?}");
}

#[test]
fn factory_host_footer_rows_fit_and_align_at_40_44_52() {
    // Alex, 2026-10-05 ~20:50 ET: "this herdr ui is shit lol, text not fitting".
    // These are the summaries the overlay writer sent at the time.
    let (snapshot, mut overlay) = lab_fixture();
    overlay.usage = vec![
        HostRow { name: "claude".into(), summary: Some("4/8 · 75%".into()), ..HostRow::default() },
        HostRow { name: "codex".into(), summary: Some("5/5 · 97%".into()), ..HostRow::default() },
    ];
    overlay.hosts = vec![
        HostRow { name: "Studio".into(),
            summary: Some("2 running · 2G free · waiting on memory · 1 kept: 1 secret".into()), ..HostRow::default() },
        HostRow { name: "PC".into(), summary: Some("3 running · 3.6G free".into()), ..HostRow::default() },
        HostRow { name: "ax42".into(),
            summary: Some("2 running · 12G free · waiting on slowdown:check".into()), ..HostRow::default() },
        HostRow { name: "forge".into(),
            summary: Some("1 running · 9.2G free · waiting on memory".into()), ..HostRow::default() },
    ];
    // Print every width first, so a failing run still shows the whole picture.
    for width in [40u16, 44, 52] {
        let (rows, _, _) = rendered_factory_rows_at_width(&snapshot, &overlay, &ClientTreeChrome::default(), width);
        eprintln!("width {width}:\n{}", rows[55..].iter().map(|row| row.trim_end()).collect::<Vec<_>>().join("\n"));
    }
    for (width, studio, ax42) in [
        // At 40 the kept note and "wait slowdown" are left out whole; a dropped
        // field leaves a trailing "…" so the cut shows at the column.
        (40u16, "studio 2 running   2G free wait mem …", " ax42   2 running  12G free …"),
        (44, "studio 2 running   2G free wait mem 1 kept…", " ax42   2 running  12G free wait slowdown"),
        (52, "studio 2 running   2G free wait mem 1 kept: 1 secr…", " ax42   2 running  12G free wait slowdown"),
    ] {
        let (rows, _, _) = rendered_factory_rows_at_width(&snapshot, &overlay, &ClientTreeChrome::default(), width);
        let footer: Vec<&str> = rows[55..].iter().map(|row| row.trim_end()).collect();
        assert_eq!(footer[0].trim(), "claude 4/8 75% · codex 5/5 97%", "width {width}");
        assert_eq!(footer[1].trim(), studio, "width {width}");
        assert_eq!(&footer[2..], [
            " pc     3 running 3.6G free",
            ax42,
            " forge  1 running 9.2G free wait mem",
        ], "width {width}");
        for row in &footer {
            assert!(row.chars().count() <= usize::from(width), "width {width}: {row:?}");
        }
    }
}

/// What a terminal shows for row `y`, with the column each grapheme lands in. It
/// replays the buffer diff: a cell under a wide grapheme is not sent, a cell next
/// to the last one sent is written without a cursor move, and the cursor moves on
/// by the grapheme's own width.
fn terminal_row(buffer: &Buffer, y: u16) -> Vec<(u16, String)> {
    let mut out = Vec::new();
    let (mut skip, mut last, mut cursor) = (0u16, None::<u16>, 0u16);
    for x in 0..buffer.area.width {
        if skip > 0 {
            skip -= 1;
            continue;
        }
        let symbol = buffer[(x, y)].symbol();
        let width = unicode_width::UnicodeWidthStr::width(symbol) as u16;
        if last != Some(x.wrapping_sub(1)) {
            cursor = x;
        }
        out.push((cursor, symbol.to_owned()));
        cursor += width;
        last = Some(x);
        skip = width.saturating_sub(1);
    }
    out
}

#[test]
fn wide_and_emoji_titles_keep_counts_and_alert_in_their_columns() {
    // Alex, 2026-10-05: "something fucked the alignment due to text overflow".
    // A title with CJK, a variation-selector emoji and a ZWJ emoji, long enough to
    // truncate, must end in "…" at its column without losing or splitting a grapheme,
    // the folded roll-up count must stay whole, and "!" must sit where it sits on a
    // plain title.
    let (snapshot, mut overlay) = fixture();
    overlay.tabs.get_mut("orch").unwrap().attention = Attention::Act;
    let (_, hits, plain) = rendered_factory_rows_at_width(&snapshot, &overlay, &ClientTreeChrome::default(), 32);
    let y = hits.tree_headers.iter().find(|hit| hit.key == "orch").unwrap().rect.y;
    let columns = |row: &[(u16, String)], symbol: &str| row.iter().filter(|(_, s)| s == symbol).map(|(x, _)| *x).collect::<Vec<_>>();
    let plain_row = terminal_row(&plain, y);
    for title in ["研究ノート orchestrator", "⚙️ orchestrator with a long name", "👩‍💻 team 研究 and a long name"] {
        overlay.tabs.get_mut("orch").unwrap().name = Some(title.into());
        let (_, hits, buffer) = rendered_factory_rows_at_width(&snapshot, &overlay, &ClientTreeChrome::default(), 32);
        assert_eq!(hits.tree_headers.iter().find(|hit| hit.key == "orch").unwrap().rect.y, y);
        let row = terminal_row(&buffer, y);
        let text: String = row.iter().map(|(_, s)| s.as_str()).collect();
        assert_eq!(columns(&row, "!"), columns(&plain_row, "!"), "{title}: {text}");
        // The folded roll-up keeps its count; "1 workflow" may shrink to "1", never past the "!".
        let count = columns(&row, "1");
        assert_eq!(count.len(), 1, "{title}: {text}");
        assert!(count[0] >= columns(&plain_row, "1")[0] && count[0] + 1 < columns(&row, "!")[0], "{title}: {text}");
        let shown = text.trim().trim_start_matches(['▸', '▾', '●', ' ']);
        let name = shown.split('…').next().unwrap();
        assert!(title.starts_with(name) && shown.contains('…'), "{title}: shown {shown:?}");
        let end = row.iter().find(|(_, s)| s == "…").map(|(x, _)| *x).unwrap();
        assert!(end + 1 < count[0], "{title}: the ellipsis ends before the count: {text}");
    }
}
