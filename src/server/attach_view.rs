use ratatui::layout::Rect;

use crate::pane::{ScrollMetrics, TerminalCursorState};
use crate::terminal::TerminalRuntime;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AttachViewKey {
    terminal_id: String,
    runtime: usize,
    content_seq: u64,
    area: Rect,
    synchronized_epoch: u64,
    viewport: Option<ScrollMetrics>,
    alternate_screen: bool,
    theme_generation: Option<u64>,
    hide_kitty_placeholders: bool,
    cursor: Option<TerminalCursorState>,
}

impl AttachViewKey {
    pub(crate) fn capture(
        terminal_id: &str,
        runtime: &TerminalRuntime,
        area: Rect,
        synchronized_epoch: u64,
    ) -> Self {
        let (identity, theme_generation) = runtime.attach_view_identity();
        Self {
            terminal_id: terminal_id.to_owned(),
            runtime: identity,
            content_seq: runtime.content_seq(),
            area,
            synchronized_epoch,
            viewport: runtime.scroll_metrics(),
            alternate_screen: runtime.alternate_screen_active(),
            theme_generation,
            hide_kitty_placeholders: crate::kitty_graphics::is_enabled(),
            cursor: runtime.cursor_state(area, true),
        }
    }

    pub(crate) fn theme_changed(&self, previous: &Self) -> bool {
        self.theme_generation != previous.theme_generation
    }

    pub(crate) fn stable(&self) -> bool {
        self.content_seq.is_multiple_of(2) && self.theme_generation.is_some()
    }
}
