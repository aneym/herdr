//! The sidebar footer's host table: one aligned row per factory host.
//!
//! The overlay writer (herdr-control) sends each host as one summary string,
//! e.g. `4 running · 6.4G free · waiting on slowdown:check · 1 kept: 1 secret`.
//! Drawn as-is, that string is wider than the sidebar, so the host name and the
//! wait reason were clipped (Alex, 2026-10-05: "text not fitting"). Here the
//! summary is split into fields and laid out in fixed columns: host, running,
//! free, wait reason, then anything else. Fields are kept whole or dropped,
//! highest priority first; only the trailing extras truncate, with an ellipsis.

use crate::factory_overlay::HostRow;
use crate::ui::truncate_end;
use unicode_width::UnicodeWidthStr;

fn display_width(text: &str) -> usize {
    UnicodeWidthStr::width(text)
}

/// Columns of the host name. Fits studio, pc, ax42 and forge.
pub(super) const HOST_COLUMNS: usize = 6;
/// The narrowest truncated extra worth drawing: five columns and the ellipsis.
const MIN_EXTRA: usize = 6;

/// One drawn row: the padded host column and the text after it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct HostLine {
    pub(super) host: String,
    pub(super) value: String,
}

#[derive(Debug, Default)]
struct Fields {
    running: Option<String>,
    free: Option<String>,
    wait: Option<String>,
    extras: Vec<String>,
}

/// Lowercase short host name: the first word of the name, at most six columns.
pub(super) fn short_host(name: &str) -> String {
    let first = name
        .split(|c: char| c.is_whitespace() || c == '-' || c == '_' || c == '.')
        .find(|part| !part.is_empty())
        .unwrap_or("")
        .to_lowercase();
    truncate_end(&first, HOST_COLUMNS)
}

/// Short wait reason. Known reasons come from a fixed table so none is cut
/// mid-word; an unknown one keeps its first word, ellipsized past six columns.
pub(super) fn wait_word(reason: &str) -> String {
    let reason = reason.trim();
    let head = reason
        .split(|c: char| c == ':' || c.is_whitespace())
        .find(|part| !part.is_empty())
        .unwrap_or("")
        .to_lowercase();
    let known = match head.as_str() {
        "memory" | "mem" => Some("mem"),
        "slowdown" => Some("slow"),
        "no_box" | "nobox" => Some("nobox"),
        "lock" => Some("lock"),
        "cpu" => Some("cpu"),
        "disk" => Some("disk"),
        "stale" => Some("stale"),
        "owner" | "owner_reserve" => Some("owner"),
        "signals" | "signals-unreadable" => Some("sig"),
        "pressure" => Some("press"),
        "provider" => Some("prov"),
        "hold" => Some("hold"),
        _ => None,
    };
    known.map_or_else(|| truncate_end(&head, HOST_COLUMNS), str::to_owned)
}

fn fields(summary: &str) -> Fields {
    let mut out = Fields::default();
    for part in summary.split(" · ").map(str::trim).filter(|part| !part.is_empty()) {
        if out.running.is_none() {
            if let Some(count) = part.strip_suffix(" running").filter(|n| n.chars().all(|c| c.is_ascii_digit()) && !n.is_empty()) {
                out.running = Some(count.to_owned());
                continue;
            }
        }
        if out.free.is_none() {
            if let Some(amount) = part.strip_suffix(" free").filter(|amount| !amount.contains(' ')) {
                out.free = Some(amount.to_owned());
                continue;
            }
        }
        if out.wait.is_none() {
            if let Some(reason) = part.strip_prefix("waiting on ") {
                out.wait = Some(wait_word(reason));
                continue;
            }
        }
        // The same trims the one-line footer applies: "load 150/16" and "3 live" read as "150/16" and "3".
        let part = part.strip_prefix("load ").unwrap_or(part).replace(" live", "");
        out.extras.push(part);
    }
    out
}

