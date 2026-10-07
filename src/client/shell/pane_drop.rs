use ratatui::layout::Rect;

use super::motion_tokens;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DropSide {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DropZone {
    Centre { target: usize },
    PaneEdge { target: usize, side: DropSide },
    TabEdge { side: DropSide },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct DropMetrics {
    pub tab_edge: f32,
    pub band_min: f32,
    pub band_fraction: f32,
    pub band_max_fraction: f32,
}

impl DropMetrics {
    pub(crate) const TUI: DropMetrics = DropMetrics {
        tab_edge: 1.0,
        band_min: 2.0,
        band_fraction: motion_tokens::EDGE_BAND_FRACTION,
        band_max_fraction: motion_tokens::EDGE_BAND_MAX_FRACTION,
    };
}

fn contains(rect: Rect, point: (f32, f32)) -> bool {
    point.0 >= f32::from(rect.x)
        && point.0 < f32::from(rect.right())
        && point.1 >= f32::from(rect.y)
        && point.1 < f32::from(rect.bottom())
}

fn edge_at(rect: Rect, point: (f32, f32), bands: (f32, f32)) -> Option<DropSide> {
    let candidates = [
        (DropSide::Left, point.0 - f32::from(rect.x), bands.0),
        (DropSide::Right, f32::from(rect.right()) - point.0, bands.0),
        (DropSide::Up, point.1 - f32::from(rect.y), bands.1),
        (DropSide::Down, f32::from(rect.bottom()) - point.1, bands.1),
    ];
    let mut best = None;
    let mut best_score = f32::INFINITY;
    for (side, distance, band) in candidates {
        if distance < band {
            let score = distance / band;
            if score < best_score {
                best = Some(side);
                best_score = score;
            }
        }
    }
    best
}

/// Panes are in layout order; source is absent when dragging from another tab.
/// Points are fractional cells (the TUI passes the centre of the pointer cell).
/// Lookup runs on pointer movement, O(panes), without allocation.
pub(crate) fn drop_zone_at(
    area: Rect,
    panes: &[Rect],
    source: Option<usize>,
    point: (f32, f32),
    metrics: &DropMetrics,
) -> Option<DropZone> {
    if !contains(area, point) {
        return None;
    }
    if panes.len() >= 2 {
        if let Some(side) = edge_at(area, point, (metrics.tab_edge, metrics.tab_edge)) {
            return Some(DropZone::TabEdge { side });
        }
    }

    let mut nearest = None;
    let mut best_distance = f32::INFINITY;
    for (target, &rect) in panes.iter().enumerate() {
        if rect.width == 0 || rect.height == 0 {
            continue;
        }
        if contains(rect, point) {
            nearest = Some((target, rect, point));
            break;
        }
        // Gaps use distance to the continuous rect boundary, not cell centres.
        let clamped = (
            point.0.clamp(f32::from(rect.x), f32::from(rect.right())),
            point.1.clamp(f32::from(rect.y), f32::from(rect.bottom())),
        );
        let distance = (point.0 - clamped.0).abs().max((point.1 - clamped.1).abs());
        if distance <= 1.0 && distance < best_distance {
            nearest = Some((target, rect, clamped));
            best_distance = distance;
        }
    }
    let (target, rect, point) = nearest?;
    if source == Some(target) {
        return None;
    }
    let band = |dimension: u16| {
        (f32::from(dimension) * metrics.band_fraction)
            .max(metrics.band_min)
            .min(f32::from(dimension) * metrics.band_max_fraction)
    };
    Some(
        match edge_at(rect, point, (band(rect.width), band(rect.height))) {
            Some(side) => DropZone::PaneEdge { target, side },
            None => DropZone::Centre { target },
        },
    )
}

/// Local preview until the server returns the exact post-removal placed rect.
pub(crate) fn zone_estimate_rect(area: Rect, panes: &[Rect], zone: DropZone) -> Rect {
    let (rect, side, share) = match zone {
        DropZone::Centre { target } => return panes.get(target).copied().unwrap_or_default(),
        DropZone::PaneEdge { target, side } => {
            let Some(&rect) = panes.get(target) else {
                return Rect::default();
            };
            (rect, side, 0.5)
        }
        DropZone::TabEdge { side } => (area, side, 1.0 / 3.0),
    };
    // Match layout::split_rect: first length rounds, second gets the remainder.
    let first_share = match side {
        DropSide::Left | DropSide::Up => share,
        DropSide::Right | DropSide::Down => 1.0 - share,
    };
    match side {
        DropSide::Left | DropSide::Right => {
            let first = (f32::from(rect.width) * first_share).round() as u16;
            if side == DropSide::Left {
                Rect::new(rect.x, rect.y, first, rect.height)
            } else {
                Rect::new(
                    rect.x.saturating_add(first),
                    rect.y,
                    rect.width.saturating_sub(first),
                    rect.height,
                )
            }
        }
        DropSide::Up | DropSide::Down => {
            let first = (f32::from(rect.height) * first_share).round() as u16;
            if side == DropSide::Up {
                Rect::new(rect.x, rect.y, rect.width, first)
            } else {
                Rect::new(
                    rect.x,
                    rect.y.saturating_add(first),
                    rect.width,
                    rect.height.saturating_sub(first),
                )
            }
        }
    }
}
