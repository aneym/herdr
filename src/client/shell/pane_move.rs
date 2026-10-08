//! Keyboard placement uses the same lift, preview and commit path as pointer drag.
use super::pane_drag::{pane_grip_rect, PaneDragTarget};
use super::pane_drop::{DropSide, DropZone};
use super::*;
use crate::layout::{find_in_direction, NavDirection, PaneId, PaneInfo};
use crossterm::event::KeyModifiers;

pub(super) struct ClientPaneMove {
    pub(super) target_pane_id: String,
}

impl ClientShellState {
    pub(super) fn pane_move_supported(&self) -> bool {
        self.endpoint_is_online(&self.active_endpoint_id)
            && self
                .endpoints
                .iter()
                .find(|e| e.endpoint_id == self.active_endpoint_id)
                .and_then(|e| e.methods.as_ref())
                .is_some_and(|m| m.contains("pane.place"))
            && self.snapshot.as_ref().is_some_and(|s| {
                s.tabs
                    .iter()
                    .any(|t| Some(&t.tab_id) == s.focused_tab_id.as_ref() && !t.zoomed)
            })
            && self.hits.panes.iter().filter(|h| !h.popup).count() >= 2
            && self.popup_terminal_id.is_none()
            && !self.popup_pending
            && self.hits.popup.is_none()
    }

    fn move_neighbour(&self, pane_id: &str, direction: NavDirection) -> Option<String> {
        let panes: Vec<PaneInfo> = self
            .hits
            .panes
            .iter()
            .enumerate()
            .filter(|(_, h)| !h.popup)
            .map(|(index, h)| PaneInfo {
                id: PaneId::from_raw(index as u32),
                rect: h.rect,
                inner_rect: h.inner_rect,
                scrollbar_rect: h.scrollbar_rect,
                borders: ratatui::widgets::Borders::NONE,
                is_focused: h.pane_id == pane_id,
            })
            .collect();
        let focused = panes
            .iter()
            .find(|p| self.hits.panes[p.id.raw() as usize].pane_id == pane_id)?;
        let id = find_in_direction(focused, direction, &panes)?;
        Some(self.hits.panes[id.raw() as usize].pane_id.clone())
    }

    pub(super) fn enter_pane_move_mode(&mut self, outcome: &mut ClientShellInput) {
        if !self.pane_move_supported() {
            if self.hits.panes.iter().filter(|h| !h.popup).count() < 2 {
                self.copy_feedback = Some(crate::app::state::CopyFeedback {
                    message: "nothing to move".into(),
                });
                self.copy_feedback_deadline =
                    Some(std::time::Instant::now() + std::time::Duration::from_secs(2));
                outcome.repaint = true;
            }
            return;
        }
        let Some(source) = self.focused_pane_id() else {
            return;
        };
        let Some(origin) = self
            .snapshot
            .as_ref()
            .and_then(|s| s.focused_tab_id.clone())
        else {
            return;
        };
        let Some(hit) = self
            .hits
            .panes
            .iter()
            .find(|h| h.pane_id == source && !h.popup)
        else {
            return;
        };
        let rect = pane_grip_rect(hit).unwrap_or(hit.rect);
        let target = [
            NavDirection::Right,
            NavDirection::Down,
            NavDirection::Left,
            NavDirection::Up,
        ]
        .into_iter()
        .find_map(|direction| self.move_neighbour(&source, direction));
        if !self.lift_pane(source.clone(), origin, (rect.x, rect.y)) {
            return;
        }
        self.pane_move = Some(ClientPaneMove {
            target_pane_id: target.unwrap_or(source),
        });
        self.mode = ClientShellMode::Move;
        self.move_centre(outcome);
    }

    fn move_centre(&mut self, outcome: &mut ClientShellInput) {
        let Some(movement) = self.pane_move.as_ref() else {
            return;
        };
        let target = movement.target_pane_id.clone();
        let source = matches!(&self.chrome_drag, Some(ClientChromeDrag::Pane { source_pane_id, .. }) if *source_pane_id == target);
        let index = self.hits.panes.iter().position(|h| h.pane_id == target);
        if source || index.is_none() {
            self.retarget_pane_drag(None, None, None, outcome);
        } else if let Some(index) = index {
            self.retarget_pane_drag(
                Some(PaneDragTarget::Centre { pane_id: target }),
                Some(DropZone::Centre { target: index }),
                None,
                outcome,
            );
        }
    }

    pub(super) fn route_move_key(
        &mut self,
        key: &crate::input::TerminalKey,
        outcome: &mut ClientShellInput,
    ) {
        if key.kind == crossterm::event::KeyEventKind::Release {
            return;
        }
        if matches!(key.code, KeyCode::Enter | KeyCode::Char(' ')) && key.modifiers.is_empty() {
            let pointer = match self.chrome_drag.as_ref() {
                Some(ClientChromeDrag::Pane { pointer, .. }) => *pointer,
                _ => {
                    self.cancel_pane_drag();
                    return;
                }
            };
            self.drop_pane(pointer, outcome);
            self.pane_move = None;
            self.mode = self.copy_or_terminal_mode();
            outcome.repaint = true;
            return;
        }
        if key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER)
        {
            return;
        }
        let (direction, side) = match key.code {
            KeyCode::Char('h' | 'H') | KeyCode::Left => (NavDirection::Left, DropSide::Left),
            KeyCode::Char('j' | 'J') | KeyCode::Down => (NavDirection::Down, DropSide::Down),
            KeyCode::Char('k' | 'K') | KeyCode::Up => (NavDirection::Up, DropSide::Up),
            KeyCode::Char('l' | 'L') | KeyCode::Right => (NavDirection::Right, DropSide::Right),
            _ => return,
        };
        let Some(movement) = self.pane_move.as_ref() else {
            return;
        };
        let target = movement.target_pane_id.clone();
        if key.modifiers.contains(KeyModifiers::SHIFT)
            || matches!(key.code, KeyCode::Char('H' | 'J' | 'K' | 'L'))
        {
            let source = matches!(&self.chrome_drag, Some(ClientChromeDrag::Pane { source_pane_id, .. }) if *source_pane_id == target);
            if source {
                self.retarget_pane_drag(
                    Some(PaneDragTarget::TabEdge { side }),
                    Some(DropZone::TabEdge { side }),
                    None,
                    outcome,
                );
            } else if let Some(index) = self.hits.panes.iter().position(|h| h.pane_id == target) {
                self.retarget_pane_drag(
                    Some(PaneDragTarget::PaneEdge {
                        pane_id: target,
                        side,
                    }),
                    Some(DropZone::PaneEdge {
                        target: index,
                        side,
                    }),
                    None,
                    outcome,
                );
            }
        } else if let Some(neighbour) = self.move_neighbour(&target, direction) {
            if let Some(movement) = self.pane_move.as_mut() {
                movement.target_pane_id = neighbour;
            }
            self.move_centre(outcome);
        }
    }
}
