//! Overlay pane scrollbar: color ramp and reveal state.
//!
//! The bar is hidden at the live bottom and appears only when the user scrolls.
//! It holds at the rest color, fades in a few steps, and then either hides or,
//! when the pane is still scrolled back, settles on a faint "parked" color.
//! This is presentation state for the server-side renderer; it has no
//! protocol, API or event fields. Pointer hover and drag brightening belong to
//! the client shell, which knows where the pointer is.

use std::time::{Duration, Instant};

use ratatui::style::Color;

use super::state::{AppState, Palette};
use crate::layout::PaneId;
use crate::terminal_theme::{HostAppearance, RgbColor};

/// Time the bar stays at the rest color after the last scroll.
pub(crate) const SCROLLBAR_HOLD: Duration = Duration::from_millis(900);
/// Spacing between fade steps, and from the end of the hold to the first step.
pub(crate) const SCROLLBAR_FADE_STEP: Duration = Duration::from_millis(60);
/// Number of intermediate fade steps between rest and the target state.
pub(crate) const SCROLLBAR_FADE_STEPS: u8 = 3;

/// Bar colors mixed from the pane background toward the theme text color.
/// Computed when the palette or host background changes, never per frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ScrollbarRamp {
    pub rest: Color,
    pub hover: Color,
    pub drag: Color,
    pub parked: Color,
    /// Fade steps toward hidden. `None` draws nothing (non-RGB themes skip
    /// straight to the target).
    pub fade_hidden: [Option<Color>; 3],
    /// Fade steps toward the parked color.
    pub fade_parked: [Color; 3],
}

/// Mix alphas in per-mille: (parked, rest, hover, drag).
const DARK_ALPHAS: (i32, i32, i32, i32) = (160, 340, 500, 620);
const LIGHT_ALPHAS: (i32, i32, i32, i32) = (200, 460, 600, 720);

fn rgb_of(color: Color) -> Option<(u8, u8, u8)> {
    match color {
        Color::Rgb(r, g, b) => Some((r, g, b)),
        _ => None,
    }
}

/// `base + (target - base) * per_mille / 1000`, rounded half to even.
fn mix_channel(base: u8, target: u8, per_mille: i32) -> u8 {
    let scaled = i32::from(base) * 1000 + (i32::from(target) - i32::from(base)) * per_mille;
    let (quotient, remainder) = (scaled / 1000, scaled % 1000);
    let rounded = if remainder > 500 || (remainder == 500 && quotient % 2 == 1) {
        quotient + 1
    } else {
        quotient
    };
    rounded.clamp(0, 255) as u8
}

fn mix(bg: (u8, u8, u8), fg: (u8, u8, u8), per_mille: i32) -> Color {
    Color::Rgb(
        mix_channel(bg.0, fg.0, per_mille),
        mix_channel(bg.1, fg.1, per_mille),
        mix_channel(bg.2, fg.2, per_mille),
    )
}

impl ScrollbarRamp {
    /// Derive the ramp from the host terminal background (OSC 11 reply),
    /// falling back to `surface_dim`, and the palette text color.
    pub(crate) fn derive(host_background: Option<RgbColor>, palette: &Palette) -> Self {
        let bg = host_background
            .map(|color| (color.r, color.g, color.b))
            .or_else(|| rgb_of(palette.surface_dim));
        let (Some(bg), Some(fg)) = (bg, rgb_of(palette.text)) else {
            return Self {
                rest: palette.overlay0,
                hover: palette.overlay1,
                drag: palette.text,
                parked: palette.surface_dim,
                fade_hidden: [None; 3],
                fade_parked: [palette.surface_dim; 3],
            };
        };
        let appearance = RgbColor {
            r: bg.0,
            g: bg.1,
            b: bg.2,
        }
        .inferred_appearance();
        let (parked, rest, hover, drag) = match appearance {
            HostAppearance::Dark => DARK_ALPHAS,
            HostAppearance::Light => LIGHT_ALPHAS,
        };
        // Step k of 3 keeps (4 - k) / 4 of the distance from the target.
        let fade_hidden = [1, 2, 3].map(|k| Some(mix(bg, fg, rest * (4 - k) / 4)));
        let fade_parked = [1, 2, 3].map(|k| mix(bg, fg, parked + (rest - parked) * (4 - k) / 4));
        Self {
            rest: mix(bg, fg, rest),
            hover: mix(bg, fg, hover),
            drag: mix(bg, fg, drag),
            parked: mix(bg, fg, parked),
            fade_hidden,
            fade_parked,
        }
    }

