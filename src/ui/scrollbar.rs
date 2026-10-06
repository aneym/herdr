use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    Frame,
};

use crate::app::AppState;
use crate::layout::PaneInfo;

pub(crate) fn pane_scrollbar_rect(info: &PaneInfo) -> Option<Rect> {
    info.scrollbar_rect
}

pub(crate) fn release_notes_scrollbar_rect(
    body: Rect,
    metrics: crate::pane::ScrollMetrics,
) -> Option<Rect> {
    (should_show_scrollbar(metrics) && body.width > 1).then_some(Rect::new(
        body.x + body.width - 1,
        body.y,
        1,
        body.height,
    ))
}

pub(crate) fn should_show_scrollbar(metrics: crate::pane::ScrollMetrics) -> bool {
    metrics.max_offset_from_bottom > 0
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ScrollbarThumb {
    pub top: u16,
    pub len: u16,
}

pub(crate) fn scrollbar_thumb(
    metrics: crate::pane::ScrollMetrics,
    track: Rect,
) -> Option<ScrollbarThumb> {
    if metrics.max_offset_from_bottom == 0 || track.height == 0 {
        return None;
    }

    let track_height = track.height as usize;
    let total_rows = metrics.max_offset_from_bottom + metrics.viewport_rows;
    if total_rows == 0 {
        return None;
    }

    let thumb_len = ((metrics.viewport_rows * track_height) as f32 / total_rows as f32)
        .round()
        .max(1.0)
        .min(track_height as f32) as usize;
    let max_thumb_top = track_height.saturating_sub(thumb_len);
    let scrolled_from_top = metrics
        .max_offset_from_bottom
        .saturating_sub(metrics.offset_from_bottom);
    let thumb_top = if max_thumb_top == 0 || metrics.max_offset_from_bottom == 0 {
        0
    } else {
        ((scrolled_from_top * max_thumb_top) as f32 / metrics.max_offset_from_bottom as f32)
            .round()
            .clamp(0.0, max_thumb_top as f32) as usize
    };

    Some(ScrollbarThumb {
        top: track.y + thumb_top as u16,
        len: thumb_len as u16,
    })
}

pub(crate) fn scrollbar_thumb_grab_offset(
    metrics: crate::pane::ScrollMetrics,
    track: Rect,
    row: u16,
) -> Option<u16> {
    let thumb = scrollbar_thumb(metrics, track)?;
    (row >= thumb.top && row < thumb.top + thumb.len).then(|| row - thumb.top)
}

fn scrollbar_offset_from_thumb_top(
    metrics: crate::pane::ScrollMetrics,
    track: Rect,
    thumb_top: usize,
) -> usize {
    if metrics.max_offset_from_bottom == 0 {
        return 0;
    }

    let thumb_len = scrollbar_thumb(metrics, track)
        .map(|thumb| thumb.len as usize)
        .unwrap_or(1);
    let max_thumb_top = track.height as usize - thumb_len.min(track.height as usize);
    if max_thumb_top == 0 {
        return 0;
    }

    let desired_top = thumb_top.min(max_thumb_top);
    let scrolled_from_top = ((desired_top * metrics.max_offset_from_bottom) as f32
        / max_thumb_top as f32)
        .round() as usize;
    metrics
        .max_offset_from_bottom
        .saturating_sub(scrolled_from_top)
}

pub(crate) fn scrollbar_offset_from_row(
    metrics: crate::pane::ScrollMetrics,
    track: Rect,
    row: u16,
) -> usize {
    let thumb = match scrollbar_thumb(metrics, track) {
        Some(thumb) => thumb,
        None => return 0,
    };
    let clamped_row = row.clamp(track.y, track.y + track.height.saturating_sub(1));
    let row_offset = clamped_row.saturating_sub(track.y) as usize;
    let thumb_center = (thumb.len as usize) / 2;
    let desired_top = row_offset.saturating_sub(thumb_center);
    scrollbar_offset_from_thumb_top(metrics, track, desired_top)
}

pub(crate) fn scrollbar_offset_from_drag_row(
    metrics: crate::pane::ScrollMetrics,
    track: Rect,
    row: u16,
    grab_row_offset: u16,
) -> usize {
    let clamped_row = row.clamp(track.y, track.y + track.height.saturating_sub(1));
    let row_offset = clamped_row.saturating_sub(track.y) as usize;
    let desired_top = row_offset.saturating_sub(grab_row_offset as usize);
    scrollbar_offset_from_thumb_top(metrics, track, desired_top)
}

pub(crate) fn render_scrollbar_buffer(
    buffer: &mut Buffer,
    metrics: crate::pane::ScrollMetrics,
    track: Rect,
    track_color: Color,
    thumb_color: Color,
    thumb_symbol: &str,
) {
    if metrics.max_offset_from_bottom == 0 {
        return;
    }

    let Some(thumb) = scrollbar_thumb(metrics, track) else {
        return;
    };

    for y in track.y..track.y + track.height {
        let cell = &mut buffer[(track.x, y)];
        cell.set_symbol("▕");
        cell.set_style(Style::default().fg(track_color));
    }
    for y in thumb.top..thumb.top + thumb.len {
        let cell = &mut buffer[(track.x, y)];
        cell.set_symbol(thumb_symbol);
        cell.set_style(Style::default().fg(thumb_color));
    }
}

/// Overlay thumb position in half rows. `U = 2 * track.height`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct HalfRowThumb {
    pub top_u: usize,
    pub len_u: usize,
}

