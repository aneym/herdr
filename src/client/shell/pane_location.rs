use super::*;
use crossterm::event::{MouseButton, MouseEventKind};

fn button_rect(hit: &PaneHit) -> Option<Rect> {
    (!hit.popup && hit.rect.width >= 5 && hit.inner_rect.y > hit.rect.y)
        .then(|| Rect::new(hit.rect.right().saturating_sub(4), hit.rect.y, 3, 1))
}

impl ClientShellState {
    pub(super) fn handle_pane_location_mouse(
        &mut self,
        mouse: crossterm::event::MouseEvent,
        outcome: &mut ClientShellInput,
    ) -> bool {
        let point = (mouse.column, mouse.row);
        let allowed = self.overlay.is_none()
            && self.popup_terminal_id.is_none()
            && self.mode == ClientShellMode::Terminal;
        let hit = allowed
            .then(|| {
                self.hits
                    .panes
                    .iter()
                    .find(|hit| button_rect(hit).is_some_and(|rect| super::contains(rect, point)))
            })
            .flatten();
        if let Some(pressed) = self.pane_location_pressed.as_ref() {
            match mouse.kind {
                MouseEventKind::Up(MouseButton::Left) => {
                    if hit.is_some_and(|hit| &hit.pane_id == pressed)
                        && self.snapshot.as_deref().is_some_and(|snapshot| {
                            snapshot.panes.iter().any(|pane| &pane.pane_id == pressed)
                        })
                    {
                        outcome.actions.push(ClientShellAction::ClipboardWrite(
                            pressed.as_bytes().to_vec(),
                        ));
                        outcome.repaint |= self.show_copy_feedback(std::time::Instant::now());
                    }
                    self.pane_location_pressed = None;
                    return true;
                }
                MouseEventKind::Drag(MouseButton::Left) => return true,
                _ => {}
            }
        }
        if mouse.kind == MouseEventKind::Moved {
            let hover = allowed
                .then(|| {
                    self.hits
                        .panes
                        .iter()
                        .find(|hit| super::contains(hit.rect, point) && button_rect(hit).is_some())
                        .map(|hit| hit.pane_id.clone())
                })
                .flatten();
            if hover != self.pane_location_hover {
                self.pane_location_hover = hover;
                outcome.repaint = true;
            }
        }
        if mouse.kind == MouseEventKind::Down(MouseButton::Left) && mouse.modifiers.is_empty() {
            if let Some(hit) = hit {
                self.pane_location_pressed = Some(hit.pane_id.clone());
                self.pane_location_hover = Some(hit.pane_id.clone());
                outcome.repaint = true;
                return true;
            }
        }
        false
    }

    pub(super) fn render_pane_location(
        &self,
        frame: &mut FrameData,
        occlusion: &mut crate::kitty_graphics::surface::Occlusion,
    ) {
        if self.overlay.is_some() || self.popup_terminal_id.is_some() {
            return;
        }
        let Some(hit) = self
            .pane_location_hover
            .as_ref()
            .and_then(|id| self.hits.panes.iter().find(|hit| &hit.pane_id == id))
        else {
            return;
        };
        let Some(rect) = button_rect(hit) else {
            return;
        };
        if rect.right() > frame.width || rect.bottom() > frame.height {
            return;
        }
        for (offset, symbol) in [" ", "⧉", " "].into_iter().enumerate() {
            let index =
                usize::from(rect.y) * usize::from(frame.width) + usize::from(rect.x) + offset;
            if let Some(cell) = frame.cells.get_mut(index) {
                cell.symbol = symbol.to_owned();
                cell.hyperlink = None;
                cell.skip = false;
            }
        }
        occlusion.cover(rect);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> ClientShellState {
        let mut state = ClientShellState::new(ClientShellConfig::from_config(
            &crate::config::Config::default(),
        ));
        state.set_snapshot(Box::new(super::super::tests::snapshot()));
        state.hits.panes.push(PaneHit {
            rect: Rect::new(0, 0, 20, 8),
            inner_rect: Rect::new(1, 1, 18, 6),
            scrollbar_rect: None,
            scroll: None,
            pane_id: "pane_1".into(),
            popup: false,
            mouse_reporting: true,
            sgr_pixel_mouse: false,
            pixel_width: 0,
            pixel_height: 0,
        });
        state
    }

    fn mouse(kind: MouseEventKind, column: u16, row: u16) -> crossterm::event::MouseEvent {
        crossterm::event::MouseEvent {
            kind,
            column,
            row,
            modifiers: crossterm::event::KeyModifiers::empty(),
        }
    }

    #[test]
    fn pane_location_copies_public_id_and_consumes_the_full_gesture() {
        let mut state = state();
        let mut outcome = ClientShellInput::default();
        state.handle_mouse(
            mouse(MouseEventKind::Down(MouseButton::Left), 17, 0),
            &mut outcome,
        );
        state.handle_mouse(
            mouse(MouseEventKind::Drag(MouseButton::Left), 17, 0),
            &mut outcome,
        );
        state.handle_mouse(
            mouse(MouseEventKind::Up(MouseButton::Left), 17, 0),
            &mut outcome,
        );
        assert!(
            matches!(&outcome.actions[..], [ClientShellAction::ClipboardWrite(bytes)] if bytes == b"pane_1")
        );
        assert!(
            outcome.requests.is_empty(),
            "copy gesture must not reach the pane app"
        );
        assert!(state.pane_location_pressed.is_none());
    }

    #[test]
    fn pane_location_release_outside_or_after_pane_closes_cancels_copy() {
        let mut state = state();
        for closed in [false, true] {
            let mut outcome = ClientShellInput::default();
            assert!(state.handle_pane_location_mouse(
                mouse(MouseEventKind::Down(MouseButton::Left), 17, 0),
                &mut outcome
            ));
            if closed {
                state.snapshot.as_deref_mut().unwrap().panes.clear();
            }
            let x = if closed { 17 } else { 1 };
            assert!(state.handle_pane_location_mouse(
                mouse(MouseEventKind::Up(MouseButton::Left), x, 0),
                &mut outcome
            ));
            assert!(outcome.actions.is_empty());
        }
    }

    #[test]
    fn pane_location_renders_only_on_hovered_bordered_pane() {
        let mut state = state();
        let mut outcome = ClientShellInput::default();
        state.handle_pane_location_mouse(mouse(MouseEventKind::Moved, 5, 3), &mut outcome);
        let buffer = Buffer::empty(Rect::new(0, 0, 20, 8));
        let mut frame = FrameData::from_ratatui_buffer_with_hyperlinks(&buffer, None, &[]);
        let mut occlusion = crate::kitty_graphics::surface::Occlusion::default();
        state.render_pane_location(&mut frame, &mut occlusion);
        assert_eq!(frame.cells[17].symbol, "⧉");
        state.hits.panes[0].inner_rect.y = 0;
        assert!(button_rect(&state.hits.panes[0]).is_none());
    }
}