    /// Color for a pane bar, or `None` when it draws nothing.
    pub(crate) fn color_for(&self, step: Option<u8>, scrolled_back: bool) -> Option<Color> {
        match step {
            Some(0) => Some(self.rest),
            Some(step) => {
                let index = usize::from(step.clamp(1, SCROLLBAR_FADE_STEPS) - 1);
                if scrolled_back {
                    Some(self.fade_parked[index])
                } else {
                    self.fade_hidden[index]
                }
            }
            None => scrolled_back.then_some(self.parked),
        }
    }
}

/// The one pane whose bar is currently revealed. Step 0 is rest (the hold);
/// steps 1..=3 are the fade. `next_at` is when the next step is due.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ScrollbarReveal {
    pub pane: PaneId,
    pub step: u8,
    pub next_at: Instant,
}

impl AppState {
    pub(crate) fn refresh_scrollbar_ramp(&mut self) {
        self.scrollbar_ramp =
            ScrollbarRamp::derive(self.host_terminal_theme.background, &self.palette);
    }

    /// Bar color for a pane given its current scroll metrics. Pure: reads the
    /// reveal step, never the clock.
    pub(crate) fn pane_scrollbar_color(
        &self,
        pane: PaneId,
        metrics: crate::pane::ScrollMetrics,
    ) -> Option<Color> {
        if metrics.max_offset_from_bottom == 0 || metrics.offset_from_bottom == 0 {
            return None;
        }
        let step = self
            .scrollbar_reveal
            .filter(|reveal| reveal.pane == pane)
            .map(|reveal| reveal.step);
        self.scrollbar_ramp
            .color_for(step, metrics.offset_from_bottom > 0)
    }

    /// A user scroll in `pane`: show rest now and restart the hold. Another
    /// pane that was revealed drops straight to its target.
    pub(crate) fn note_scrollbar_user_scroll(&mut self, pane: PaneId, now: Instant) -> bool {
        let previous = self.scrollbar_reveal;
        self.scrollbar_reveal = Some(ScrollbarReveal {
            pane,
            step: 0,
            next_at: now + SCROLLBAR_HOLD + SCROLLBAR_FADE_STEP,
        });
        previous.is_none_or(|previous| previous.pane != pane || previous.step != 0)
    }

    /// The pane snapped back to the live bottom (typing, or the app reset the
    /// viewport): hide at once with no fade.
    pub(crate) fn note_scrollbar_snapped_to_bottom(&mut self, pane: PaneId) -> bool {
        if self
            .scrollbar_reveal
            .is_some_and(|reveal| reveal.pane == pane)
        {
            self.scrollbar_reveal = None;
            return true;
        }
        false
    }

    /// Advance the hold and fade. Returns true when the bar changes.
    pub(crate) fn advance_scrollbar_reveal(&mut self, now: Instant) -> bool {
        let Some(reveal) = self.scrollbar_reveal.as_mut() else {
            return false;
        };
        if now < reveal.next_at {
            return false;
        }
        if reveal.step >= SCROLLBAR_FADE_STEPS {
            self.scrollbar_reveal = None;
        } else {
            reveal.step += 1;
            reveal.next_at = now + SCROLLBAR_FADE_STEP;
        }
        true
    }

    fn scrollbar_pane_visible(&self, pane: PaneId) -> bool {
        self.active
            .is_some_and(|ws_idx| self.pane_visible_on_active_surface(ws_idx, pane))
    }

    pub(crate) fn scrollbar_reveal_deadline(&self) -> Option<Instant> {
        self.scrollbar_reveal
            .filter(|reveal| self.scrollbar_pane_visible(reveal.pane))
            .map(|reveal| reveal.next_at)
    }
}

