//! Tab-bar chrome ported from the fork: the session badge and per-pane status
//! glyphs. Sources: `docs/fork/port-0.9/orig/src/ui/tabs.rs` and
//! `orig/src/app/tab_bar_status.rs`.

use super::*;
use crate::client::shell::render::{session_badge_rect, session_badge_text};

fn badge_snapshot(session_name: Option<&str>, active_profile: &str) -> ClientShellSnapshot {
    let mut snapshot = snapshot();
    snapshot.session_name = session_name.map(str::to_owned);
    snapshot.active_profile = active_profile.to_owned();
    snapshot
}

#[test]
fn badge_names_the_session_or_falls_back_to_the_active_profile() {
    for (session_name, active_profile, expected) in [
        (Some("work"), "personal", "work"),
        (Some("default"), "personal", "personal"),
        (None, "personal", "personal"),
        (Some("default"), "default", "default"),
        (None, "default", "default"),
    ] {
        assert_eq!(
            session_badge_text(&badge_snapshot(session_name, active_profile)),
            expected
        );
    }
}

#[test]
fn badge_truncates_long_names_by_display_width() {
    let snapshot = badge_snapshot(Some("提交-herdr-session-name"), "default");
    let text = session_badge_text(&snapshot);

    assert_eq!(unicode_width::UnicodeWidthStr::width(text.as_str()), 16);
    assert!(text.ends_with('…'));
}

#[test]
fn badge_reserves_right_aligned_geometry() {
    let snapshot = badge_snapshot(Some("work"), "default");
    let area = Rect::new(4, 2, 30, 1);

    let badge = session_badge_rect(&snapshot, area, true);

    assert_eq!(badge, Rect::new(30, 2, 4, 1));
}

#[test]
fn badge_is_hidden_before_mouse_chrome_squeezes_tab_width() {
    let snapshot = badge_snapshot(None, "personal");
    // "personal" plus the gap, one column short of leaving a minimum tab and
    // the new-tab control room.
    let area = Rect::new(0, 0, 8 + 1 + 8 + 3 - 1, 1);

    assert_eq!(session_badge_rect(&snapshot, area, true), Rect::default());
}

#[test]
fn badge_is_hidden_when_the_tab_bar_is_too_narrow() {
    let snapshot = badge_snapshot(None, "personal");

    assert_eq!(
        session_badge_rect(&snapshot, Rect::new(0, 0, 15, 1), false),
        Rect::default()
    );
}

#[test]
fn a_default_session_on_the_default_profile_shows_the_profile_badge() {
    let snapshot = badge_snapshot(None, "default");

    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(snapshot));
    state.set_pane_surface(surface());
    let frame = state.compose(80, 20).expect("composed frame");
    let badge = state.hits.session_badge;
    assert_eq!(badge.width, 7);
    let rows = frame_rows(&frame);
    assert_eq!(
        rows[badge.y as usize]
            .chars()
            .skip(badge.x as usize)
            .take(7)
            .collect::<String>(),
        "default"
    );
}

#[test]
fn badge_sits_left_of_the_tab_bar_status_segments() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    let mut snapshot = badge_snapshot(Some("work"), "default");
    snapshot.tab_bar_right = vec![crate::protocol::ClientShellTabStatusSegment {
        text: "status".into(),
        accent: false,
    }];
    state.set_snapshot(Box::new(snapshot));
    state.set_pane_surface(surface());
    let frame = state.compose(106, 20).expect("composed frame");

    let badge = state.hits.session_badge;
    assert!(badge.width > 0);
    let rows = frame_rows(&frame);
    let row = &rows[badge.y as usize];
    let badge_text = row
        .chars()
        .skip(badge.x as usize)
        .take(badge.width as usize)
        .collect::<String>();
    assert_eq!(badge_text, "work");
    // The status segments own the far right; the badge stops short of them.
    assert!(row.ends_with("status"));
}

