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