/// Minimum thumb length in half rows (1.5 rows).
const MIN_THUMB_UNITS: usize = 3;

fn div_round(numerator: usize, denominator: usize) -> usize {
    (numerator + denominator / 2) / denominator
}

/// Half-row thumb geometry for the overlay pane scrollbar. Integer math only.
pub(crate) fn half_row_thumb(
    metrics: crate::pane::ScrollMetrics,
    track_height: u16,
) -> Option<HalfRowThumb> {
    let max_offset = metrics.max_offset_from_bottom;
    if max_offset == 0 || track_height == 0 {
        return None;
    }
    let units = 2 * usize::from(track_height);
    let total_rows = max_offset + metrics.viewport_rows;
    let len_u = if units < MIN_THUMB_UNITS {
        units
    } else {
        div_round(metrics.viewport_rows * units, total_rows).clamp(MIN_THUMB_UNITS, units)
    };
    let scrolled_from_top = max_offset.saturating_sub(metrics.offset_from_bottom);
    let top_u = div_round(scrolled_from_top * (units - len_u), max_offset);
    Some(HalfRowThumb { top_u, len_u })
}

/// Glyph for track row `row`: `┃` when the thumb covers both halves, `╹` for
/// only the top half, `╻` for only the bottom half, `None` when uncovered.
pub(crate) fn half_row_thumb_glyph(thumb: HalfRowThumb, row: usize) -> Option<&'static str> {
    let end = thumb.top_u + thumb.len_u;
    let covers = |unit: usize| thumb.top_u <= unit && unit < end;
    match (covers(2 * row), covers(2 * row + 1)) {
        (true, true) => Some("┃"),
        (true, false) => Some("╹"),
        (false, true) => Some("╻"),
        (false, false) => None,
    }
}

/// Draw the overlay thumb in `color`. Writes only thumb cells and only their
/// symbol and foreground, so the gutter background stays transparent.
pub(crate) fn render_pane_scrollbar_buffer(
    buffer: &mut Buffer,
    metrics: crate::pane::ScrollMetrics,
    track: Rect,
    color: Color,
) {
    let Some(thumb) = half_row_thumb(metrics, track.height) else {
        return;
    };
    let first_row = thumb.top_u / 2;
    let end_row = (thumb.top_u + thumb.len_u).div_ceil(2);
    for row in first_row..end_row {
        let Some(glyph) = half_row_thumb_glyph(thumb, row) else {
            continue;
        };
        let Ok(offset) = u16::try_from(row) else {
            break;
        };
        if let Some(cell) = buffer.cell_mut((track.x, track.y.saturating_add(offset))) {
            cell.set_symbol(glyph);
            cell.set_fg(color);
        }
    }
}

