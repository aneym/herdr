//! Drag a pinned chat to a new place in its machine's pin order.
//!
//! The order is a server fact (`tab.pin_move`); this client only previews it.
//! While a drag runs, the dragged chat is drawn in the slot under the pointer
//! by rewriting the owning machine's cached `pinned_tabs`, so the section,
//! its Cmd digits and Cmd+1..9 all follow at once. A drop sends the move to
//! that machine; a cancel puts the cached order back. Pins reorder within
//! their own machine: every client lists machines' pins in machine order, so
//! there is no cross-machine order to keep.

use super::*;
use crate::protocol::ClientShellPinnedTab;

impl ClientShellState {
    /// A left press on a pinned row arms a reorder drag. The row's click is
    /// replayed on release when the pointer never left the row.
    pub(super) fn arm_pin_press(&mut self, mouse: crossterm::event::MouseEvent) -> bool {
        let point = (mouse.column, mouse.row);
        let Some(hit) = self
            .hits
            .pinned_rows
            .iter()
            .find(|hit| super::contains(hit.rect, point))
        else {
            return false;
        };
        self.pin_press = Some(ClientPinPress {
            endpoint_id: hit
                .endpoint_id
                .clone()
                .unwrap_or_else(|| self.active_endpoint_id.clone()),
            aggregate: hit.endpoint_id.is_some(),
            tab_id: hit.tab_id.clone(),
            down: mouse,
        });
        true
    }

    /// Pointer moved with a pin press held: once it leaves the pressed row the
    /// press becomes a drag and can no longer click.
    pub(super) fn start_pin_drag(
        &mut self,
        mouse: crossterm::event::MouseEvent,
        outcome: &mut ClientShellInput,
    ) {
        let Some(press) = self.pin_press.as_ref() else {
            return;
        };
        if mouse.row == press.down.row {
            return;
        }
        let Some(press) = self.pin_press.take() else {
            return;
        };
        if !self.endpoint_supports_pin_move(&press.endpoint_id) {
            return;
        }
        let Some(original) = self.endpoint_pin_order(&press.endpoint_id) else {
            return;
        };
        self.pin_preview = Some(ClientPinPreview {
            endpoint_id: press.endpoint_id.clone(),
            applied: original.clone(),
            original,
            committed: false,
        });
        self.chrome_drag = Some(ClientChromeDrag::Pin {
            endpoint_id: press.endpoint_id,
            tab_id: press.tab_id,
            slot: None,
        });
        self.drag_pin((mouse.column, mouse.row), outcome);
    }

    pub(super) fn drag_pin(&mut self, point: (u16, u16), outcome: &mut ClientShellInput) {
        let Some(ClientChromeDrag::Pin {
            endpoint_id,
            tab_id,
            slot,
        }) = self.chrome_drag.as_ref()
        else {
            return;
        };
        let (endpoint_id, tab_id, current) = (endpoint_id.clone(), tab_id.clone(), *slot);
        if self.rebase_pin_preview(&endpoint_id, &tab_id, current) {
            outcome.repaint = true;
        }
        if self.pin_preview.is_none() {
            return;
        }
        let next = self.pin_slot_at(&endpoint_id, &tab_id, point);
        if next == current {
            return;
        }
        if let Some(ClientChromeDrag::Pin { slot, .. }) = self.chrome_drag.as_mut() {
            *slot = next;
        }
        let Some(original) = self
            .pin_preview
            .as_ref()
            .map(|preview| preview.original.clone())
        else {
            return;
        };
        let order = match next {
            Some(slot) => self.pin_order_with_move(&endpoint_id, &original, &tab_id, slot),
            None => original,
        };
        self.show_pin_order(&endpoint_id, order);
        outcome.repaint = true;
    }

    /// Release of a pin drag: a drop on the section moves the pin on the
    /// machine that owns it; anywhere else puts the order back.
    pub(super) fn drop_pin(
        &mut self,
        endpoint_id: ClientEndpointId,
        tab_id: String,
        slot: Option<usize>,
        outcome: &mut ClientShellInput,
    ) {
        outcome.repaint = true;
        self.rebase_pin_preview(&endpoint_id, &tab_id, slot);
        let Some(preview) = self.pin_preview.as_ref() else {
            return;
        };
        let original = preview.original.clone();
        let Some(slot) = slot else {
            self.cancel_pin_drag();
            return;
        };
        let order = self.pin_order_with_move(&endpoint_id, &original, &tab_id, slot);
        let from = original.iter().position(|pin| pin.tab_id == tab_id);
        let to = order.iter().position(|pin| pin.tab_id == tab_id);
        let (Some(from), Some(to)) = (from, to) else {
            self.cancel_pin_drag();
            return;
        };
        if from == to {
            self.cancel_pin_drag();
            return;
        }
        self.show_pin_order(&endpoint_id, order);
        if let Some(preview) = self.pin_preview.as_mut() {
            preview.committed = true;
        }
        self.push_pin_move(endpoint_id, tab_id, to, outcome);
    }

