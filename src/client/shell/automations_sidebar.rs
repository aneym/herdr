use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
};

use super::render::{display_width, put_text};
use super::tree::AgentPanelListEntry;
use super::{ClientShellConfig, ShellHitMap};
use crate::api::schema::AutomationSummary;

pub(super) fn entries(automations: &[AutomationSummary]) -> Vec<AgentPanelListEntry> {
    if automations.is_empty() {
        return Vec::new();
    }
    std::iter::once(AgentPanelListEntry::ServerAutomationsHeader)
        .chain(
            automations
                .iter()
                .map(|item| AgentPanelListEntry::ServerAutomation(Box::new(item.clone()))),
        )
        .collect()
}

pub(super) fn line_count(item: &AutomationSummary) -> usize {
    if result(item).is_some() {
        2
    } else {
        1
    }
}

fn result(item: &AutomationSummary) -> Option<&str> {
    item.result
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
}

pub(super) fn render_header(buffer: &mut Buffer, rect: Rect, config: &ClientShellConfig) {
    put_text(
        buffer,
        rect.x,
        rect.y,
        rect.width,
        " automations",
        Style::default()
            .fg(config.palette.overlay0)
            .add_modifier(Modifier::BOLD),
    );
}

pub(super) fn render_row(
    buffer: &mut Buffer,
    rect: Rect,
    item: &AutomationSummary,
    config: &ClientShellConfig,
    hits: &mut ShellHitMap,
) {
    let palette = &config.palette;
    let (icon, color) = match item.status.as_deref() {
        Some("ok" | "acted") => ("■", palette.green),
        Some("error") => ("■", palette.red),
        _ => ("●", palette.overlay0),
    };
    let icon_x = rect.x.saturating_add(1);
    put_text(
        buffer,
        icon_x,
        rect.y,
        1.min(rect.right().saturating_sub(icon_x)),
        icon,
        Style::default().fg(color),
    );
    let name_x = icon_x.saturating_add(2);
    let right = rect.right().saturating_sub(1);
    let trailing = trailing(item);
    let budget = right.saturating_sub(name_x);
    let trailing_width = display_width(&trailing).min(budget.saturating_sub(1) / 2);
    let trailing_x = right.saturating_sub(trailing_width);
    put_text(
        buffer,
        trailing_x,
        rect.y,
        trailing_width,
        &crate::ui::truncate_end(&trailing, trailing_width as usize),
        Style::default().fg(palette.overlay0),
    );
    let name_width = trailing_x
        .saturating_sub(name_x)
        .saturating_sub(u16::from(trailing_width > 0));
    put_text(
        buffer,
        name_x,
        rect.y,
        name_width,
        &crate::ui::truncate_end(&item.name, name_width as usize),
        Style::default().fg(palette.text),
    );
    if let Some(text) = result(item).filter(|_| rect.height >= 2) {
        let detail_x = name_x.saturating_add(2);
        let width = right.saturating_sub(detail_x);
        put_text(
            buffer,
            detail_x,
            rect.y + 1,
            width,
            &crate::ui::truncate_end(text, width as usize),
            Style::default()
                .fg(palette.overlay0)
                .add_modifier(Modifier::DIM),
        );
    }
    if let Some(url) = &item.report_url {
        hits.automation_urls.push((rect, url.clone()));
    }
}

fn short_cadence(cadence: &str) -> String {
    let words = cadence.split_whitespace().collect::<Vec<_>>();
    match words.as_slice() {
        ["every", n, unit] if n.parse::<u32>().is_ok() => {
            let unit = unit.to_ascii_lowercase();
            let short = if unit.starts_with("min") {
                "m"
            } else if unit.starts_with('h') {
                "h"
            } else if unit.starts_with("day") {
                "d"
            } else {
                return cadence.to_owned();
            };
            format!("{n}{short}")
        }
        _ => cadence.to_owned(),
    }
}

fn trailing(item: &AutomationSummary) -> String {
    [
        item.cadence.as_deref().map(short_cadence),
        item.next_run
            .as_deref()
            .and_then(clock)
            .map(|at| format!("next {at}")),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" · ")
}

fn clock(iso: &str) -> Option<String> {
    let iso = iso.strip_suffix('Z')?;
    let (date, time_part) = iso.split_once('T')?;
    let mut date = date.split('-').map(str::parse::<i32>);
    let (year, month, day) = (date.next()?.ok()?, date.next()?.ok()?, date.next()?.ok()?);
    let mut time_part = time_part
        .split(':')
        .map(|part| part.split('.').next().unwrap_or("").parse::<u8>());
    let (hour, minute) = (time_part.next()?.ok()?, time_part.next()?.ok()?);
    let second = time_part.next().and_then(Result::ok).unwrap_or(0);
    let date = time::Date::from_calendar_date(
        year,
        time::Month::try_from(u8::try_from(month).ok()?).ok()?,
        u8::try_from(day).ok()?,
    )
    .ok()?;
    let moment =
        time::PrimitiveDateTime::new(date, time::Time::from_hms(hour, minute, second).ok()?)
            .assume_utc();
    local_hour_minute(moment.unix_timestamp())
}

#[cfg(unix)]
fn local_hour_minute(timestamp: i64) -> Option<String> {
    let timestamp = libc::time_t::try_from(timestamp).ok()?;
    let mut local: libc::tm = unsafe { std::mem::zeroed() };
    if unsafe { libc::localtime_r(&timestamp, &mut local) }.is_null() {
        return None;
    }
    Some(format!("{:02}:{:02}", local.tm_hour, local.tm_min))
}

#[cfg(not(unix))]
fn local_hour_minute(timestamp: i64) -> Option<String> {
    let moment = time::OffsetDateTime::from_unix_timestamp(timestamp).ok()?;
    Some(format!("{:02}:{:02}", moment.hour(), moment.minute()))
}