#[test]
fn overflowing_tabs_never_overwrite_the_badge() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    let mut snapshot = badge_snapshot(Some("work"), "default");
    for number in 2..24 {
        snapshot.tabs.push(ClientShellTab {
            tab_id: format!("tab_{number}"),
            workspace_id: "ws_1".into(),
            number,
            label: format!("a long tab label {number}"),
            custom_label: true,
            zoomed: false,
            focused: false,
            agent_status: AgentStatus::Idle,
        });
    }
    state.set_snapshot(Box::new(snapshot));
    state.set_pane_surface(surface());
    state.compose(106, 20).expect("composed frame");

    let badge = state.hits.session_badge;
    assert!(badge.width > 0);
    assert!(state
        .hits
        .tabs
        .iter()
        .map(|(rect, _)| *rect)
        .chain([state.hits.new_tab, state.hits.tab_scroll_right])
        .all(|rect| rect.width == 0 || rect.right() <= badge.x));
}

fn tab_status_snapshot() -> ClientShellSnapshot {
    let mut snapshot = snapshot();
    snapshot.panes.push(ClientShellPane {
        pane_id: "pane_2".into(),
        workspace_id: "ws_1".into(),
        tab_id: "tab_1".into(),
        label: None,
        cwd: None,
        foreground_cwd: None,
        focused: false,
        right_click_passthrough: false,
    });
    snapshot.agents.push(ClientShellAgent {
        pane_id: "pane_1".into(),
        workspace_id: "ws_1".into(),
        tab_id: "tab_1".into(),
        name: Some("one".into()),
        display_agent: Some("one".into()),
        agent: None,
        title: None,
        terminal_title: None,
        terminal_title_stripped: None,
        agent_status: AgentStatus::Blocked,
        state_change_seq: 1,
        state_labels: Vec::new(),
        tokens: Vec::new(),
        focused: true,
        visible_in_profile: true,
        owner_pane_id: None,
        orphaned: false,
        group: Default::default(),
    });
    snapshot.tabs[0].agent_status = AgentStatus::Blocked;
    snapshot.tabs[0].label = "a wide tab label".into();
    snapshot.tabs[0].custom_label = true;
    snapshot
}

fn glyphs_for(mode: crate::config::ShowTabStatusConfig) -> Vec<String> {
    let mut config = Config::default();
    config.ui.show_tab_status = mode;
    let config = ClientShellConfig::from_config(&config);
    let snapshot = tab_status_snapshot();
    crate::client::shell::render::tab_status_glyphs(&snapshot, &snapshot.tabs[0], &config, false)
        .into_iter()
        .map(|(glyph, _)| glyph.to_owned())
        .collect()
}

#[test]
fn tab_status_modes_gate_on_the_tabs_highest_attention_pane() {
    use crate::config::ShowTabStatusConfig;

    assert!(glyphs_for(ShowTabStatusConfig::Off).is_empty());
    // One glyph per pane: the blocked agent and the plain shell beside it.
    assert_eq!(glyphs_for(ShowTabStatusConfig::Attention).len(), 2);
    assert_eq!(glyphs_for(ShowTabStatusConfig::Active).len(), 2);
    assert_eq!(glyphs_for(ShowTabStatusConfig::All).len(), 2);
}

#[test]
fn an_idle_tab_shows_status_only_in_all_mode() {
    use crate::config::ShowTabStatusConfig;

    for mode in [
        ShowTabStatusConfig::Off,
        ShowTabStatusConfig::Attention,
        ShowTabStatusConfig::Active,
        ShowTabStatusConfig::All,
    ] {
        let mut config = Config::default();
        config.ui.show_tab_status = mode;
        let config = ClientShellConfig::from_config(&config);
        let mut snapshot = tab_status_snapshot();
        snapshot.tabs[0].agent_status = AgentStatus::Idle;
        snapshot.agents[0].agent_status = AgentStatus::Idle;

        let glyphs =
            crate::client::shell::render::tab_status_glyphs(&snapshot, &snapshot.tabs[0], &config, false);

        assert_eq!(
            glyphs.is_empty(),
            mode != ShowTabStatusConfig::All,
            "mode {mode:?}"
        );
    }
}