    /// Esc, or a drag the shell drops: the cached order goes back to what the
    /// machine last sent.
    pub(super) fn cancel_pin_drag(&mut self) -> bool {
        let rebased = self.rebase_active_pin_preview();
        let dragging = matches!(self.chrome_drag, Some(ClientChromeDrag::Pin { .. }));
        if dragging {
            self.chrome_drag = None;
        }
        self.settle_pin_preview() || dragging || rebased
    }

    /// A fresh machine order replaces the drag's baseline, not its target.
    fn rebase_pin_preview(
        &mut self,
        endpoint_id: &ClientEndpointId,
        tab_id: &str,
        slot: Option<usize>,
    ) -> bool {
        let Some(preview) = self.pin_preview.as_ref() else {
            return false;
        };
        if preview.committed || &preview.endpoint_id != endpoint_id {
            return false;
        }
        let Some(original) = self.endpoint_pin_order(endpoint_id) else {
            self.chrome_drag = None;
            self.pin_preview = None;
            return true;
        };
        if original == preview.applied {
            return false;
        }
        if !original.iter().any(|pin| pin.tab_id == tab_id) {
            self.chrome_drag = None;
            self.pin_preview = None;
            return true;
        }
        let order = match slot {
            Some(slot) => self.pin_order_with_move(endpoint_id, &original, tab_id, slot),
            None => original.clone(),
        };
        if let Some(preview) = self.pin_preview.as_mut() {
            preview.original = original;
        }
        self.show_pin_order(endpoint_id, order);
        true
    }

    fn rebase_active_pin_preview(&mut self) -> bool {
        let Some(ClientChromeDrag::Pin {
            endpoint_id,
            tab_id,
            slot,
        }) = self.chrome_drag.as_ref()
        else {
            return false;
        };
        let (endpoint_id, tab_id, slot) = (endpoint_id.clone(), tab_id.clone(), *slot);
        self.rebase_pin_preview(&endpoint_id, &tab_id, slot)
    }

    /// Restores an uncommitted preview no drag owns any more. A machine
    /// snapshot that replaced the preview wins.
    pub(super) fn settle_pin_preview(&mut self) -> bool {
        let rebased = self.rebase_active_pin_preview();
        if matches!(self.chrome_drag, Some(ClientChromeDrag::Pin { .. })) {
            return rebased;
        }
        let Some(preview) = self.pin_preview.take() else {
            return rebased;
        };
        if preview.committed {
            return false;
        }
        if self.endpoint_pin_order(&preview.endpoint_id).as_ref() == Some(&preview.applied) {
            self.show_pin_order(&preview.endpoint_id, preview.original);
            return true;
        }
        false
    }

    /// A release with no drag: the row's own click, as if the press had gone
    /// straight to it.
    pub(super) fn release_pin_press(
        &mut self,
        press: ClientPinPress,
        mouse: crossterm::event::MouseEvent,
        outcome: &mut ClientShellInput,
    ) {
        let point = (press.down.column, press.down.row);
        if press.aggregate {
            self.handle_endpoint_agent_click(point, outcome);
        } else if self.handle_tree_header_click(point, press.down, outcome) {
            if let Some(tab_press) = self.tree_tab_press.take() {
                self.finish_tree_tab_press(tab_press, mouse, outcome);
            }
        }
    }

    pub(super) fn endpoint_supports_tab_role(&self, endpoint_id: &ClientEndpointId) -> bool {
        self.endpoint_is_online(endpoint_id)
            && self
                .endpoints
                .iter()
                .find(|endpoint| &endpoint.endpoint_id == endpoint_id)
                .and_then(|endpoint| endpoint.methods.as_ref())
                .is_some_and(|methods| methods.contains("tab.set_role"))
    }

