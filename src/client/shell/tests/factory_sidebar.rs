use super::*;
use crate::client::shell::tree::{
    tree_list_entries_with_overlay, AgentPanelListEntry, ClientTreeChrome,
};
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

fn entries(
    snapshot: &ClientShellSnapshot,
    overlay: Option<&FactoryOverlay>,
    tree: &ClientTreeChrome,
) -> Vec<AgentPanelListEntry> {
    let config = ClientShellConfig::from_config(&Config::default());
    let rows = crate::client::shell::agent_sidebar::agent_rows(snapshot, &config, None);
    tree_list_entries_with_overlay(snapshot, tree, rows, overlay)
}

fn labels(entries: &[AgentPanelListEntry]) -> Vec<String> {
    entries
        .iter()
        .map(|entry| match entry {
            AgentPanelListEntry::SpaceHeader(row) => format!("space:{}", row.label),
            AgentPanelListEntry::FactorySection { label, .. } => format!("section:{label}"),
            AgentPanelListEntry::FactoryTab(row) => {
                format!("tag:{}:{}", row.header.label, row.header.indent)
            }
            AgentPanelListEntry::FactoryBackground { count, .. } => format!("background:{count}"),
            AgentPanelListEntry::FactoryHost { name, .. } => format!("host:{name}"),
            AgentPanelListEntry::TabHeader(row) => format!("tab:{}", row.label),
            AgentPanelListEntry::Agent(row) => format!("agent:{}", row.pane_id),
            _ => "other".into(),
        })
        .collect()
}

