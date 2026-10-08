//! Event-owned motion; compose reads stored geometry only.
use super::*;
use ratatui::style::Color;
use std::time::{Duration, Instant};
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum PaneMotionKind {
    Zone,
    Cancel,
    Settle,
}
pub(super) struct PaneMotion {
    pub(super) from: Rect,
    pub(super) to: Rect,
    pub(super) started_at: Instant,
    pub(super) duration: Duration,
    pub(super) kind: PaneMotionKind,
    pub(super) drawn: Rect,
    pub(super) colours: [u32; 3],
    pub(super) step: usize,
}
impl PaneMotion {
    fn new(
        from: Rect,
        to: Rect,
        now: Instant,
        ms: u64,
        kind: PaneMotionKind,
        colours: [u32; 3],
    ) -> Self {
        Self {
            from,
            to,
            started_at: now,
            duration: Duration::from_millis(ms),
            kind,
            drawn: from,
            colours,
            step: 0,
        }
    }
}
fn ease(x: f32) -> f32 {
    let [x1, y1, x2, y2] = motion_tokens::EASE;
    let curve = |t: f32, a: f32, b: f32| {
        3.0 * (1.0 - t).powi(2) * t * a + 3.0 * (1.0 - t) * t * t * b + t * t * t
    };
    let (mut lo, mut hi) = (0.0, 1.0);
    for _ in 0..20 {
        let t = (lo + hi) * 0.5;
        if curve(t, x1, x2) < x {
            lo = t;
        } else {
            hi = t;
        }
    }
    curve((lo + hi) * 0.5, y1, y2)
}
fn interpolate(a: Rect, b: Rect, t: f32) -> Rect {
    let edge = |a: u16, b: u16| (f32::from(a) + (f32::from(b) - f32::from(a)) * t).round() as u16;
    let (x, y, r, d) = (
        edge(a.x, b.x),
        edge(a.y, b.y),
        edge(a.right(), b.right()),
        edge(a.bottom(), b.bottom()),
    );
    Rect::new(x, y, r.saturating_sub(x), d.saturating_sub(y))
}
fn order(a: Rect, b: Rect) -> [bool; 4] {
    [
        a.right() <= b.x,
        b.right() <= a.x,
        a.bottom() <= b.y,
        b.bottom() <= a.y,
    ]
}
impl ClientShellState {
    pub(super) fn drawn_ghost(&self) -> Option<Rect> {
        self.pane_motion
            .iter()
            .find(|m| m.kind != PaneMotionKind::Settle)
            .map(|m| m.drawn)
            .or_else(|| self.pane_drag.as_ref().and_then(|p| p.ghost))
    }
    pub(super) fn retarget_ghost(&mut self, from: Option<Rect>) {
        self.pane_motion
            .retain(|m| m.kind == PaneMotionKind::Settle);
        if self.config.reduce_motion || self.mode == ClientShellMode::Move {
            return;
        }
        if let Some(p) = self.pane_drag.as_mut() {
            if let Some(to) = p.ghost {
                let from = from.unwrap_or(p.source.rect);
                p.ghost = Some(from);
                self.pane_motion.clear();
                self.pane_motion.push(PaneMotion::new(
                    from,
                    to,
                    Instant::now(),
                    motion_tokens::ZONE_MORPH_MS,
                    PaneMotionKind::Zone,
                    [p.accent; 3],
                ));
            }
        }
    }
    pub(super) fn user_cancel_pane_drag(&mut self) -> bool {
        if self.mode == ClientShellMode::Move {
            return self.cancel_pane_drag();
        }
        let from = self.drawn_ghost();
        self.chrome_drag = None;
        self.pane_press = None;
        if !self.config.reduce_motion {
            if let (Some(from), Some(p)) = (from, self.pane_drag.as_mut()) {
                p.committed = false;
                p.row = None;
                p.in_flight = None;
                p.queued = None;
                p.ghost = Some(p.source.rect);
                self.pane_motion.clear();
                self.pane_motion.push(PaneMotion::new(
                    from,
                    p.source.rect,
                    Instant::now(),
                    motion_tokens::CANCEL_MS,
                    PaneMotionKind::Cancel,
                    [p.accent; 3],
                ));
                return true;
            }
        }
        self.cancel_pane_drag()
    }
    pub(crate) fn tick_pane_motion(&mut self, now: Instant) -> bool {
        if self.pane_motion.is_empty() {
            return false;
        }
        let mut cancel_done = false;
        let mut ghost = None;
        self.pane_motion.retain_mut(|m| {
            let elapsed = now.saturating_duration_since(m.started_at);
            if elapsed >= m.duration {
                cancel_done |= m.kind == PaneMotionKind::Cancel;
                if m.kind == PaneMotionKind::Zone {
                    ghost = Some(m.to);
                }
                return false;
            }
            let progress = elapsed.as_secs_f32() / m.duration.as_secs_f32();
            m.drawn = interpolate(m.from, m.to, ease(progress));
            if m.kind != PaneMotionKind::Settle {
                ghost = Some(m.drawn);
            }
            m.step = if self.config.reduce_motion {
                0
            } else {
                (progress * 3.0) as usize
            };
            true
        });
        if let (Some(rect), Some(p)) = (ghost, self.pane_drag.as_mut()) {
            p.ghost = Some(rect);
        }
        if cancel_done {
            self.pane_drag = None;
            self.pane_grip_hover = None;
        }
        true
    }
    pub(super) fn start_pane_settle(&mut self, new: &PaneSurfaceFrame) {
        let Some(old) = self.pane_surface.as_ref() else {
            return;
        };
        if old.frame.width != new.frame.width
            || old.frame.height != new.frame.height
            || old.panes.len() != new.panes.len()
        {
            return;
        }
        if !old
            .panes
            .iter()
            .all(|a| new.panes.iter().any(|b| b.pane_id == a.pane_id))
        {
            return;
        }
        if old.panes.iter().all(|a| {
            new.panes
                .iter()
                .any(|b| b.pane_id == a.pane_id && b.rect == a.rect)
        }) {
            return;
        }
        if matches!(self.chrome_drag, Some(ClientChromeDrag::PaneSplit { .. })) {
            return;
        }
        let committed = self.pane_drag.as_ref().filter(|p| p.committed);
        if committed.is_none()
            && old.panes.iter().all(|a| {
                old.panes.iter().all(|b| {
                    let na = new.panes.iter().find(|p| p.pane_id == a.pane_id);
                    let nb = new.panes.iter().find(|p| p.pane_id == b.pane_id);
                    match (na, nb) {
                        (Some(na), Some(nb)) => {
                            order(surface_rect(a.rect), surface_rect(b.rect))
                                == order(surface_rect(na.rect), surface_rect(nb.rect))
                        }
                        _ => false,
                    }
                })
            })
        {
            return;
        }
        let source = committed.map(|p| p.source.pane_id.as_str());
        let ghost = self.drawn_ghost();
        let accent = crate::protocol::color_to_u32(self.config.palette.accent);
        let border = crate::protocol::color_to_u32(self.config.palette.overlay0);
        let mixed = match (self.config.palette.accent, self.config.palette.overlay0) {
            (Color::Rgb(r, g, b), Color::Rgb(x, y, z)) => {
                crate::protocol::color_to_u32(Color::Rgb(
                    ((u16::from(r) + u16::from(x)) / 2) as u8,
                    ((u16::from(g) + u16::from(y)) / 2) as u8,
                    ((u16::from(b) + u16::from(z)) / 2) as u8,
                ))
            }
            _ => accent,
        };
        let offset = self
            .last_composed_size
            .map(|(c, r)| self.layout(c, r).pane_surface)
            .unwrap_or_default();
        let now = Instant::now();
        self.pane_motion.clear();
        for first in [true, false] {
            for a in &old.panes {
                if (source == Some(a.pane_id.as_str())) != first {
                    continue;
                }
                let Some(b) = new
                    .panes
                    .iter()
                    .find(|p| p.pane_id == a.pane_id && p.rect != a.rect)
                else {
                    continue;
                };
                let translate = |r| {
                    let mut r = surface_rect(r);
                    r.x += offset.x;
                    r.y += offset.y;
                    r
                };
                let to = translate(b.rect);
                let from = if first {
                    ghost.unwrap_or_else(|| translate(a.rect))
                } else {
                    translate(a.rect)
                };
                let reduced = self.config.reduce_motion;
                self.pane_motion.push(PaneMotion::new(
                    if reduced { to } else { from },
                    to,
                    now,
                    if reduced {
                        motion_tokens::REDUCED_FADE_MS
                    } else {
                        motion_tokens::SETTLE_MS
                    },
                    PaneMotionKind::Settle,
                    [accent, mixed, border],
                ));
                if self.pane_motion.len() == 4 {
                    return;
                }
            }
        }
    }
}
fn surface_rect(r: crate::protocol::SurfaceRect) -> Rect {
    Rect::new(r.x, r.y, r.width, r.height)
}
impl ClientShellState {
    pub(super) fn render_pane_settles(
        &self,
        frame: &mut FrameData,
        occlusion: &mut crate::kitty_graphics::surface::Occlusion,
    ) {
        for m in &self.pane_motion {
            if m.kind != PaneMotionKind::Settle {
                continue;
            }
            let r = m.drawn;
            if r.width < 2 || r.height < 2 {
                continue;
            }
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
                        continue;
                    };
                    if x < frame.width && y < frame.height {
                        if let Some(c) = frame
                            .cells
                            .get_mut(usize::from(y) * usize::from(frame.width) + usize::from(x))
                        {
                            c.symbol.clear();
                            c.symbol.push_str(symbol);
                            c.fg = m.colours[m.step];
                        }
                    }
                }
            }
        }
    }
}