#[test]
fn completion_and_working_status_ink_matches_sidebar_and_tab_bar() {
    let mut config = Config::default();
    config.ui.show_tab_status = crate::config::ShowTabStatusConfig::All;
    config.ui.attention_read = crate::config::AttentionReadConfig::OnUnfocus;
    config.ui.sidebar.agents.rows = vec![vec![crate::config::AgentSidebarToken::StateIcon]];
    config
        .ui
        .sidebar
        .agents
        .state_icons
        .insert("working".into(), "W".into());
    config
        .ui
        .sidebar
        .agents
        .state_icons
        .insert("idle_unseen".into(), "D".into());
    config
        .ui
        .sidebar
        .agents
        .state_icons
        .insert("idle".into(), "R".into());
    let shell_config = ClientShellConfig::from_config(&config);
    for (status, glyph, expected) in [
        (AgentStatus::Working, "W", shell_config.palette.working),
        (AgentStatus::Done, "D", shell_config.palette.green),
        (AgentStatus::Idle, "R", shell_config.palette.overlay0),
    ] {
        config.ui.attention_read = if status == AgentStatus::Idle {
            crate::config::AttentionReadConfig::OnFocus
        } else {
            crate::config::AttentionReadConfig::OnUnfocus
        };
        let mut state = ClientShellState::new(ClientShellConfig::from_config(&config));
        let mut projected = tab_status_snapshot();
        projected.agents[0].agent_status = AgentStatus::Working;
        projected.tabs[0].agent_status = AgentStatus::Working;
        state.set_snapshot(Box::new(projected.clone()));
        state.set_pane_surface(surface());
        projected.revision += 1;
        projected.agents[0].agent_status = status;
        projected.agents[0].state_change_seq += 1;
        projected.tabs[0].agent_status = status;
        state.set_snapshot(Box::new(projected));
        let mut pane_surface = surface();
        pane_surface.projection_revision += 1;
        state.set_pane_surface(pane_surface);
        let frame = state.compose(106, 30).expect("status frame");
        let tab = state.hits.tabs[0].0;
        let agent = state.hits.agents[0].0;
        for area in [tab, agent] {
            let (x, y) = cell_symbol_position(&frame, area, glyph);
            let cell = &frame.cells[usize::from(y) * usize::from(frame.width) + usize::from(x)];
            assert_eq!(
                cell.fg,
                crate::protocol::color_to_u32(expected),
                "status {status:?} in {area:?}"
            );
        }
    }
}

#[test]
fn configured_state_icons_replace_the_default_glyphs() {
    let mut config = Config::default();
    config.ui.show_tab_status = crate::config::ShowTabStatusConfig::All;
    config.ui.sidebar.agents =
        toml::from_str("state_icons = { blocked = \"!!\", unknown = \"\" }").unwrap();
    let config = ClientShellConfig::from_config(&config);
    let snapshot = tab_status_snapshot();

    let glyphs =
        crate::client::shell::render::tab_status_glyphs(&snapshot, &snapshot.tabs[0], &config, false)
            .into_iter()
            .map(|(glyph, _)| glyph.to_owned())
            .collect::<Vec<_>>();

    // The blocked pane takes the override; the empty glyph drops the shell pane.
    assert_eq!(glyphs, ["!!"]);
}

#[test]
fn tab_status_glyphs_widen_the_tab_and_render_after_its_label() {
    let mut config = Config::default();
    config.ui.show_tab_status = crate::config::ShowTabStatusConfig::All;
    let mut with_status = ClientShellState::new(ClientShellConfig::from_config(&config));
    let mut without = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    for state in [&mut with_status, &mut without] {
        state.set_snapshot(Box::new(tab_status_snapshot()));
        state.set_pane_surface(surface());
    }
    let frame = with_status.compose(106, 20).expect("composed frame");
    without.compose(106, 20).expect("composed frame");

    let wide = with_status.hits.tabs[0].0;
    assert!(wide.width > without.hits.tabs[0].0.width);
    let rows = frame_rows(&frame);
    let tab_text = rows[wide.y as usize]
        .chars()
        .skip(wide.x as usize)
        .take(wide.width as usize)
        .collect::<String>();
    assert!(
        tab_text.trim_end().ends_with("●·"),
        "tab text: {tab_text:?}"
    );
}