#[test]
fn off_and_other_space_keep_the_stock_tree() {
    let (mut snapshot, overlay) = fixture();
    let tree = ClientTreeChrome::default();
    assert_eq!(
        labels(&entries(&snapshot, None, &tree)),
        labels(&entries(&snapshot, Some(&FactoryOverlay::default()), &tree))
    );
    let other_tab = ClientShellTab {
        tab_id: "other-tab".into(),
        workspace_id: "ws_2".into(),
        number: 1,
        label: "other".into(),
        custom_label: true,
        zoomed: false,
        focused: false,
        agent_status: AgentStatus::Idle,
    };
    snapshot.tabs.push(other_tab);
    snapshot.workspaces.push(ClientShellWorkspace {
        workspace_id: "ws_2".into(),
        active_tab_id: "other-tab".into(),
        new_workspace_cwd: "".into(),
        number: 2,
        label: "untagged space".into(),
        custom_label: true,
        branch: None,
        git_ahead_behind: None,
        tokens: Vec::new(),
        worktree: None,
        focused: false,
        agent_status: AgentStatus::Idle,
        orchestrator_mode: false,
        tab_count: 1,
        visible_in_profile: true,
    });
    snapshot.agents.push(ClientShellAgent {
        pane_id: "other-pane".into(),
        workspace_id: "ws_2".into(),
        tab_id: "other-tab".into(),
        name: Some("agent".into()),
        display_agent: Some("agent".into()),
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
    let ordinary = labels(&entries(&snapshot, None, &tree));
    let tagged = labels(&entries(&snapshot, Some(&overlay), &tree));
    // The untagged space and everything under it draw exactly as without an overlay.
    let from = |rows: &[String]| {
        let start = rows
            .iter()
            .position(|row| row == "space:untagged space")
            .unwrap();
        rows[start..].to_vec()
    };
    assert_eq!(from(&ordinary), from(&tagged));
}

#[test]
fn groups_tabs_and_expands_lanes_only_for_focus_or_toggle() {
    let (mut snapshot, mut overlay) = fixture();
    let mut tree = ClientTreeChrome::default();
    let group = labels(&entries(&snapshot, Some(&overlay), &tree));
    assert_eq!(
        group,
        [
            "space:client-shell",
            "section:ORCHESTRATOR",
            "tag:orch:1",
            "tag:orphan:2",
            "section:LANES",
            "tag:lane-a:1",
            "tag:lane-b:1",
            "background:1",
            "section:TABS",
            "tab:plain-a",
            "agent:plain-pane",
            "tab:plain-b"
        ]
    );
    let rows = entries(&snapshot, Some(&overlay), &tree);
    let lane = rows
        .iter()
        .find_map(|entry| match entry {
            AgentPanelListEntry::FactoryTab(row) if row.header.label == "lane-a" => Some(row),
            _ => None,
        })
        .unwrap();
    assert_eq!(lane.summary.as_deref(), Some("2"));
    assert!(lane.header.collapsed);
    overlay.tabs.get_mut("wf-b").unwrap().attention = Attention::Act;
    let group = labels(&entries(&snapshot, Some(&overlay), &tree));
    assert!(!group.contains(&"tag:wf-a:2".to_owned()));
    let lane = entries(&snapshot, Some(&overlay), &tree).into_iter()
        .find_map(|entry| match entry {
            AgentPanelListEntry::FactoryTab(row) if row.header.label == "lane-a" => Some(row),
            _ => None,
        }).unwrap();
    assert_eq!(lane.attention, Attention::Act);
    overlay.tabs.get_mut("wf-b").unwrap().attention = Attention::None;
    snapshot.focused_tab_id = Some("wf-a".into());
    assert!(labels(&entries(&snapshot, Some(&overlay), &tree)).contains(&"tag:wf-a:2".into()));
    snapshot.focused_tab_id = Some("other".into());
    tree.factory_expanded_lanes.insert("lane-a".into());
    tree.factory_background_expanded.insert("ws_1".into());
    let encoded = serde_json::to_string(&tree.to_preferences()).unwrap();
    let decoded = serde_json::from_str(&encoded).unwrap();
    let restored = ClientTreeChrome::from_preferences(decoded);
    let group = labels(&entries(&snapshot, Some(&overlay), &restored));
    assert!(group.contains(&"tag:wf-a:2".into()));
    assert!(group.contains(&"tag:advisor:2".into()));
    assert!(!group.contains(&"tag:done:2".into()));
}

#[test]
fn frame_shows_workflow_badge_section_and_space_attention() {
    let (snapshot, mut overlay) = fixture();
    overlay.spaces.insert(
        "ws_1".into(),
        SpaceTag {
            attention: Attention::Act,
            target_tab: Some("wf-a".into()),
            summary: Some("needs you".into()),
        },
    );
    let mut tree = ClientTreeChrome::default();
    tree.factory_expanded_lanes.insert("lane-a".into());
    let mut config = ClientShellConfig::from_config(&Config::default());
    config.agent_panel_sort = crate::config::AgentPanelSortConfig::Tree;
    let mut buffer = Buffer::empty(Rect::new(0, 0, 60, 35));
    let mut hits = ShellHitMap::default();
    let mut scroll = 0;
    crate::client::shell::agent_sidebar::render_agent_panel_with_overlay(
        &mut buffer,
        Rect::new(0, 0, 60, 35),
        &snapshot,
        &config,
        &tree,
        Some(&overlay),
        &mut scroll,
        &mut hits,
    );
    let frame = FrameData::from_ratatui_buffer_with_hyperlinks(&buffer, None, &[]);
    let rows = frame_rows(&frame);
    assert!(rows.iter().any(|row| row.contains("LANES")));
    assert!(rows
        .iter()
        .any(|row| row.contains("needs you") && row.contains('●')));
    let workflow = rows.iter().find(|row| row.contains("wf-a")).unwrap();
    assert!(!workflow.contains("review 3/5"));
    assert!(workflow.trim_end().ends_with("PC"));
    let progress = rows.iter().find(|row| row.contains("review 3/5")).unwrap();
    assert!(!progress.contains("PC"));
    assert!(hits
        .tree_headers
        .iter()
        .any(|hit| hit.tab_id.as_deref() == Some("wf-a")));
}

#[test]
fn actionable_space_jump_focuses_its_tagged_tab_after_workspace() {
    let (snapshot, mut overlay) = fixture();
    overlay.spaces.insert(
        "ws_1".into(),
        SpaceTag {
            attention: Attention::Act,
            target_tab: Some("wf-a".into()),
            summary: None,
        },
    );
    let mut config = ClientShellConfig::from_config(&Config::default());
    config.factory.enabled = true;
    let mut state = ClientShellState::new(config);
    state.set_snapshot(Box::new(snapshot));
    state.factory_overlay = Some(std::sync::Arc::new(overlay));
    let mut outcome = ClientShellInput::default();
    state.focus_factory_space_target("ws_1", &mut outcome);
    assert!(
        matches!(&outcome.actions[..], [ClientShellAction::Endpoint { request, .. }]
        if matches!(&request.method, crate::api::schema::Method::TabFocus(target) if target.tab_id == "wf-a"))
    );
    let mut outcome = ClientShellInput::default();
    state.focus_factory_space_target("ws_2", &mut outcome);
    assert!(outcome.actions.is_empty());
}

fn factory_state(snapshot: ClientShellSnapshot, overlay: FactoryOverlay) -> ClientShellState {
    let mut config = ClientShellConfig::from_config(&Config::default());
    config.factory.enabled = true;
    let mut state = ClientShellState::new(config);
    state.set_snapshot(Box::new(snapshot));
    state.factory_overlay = Some(std::sync::Arc::new(overlay));
    state
}

fn press_cycle(
    state: &mut ClientShellState,
    action: crate::input::KeybindAction,
) -> Option<String> {
    let mut outcome = ClientShellInput::default();
    state.record_binding(crate::input::KeybindMatch::Action(action), &mut outcome);
    match &outcome.actions[..] {
        [ClientShellAction::Endpoint { request, .. }] => match &request.method {
            crate::api::schema::Method::TabFocus(target) => Some(target.tab_id.clone()),
            other => Some(format!("other:{other:?}")),
        },
        _ => None,
    }
}

#[test]
fn space_row_takes_the_worst_attention_of_its_tabs() {
    let (snapshot, mut overlay) = fixture();
    let tree = ClientTreeChrome::default();
    let space_dot = |overlay: &FactoryOverlay| {
        entries(&snapshot, Some(overlay), &tree)
            .into_iter()
            .find_map(|entry| match entry {
                AgentPanelListEntry::SpaceHeader(row) => Some(row.space_attention),
                _ => None,
            })
            .unwrap()
    };
    assert_eq!(space_dot(&overlay), None);
    overlay.tabs.get_mut("wf-a").unwrap().attention = Attention::Warn;
    assert_eq!(space_dot(&overlay).map(|dot| dot.0), Some(Attention::Warn));
    overlay.tabs.get_mut("lane-b").unwrap().attention = Attention::Act;
    assert_eq!(space_dot(&overlay).map(|dot| dot.0), Some(Attention::Act));
    // A space tag below the rolled-up tab attention never lowers it.
    overlay.spaces.insert(
        "ws_1".into(),
        SpaceTag {
            attention: Attention::Warn,
            target_tab: None,
            summary: Some("1".into()),
        },
    );
    assert_eq!(space_dot(&overlay), Some((Attention::Act, "1".to_string())));
}

#[test]
fn idle_lane_and_background_rows_draw_dimmed() {
    let (snapshot, overlay) = fixture();
    let mut tree = ClientTreeChrome::default();
    tree.factory_background_expanded.insert("ws_1".into());
    let mut config = ClientShellConfig::from_config(&Config::default());
    config.agent_panel_sort = crate::config::AgentPanelSortConfig::Tree;
    let area = Rect::new(0, 0, 60, 35);
    let mut buffer = Buffer::empty(area);
    let mut scroll = 0;
    crate::client::shell::agent_sidebar::render_agent_panel_with_overlay(
        &mut buffer,
        area,
        &snapshot,
        &config,
        &tree,
        Some(&overlay),
        &mut scroll,
        &mut ShellHitMap::default(),
    );
    let dimmed = |label: &str| {
        (0..area.height).any(|y| {
            let text = (0..area.width)
                .map(|x| buffer[(x, y)].symbol().to_string())
                .collect::<String>();
            text.contains(label)
                && (0..area.width).any(|x| {
                    buffer[(x, y)].symbol() == &label[..1]
                        && buffer[(x, y)].modifier.contains(Modifier::DIM)
                })
        })
    };
    assert!(dimmed("lane-b"), "idle lane is dimmed");
    assert!(dimmed("advisor"), "background rows are dimmed");
    assert!(!dimmed("lane-a"), "a live lane is not dimmed");
}

#[test]
fn cmd_e_cycles_only_rows_that_want_the_user() {
    use crate::input::KeybindAction::{NextAgent, PreviousAgent};
    let (snapshot, mut overlay) = fixture();
    overlay.tabs.get_mut("wf-a").unwrap().attention = Attention::Warn;
    overlay.tabs.get_mut("wf-b").unwrap().attention = Attention::Act;
    // Background tabs never catch the key, even when they ask.
    overlay.tabs.get_mut("advisor").unwrap().attention = Attention::Act;
    let mut state = factory_state(snapshot.clone(), overlay.clone());
    // Most urgent first from an unrelated focus.
    assert_eq!(press_cycle(&mut state, NextAgent).as_deref(), Some("wf-b"));
    assert_eq!(
        press_cycle(&mut state, PreviousAgent).as_deref(),
        Some("wf-b")
    );

    // From the act row, the remaining asking row is next; plain tabs never are.
    let mut focused = snapshot.clone();
    focused.focused_tab_id = Some("wf-b".into());
    let mut state = factory_state(focused, overlay.clone());
    assert_eq!(press_cycle(&mut state, NextAgent).as_deref(), Some("wf-a"));

    // Off: the stock rotation runs and never focuses a tab by id.
    let mut config = ClientShellConfig::from_config(&Config::default());
    config.factory.enabled = false;
    let mut state = ClientShellState::new(config);
    state.set_snapshot(Box::new(snapshot.clone()));
    state.factory_overlay = Some(std::sync::Arc::new(overlay.clone()));
    assert!(!matches!(
        press_cycle(&mut state, NextAgent).as_deref(),
        Some("wf-a" | "wf-b")
    ));

    // On, but nothing wants the user: today's behavior.
    let mut calm = overlay;
    for tag in calm.tabs.values_mut() {
        tag.attention = Attention::None;
    }
    let mut state = factory_state(snapshot, calm);
    assert!(state
        .factory_attention_tabs(state.snapshot.as_deref().unwrap())
        .is_empty());
    assert!(!matches!(
        press_cycle(&mut state, NextAgent).as_deref(),
        Some("wf-a" | "wf-b")
    ));
}

#[test]
fn collapse_survives_focus_landing_on_a_tagged_space() {
    let (mut snapshot, overlay) = fixture();
    snapshot.panes.push(crate::protocol::ClientShellPane {
        pane_id: "wf-pane".into(),
        workspace_id: "ws_1".into(),
        tab_id: "wf-a".into(),
        label: None,
        cwd: None,
        foreground_cwd: None,
        focused: true,
        right_click_passthrough: false,
    });
    snapshot.focused_pane_id = Some("wf-pane".into());
    snapshot.focused_tab_id = Some("wf-a".into());
    let mut state = factory_state(snapshot.clone(), overlay);
    state.config.agent_panel_sort = crate::config::AgentPanelSortConfig::Tree;
    state
        .tree_chrome_mut()
        .collapsed_spaces
        .insert("ws_1".into());
    state
        .tree_chrome_mut()
        .collapsed_tabs
        .insert("ws_1#1".into());
    assert!(!state.reveal_tree_ancestors_for_pane("wf-pane"));
    assert!(state.tree_chrome_mut().collapsed_spaces.contains("ws_1"));
    assert!(state.tree_chrome_mut().collapsed_tabs.contains("ws_1#1"));

    // Overlay off: the stock reveal still opens the folds.
    let mut config = ClientShellConfig::from_config(&Config::default());
    config.agent_panel_sort = crate::config::AgentPanelSortConfig::Tree;
    let mut stock = ClientShellState::new(config);
    stock.set_snapshot(Box::new(snapshot));
    stock
        .tree_chrome_mut()
        .collapsed_spaces
        .insert("ws_1".into());
    assert!(stock.reveal_tree_ancestors_for_pane("wf-pane"));
    assert!(!stock.tree_chrome_mut().collapsed_spaces.contains("ws_1"));
}

#[test]
fn lane_fold_persists_when_the_lane_row_takes_focus() {
    let (mut snapshot, overlay) = fixture();
    let tree = ClientTreeChrome::default();
    snapshot.focused_tab_id = Some("lane-a".into());
    let group = labels(&entries(&snapshot, Some(&overlay), &tree));
    assert!(group.contains(&"tag:lane-a:1".into()));
    assert!(!group.contains(&"tag:wf-a:2".into()));
}

fn composed_sidebar_text(config: Config, overlay: FactoryOverlay) -> String {
    let (snapshot, _) = fixture();
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&config));
    state.set_snapshot(Box::new(snapshot));
    state.factory_overlay = Some(std::sync::Arc::new(overlay));
    state.set_pane_surface(surface());
    let frame = state.compose(120, 50).expect("sidebar frame");
    frame
        .cells
        .chunks(frame.width as usize)
        .map(|row| row.iter().map(|cell| cell.symbol.as_str()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

fn rendered_factory_rows(snapshot: &ClientShellSnapshot, overlay: &FactoryOverlay) -> (Vec<String>, ShellHitMap, Buffer) {
    rendered_factory_rows_with_tree(snapshot, overlay, &ClientTreeChrome::default())
}

fn rendered_factory_rows_with_tree(snapshot: &ClientShellSnapshot, overlay: &FactoryOverlay, tree: &ClientTreeChrome) -> (Vec<String>, ShellHitMap, Buffer) {
    let area = Rect::new(0, 0, 25, 60);
    let mut buffer = Buffer::empty(area);
    let mut config = ClientShellConfig::from_config(&Config::default());
    config.agent_panel_sort = crate::config::AgentPanelSortConfig::Tree;
    config.agents.row_gap = 1;
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

#[test]
fn factory_rows_fit_chevron_glyph_summary_devloop_and_hosts_at_25_columns() {
    let (mut snapshot, mut overlay) = fixture();
    snapshot.tabs.retain(|tab| !matches!(tab.tab_id.as_str(), "orphan" | "advisor" | "done" | "plain-a" | "plain-b"));
    snapshot.agents.clear();
    let long_name = "abcdefghijklmnopqrst";
    overlay.tabs.get_mut("lane-a").unwrap().name = Some(long_name.into());
    overlay.tabs.get_mut("lane-a").unwrap().devloop = true;
    overlay.tabs.get_mut("lane-a").unwrap().summary = Some("stale 99 wf".into());
    overlay.tabs.get_mut("lane-b").unwrap().idle = false;
    overlay.tabs.get_mut("lane-b").unwrap().name = Some("no status".into());
    snapshot.tabs.iter_mut().find(|tab| tab.tab_id == "lane-b").unwrap().agent_status = AgentStatus::Unknown;
    overlay.hosts = vec![
        HostRow { name: "Studio".into(), summary: Some("load 48/16".into()), attention: Attention::Warn },
        HostRow { name: "PC".into(), summary: Some("3/28 live".into()), attention: Attention::None },
        HostRow { name: "extraordinarily-long-host".into(), summary: Some("0/4".into()), attention: Attention::None },
    ];
    let (rows, hits, buffer) = rendered_factory_rows(&snapshot, &overlay);
    let lane = rows.iter().find(|row| row.contains(long_name.chars().take(6).collect::<String>().as_str())).unwrap_or_else(|| panic!("{rows:?}"));
    assert!(lane.contains("▸ ●") && lane.contains("⟳"), "{lane:?}");
    assert!(lane.trim_end().ends_with('2'), "{lane:?}");
    assert!(rows.iter().any(|row| row.contains("● no status")), "{rows:?}");
    assert!(rows.iter().any(|row| row.contains("ORCHESTRATOR")));
    assert!(rows.iter().any(|row| row.contains("● orch") && row.trim_end().ends_with("inbox 3")), "{rows:?}");
    let hosts_y = rows.iter().position(|row| row.contains("HOSTS")).unwrap();
    assert!(rows[hosts_y + 1].contains("Studio") && rows[hosts_y + 1].trim_end().ends_with("load 48/16"));
    assert!(rows[hosts_y + 2].contains("PC") && rows[hosts_y + 2].trim_end().ends_with("3/28 live"));
    assert!(rows[hosts_y + 3].contains('…') && rows[hosts_y + 3].trim_end().ends_with("0/4"));
    let warn_x = rows[hosts_y + 1].find("load").unwrap() as u16;
    assert_eq!(buffer[(warn_x, (hosts_y + 1) as u16)].fg, ClientShellConfig::from_config(&Config::default()).palette.peach);
    assert!(hits.tree_headers.iter().all(|hit| !((hosts_y + 1)..=(hosts_y + 3)).contains(&(hit.rect.y as usize))));
    let (without, _, _) = rendered_factory_rows(&snapshot, &fixture().1);
    assert!(!without.iter().any(|row| row.contains("HOSTS")));
}

#[test]
fn tagged_spaces_show_one_hosts_footer_after_all_spaces() {
    let (mut snapshot, mut overlay) = fixture();
    snapshot.workspaces.push(ClientShellWorkspace {
        workspace_id: "ws_2".into(), active_tab_id: "other-lane".into(),
        new_workspace_cwd: String::new(), number: 2, label: "poker".into(),
        custom_label: true, branch: None, git_ahead_behind: None, tokens: Vec::new(),
        worktree: None, focused: false, agent_status: AgentStatus::Idle,
        orchestrator_mode: false, tab_count: 1, visible_in_profile: true,
    });
    snapshot.tabs.push(ClientShellTab {
        tab_id: "other-lane".into(), workspace_id: "ws_2".into(), number: 1,
        label: "poker coach".into(), custom_label: true, zoomed: false,
        focused: false, agent_status: AgentStatus::Working,
    });
    overlay.tabs.insert("other-lane".into(), TabTag { kind: TabKind::Lane, ..TabTag::default() });
    overlay.hosts.push(HostRow { name: "Studio".into(), summary: None, attention: Attention::None });
    let (rows, _, _) = rendered_factory_rows(&snapshot, &overlay);
    assert_eq!(rows.iter().filter(|row| row.contains("HOSTS")).count(), 1, "{rows:?}");
    let hosts = rows.iter().position(|row| row.contains("HOSTS")).unwrap();
    let poker = rows.iter().position(|row| row.contains("poker")).unwrap();
    assert!(hosts > poker, "{rows:?}");
    assert_eq!(hosts + 2, rows.len(), "footer should end immediately above the menu: {rows:?}");
    // If only the lane space is tagged, that space receives the hosts instead.
    overlay.tabs.retain(|id, _| id == "other-lane");
    let (rows, _, _) = rendered_factory_rows(&snapshot, &overlay);
    assert_eq!(rows.iter().filter(|row| row.contains("HOSTS")).count(), 1, "{rows:?}");
    assert!(rows.iter().position(|row| row.contains("HOSTS")).unwrap()
        > rows.iter().position(|row| row.contains("poker")).unwrap());
}

#[test]
fn workflow_siblings_and_lane_metadata_remain_readable_at_25_columns() {
    let (mut snapshot, mut overlay) = fixture();
    snapshot.tabs.retain(|tab| matches!(tab.tab_id.as_str(), "lane-a" | "lane-b"));
    overlay.tabs.get_mut("lane-a").unwrap().name = Some("issues".into());
    overlay.tabs.get_mut("lane-a").unwrap().summary = Some("stale count".into());
    overlay.tabs.get_mut("lane-b").unwrap().name = Some("conversation evals".into());
    for n in 1..=8 {
        let id = format!("wf-{n}");
        snapshot.tabs.push(ClientShellTab {
            tab_id: id.clone(), workspace_id: "ws_1".into(), number: n + 2,
            label: format!("wf issues {n}"), custom_label: true, zoomed: false,
            focused: false, agent_status: AgentStatus::Working,
        });
        overlay.tabs.insert(id, TabTag {
            kind: TabKind::Workflow, parent: Some("lane-a".into()),
            badge: Some("Studio".into()), phase: Some("Verify 6/7".into()),
            done: n == 8, ..TabTag::default()
        });
    }
    let collapsed = rendered_factory_rows(&snapshot, &overlay).0;
    let collapsed_lane = collapsed.iter().find(|row| row.contains("issues")).unwrap();
    assert!(collapsed_lane.trim_end().ends_with("7") && !collapsed_lane.contains("done"), "{collapsed_lane:?}");
    let idle = collapsed.iter().find(|row| row.contains("conversation")).unwrap_or_else(|| panic!("{collapsed:?}"));
    assert!(idle.contains('○') && idle.trim_end().ends_with("idle"), "{idle:?}");
    let mut tree = ClientTreeChrome::default();
    tree.factory_expanded_lanes.insert("lane-a".into());
    let (expanded, _, buffer) = rendered_factory_rows_with_tree(&snapshot, &overlay, &tree);
    let expanded_lane = expanded.iter().find(|row| row.contains("issues")).unwrap();
    assert_eq!(expanded_lane.replace('▾', "▸"), *collapsed_lane);
    for n in 1..=7 {
        let row = expanded.iter().find(|row| row.contains(&format!("issues {n}")))
            .unwrap_or_else(|| panic!("missing issues {n}: {expanded:?}"));
        assert!(row.trim_end().ends_with("Studio") && row.contains('◐'), "{row:?}");
    }
    assert!(!expanded.iter().any(|row| row.contains('✓')));
    let _ = buffer;
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
fn compact_factory_rows_and_click_targets_work_at_25_columns_with_gap_one() {
    let (mut snapshot, mut overlay) = fixture();
    snapshot.tabs.retain(|tab| matches!(tab.tab_id.as_str(), "orch" | "lane-a" | "wf-a"));
    snapshot.agents.clear();
    overlay.tabs.get_mut("wf-a").unwrap().name = Some("wf issues 3".into());
    overlay.tabs.get_mut("wf-a").unwrap().badge = Some("Studio".into());
    let mut state = factory_state(snapshot.clone(), overlay.clone());
    state.config.agent_panel_sort = crate::config::AgentPanelSortConfig::Tree;
    state.config.agents.row_gap = 1;
    state.config.factory.enabled = true;
    let tree = state.tree_chrome_mut();
    tree.factory_expanded_lanes.insert("lane-a".into());
    let (rows, hits, _) = rendered_factory_rows_with_tree(&snapshot, &overlay, tree);
    let section = rows.iter().position(|row| row.contains("ORCHESTRATOR")).unwrap();
    let space = rows.iter().position(|row| row.contains("client-shell")).unwrap();
    assert_eq!(section, space + 1, "{rows:?}");
    let orch = rows.iter().position(|row| row.contains("● orch")).unwrap();
    let lane = rows.iter().position(|row| row.contains("● lane-a")).unwrap();
    let workflow = rows.iter().position(|row| row.contains("issues 3")).unwrap();
    assert_eq!(orch, section + 1, "{rows:?}");
    assert_eq!(workflow, lane + 1, "{rows:?}");
    assert!(rows[workflow + 1].contains("review 3/5"), "{rows:?}");
    assert!(rows[workflow].contains("issues 3") && rows[workflow].trim_end().ends_with("Studio"));
    assert!(!rows[1].contains("usage") && !rows[1].contains("tree"));
    let lane_hit = hits.tree_headers.iter().find(|hit| hit.tab_id.as_deref() == Some("lane-a")).unwrap();
    let workflow_hit = hits.tree_headers.iter().find(|hit| hit.tab_id.as_deref() == Some("wf-a")).unwrap();
    let lane_point = (lane_hit.rect.x + 9, lane_hit.rect.y);
    let wf_point = (workflow_hit.rect.x + 9, workflow_hit.rect.y);
    assert_eq!(lane_hit.chevron.width, 2);
    let chevron = (lane_hit.chevron.x + 1, lane_hit.chevron.y);
    let glyph = (lane_hit.chevron.x + 2, lane_hit.chevron.y);
    state.hits = hits;
    state.last_composed_size = Some((120, 60));
    factory_click(&mut state, MouseEventKind::Down(MouseButton::Left), lane_point.0, lane_point.1);
    let focus = factory_click(&mut state, MouseEventKind::Up(MouseButton::Left), lane_point.0 + 1, lane_point.1);
    assert_eq!(focused_tab(&focus), ["lane-a"]);
    assert!(state.detail_panel.is_none());
    factory_click(&mut state, MouseEventKind::Down(MouseButton::Left), wf_point.0, wf_point.1);
    let focus = factory_click(&mut state, MouseEventKind::Up(MouseButton::Left), wf_point.0, wf_point.1);
    assert_eq!(focused_tab(&focus), ["wf-a"]);
    assert!(state.detail_panel.is_none());
    factory_click(&mut state, MouseEventKind::Down(MouseButton::Left), chevron.0, chevron.1);
    let toggle = factory_click(&mut state, MouseEventKind::Up(MouseButton::Left), chevron.0, chevron.1);
    assert!(focused_tab(&toggle).is_empty());
    assert!(!state.tree_chrome_mut().factory_expanded_lanes.contains("lane-a"));

    factory_click(&mut state, MouseEventKind::Down(MouseButton::Left), glyph.0, glyph.1);
    let glyph_focus = factory_click(&mut state, MouseEventKind::Up(MouseButton::Left), glyph.0, glyph.1);
    assert_eq!(focused_tab(&glyph_focus), ["lane-a"]);
    assert!(!state.tree_chrome_mut().factory_expanded_lanes.contains("lane-a"));

    factory_click(&mut state, MouseEventKind::Down(MouseButton::Left), lane_point.0, lane_point.1);
    let drift = factory_click(&mut state, MouseEventKind::Up(MouseButton::Left), lane_point.0 + 10, lane_point.1);
    assert_eq!(focused_tab(&drift), ["lane-a"]);
    factory_click(&mut state, MouseEventKind::Down(MouseButton::Left), lane_point.0, lane_point.1);
    let adjacent = factory_click(&mut state, MouseEventKind::Up(MouseButton::Left), lane_point.0, lane_point.1 + 1);
    assert!(focused_tab(&adjacent).is_empty());
    assert!(state.detail_panel.is_none());

    let alt = |kind, x, y| RawInputEvent::Mouse(crossterm::event::MouseEvent {
        kind, column: x, row: y, modifiers: KeyModifiers::ALT,
    });
    let detail = state.handle_raw_events(vec![
        alt(MouseEventKind::Down(MouseButton::Left), lane_point.0, lane_point.1),
        alt(MouseEventKind::Up(MouseButton::Left), lane_point.0, lane_point.1),
    ]);
    assert!(focused_tab(&detail).is_empty());
    let panel = state.detail_panel.as_ref().expect("Alt click opens detail");
    assert_eq!(panel.key, "tab:lane-a");
    assert!(!panel.focused, "keyboard focus remains outside the detail panel");
}

#[test]
fn live_children_count_done_without_rendering_done_rows_and_color_collapsed_lane() {
    let (mut snapshot, mut overlay) = fixture();
    snapshot.tabs.retain(|tab| matches!(tab.tab_id.as_str(), "orch" | "lane-a" | "wf-a" | "wf-b"));
    snapshot.agents.clear();
    for n in 0..3 {
        let id = format!("finished-{n}");
        snapshot.tabs.push(ClientShellTab {
            tab_id: id.clone(), workspace_id: "ws_1".into(), number: n + 11,
            label: format!("wf finished {n}"), custom_label: true, zoomed: false,
            focused: false, agent_status: AgentStatus::Done,
        });
        overlay.tabs.insert(id, TabTag {
            kind: TabKind::Workflow, parent: Some("lane-a".into()), done: true,
            ..TabTag::default()
        });
    }
    overlay.tabs.get_mut("lane-a").unwrap().summary = Some("stale 99".into());
    overlay.tabs.get_mut("wf-b").unwrap().attention = Attention::Act;
    let (collapsed, _, buffer) = rendered_factory_rows(&snapshot, &overlay);
    let lane_y = collapsed.iter().position(|row| row.contains("lane-a")).unwrap() as u16;
    assert!(collapsed[lane_y as usize].trim_end().ends_with("2") && !collapsed[lane_y as usize].contains("done"));
    assert!(!collapsed.iter().any(|row| row.contains('✓') || row.contains("finished-")));
    assert!(!collapsed.iter().any(|row| row.contains("wf-b")));
    let glyph_x = collapsed[lane_y as usize].chars().position(|ch| ch == '●').unwrap() as u16;
    let palette = ClientShellConfig::from_config(&Config::default()).palette;
    assert_eq!(buffer[(glyph_x, lane_y)].fg, palette.red);
    let mut tree = ClientTreeChrome::default();
    tree.factory_expanded_lanes.insert("lane-a".into());
    let (expanded, _, _) = rendered_factory_rows_with_tree(&snapshot, &overlay, &tree);
    assert!(expanded.iter().find(|row| row.contains("lane-a")).unwrap().trim_end().ends_with("2"));
    assert!(!expanded.iter().any(|row| row.contains('✓')));
    overlay.tabs.get_mut("wf-a").unwrap().done = true;
    overlay.tabs.get_mut("wf-b").unwrap().done = true;
    // Use exactly three done children, all still in the live snapshot.
    snapshot.tabs.retain(|tab| !matches!(tab.tab_id.as_str(), "finished-1" | "finished-2"));
    let (all_done, _, _) = rendered_factory_rows(&snapshot, &overlay);
    assert!(!all_done.iter().find(|row| row.contains("lane-a")).unwrap().contains("done"));
}

#[test]
fn tagged_space_keeps_full_name_when_count_and_controls_compete_at_25_columns() {
    let (mut snapshot, mut overlay) = fixture();
    snapshot.workspaces[0].label = "agent-rails".into();
    overlay.spaces.insert("ws_1".into(), SpaceTag {
        attention: Attention::Act, target_tab: None, summary: Some("13".into()),
    });
    let (rows, _, _) = rendered_factory_rows(&snapshot, &overlay);
    let header = rows.iter().find(|row| row.contains("agent-rails")).unwrap();
    assert!(header.contains("agent-rails") && !header.contains('⚲') && !header.contains('+'), "{header:?}");
    assert!(header.contains("● 13"), "{header:?}");

    let area = Rect::new(0, 0, 40, 20);
    let mut buffer = Buffer::empty(area);
    let mut config = ClientShellConfig::from_config(&Config::default());
    config.agent_panel_sort = crate::config::AgentPanelSortConfig::Tree;
    let mut scroll = 0;
    crate::client::shell::agent_sidebar::render_agent_panel_with_overlay(
        &mut buffer, area, &snapshot, &config, &ClientTreeChrome::default(),
        Some(&overlay), &mut scroll, &mut ShellHitMap::default(),
    );
    let wide = (0..area.width).map(|x| buffer[(x, 2)].symbol()).collect::<String>();
    assert!(wide.contains("agent-rails") && wide.contains('⚲') && wide.contains('+') && wide.contains("● 13"), "{wide:?}");
}

#[test]
fn non_factory_spacer_belongs_to_its_previous_clickable_row() {
    let (snapshot, _) = fixture();
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
    let space = hits.tree_headers.iter().find(|hit| hit.tab_id.is_none() && hit.key == "ws_1").unwrap();
    assert_eq!(space.rect.height, 2);
    let gap_y = space.rect.y + 1;
    assert_eq!(buffer[(0, gap_y)].symbol(), " ");
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

#[test]
fn enabling_factory_groups_the_default_spaces_sort() {
    let (_, overlay) = fixture();
    let mut config = Config::default();
    assert_eq!(
        config.ui.agent_panel_sort,
        crate::config::AgentPanelSortConfig::Spaces
    );
    config.ui.factory.enabled = true;
    let text = composed_sidebar_text(config, overlay);
    for heading in ["ORCHESTRATOR", "LANES"] {
        assert!(text.contains(heading), "missing {heading}: {text}");
    }
    assert!(text.contains("background"), "frame: {text}");
}

#[test]
fn factory_disabled_through_config_draws_no_grouping_even_with_an_overlay() {
    let (_, overlay) = fixture();
    // The tree sort would draw the grouping if the enabled gate were removed.
    let mut config = Config::default();
    config.ui.agent_panel_sort = crate::config::AgentPanelSortConfig::Tree;
    config.ui.factory.enabled = false;
    let text = composed_sidebar_text(config, overlay);
    assert!(!text.contains("ORCHESTRATOR"), "frame: {text}");
    assert!(!text.contains("LANES"), "frame: {text}");
}

#[test]
fn factory_done_only_lanes_keep_names_and_blank_right_slots_at_25_columns() {
    let (mut snapshot, mut overlay) = fixture();
    snapshot.tabs.retain(|tab| matches!(tab.tab_id.as_str(), "lane-a" | "lane-b" | "wf-a" | "wf-b" | "done"));
    for id in ["wf-a", "wf-b", "done"] {
        let tag = overlay.tabs.get_mut(id).unwrap();
        tag.parent = Some("lane-a".into());
        tag.done = true;
    }
    overlay.tabs.get_mut("lane-a").unwrap().name = Some("always-on infra".into());
    overlay.tabs.get_mut("lane-a").unwrap().summary = Some("3 done".into());
    overlay.tabs.get_mut("lane-b").unwrap().name = Some("local dev loop".into());
    overlay.tabs.get_mut("lane-b").unwrap().devloop = true;
    overlay.tabs.get_mut("lane-b").unwrap().idle = false;
    let (rows, _, _) = rendered_factory_rows(&snapshot, &overlay);
    let lane = rows.iter().find(|row| row.contains("always-on infra")).unwrap();
    assert!(lane.contains("● always-on infra") && !lane.contains("done"), "{lane:?}");
    assert!(rows.iter().any(|row| row.contains("● local dev loop ⟳")), "{rows:?}");
    assert!(!rows.iter().any(|row| row.contains("wf-a") || row.contains("wf-b") || row.contains("✓")));
}

#[test]
fn factory_progress_age_row_click_focuses_workflow_at_25_columns() {
    let (mut snapshot, mut overlay) = fixture();
    snapshot.tabs.retain(|tab| matches!(tab.tab_id.as_str(), "lane-a" | "wf-a" | "done"));
    snapshot.agents.clear();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    overlay.tabs.get_mut("wf-a").unwrap().phase = Some("Verify 3/5".into());
    overlay.tabs.get_mut("wf-a").unwrap().started = Some((now - 22 * 60) as i64);
    overlay.tabs.get_mut("done").unwrap().phase = Some("Verify 2/5".into());
    overlay.tabs.get_mut("done").unwrap().started = Some((now - 90 * 60) as i64);
    let mut tree = ClientTreeChrome::default();
    tree.factory_expanded_lanes.insert("lane-a".into());
    let (rows, hits, _) = rendered_factory_rows_with_tree(&snapshot, &overlay, &tree);
    let workflow = rows.iter().position(|row| row.contains("◐ wf-a")).unwrap();
    assert!(rows[workflow + 1].contains("verify 3/5 · 22m"), "{rows:?}");
    assert!(!rows.iter().any(|row| row.contains("Verify 2/5") || row.contains("90m") || row.contains("done")));
    let hit_rect = hits.tree_headers.iter().find(|hit| hit.tab_id.as_deref() == Some("wf-a")).unwrap().rect;
    assert_eq!(hit_rect.y, workflow as u16);
    assert_eq!(hit_rect.height, 2);
    let mut state = factory_state(snapshot, overlay);
    state.hits = hits;
    state.last_composed_size = Some((120, 60));
    let x = hit_rect.x + 10;
    factory_click(&mut state, MouseEventKind::Down(MouseButton::Left), x, hit_rect.y + 1);
    let focus = factory_click(&mut state, MouseEventKind::Up(MouseButton::Left), x, hit_rect.y + 1);
    assert_eq!(focused_tab(&focus), ["wf-a"]);
}

#[test]
fn factory_header_immediately_precedes_space_and_count_has_chevron_gap() {
    let (snapshot, mut overlay) = fixture();
    overlay.spaces.insert("ws_1".into(), SpaceTag { attention: Attention::Act, target_tab: None, summary: Some("14".into()) });
    let (rows, _, _) = rendered_factory_rows(&snapshot, &overlay);
    assert!(rows[1].contains("agents"));
    assert!(rows[2].contains("client-shell"), "{rows:?}");
    assert!(rows[2].contains("● 14 ▾"), "{:?}", rows[2]);
}