    pub(super) fn push_tab_role(
        &mut self,
        endpoint_id: ClientEndpointId,
        tab_id: String,
        agent: bool,
        outcome: &mut ClientShellInput,
    ) {
        if !self.endpoint_supports_tab_role(&endpoint_id) {
            return;
        }
        let Some(boot_id) = self
            .endpoints
            .iter()
            .find(|endpoint| endpoint.endpoint_id == endpoint_id)
            .and_then(|endpoint| endpoint.snapshot.as_deref())
            .map(|snapshot| snapshot.boot_id.clone())
        else {
            return;
        };
        let id = format!("client-shell:{}", self.next_request_id);
        self.next_request_id = self.next_request_id.saturating_add(1);
        self.pending_requests.insert(
            id.clone(),
            PendingEndpointRequest {
                boot_id: boot_id.clone(),
                method_name: "tab.set_role".into(),
                confirmation_workspace_id: None,
                kind: PendingEndpointKind::Generic,
            },
        );
        outcome.actions.push(ClientShellAction::Endpoint {
            endpoint_id,
            boot_id,
            request: Box::new(crate::api::schema::Request {
                id,
                method: crate::api::schema::Method::TabSetRole(
                    crate::api::schema::TabSetRoleParams {
                        tab_id,
                        role: agent.then_some(crate::api::schema::TabRole::Agent),
                    },
                ),
            }),
        });
        outcome.repaint = true;
    }

    fn endpoint_supports_pin_move(&self, endpoint_id: &ClientEndpointId) -> bool {
        self.endpoint_is_online(endpoint_id)
            && self
                .endpoints
                .iter()
                .find(|endpoint| &endpoint.endpoint_id == endpoint_id)
                .is_some_and(|endpoint| {
                    endpoint
                        .methods
                        .as_ref()
                        .is_none_or(|methods| methods.contains("tab.pin_move"))
                })
    }

    fn endpoint_pin_order(
        &self,
        endpoint_id: &ClientEndpointId,
    ) -> Option<Vec<ClientShellPinnedTab>> {
        self.endpoints
            .iter()
            .find(|endpoint| &endpoint.endpoint_id == endpoint_id)
            .and_then(|endpoint| endpoint.snapshot.as_deref())
            .map(|snapshot| snapshot.pinned_tabs.clone())
    }

    /// The machine's live pins (the rows drawn) are what slots count; pins
    /// whose chat the snapshot does not list keep their place around them.
    fn pin_order_with_move(
        &self,
        endpoint_id: &ClientEndpointId,
        original: &[ClientShellPinnedTab],
        tab_id: &str,
        slot: usize,
    ) -> Vec<ClientShellPinnedTab> {
        let live_tab = |pin: &ClientShellPinnedTab| {
            self.endpoints
                .iter()
                .find(|endpoint| &endpoint.endpoint_id == endpoint_id)
                .and_then(|endpoint| endpoint.snapshot.as_deref())
                .is_some_and(|snapshot| snapshot.tabs.iter().any(|tab| tab.tab_id == pin.tab_id))
        };
        let Some(moved) = original.iter().find(|pin| pin.tab_id == tab_id).cloned() else {
            return original.to_vec();
        };
        let mut order = original
            .iter()
            .filter(|pin| pin.tab_id != tab_id)
            .cloned()
            .collect::<Vec<_>>();
        let live = order
            .iter()
            .enumerate()
            .filter(|(_, pin)| live_tab(pin))
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        let index = match live.get(slot) {
            Some(index) => *index,
            None => live.last().map_or(order.len(), |index| index + 1),
        };
        let start = order
            .iter()
            .position(|pin| pin.role == moved.role)
            .unwrap_or_else(|| if moved.role.is_some() { 0 } else { order.len() });
        let end = order
            .iter()
            .rposition(|pin| pin.role == moved.role)
            .map_or(start, |i| i + 1);
        order.insert(index.clamp(start, end), moved);
        order
    }

    fn show_pin_order(&mut self, endpoint_id: &ClientEndpointId, order: Vec<ClientShellPinnedTab>) {
        if let Some(snapshot) = self
            .endpoints
            .iter_mut()
            .find(|endpoint| &endpoint.endpoint_id == endpoint_id)
            .and_then(|endpoint| endpoint.snapshot.as_deref_mut())
        {
            snapshot.pinned_tabs = order.clone();
        }
        if endpoint_id == &self.active_endpoint_id {
            if let Some(snapshot) = self.snapshot.as_deref_mut() {
                snapshot.pinned_tabs = order.clone();
            }
        }
        if let Some(preview) = self
            .pin_preview
            .as_mut()
            .filter(|preview| &preview.endpoint_id == endpoint_id)
        {
            preview.applied = order;
        }
    }

