//! Pointer emphasis for the overlay pane scrollbar.
//!
//! The server draws the bar for scroll reveal, hold, fade and parked states.
//! Hover and drag are pointer facts only this client knows, so the client
//! brightens the thumb itself while the pointer is on a pane's gutter or a
//! thumb drag is in progress.

use super::*;
use crossterm::event::MouseEvent;

impl ClientShellState {
    /// Track which pane gutter, if any, the pointer is on.
    pub(super) fn update_pane_scrollbar_hover(
        &mut self,
        mouse: MouseEvent,
        outcome: &mut ClientShellInput,
    ) {
        let point = (mouse.column, mouse.row);
        let hovered = self
            .hits
            .panes
            .iter()
            .find(|hit| {
                hit.scrollbar_rect.is_some_and(|rect| contains(rect, point))
                    && hit.scroll.is_some_and(|metrics| {
                        metrics.max_offset_from_bottom > 0 && metrics.offset_from_bottom > 0
                    })
            })
            .map(|hit| &hit.pane_id);
        if hovered != self.pane_scrollbar_hover.as_ref() {
            self.pane_scrollbar_hover = hovered.cloned();
            outcome.repaint = true;
        }
    }

    /// The pane whose thumb is emphasized, and whether it is being dragged.
    fn pane_scrollbar_emphasis(&self) -> Option<(&str, bool)> {
        if let Some(ClientChromeDrag::PaneScrollbar { hit, .. }) = self.chrome_drag.as_ref() {
            return Some((hit.pane_id.as_str(), true));
        }
        self.pane_scrollbar_hover
            .as_deref()
            .map(|pane_id| (pane_id, false))
    }

    /// Retained patches would overwrite the emphasized thumb, so a patch for
    /// that pane takes the full compose path while the pointer is on it.
    pub(super) fn pane_scrollbar_emphasis_blocks_patch(
        &self,
        patch: &crate::protocol::PaneSurfacePatch,
    ) -> bool {
        self.pane_scrollbar_emphasis()
            .is_some_and(|(pane_id, _)| patch.panes.iter().any(|pane| pane.pane_id == pane_id))
    }

    /// Draw the hover or drag thumb over the composed frame. Runs for at most
    /// one pane, and only while the pointer is on its gutter or dragging.
    pub(super) fn render_pane_scrollbar_emphasis(&self, frame: &mut FrameData) {
        let Some((pane_id, dragging)) = self.pane_scrollbar_emphasis() else {
            return;
        };
        let Some(hit) = self.hits.panes.iter().find(|hit| hit.pane_id == pane_id) else {
            return;
        };
        let (Some(track), Some(metrics)) = (hit.scrollbar_rect, hit.scroll) else {
            return;
        };
        if metrics.offset_from_bottom == 0 {
            return;
        }
        let ramp = crate::app::scrollbar_reveal::ScrollbarRamp::derive(
            self.host_background,
            &self.config.palette,
        );
        let color = if dragging { ramp.drag } else { ramp.hover };
        let local = Rect::new(0, 0, 1, track.height);
        let mut thumb = ratatui::buffer::Buffer::empty(local);
        crate::ui::render_pane_scrollbar_buffer(&mut thumb, metrics, local, color);
        let fg = crate::protocol::color_to_u32(color);
        for (row, cell) in thumb.content.iter().enumerate() {
            if cell.symbol() == " " {
                continue;
            }
            let Ok(row) = u16::try_from(row) else {
                break;
            };
            let y = track.y.saturating_add(row);
            if track.x >= frame.width || y >= frame.height {
                continue;
            }
            let index = usize::from(y) * usize::from(frame.width) + usize::from(track.x);
            if let Some(target) = frame.cells.get_mut(index) {
                target.symbol.clear();
                target.symbol.push_str(cell.symbol());
                target.fg = fg;
            }
        }
    }
}