impl super::App {
    /// Fold user scroll gestures recorded on runtimes into the reveal state
    /// and advance its timer. Cheap when idle: one atomic load per call, plus
    /// one lookup of the revealed pane while a bar is showing.
    pub(crate) fn tick_scrollbar_reveal(&mut self, now: Instant) -> bool {
        let mut changed = false;
        if self
            .state
            .scrollbar_reveal
            .is_some_and(|reveal| !self.state.scrollbar_pane_visible(reveal.pane))
        {
            // No render request or fade deadline for an off-screen pane.
            self.state.scrollbar_reveal = None;
        }
        let epoch = crate::terminal::user_scroll_epoch();
        let mut scrolled = None;
        if epoch != self.scrollbar_scroll_epoch {
            self.scrollbar_scroll_epoch = epoch;
            for (ws_idx, workspace) in self.state.workspaces.iter().enumerate() {
                for tab in &workspace.tabs {
                    for &pane_id in tab.panes.keys() {
                        let Some(runtime) = self.state.runtime_for_pane_in_workspace(
                            &self.terminal_runtimes,
                            ws_idx,
                            pane_id,
                        ) else {
                            continue;
                        };
                        if runtime.take_user_scrolled() {
                            // Read final metrics after the input batch: a later typing
                            // snap must win over an earlier scroll gesture.
                            if self.state.pane_visible_on_active_surface(ws_idx, pane_id)
                                && runtime
                                    .scroll_metrics()
                                    .is_some_and(|metrics| metrics.offset_from_bottom > 0)
                            {
                                scrolled = Some(pane_id);
                            }
                        }
                    }
                }
            }
        }
        if let Some(pane_id) = scrolled {
            changed |= self.state.note_scrollbar_user_scroll(pane_id, now);
        }
        if let Some(reveal) = self.state.scrollbar_reveal {
            let runtime = self.find_pane(reveal.pane).and_then(|(ws_idx, _)| {
                self.state.runtime_for_pane_in_workspace(
                    &self.terminal_runtimes,
                    ws_idx,
                    reveal.pane,
                )
            });
            match runtime {
                None => {
                    self.state.scrollbar_reveal = None;
                    changed = true;
                }
                Some(runtime) => {
                    runtime.take_scroll_snapped();
                    let snapped_to_bottom = runtime
                        .scroll_metrics()
                        .is_none_or(|metrics| metrics.offset_from_bottom == 0);
                    if snapped_to_bottom {
                        changed |= self.state.note_scrollbar_snapped_to_bottom(reveal.pane);
                    }
                }
            }
        }
        changed |= self.state.advance_scrollbar_reveal(now);
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metrics(offset: usize, max: usize) -> crate::pane::ScrollMetrics {
        crate::pane::ScrollMetrics {
            offset_from_bottom: offset,
            max_offset_from_bottom: max,
            viewport_rows: 10,
        }
    }

    fn hex(color: Color) -> String {
        match color {
            Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
            other => format!("{other:?}"),
        }
    }

    #[test]
    fn mocha_ramp_matches_the_spec_values() {
        let ramp = ScrollbarRamp::derive(
            Some(RgbColor {
                r: 0x1e,
                g: 0x1e,
                b: 0x2e,
            }),
            &Palette::catppuccin(),
        );
        assert_eq!(hex(ramp.rest), "#5a5d71");
        assert_eq!(hex(ramp.hover), "#767a91");
        assert_eq!(hex(ramp.drag), "#8a90a9");
        assert_eq!(hex(ramp.parked), "#3a3b4e");
        assert_eq!(
            ramp.fade_hidden.map(|c| c.map(hex)),
            [
                Some("#4b4d60".to_owned()),
                Some("#3c3d50".to_owned()),
                Some("#2d2e3f".to_owned())
            ]
        );
        assert_eq!(
            ramp.fade_parked.map(hex),
            ["#525468", "#4a4c60", "#424457"].map(str::to_owned)
        );
    }

    #[test]
    fn light_background_uses_the_light_alphas() {
        let mut palette = Palette::catppuccin_latte();
        palette.text = Color::Rgb(0x2e, 0x31, 0x45);
        let ramp = ScrollbarRamp::derive(
            Some(RgbColor {
                r: 0xef,
                g: 0xf1,
                b: 0xf5,
            }),
            &palette,
        );
        assert_eq!(hex(ramp.rest), "#9699a4");
        assert_eq!(hex(ramp.hover), "#7b7e8b");
        assert_eq!(hex(ramp.drag), "#646776");
        assert_eq!(hex(ramp.parked), "#c8cbd2");
    }

    #[test]
    fn named_color_theme_falls_back_to_palette_tokens_without_fade_steps() {
        let mut palette = Palette::catppuccin();
        palette.text = Color::White;
        let ramp = ScrollbarRamp::derive(None, &palette);
        assert_eq!(ramp.rest, palette.overlay0);
        assert_eq!(ramp.hover, palette.overlay1);
        assert_eq!(ramp.drag, palette.text);
        assert_eq!(ramp.parked, palette.surface_dim);
        assert_eq!(ramp.color_for(Some(2), false), None);
        assert_eq!(ramp.color_for(Some(2), true), Some(palette.surface_dim));
    }

    #[test]
    fn scroll_holds_rest_then_fades_while_scrolled_back() {
        let mut state = AppState::test_new();
        let pane = PaneId::from_raw(7);
        let start = Instant::now();
        let ramp = state.scrollbar_ramp;

        assert_eq!(state.pane_scrollbar_color(pane, metrics(0, 50)), None);
        assert!(state.note_scrollbar_user_scroll(pane, start));
        assert_eq!(
            state.pane_scrollbar_color(pane, metrics(25, 50)),
            Some(ramp.rest)
        );

        // Still holding at 959 ms.
        assert!(!state.advance_scrollbar_reveal(start + Duration::from_millis(959)));
        let mut now = start + Duration::from_millis(960);
        for k in 0..3 {
            assert!(state.advance_scrollbar_reveal(now));
            assert_eq!(
                state.pane_scrollbar_color(pane, metrics(25, 50)),
                Some(ramp.fade_parked[k])
            );
            now += SCROLLBAR_FADE_STEP;
        }
        assert!(state.advance_scrollbar_reveal(now));
        assert_eq!(state.scrollbar_reveal, None);
        assert_eq!(state.scrollbar_reveal_deadline(), None);
        assert_eq!(
            state.pane_scrollbar_color(pane, metrics(25, 50)),
            Some(ramp.parked)
        );
    }

    #[test]
    fn fade_ends_parked_while_scrolled_back() {
        let mut state = AppState::test_new();
        let pane = PaneId::from_raw(7);
        let start = Instant::now();
        let ramp = state.scrollbar_ramp;
        state.note_scrollbar_user_scroll(pane, start);
        let mut now = start + SCROLLBAR_HOLD + SCROLLBAR_FADE_STEP;
        for k in 0..3 {
            state.advance_scrollbar_reveal(now);
            assert_eq!(
                state.pane_scrollbar_color(pane, metrics(25, 50)),
                Some(ramp.fade_parked[k])
            );
            now += SCROLLBAR_FADE_STEP;
        }
        state.advance_scrollbar_reveal(now);
        assert_eq!(
            state.pane_scrollbar_color(pane, metrics(25, 50)),
            Some(ramp.parked)
        );
    }

    #[test]
    fn output_growth_and_other_panes_never_reveal() {
        let mut state = AppState::test_new();
        let pane = PaneId::from_raw(7);
        let other = PaneId::from_raw(8);
        // Output grows the scrollback while the pane sits at the bottom.
        for max in [1, 10, 500] {
            assert_eq!(state.pane_scrollbar_color(pane, metrics(0, max)), None);
        }
        let now = Instant::now();
        state.note_scrollbar_user_scroll(other, now);
        assert_eq!(state.pane_scrollbar_color(pane, metrics(0, 500)), None);
        // Scrolling a different pane moves the single slot.
        state.note_scrollbar_user_scroll(pane, now);
        assert_eq!(state.pane_scrollbar_color(other, metrics(0, 500)), None);
    }

    #[test]
    fn snapping_to_bottom_hides_without_a_fade() {
        let mut state = AppState::test_new();
        let pane = PaneId::from_raw(7);
        state.note_scrollbar_user_scroll(pane, Instant::now());
        assert!(state.note_scrollbar_snapped_to_bottom(pane));
        assert_eq!(state.pane_scrollbar_color(pane, metrics(0, 50)), None);
        assert!(!state.note_scrollbar_snapped_to_bottom(pane));
    }

    fn app_with_scrollback_pane() -> (crate::app::App, PaneId) {
        let mut app = crate::app::App::new(
            &crate::config::Config::default(),
            crate::app::AppPolicy::TEST,
            None,
            tokio::sync::mpsc::unbounded_channel().1,
            crate::api::EventHub::default(),
        );
        let ws = crate::workspace::Workspace::test_new("test");
        let pane_id = ws.tabs[0].root_pane;
        app.state.workspaces.push(ws);
        app.state.active = Some(0);
        let lines = (0..40)
            .map(|n| format!("line {n:02}\n"))
            .collect::<String>();
        app.state.insert_test_runtime(
            pane_id,
            crate::terminal::TerminalRuntime::test_with_scrollback_bytes(
                20,
                5,
                10_000,
                lines.as_bytes(),
            ),
        );
        (app, pane_id)
    }

    fn color(app: &crate::app::App, pane: PaneId) -> Option<Color> {
        let metrics = app
            .state
            .runtime_for_pane_in_workspace(&app.terminal_runtimes, 0, pane)
            .and_then(crate::terminal::TerminalRuntime::scroll_metrics)
            .expect("metrics");
        app.state.pane_scrollbar_color(pane, metrics)
    }

    #[tokio::test]
    async fn runtime_scrolls_reveal_and_output_or_typing_never_does() {
        let (mut app, pane) = app_with_scrollback_pane();
        let ramp = app.state.scrollbar_ramp;
        fn runtime(app: &crate::app::App, pane: PaneId) -> &crate::terminal::TerminalRuntime {
            app.state
                .runtime_for_pane_in_workspace(&app.terminal_runtimes, 0, pane)
                .expect("runtime")
        }
        let start = Instant::now();

        // Streaming output at the live bottom keeps the bar hidden.
        runtime(&app, pane).test_process_pty_bytes(b"more\nmore\nmore\n");
        assert!(!app.tick_scrollbar_reveal(start));
        assert_eq!(app.state.scrollbar_reveal, None);
        assert_eq!(color(&app, pane), None);

        // Wheel-down at the live bottom and a scroll/typing batch never reveal.
        runtime(&app, pane).scroll_down(3);
        assert!(!app.tick_scrollbar_reveal(start));
        assert_eq!(color(&app, pane), None);
        runtime(&app, pane).scroll_up(3);
        runtime(&app, pane).scroll_reset();
        app.tick_scrollbar_reveal(start);
        assert_eq!(app.state.scrollbar_reveal, None);
        assert_eq!(color(&app, pane), None);

        // A user scroll reveals the rest color on the next tick.
        runtime(&app, pane).scroll_up(3);
        assert!(app.tick_scrollbar_reveal(start));
        assert_eq!(color(&app, pane), Some(ramp.rest));

        // Typing snaps to the bottom and hides at once, with no fade.
        runtime(&app, pane).scroll_reset();
        assert!(app.tick_scrollbar_reveal(start));
        assert_eq!(app.state.scrollbar_reveal, None);
        assert_eq!(color(&app, pane), None);

        // Scrolled back and idle: hold, fade, then park.
        runtime(&app, pane).scroll_up(3);
        app.tick_scrollbar_reveal(start);
        let mut now = start;
        for _ in 0..8 {
            now += Duration::from_millis(300);
            app.tick_scrollbar_reveal(now);
        }
        assert_eq!(app.state.scrollbar_reveal, None);
        assert_eq!(color(&app, pane), Some(ramp.parked));

        // Output while parked does not wake the bar.
        runtime(&app, pane).test_process_pty_bytes(b"more\n");
        assert!(!app.tick_scrollbar_reveal(now));
        assert_eq!(app.state.scrollbar_reveal, None);

        // Switching away cancels all hidden-pane fade deadlines and repaints.
        runtime(&app, pane).scroll_up(3);
        app.tick_scrollbar_reveal(now);
        assert!(app.state.scrollbar_reveal_deadline().is_some());
        app.state.active = None;
        assert_eq!(app.state.scrollbar_reveal_deadline(), None);
        assert!(!app.tick_scrollbar_reveal(now + Duration::from_secs(1)));
        assert_eq!(app.state.scrollbar_reveal, None);
    }
}