/// Lay out every host as one row of `width` columns (the sidebar's full width,
/// including the one-column left margin). Columns align across rows.
pub(super) fn host_lines(rows: &[HostRow], width: u16) -> Vec<HostLine> {
    let parsed: Vec<Fields> = rows.iter().map(|row| fields(row.summary.as_deref().unwrap_or(""))).collect();
    let column = |get: fn(&Fields) -> Option<&String>| {
        parsed.iter().filter_map(|f| get(f).map(|v| display_width(v))).max().unwrap_or(0)
    };
    let count_w = column(|f| f.running.as_ref());
    let free_w = column(|f| f.free.as_ref());
    // Room after the left margin, the host column and its space.
    let room = usize::from(width).saturating_sub(1 + HOST_COLUMNS + 1);
    rows.iter().zip(&parsed).map(|(row, fields)| {
        let mut value = String::new();
        let mut used = 0usize;
        let push = |value: &mut String, used: &mut usize, cell: String, cell_w: usize, gap: bool| -> bool {
            let need = cell_w + usize::from(gap && *used > 0);
            if *used + need > room {
                return false;
            }
            if gap && *used > 0 {
                value.push(' ');
            }
            value.push_str(&cell);
            *used += need;
            true
        };
        // Each fixed column is padded so the next one starts at the same place
        // on every row; a row without the field leaves the column blank.
        let mut fits = true;
        if count_w > 0 {
            let cell = fields.running.as_ref().map_or_else(
                || " ".repeat(count_w + " running".len()),
                |n| format!("{n:>count_w$} running"));
            fits = push(&mut value, &mut used, cell, count_w + " running".len(), true);
        }
        if fits && free_w > 0 {
            let cell = fields.free.as_ref().map_or_else(
                || " ".repeat(free_w + " free".len()),
                |amount| format!("{amount:>free_w$} free"));
            fits = push(&mut value, &mut used, cell, free_w + " free".len(), true);
        }
        if fits {
            if let Some(wait) = fields.wait.as_deref() {
                // Kept whole or dropped, never clipped.
                let cell = format!("wait {wait}");
                let cell_w = display_width(&cell);
                fits = push(&mut value, &mut used, cell, cell_w, true);
            }
        }
        if fits && !fields.extras.is_empty() {
            let extra = fields.extras.join(" ");
            let gap = usize::from(used > 0);
            let left = room.saturating_sub(used + gap);
            if left >= MIN_EXTRA.min(display_width(&extra)) && left > 0 {
                let cell = shorten_extra(&extra, left);
                let cell_w = display_width(&cell);
                push(&mut value, &mut used, cell, cell_w, true);
            }
        }
        HostLine {
            host: format!("{:<HOST_COLUMNS$}", short_host(&row.name)),
            value: value.trim_end().to_owned(),
        }
    }).collect()
}

/// `truncate_end`, minus any space or separator left hanging before the ellipsis.
fn shorten_extra(text: &str, width: usize) -> String {
    let cut = truncate_end(text, width);
    match cut.strip_suffix('…') {
        Some(head) => format!("{}…", head.trim_end_matches([' ', ':', ',', '·'])),
        None => cut,
    }
}

#[cfg(test)]
mod tests {
    //! Edge cases of the pure layout; the 40/44/52 render check with Alex's real
    //! summaries is `factory_host_footer_rows_fit_and_align_at_40_44_52`.
    use super::*;

    fn host(name: &str, summary: &str) -> HostRow {
        HostRow { name: name.into(), summary: Some(summary.into()), ..HostRow::default() }
    }

    /// The four hosts as Alex saw them on 2026-10-05 ~20:50 ET.
    fn alex_hosts() -> Vec<HostRow> {
        vec![
            host("Studio", "2 running · 2G free · waiting on memory · 1 kept: 1 secret"),
            host("PC", "3 running · 3.6G free"),
            host("ax42", "2 running · 12G free · waiting on slowdown:check"),
            host("forge", "1 running · 9.2G free · waiting on memory"),
        ]
    }

    fn render(rows: &[HostRow], width: u16) -> Vec<String> {
        host_lines(rows, width).into_iter()
            .map(|line| format!(" {} {}", line.host, line.value).trim_end().to_owned())
            .collect()
    }

    #[test]
    fn narrow_rows_drop_whole_fields_from_the_lowest_priority() {
        let lines = render(&alex_hosts(), 28);
        assert_eq!(lines[2], " ax42   2 running  12G free");
        let lines = render(&alex_hosts(), 18);
        assert_eq!(lines[0], " studio 2 running");
        let lines = render(&alex_hosts(), 12);
        assert_eq!(lines[0], " studio");
    }

    #[test]
    fn wait_reasons_come_from_the_table_and_unknown_ones_ellipsize() {
        for (reason, word) in [("memory", "mem"), ("slowdown:check", "slow"), ("slowdown", "slow"),
            ("no_box", "nobox"), ("lock", "lock"), ("CPU", "cpu"), ("stale stall", "stale"),
            ("owner reserve", "owner"), ("anthropic", "anthr…")] {
            assert_eq!(wait_word(reason), word, "{reason}");
        }
        assert_eq!(short_host("forge-lanes"), "forge");
        assert_eq!(short_host("Studio workstation"), "studio");
    }

    #[test]
    fn non_admit_summaries_fall_through_as_extras() {
        let rows = vec![host("forge", "down"), host("old", "3 live · drained")];
        assert_eq!(render(&rows, 40), vec![" forge  down", " old    3 drained"]);
    }
}