#[test]
fn overlay_attention_marks_follow_status_glyphs_and_reserve_width() {
    use crate::factory_overlay::{Attention, FactoryOverlay, TabTag};
    use std::sync::Arc;

    let mut config = Config::default();
    config.ui.factory.enabled = true;
    config.ui.show_tab_status = crate::config::ShowTabStatusConfig::All;
    config.ui.sidebar.agents.state_icons.insert("idle".into(), "i".into());
    config.ui.sidebar.agents.state_icons.insert("blocked".into(), "!".into());
    let shell_config = ClientShellConfig::from_config(&config);
    let mut base = snapshot();
    base.tabs[0].label = "a long tab label".into();
    base.agents = tab_status_snapshot().agents;
    base.agents[0].agent_status = AgentStatus::Idle;

    let render = |attention: Option<Attention>, blocked: bool| {
        let mut state = ClientShellState::new(ClientShellConfig::from_config(&config));
        let mut snap = base.clone();
        if blocked {
            snap.tabs[0].agent_status = AgentStatus::Blocked;
            snap.agents[0].agent_status = AgentStatus::Blocked;
        }
        state.set_snapshot(Box::new(snap));
        if let Some(attention) = attention {
            let mut overlay = FactoryOverlay::default();
            overlay.tabs.insert("tab_1".into(), TabTag { attention, ..Default::default() });
            state.factory_overlay = Some(Arc::new(overlay));
        }
        state.set_pane_surface(surface());
        let frame = state.compose(106, 20).expect("tab bar frame");
        let tab = state.hits.tabs[0].0;
        let row = frame_rows(&frame)[tab.y as usize].clone();
        (frame, tab, row)
    };

    let (_, plain, plain_row) = render(None, false);
    let (_, none, none_row) = render(Some(Attention::None), false);
    assert_eq!(none_row, plain_row, "no attention must preserve the tab row");
    assert_eq!(none.width, plain.width);
    assert!(!plain_row.contains('!'));
    for (attention, expected) in [
        (Attention::Act, shell_config.palette.red),
        (Attention::Warn, shell_config.palette.peach),
    ] {
        let (frame, tab, row) = render(Some(attention), false);
        assert_eq!(tab.width, plain.width + 2);
        let tab_text = row.chars().skip(tab.x as usize).take(tab.width as usize).collect::<String>();
        assert!(tab_text.ends_with("i ! "), "tab text: {tab_text:?}");
        let (x, y) = cell_symbol_position(&frame, tab, "!");
        assert_eq!(frame.cells[y as usize * frame.width as usize + x as usize].fg,
            crate::protocol::color_to_u32(expected));
        assert_eq!(frame.cells[y as usize * frame.width as usize + (x + 1) as usize].bg,
            crate::protocol::color_to_u32(shell_config.palette.accent));
    }

    let mut clipped = ClientShellState::new(ClientShellConfig::from_config(&config));
    let mut long_tab = base.clone();
    long_tab.tabs[0].label = "a tab label long enough to be clipped by the available strip".into();
    clipped.set_snapshot(Box::new(long_tab));
    let mut overlay = FactoryOverlay::default();
    overlay.tabs.insert("tab_1".into(), TabTag { attention: Attention::Act, ..Default::default() });
    clipped.factory_overlay = Some(Arc::new(overlay));
    clipped.set_pane_surface(surface());
    let frame = clipped.compose(80, 20).expect("clipped tab frame");
    let tab = clipped.hits.tabs[0].0;
    assert!(tab.width < 64, "tab must be clipped: {tab:?}");
    let last = &frame.cells[tab.y as usize * frame.width as usize + (tab.right() - 1) as usize];
    assert_eq!(last.symbol, " ");
    assert_eq!(last.bg, crate::protocol::color_to_u32(shell_config.palette.accent));

    let (_, blocked_without, _) = render(None, true);
    let (_, blocked_with, blocked_row) = render(Some(Attention::Act), true);
    assert_eq!(blocked_with.width, blocked_without.width);
    let blocked_text = blocked_row.chars().skip(blocked_with.x as usize)
        .take(blocked_with.width as usize).collect::<String>();
    assert_eq!(blocked_text.matches('!').count(), 1, "tab text: {blocked_text:?}");
}