    /// Which of `endpoint_id`'s slots the pointer stands for. Above or below
    /// that machine's rows clamps to its first or last; off the section (more
    /// than a row past it, or outside its columns) is `None`.
    pub(super) fn pin_slot_at(
        &self,
        endpoint_id: &ClientEndpointId,
        tab_id: &str,
        point: (u16, u16),
    ) -> Option<usize> {
        let rows = &self.hits.pinned_rows;
        let first = rows.first()?.rect;
        let last = rows.last()?.rect;
        let top = first.y.saturating_sub(1);
        let bottom = last.bottom().saturating_add(1);
        if point.0 < first.x || point.0 >= first.right() || point.1 < top || point.1 >= bottom {
            return None;
        }
        let snapshot = self
            .endpoints
            .iter()
            .find(|endpoint| &endpoint.endpoint_id == endpoint_id)?
            .snapshot
            .as_deref()?;
        let role = snapshot
            .pinned_tabs
            .iter()
            .find(|pin| pin.tab_id == tab_id)?
            .role;
        let own = rows
            .iter()
            .filter(|hit| {
                hit.endpoint_id.as_ref().unwrap_or(&self.active_endpoint_id) == endpoint_id
                    && snapshot
                        .pinned_tabs
                        .iter()
                        .any(|pin| pin.tab_id == hit.tab_id && pin.role == role)
            })
            .collect::<Vec<_>>();
        let first_own = own.first()?;
        let last_own = own.last()?;
        if point.1 < first_own.rect.y {
            return Some(first_own.slot);
        }
        Some(
            own.iter()
                .find(|hit| point.1 < hit.rect.bottom())
                .unwrap_or(last_own)
                .slot,
        )
    }

    fn push_pin_move(
        &mut self,
        endpoint_id: ClientEndpointId,
        tab_id: String,
        pin_index: usize,
        outcome: &mut ClientShellInput,
    ) {
        let Some(boot_id) = self
            .endpoints
            .iter()
            .find(|endpoint| endpoint.endpoint_id == endpoint_id)
            .and_then(|endpoint| endpoint.snapshot.as_deref())
            .map(|snapshot| snapshot.boot_id.clone())
        else {
            return;
        };
        let id = format!("client-shell:{}", self.next_request_id);
        self.next_request_id = self.next_request_id.saturating_add(1);
        self.pending_requests.insert(
            id.clone(),
            PendingEndpointRequest {
                boot_id: boot_id.clone(),
                method_name: "tab.pin_move".into(),
                confirmation_workspace_id: None,
                kind: PendingEndpointKind::Generic,
            },
        );
        // Surface-independent, like a pin toggle: it reaches the owning
        // machine whether or not that machine holds the surface.
        outcome.actions.push(ClientShellAction::Endpoint {
            endpoint_id,
            boot_id,
            request: Box::new(crate::api::schema::Request {
                id,
                method: crate::api::schema::Method::TabPinMove(
                    crate::api::schema::TabPinMoveParams { tab_id, pin_index },
                ),
            }),
        });
    }

    /// Keyboard reorder: move the focused chat's pin one slot up or down on
    /// the active machine.
    pub(super) fn move_focused_pin(
        &mut self,
        delta: isize,
        outcome: &mut ClientShellInput,
    ) -> bool {
        let endpoint_id = self.active_endpoint_id.clone();
        if !self.endpoint_supports_pin_move(&endpoint_id) {
            return false;
        }
        let Some(snapshot) = self.snapshot.as_deref() else {
            return false;
        };
        let Some(tab_id) = snapshot.focused_tab_id.clone() else {
            return false;
        };
        let live = snapshot
            .pinned_tabs
            .iter()
            .filter(|pin| snapshot.tabs.iter().any(|tab| tab.tab_id == pin.tab_id))
            .map(|pin| pin.tab_id.clone())
            .collect::<Vec<_>>();
        let Some(from) = live.iter().position(|pin| *pin == tab_id) else {
            return false;
        };
        let Some(slot) = from
            .checked_add_signed(delta)
            .filter(|slot| *slot < live.len())
        else {
            return true;
        };
        let original = snapshot.pinned_tabs.clone();
        let order = self.pin_order_with_move(&endpoint_id, &original, &tab_id, slot);
        let Some(to) = order.iter().position(|pin| pin.tab_id == tab_id) else {
            return false;
        };
        if to == from {
            return true;
        }
        self.pin_preview = Some(ClientPinPreview {
            endpoint_id: endpoint_id.clone(),
            applied: order.clone(),
            original,
            committed: true,
        });
        self.show_pin_order(&endpoint_id, order);
        self.push_pin_move(endpoint_id, tab_id, to, outcome);
        outcome.repaint = true;
        true
    }
}
