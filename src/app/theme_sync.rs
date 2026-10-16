use std::time::{Duration, Instant};

use super::App;

const PANE_APPEARANCE_COLOR_WAIT: Duration = Duration::from_millis(1500);

impl App {
    pub(crate) fn set_host_terminal_appearance_state(
        &mut self,
        appearance: Option<crate::terminal_theme::HostAppearance>,
        explicit: bool,
    ) -> bool {
        if self.state.host_terminal_appearance == appearance
            && self.state.host_terminal_appearance_explicit == explicit
        {
            return false;
        }
        self.state.host_terminal_appearance = appearance;
        self.state.host_terminal_appearance_explicit = explicit;
        if self.host_colors_lag_appearance() {
            self.pane_appearance_deadline = Some(Instant::now() + PANE_APPEARANCE_COLOR_WAIT);
        } else {
            self.pane_appearance_deadline = None;
            self.apply_host_terminal_appearance_to_panes();
        }
        self.refresh_effective_app_theme()
    }

    pub(crate) fn set_host_terminal_theme(
        &mut self,
        theme: crate::terminal_theme::TerminalTheme,
    ) -> bool {
        if theme == self.state.host_terminal_theme {
            return false;
        }
        self.state.host_terminal_theme = theme;
        self.state.refresh_scrollbar_ramp();
        self.apply_host_terminal_theme_to_panes();
        if self.pane_appearance_deadline.take().is_some() {
            self.apply_host_terminal_appearance_to_panes();
        }
        true
    }

    pub(crate) fn flush_due_pane_appearance(&mut self, now: Instant) {
        if self
            .pane_appearance_deadline
            .is_some_and(|deadline| now >= deadline)
        {
            self.pane_appearance_deadline = None;
            self.apply_host_terminal_appearance_to_panes();
        }
    }

    fn host_colors_lag_appearance(&self) -> bool {
        let (Some(appearance), Some(background)) = (
            self.state.host_terminal_appearance,
            self.state.host_terminal_theme.background,
        ) else {
            return false;
        };
        background.inferred_appearance() != appearance
    }

    pub(super) fn refresh_effective_app_theme(&mut self) -> bool {
        let (palette, theme_name) = super::resolve_effective_theme(
            &self.state.theme_runtime,
            self.state.host_terminal_appearance,
        );
        if self.state.theme_name == theme_name && self.state.palette == palette {
            return false;
        }
        self.state.theme_name = theme_name;
        self.state.palette = palette;
        self.state.refresh_scrollbar_ramp();
        self.render_dirty.request_generic();
        self.render_notify.notify_one();
        true
    }

    fn apply_host_terminal_appearance_to_panes(&self) {
        for runtime in self.terminal_runtimes.values() {
            runtime.apply_host_terminal_appearance(self.state.host_terminal_appearance);
        }
    }

    fn apply_host_terminal_theme_to_panes(&self) {
        for runtime in self.terminal_runtimes.values() {
            runtime.apply_host_terminal_theme(self.state.host_terminal_theme);
        }

        self.render_dirty.request_generic();
        self.render_notify.notify_one();
    }
}