fn done_tab_glyphs(show_finished_dot: Option<bool>, state_icons: Option<&str>) -> Vec<String> {
    let mut config = Config::default();
    config.ui.show_tab_status = crate::config::ShowTabStatusConfig::All;
    if let Some(show) = show_finished_dot {
        config.ui.show_finished_dot = show;
    }
    if let Some(icons) = state_icons {
        config.ui.sidebar.agents = toml::from_str(icons).unwrap();
    }
    let config = ClientShellConfig::from_config(&config);
    let mut snapshot = tab_status_snapshot();
    snapshot.tabs[0].agent_status = AgentStatus::Done;
    snapshot.agents[0].agent_status = AgentStatus::Done;
    crate::client::shell::render::tab_status_glyphs(&snapshot, &snapshot.tabs[0], &config, false)
        .into_iter()
        .map(|(glyph, _)| glyph.to_owned())
        .collect()
}

#[test]
fn finished_marker_is_hidden_from_tab_rows_by_default() {
    // Only the plain shell pane's glyph remains; the finished agent draws nothing.
    assert_eq!(done_tab_glyphs(None, None), ["·"]);
    assert_eq!(done_tab_glyphs(Some(false), None), ["·"]);
}

#[test]
fn finished_marker_draws_in_tab_rows_when_enabled() {
    assert_eq!(done_tab_glyphs(Some(true), None), ["●", "·"]);
}

#[test]
fn explicit_idle_unseen_icon_still_draws_with_the_finished_dot_off() {
    assert_eq!(
        done_tab_glyphs(Some(false), Some("state_icons = { idle_unseen = \"✓\" }")),
        ["✓", "·"]
    );
}

#[test]
fn hiding_the_finished_marker_keeps_the_other_status_glyphs() {
    use crate::api::schema::AgentStatus;
    use crate::config::StatusIndicatorStyle;

    // Expected glyphs are literals, not `status_icon`, so a changed glyph fails here.
    let cases = [
        (
            StatusIndicatorStyle::Dots,
            [
                (AgentStatus::Blocked, "●"),
                (AgentStatus::Working, "●"),
                (AgentStatus::Idle, "○"),
                (AgentStatus::Unknown, "·"),
            ],
            "●",
        ),
        (
            StatusIndicatorStyle::Symbols,
            [
                (AgentStatus::Blocked, "×"),
                (AgentStatus::Working, "◐"),
                (AgentStatus::Idle, "○"),
                (AgentStatus::Unknown, "·"),
            ],
            "✓",
        ),
    ];
    for (style, others, done_glyph) in cases {
        let mut hidden = Config::default();
        hidden.ui.status_indicators = style;
        let hidden = ClientShellConfig::from_config(&hidden);
        assert!(!hidden.show_finished_dot);
        assert_eq!(
            crate::client::shell::resolved_status_icon(AgentStatus::Done, &hidden).trim(),
            "",
            "{style:?} Done hidden"
        );
        let mut shown = Config::default();
        shown.ui.status_indicators = style;
        shown.ui.show_finished_dot = true;
        let shown = ClientShellConfig::from_config(&shown);
        assert_eq!(
            crate::client::shell::resolved_status_icon(AgentStatus::Done, &shown),
            done_glyph,
            "{style:?} Done shown"
        );
        for (status, glyph) in others {
            for config in [&hidden, &shown] {
                assert_eq!(
                    crate::client::shell::resolved_status_icon(status, config),
                    glyph,
                    "{style:?} {status:?}"
                );
            }
        }
    }
}