pub(super) fn render_pane_scrollbar(
    app: &AppState,
    frame: &mut Frame,
    info: &PaneInfo,
    rt: &crate::terminal::TerminalRuntime,
) {
    let Some(metrics) = rt.scroll_metrics() else {
        return;
    };
    let Some(track) = pane_scrollbar_rect(info) else {
        return;
    };
    let Some(color) = app.pane_scrollbar_color(info.id, metrics) else {
        return;
    };
    render_pane_scrollbar_buffer(frame.buffer_mut(), metrics, track, color);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metrics(offset: usize, max: usize, viewport: usize) -> crate::pane::ScrollMetrics {
        crate::pane::ScrollMetrics {
            offset_from_bottom: offset,
            max_offset_from_bottom: max,
            viewport_rows: viewport,
        }
    }

    fn glyphs(metrics: crate::pane::ScrollMetrics, height: u16) -> Vec<&'static str> {
        let track = Rect::new(0, 0, 1, height);
        let mut buffer = Buffer::empty(track);
        render_pane_scrollbar_buffer(&mut buffer, metrics, track, Color::Rgb(1, 2, 3));
        buffer
            .content
            .iter()
            .map(|cell| match cell.symbol() {
                "┃" => "┃",
                "╹" => "╹",
                "╻" => "╻",
                " " => " ",
                _ => "?",
            })
            .collect()
    }

    #[test]
    fn half_row_thumb_covers_tiny_tracks_whole() {
        // U < 3: the thumb is the whole track.
        assert_eq!(
            half_row_thumb(metrics(0, 100, 1), 1),
            Some(HalfRowThumb { top_u: 0, len_u: 2 })
        );
        assert_eq!(glyphs(metrics(0, 100, 1), 1), ["┃"]);
        // U = 4 clamps to the 3-unit minimum and still moves by half rows.
        assert_eq!(
            half_row_thumb(metrics(100, 100, 2), 2),
            Some(HalfRowThumb { top_u: 0, len_u: 3 })
        );
        assert_eq!(glyphs(metrics(100, 100, 2), 2), ["┃", "╹"]);
        assert_eq!(
            half_row_thumb(metrics(0, 100, 2), 2),
            Some(HalfRowThumb { top_u: 1, len_u: 3 })
        );
        assert_eq!(glyphs(metrics(0, 100, 2), 2), ["╻", "┃"]);
        // U = 6.
        assert_eq!(glyphs(metrics(0, 100, 3), 3), [" ", "╻", "┃"]);
    }

    #[test]
    fn half_row_thumb_handles_extremes_and_rounding() {
        assert_eq!(half_row_thumb(metrics(0, 0, 10), 10), None);
        assert_eq!(half_row_thumb(metrics(0, 10, 10), 0), None);
        // max_offset 1: the thumb is half the track and sits at either end.
        assert_eq!(
            half_row_thumb(metrics(1, 1, 10), 10),
            Some(HalfRowThumb {
                top_u: 0,
                len_u: 18
            })
        );
        assert_eq!(
            half_row_thumb(metrics(0, 1, 10), 10),
            Some(HalfRowThumb {
                top_u: 2,
                len_u: 18
            })
        );
        // Top of the scrollback and the live bottom pin the thumb to the ends.
        let top = half_row_thumb(metrics(240, 240, 20), 20).expect("thumb");
        assert_eq!(top.top_u, 0);
        let bottom = half_row_thumb(metrics(0, 240, 20), 20).expect("thumb");
        assert_eq!(bottom.top_u + bottom.len_u, 40);
        // An odd top_u starts with a bottom-half cap.
        let odd = half_row_thumb(metrics(221, 240, 20), 20).expect("thumb");
        assert_eq!(odd, HalfRowThumb { top_u: 3, len_u: 3 });
        let rows = glyphs(metrics(221, 240, 20), 20);
        assert_eq!(&rows[..4], [" ", "╻", "┃", " "]);
        // An even top_u with an odd length ends with a top-half cap.
        let even = half_row_thumb(metrics(201, 240, 20), 20).expect("thumb");
        assert_eq!(even, HalfRowThumb { top_u: 6, len_u: 3 });
        assert_eq!(
            &glyphs(metrics(201, 240, 20), 20)[..5],
            [" ", " ", " ", "┃", "╹"]
        );
        // Offsets past the end clamp to the top.
        assert_eq!(
            half_row_thumb(metrics(999, 240, 20), 20).map(|t| t.top_u),
            Some(0)
        );
    }

    #[test]
    fn overlay_thumb_writes_only_thumb_cells_and_never_a_background() {
        let track = Rect::new(0, 0, 1, 20);
        let mut buffer = Buffer::empty(track);
        render_pane_scrollbar_buffer(&mut buffer, metrics(120, 240, 20), track, Color::Red);
        let mut thumb_rows = 0;
        for cell in &buffer.content {
            assert_eq!(cell.bg, Color::Reset);
            if cell.symbol() == " " {
                assert_eq!(cell.fg, Color::Reset);
            } else {
                thumb_rows += 1;
                assert_eq!(cell.fg, Color::Red);
            }
        }
        assert!(thumb_rows > 0 && thumb_rows < 20);
    }
}
