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
        // Nothing to say: no badge, and the tab strip keeps its full width.
        (Some("default"), "default", ""),
        (None, "default", ""),
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
fn a_default_session_on_the_default_profile_shows_no_badge() {
    let snapshot = badge_snapshot(None, "default");

    assert_eq!(
        session_badge_rect(&snapshot, Rect::new(0, 0, 80, 1), true),
        Rect::default()
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
    crate::client::shell::render::tab_status_glyphs(&snapshot, &snapshot.tabs[0], &config)
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
            crate::client::shell::render::tab_status_glyphs(&snapshot, &snapshot.tabs[0], &config);

        assert_eq!(
            glyphs.is_empty(),
            mode != ShowTabStatusConfig::All,
            "mode {mode:?}"
        );
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
        crate::client::shell::render::tab_status_glyphs(&snapshot, &snapshot.tabs[0], &config)
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
