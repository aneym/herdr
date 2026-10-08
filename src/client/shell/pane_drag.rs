//! Client-only placement preview, updated on input rather than during render.
use super::pane_drop::{DropMetrics, DropSide, DropZone};
use super::*;
use crate::api::schema::{Method, PaneDirection, PanePlaceParams, PanePlaceTarget, ResponseResult};
use crossterm::event::MouseEvent;
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum PaneDragTarget {
    Centre { pane_id: String },
    PaneEdge { pane_id: String, side: DropSide },
    TabEdge { side: DropSide },
    IntoTab { tab_id: String },
    NewTabIn { workspace_id: String },
}
#[derive(Clone, Debug)]
pub(super) enum PaneDragAnswer {
    Changed(Rect),
    NoChange,
    Error,
}
impl PaneDragAnswer {
    fn rect(&self) -> Option<Rect> {
        match self {
            Self::Changed(rect) => Some(*rect),
            _ => None,
        }
    }
}
pub(super) struct ClientPaneDragPreview {
    pub(super) chip: String,
    pub(super) ghost: Option<Rect>,
    pub(super) in_flight: Option<(String, PaneDragTarget)>,
    pub(super) queued: Option<PaneDragTarget>,
    pub(super) answers: Vec<(PaneDragTarget, PaneDragAnswer)>,
    pub(super) topology: u64,
    pub(super) committed: bool,
    rects: Vec<Rect>,
    area: Rect,
    invalidated: bool,
    source: PaneHit,
    row: Option<Rect>,
    accent: u32,
    background: u32,
    text: u32,
}
pub(super) fn pane_grip_rect(hit: &PaneHit) -> Option<Rect> {
    (!hit.popup && hit.inner_rect.y > hit.rect.y && hit.rect.width >= 6)
        .then(|| Rect::new(hit.rect.x + (hit.rect.width - 2) / 2, hit.rect.y, 2, 1))
}
fn direction(side: DropSide) -> PaneDirection {
    match side {
        DropSide::Left => PaneDirection::Left,
        DropSide::Right => PaneDirection::Right,
        DropSide::Up => PaneDirection::Up,
        DropSide::Down => PaneDirection::Down,
    }
}
impl ClientShellState {
    pub(super) fn pane_drag_supported(&self) -> bool {
        self.config.mouse_capture
            && self.endpoint_is_online(&self.active_endpoint_id)
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
    }
    pub(super) fn arm_pane_press(&mut self, mouse: MouseEvent) -> bool {
        if !self.pane_drag_supported() {
            return false;
        }
        let point = (mouse.column, mouse.row);
        let hit = self.hits.panes.iter().find(|h| {
            pane_grip_rect(h).is_some_and(|g| {
                contains(g, point)
                    || (point.1 == h.rect.y
                        && point.0 > h.rect.x
                        && point.0 < h.rect.right().saturating_sub(4)
                        && !h.scrollbar_rect.is_some_and(|r| contains(r, point))
                        && !self
                            .hits
                            .pane_splits
                            .iter()
                            .any(|s| contains(s.hit_rect, point)))
            })
        });
        let Some(hit) = hit else {
            return false;
        };
        let Some(tab_id) = self
            .snapshot
            .as_ref()
            .and_then(|s| s.focused_tab_id.clone())
        else {
            return false;
        };
        self.pane_press = Some(ClientPanePress {
            pane_id: hit.pane_id.clone(),
            tab_id,
            down: mouse,
        });
        true
    }
    pub(super) fn start_pane_drag(&mut self, mouse: MouseEvent, outcome: &mut ClientShellInput) {
        let Some(p) = self.pane_press.as_ref() else {
            return;
        };
        if (p.down.column, p.down.row) == (mouse.column, mouse.row) {
            return;
        }
        let Some(p) = self.pane_press.take() else {
            return;
        };
        let Some(source) = self
            .hits
            .panes
            .iter()
            .find(|h| h.pane_id == p.pane_id)
            .cloned()
        else {
            return;
        };
        let snapshot = self.snapshot.as_deref();
        let pane = snapshot.and_then(|s| s.panes.iter().find(|h| h.pane_id == p.pane_id));
        let label = pane
            .and_then(|h| h.label.clone())
            .or_else(|| {
                snapshot
                    .and_then(|s| s.agents.iter().find(|a| a.pane_id == p.pane_id))
                    .and_then(|a| a.name.clone())
            })
            .or_else(|| {
                pane.and_then(|h| h.cwd.as_ref())
                    .and_then(|c| std::path::Path::new(c).file_name())
                    .map(|n| n.to_string_lossy().into_owned())
            })
            .unwrap_or_else(|| "pane".into());
        let mut chip = String::from(" ⠿ ");
        let mut cells = 0;
        for ch in label.chars() {
            let width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
            if cells + width > 20 {
                break;
            }
            cells += width;
            chip.push(ch);
        }
        chip.push(' ');
        self.pane_drag = Some(ClientPaneDragPreview {
            chip,
            ghost: None,
            in_flight: None,
            queued: None,
            answers: Vec::new(),
            topology: self
                .pane_surface
                .as_ref()
                .map_or(0, |s| pane_drag_surface_signature(s)),
            committed: false,
            rects: self.hits.panes.iter().map(|h| h.rect).collect(),
            area: self
                .last_composed_size
                .map(|(c, r)| self.layout(c, r).pane_surface)
                .unwrap_or_default(),
            invalidated: false,
            source,
            row: None,
            accent: crate::protocol::color_to_u32(self.config.palette.accent),
            background: crate::protocol::color_to_u32(self.config.palette.surface1),
            text: crate::protocol::color_to_u32(self.config.palette.text),
        });
        self.chrome_drag = Some(ClientChromeDrag::Pane {
            source_pane_id: p.pane_id,
            origin_tab_id: p.tab_id,
            pointer: (mouse.column, mouse.row),
            target: None,
        });
        self.drag_pane((mouse.column, mouse.row), outcome);
    }
    pub(super) fn drag_pane(&mut self, point: (u16, u16), outcome: &mut ClientShellInput) {
        let Some(ClientChromeDrag::Pane {
            source_pane_id,
            origin_tab_id,
            target: old,
            ..
        }) = self.chrome_drag.as_ref()
        else {
            return;
        };
        let source = source_pane_id.clone();
        let origin = origin_tab_id.clone();
        let old = old.clone();
        let Some((cols, rows)) = self.last_composed_size else {
            return;
        };
        let area = self.layout(cols, rows).pane_surface;
        let missing_target = matches!(&old,
            Some(PaneDragTarget::Centre { pane_id } | PaneDragTarget::PaneEdge { pane_id, .. })
                if !self.hits.panes.iter().any(|h| &h.pane_id == pane_id));
        if missing_target || !self.hits.panes.iter().any(|h| h.pane_id == source) {
            self.cancel_pane_drag();
            outcome.repaint = true;
            return;
        }
        let Some(preview) = self.pane_drag.as_mut() else {
            return;
        };
        let geometry_changed = preview.area != area
            || !preview
                .rects
                .iter()
                .copied()
                .eq(self.hits.panes.iter().map(|h| h.rect));
        if geometry_changed {
            preview.answers.clear();
            preview.in_flight = None;
            preview.queued = None;
            preview.invalidated = true;
        }
        preview.area = area;
        preview.rects.clear();
        preview.rects.extend(self.hits.panes.iter().map(|h| h.rect));
        let invalidated = std::mem::take(&mut preview.invalidated);
        let zone = super::pane_drop::drop_zone_at(
            area,
            &preview.rects,
            self.hits.panes.iter().position(|h| h.pane_id == source),
            (f32::from(point.0) + 0.5, f32::from(point.1) + 0.5),
            &DropMetrics::TUI,
        );
        let mut row = None;
        let target = zone.map(|z| match z {
            DropZone::Centre { target } => PaneDragTarget::Centre {
                pane_id: self.hits.panes[target].pane_id.clone(),
            },
            DropZone::PaneEdge { target, side } => PaneDragTarget::PaneEdge {
                pane_id: self.hits.panes[target].pane_id.clone(),
                side,
            },
            DropZone::TabEdge { side } => PaneDragTarget::TabEdge { side },
        });
        let target = target.or_else(|| {
            if contains(area, point) {
                return None;
            }
            let snapshot = self.snapshot.as_ref()?;
            let workspace = snapshot
                .tabs
                .iter()
                .find(|t| t.tab_id == origin)?
                .workspace_id
                .as_str();
            if let Some((rect, id)) = self.hits.tabs.iter().find(|(r, id)| {
                contains(*r, point)
                    && *id != origin
                    && snapshot
                        .tabs
                        .iter()
                        .any(|t| &t.tab_id == id && t.workspace_id == workspace)
            }) {
                row = Some(*rect);
                return Some(PaneDragTarget::IntoTab { tab_id: id.clone() });
            }
            if let Some(h) = self
                .hits
                .tree_headers
                .iter()
                .find(|h| contains(h.rect, point))
            {
                row = Some(h.rect);
                return match &h.tab_id {
                    Some(id) if *id != origin && h.workspace_id == workspace => {
                        Some(PaneDragTarget::IntoTab { tab_id: id.clone() })
                    }
                    None => Some(PaneDragTarget::NewTabIn {
                        workspace_id: h.workspace_id.clone(),
                    }),
                    _ => None,
                };
            }
            self.hits
                .workspaces
                .iter()
                .find(|h| h.endpoint_id == self.active_endpoint_id && contains(h.rect, point))
                .map(|h| {
                    row = Some(h.rect);
                    PaneDragTarget::NewTabIn {
                        workspace_id: h.workspace_id.clone(),
                    }
                })
        });
        if let Some(ClientChromeDrag::Pane {
            pointer,
            target: current,
            ..
        }) = self.chrome_drag.as_mut()
        {
            *pointer = point;
            *current = target.clone();
        }
        outcome.repaint = true;
        if target == old && !invalidated {
            return;
        }
        preview.row = row;
        preview.queued = None;
        preview.ghost = zone.map(|z| super::pane_drop::zone_estimate_rect(area, &preview.rects, z));
        if let Some(t) = target {
            if let Some((_, rect)) = preview.answers.iter().find(|(key, _)| *key == t) {
                preview.ghost = rect.rect();
            } else if matches!(
                t,
                PaneDragTarget::PaneEdge { .. } | PaneDragTarget::TabEdge { .. }
            ) {
                if preview.in_flight.is_some() {
                    preview.queued = Some(t);
                } else {
                    self.send_pane_drag_dry_run(source, origin, t, outcome);
                }
            }
        }
    }
    fn pane_place_method(
        source: String,
        origin: String,
        target: &PaneDragTarget,
        dry_run: bool,
    ) -> Option<Method> {
        let (target, side) = match target {
            PaneDragTarget::PaneEdge { pane_id, side } => (
                PanePlaceTarget::Pane {
                    pane_id: pane_id.clone(),
                },
                direction(*side),
            ),
            PaneDragTarget::TabEdge { side } => {
                (PanePlaceTarget::Tab { tab_id: origin }, direction(*side))
            }
            PaneDragTarget::IntoTab { tab_id } => (
                PanePlaceTarget::Tab {
                    tab_id: tab_id.clone(),
                },
                PaneDirection::Right,
            ),
            _ => return None,
        };
        Some(Method::PanePlace(PanePlaceParams {
            pane_id: source,
            target,
            side,
            size: None,
            focus: !dry_run,
            dry_run,
        }))
    }
    fn send_pane_drag_dry_run(
        &mut self,
        source: String,
        origin: String,
        target: PaneDragTarget,
        outcome: &mut ClientShellInput,
    ) {
        let Some(method) = Self::pane_place_method(source.clone(), origin, &target, true) else {
            return;
        };
        let id = format!("client-shell:{}", self.next_request_id);
        if self.push_endpoint_method_with_kind(
            method,
            PendingEndpointKind::PaneDragDryRun {
                source_pane_id: source,
                target: target.clone(),
            },
            outcome,
        ) {
            if let Some(p) = self.pane_drag.as_mut() {
                p.in_flight = Some((id, target));
            }
        }
    }
    pub(super) fn complete_pane_drag_dry_run(
        &mut self,
        request_id: &str,
        target: PaneDragTarget,
        result: Result<ResponseResult, ClientShellEndpointError>,
    ) -> bool {
        let area = self
            .last_composed_size
            .map(|(c, r)| self.layout(c, r).pane_surface)
            .unwrap_or_default();
        let Some(p) = self.pane_drag.as_mut().filter(|p| !p.committed) else {
            return false;
        };
        if !p
            .in_flight
            .as_ref()
            .is_some_and(|(id, t)| id == request_id && *t == target)
        {
            return false;
        }
        p.in_flight = None;
        let answer = match result {
            Ok(ResponseResult::PanePlace { place: result }) if result.changed => {
                let a = result.target_layout.area;
                let r = result.placed_rect;
                if a.width == 0 || a.height == 0 {
                    PaneDragAnswer::Error
                } else {
                    PaneDragAnswer::Changed(Rect::new(
                        area.x.saturating_add(
                            ((u32::from(r.x.saturating_sub(a.x)) * u32::from(area.width))
                                / u32::from(a.width)) as u16,
                        ),
                        area.y.saturating_add(
                            ((u32::from(r.y.saturating_sub(a.y)) * u32::from(area.height))
                                / u32::from(a.height)) as u16,
                        ),
                        ((u32::from(r.width) * u32::from(area.width)) / u32::from(a.width)) as u16,
                        ((u32::from(r.height) * u32::from(area.height)) / u32::from(a.height))
                            as u16,
                    ))
                }
            }
            Ok(ResponseResult::PanePlace { place }) if !place.changed => PaneDragAnswer::NoChange,
            _ => PaneDragAnswer::Error,
        };
        let rect = answer.rect();
        p.answers.push((target.clone(), answer));
        if matches!(&self.chrome_drag, Some(ClientChromeDrag::Pane { target: Some(t), .. }) if *t == target)
        {
            p.ghost = rect;
        }
        true
    }
    pub(super) fn dispatch_queued_pane_drag(&mut self, outcome: &mut ClientShellInput) {
        let Some(target) = self.pane_drag.as_mut().and_then(|p| p.queued.take()) else {
            return;
        };
        let Some(ClientChromeDrag::Pane {
            source_pane_id,
            origin_tab_id,
            ..
        }) = self.chrome_drag.as_ref()
        else {
            return;
        };
        self.send_pane_drag_dry_run(
            source_pane_id.clone(),
            origin_tab_id.clone(),
            target,
            outcome,
        );
    }
    pub(super) fn drop_pane(&mut self, _point: (u16, u16), outcome: &mut ClientShellInput) {
        let Some(ClientChromeDrag::Pane {
            source_pane_id,
            origin_tab_id,
            target: Some(target),
            ..
        }) = self.chrome_drag.take()
        else {
            self.cancel_pane_drag();
            return;
        };
        if self.pane_drag.as_ref().is_some_and(|p| {
            p.answers
                .iter()
                .any(|(t, r)| *t == target && !matches!(r, PaneDragAnswer::Changed(_)))
        }) {
            self.cancel_pane_drag();
            return;
        }
        let method = match &target {
            PaneDragTarget::Centre { pane_id } => {
                Some(Method::PaneSwap(crate::api::schema::PaneSwapParams {
                    source_pane_id: Some(source_pane_id.clone()),
                    target_pane_id: Some(pane_id.clone()),
                    direction: None,
                    pane_id: None,
                }))
            }
            PaneDragTarget::NewTabIn { workspace_id } => {
                Some(Method::PaneMove(crate::api::schema::PaneMoveParams {
                    pane_id: source_pane_id.clone(),
                    destination: crate::api::schema::PaneMoveDestination::NewTab {
                        workspace_id: Some(workspace_id.clone()),
                        label: None,
                    },
                    focus: true,
                }))
            }
            _ => Self::pane_place_method(source_pane_id, origin_tab_id, &target, false),
        };
        if let Some(method) = method {
            self.push_endpoint_method(method, outcome);
        }
        if let Some(p) = self.pane_drag.as_mut() {
            p.committed = true;
            p.in_flight = None;
            p.queued = None;
        }
        outcome.repaint = true;
    }
    pub(super) fn cancel_pane_drag(&mut self) -> bool {
        let changed = self.pane_press.take().is_some() | self.pane_drag.take().is_some();
        if matches!(self.chrome_drag, Some(ClientChromeDrag::Pane { .. })) {
            self.chrome_drag = None;
        }
        self.pane_grip_hover = None;
        changed
    }
    pub(super) fn render_pane_grips(&self, frame: &mut FrameData) {
        if !self.pane_drag_supported() {
            return;
        }
        let accent = crate::protocol::color_to_u32(self.config.palette.accent);
        let quiet = crate::protocol::color_to_u32(self.config.palette.overlay0);
        for hit in &self.hits.panes {
            let Some(g) = pane_grip_rect(hit) else {
                continue;
            };
            let lifted = matches!(&self.chrome_drag, Some(ClientChromeDrag::Pane { source_pane_id, .. }) if *source_pane_id == hit.pane_id);
            for x in g.x..g.right() {
                if let Some(c) = cell_mut(frame, x, g.y) {
                    c.symbol.clear();
                    c.symbol.push_str("⠿");
                    c.fg = if lifted || self.pane_grip_hover.as_ref() == Some(&hit.pane_id) {
                        accent
                    } else {
                        quiet
                    };
                }
            }
        }
    }
    pub(super) fn render_pane_drag(
        &self,
        frame: &mut FrameData,
        occlusion: &mut crate::kitty_graphics::surface::Occlusion,
    ) {
        let Some(p) = self.pane_drag.as_ref() else {
            return;
        };
        let accent = p.accent;
        let bg = p.background;
        if !p.committed {
            let r = p.source.rect;
            for y in r.y..r.bottom() {
                for x in r.x..r.right() {
                    if let Some(c) = cell_mut(frame, x, y) {
                        if contains(p.source.inner_rect, (x, y)) {
                            c.modifier |= Modifier::DIM.bits();
                        } else if x == r.x || x == r.right() - 1 || y == r.y || y == r.bottom() - 1
                        {
                            c.fg = accent;
                        }
                    }
                }
            }
        }
        if let Some(r) = p.ghost.filter(|r| r.width > 1 && r.height > 1) {
            occlusion.cover(r);
            for y in r.y..r.bottom() {
                for x in r.x..r.right() {
                    let symbol = if y == r.y {
                        if x == r.x {
                            "╭"
                        } else if x == r.right() - 1 {
                            "╮"
                        } else {
                            "─"
                        }
                    } else if y == r.bottom() - 1 {
                        if x == r.x {
                            "╰"
                        } else if x == r.right() - 1 {
                            "╯"
                        } else {
                            "─"
                        }
                    } else if x == r.x || x == r.right() - 1 {
                        "│"
                    } else {
                        ""
                    };
                    if let Some(c) = cell_mut(frame, x, y) {
                        if symbol.is_empty() {
                            c.bg = bg;
                        } else {
                            c.symbol.clear();
                            c.symbol.push_str(symbol);
                            c.fg = accent;
                        }
                    }
                }
            }
        }
        if p.committed {
            return;
        }
        if let Some(r) = p.row {
            occlusion.cover(r);
            for y in r.y..r.bottom() {
                for x in r.x..r.right() {
                    if let Some(c) = cell_mut(frame, x, y) {
                        c.bg = bg;
                    }
                }
            }
            if let Some(c) = cell_mut(frame, r.x, r.y) {
                c.symbol.clear();
                c.symbol.push_str("│");
                c.fg = accent;
            }
        }
        if let Some(ClientChromeDrag::Pane { pointer, .. }) = &self.chrome_drag {
            let width =
                UnicodeWidthStr::width(p.chip.as_str()).min(usize::from(frame.width)) as u16;
            let x = pointer
                .0
                .saturating_add(1)
                .min(frame.width.saturating_sub(width));
            let y = pointer
                .1
                .saturating_add(1)
                .min(frame.height.saturating_sub(1));
            occlusion.cover(Rect::new(x, y, width, 1));
            let mut col = x;
            for ch in p.chip.chars() {
                if col >= x + width {
                    break;
                }
                if let Some(c) = cell_mut(frame, col, y) {
                    c.symbol.clear();
                    c.symbol.push(ch);
                    c.bg = bg;
                    c.fg = p.text;
                }
                col = col
                    .saturating_add(unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0) as u16);
            }
        }
    }
}
fn cell_mut(frame: &mut FrameData, x: u16, y: u16) -> Option<&mut crate::protocol::CellData> {
    if x >= frame.width || y >= frame.height {
        return None;
    }
    frame
        .cells
        .get_mut(usize::from(y) * usize::from(frame.width) + usize::from(x))
}
impl ClientShellState {
    pub(super) fn rebase_pane_drag_surface(&mut self, surface: &PaneSurfaceFrame) {
        if self.pane_drag.as_ref().is_some_and(|p| {
            p.committed || !surface.panes.iter().any(|h| h.pane_id == p.source.pane_id)
                || matches!(&self.chrome_drag, Some(ClientChromeDrag::Pane {
                    target: Some(PaneDragTarget::Centre { pane_id } | PaneDragTarget::PaneEdge { pane_id, .. }), ..
                }) if !surface.panes.iter().any(|h| &h.pane_id == pane_id))
        }) {
            self.cancel_pane_drag();
            return;
        }
        if self.pane_drag.is_none() {
            return;
        }
        let signature = pane_drag_surface_signature(surface);
        let Some(p) = self.pane_drag.as_mut() else {
            return;
        };
        if p.topology != signature {
            p.topology = signature;
            p.answers.clear();
            p.in_flight = None;
            p.queued = None;
            p.ghost = None;
            p.invalidated = true;
        }
    }
}

// Event-only drag invalidation, not the pane-scaled compose topology path.
fn pane_drag_surface_signature(surface: &PaneSurfaceFrame) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    pane_surface_topology_signature(surface).hash(&mut hash);
    surface.frame.width.hash(&mut hash);
    surface.frame.height.hash(&mut hash);
    for pane in &surface.panes {
        pane.pane_id.hash(&mut hash);
        (pane.rect.x, pane.rect.y, pane.rect.width, pane.rect.height).hash(&mut hash);
    }
    hash.finish()
}
