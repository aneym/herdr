use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use super::{
    text::truncate_end,
    widgets::{centered_popup_rect, render_panel_shell},
};
use crate::api::schema::AgentUsageInfo;
use crate::app::AppState;

const POPUP_WIDTH: u16 = 88;
const MAX_VISIBLE_ROWS: u16 = 16;

pub(super) fn render_usage_overlay(app: &AppState, frame: &mut Frame) {
    let visible_rows = (app.usage.rows.len() as u16).clamp(1, MAX_VISIBLE_ROWS);
    let popup_height = visible_rows.saturating_add(4);
    let Some(popup) = centered_popup_rect(frame.area(), POPUP_WIDTH, popup_height) else {
        return;
    };
    let Some(inner) = render_panel_shell(frame, popup, app.palette.accent, app.palette.panel_bg)
    else {
        return;
    };

    let p = &app.palette;
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                " agent usage ",
                Style::default().fg(p.text).add_modifier(Modifier::BOLD),
            ),
            Span::styled("live · esc close", Style::default().fg(p.overlay0)),
        ])),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );

    let header = Rect::new(inner.x, inner.y.saturating_add(1), inner.width, 1);
    frame.render_widget(
        Paragraph::new(format_header(header.width))
            .style(Style::default().fg(p.overlay0).add_modifier(Modifier::BOLD)),
        header,
    );

    let body_y = inner.y.saturating_add(2);
    if let Some(error) = app.usage.error.as_deref() {
        frame.render_widget(
            Paragraph::new(truncate_end(error, inner.width as usize))
                .style(Style::default().fg(p.red)),
            Rect::new(inner.x, body_y, inner.width, 1),
        );
        return;
    }
    if app.usage.rows.is_empty() {
        frame.render_widget(
            Paragraph::new(" no panes")
                .style(Style::default().fg(p.overlay0).add_modifier(Modifier::DIM)),
            Rect::new(inner.x, body_y, inner.width, 1),
        );
        return;
    }

    for (index, usage) in app
        .usage
        .rows
        .iter()
        .take(MAX_VISIBLE_ROWS as usize)
        .enumerate()
    {
        let area = Rect::new(inner.x, body_y + index as u16, inner.width, 1);
        frame.render_widget(
            Paragraph::new(format_row(app, usage, area.width)).style(Style::default().fg(p.text)),
            area,
        );
    }
}

fn format_header(width: u16) -> String {
    truncate_end(
        "   CPU       MEM  PROC  AGENT        THREAD                    SPACE›TAB",
        width as usize,
    )
}

fn format_row(app: &AppState, usage: &AgentUsageInfo, width: u16) -> String {
    let agent = usage.agent.as_deref().unwrap_or("shell");
    let title = usage.title.as_deref().unwrap_or("untitled");
    let location = usage_location(app, usage);
    let line = format!(
        " {:>5.1}% {:>9}  {:>4}  {:<12} {:<25} {}",
        usage.cpu_percent,
        format_memory(usage.mem_bytes),
        usage.process_count,
        truncate_end(agent, 12),
        truncate_end(title, 25),
        location,
    );
    truncate_end(&line, width as usize)
}

fn usage_location(app: &AppState, usage: &AgentUsageInfo) -> String {
    let workspace = app
        .workspaces
        .iter()
        .find(|workspace| workspace.id == usage.workspace_id)
        .map(|workspace| workspace.display_name_from_terminals(&app.terminals))
        .unwrap_or_else(|| usage.workspace_id.clone());
    let tab = usage
        .tab_id
        .rsplit_once(':')
        .map(|(_, number)| number)
        .unwrap_or(usage.tab_id.as_str());
    format!("{workspace}›{tab}")
}

fn format_memory(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;
    let bytes = bytes as f64;
    if bytes >= GIB {
        format!("{:.1} GiB", bytes / GIB)
    } else if bytes >= MIB {
        format!("{:.1} MiB", bytes / MIB)
    } else if bytes >= KIB {
        format!("{:.1} KiB", bytes / KIB)
    } else {
        format!("{} B", bytes as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn formats_memory_at_binary_unit_boundaries() {
        assert_eq!(format_memory(512), "512 B");
        assert_eq!(format_memory(1536), "1.5 KiB");
        assert_eq!(format_memory(2 * 1024 * 1024), "2.0 MiB");
        assert_eq!(format_memory(3 * 1024 * 1024 * 1024), "3.0 GiB");
    }

    #[test]
    fn row_contains_required_usage_fields() {
        let app = AppState::test_new();
        let usage = AgentUsageInfo {
            pane_id: "pane:1".into(),
            workspace_id: "workspace:missing".into(),
            tab_id: "tab:7".into(),
            agent: Some("claude".into()),
            title: Some("thread title".into()),
            cpu_percent: 12.5,
            mem_bytes: 4 * 1024 * 1024,
            process_count: 3,
        };

        let row = format_row(&app, &usage, 120);
        assert!(row.contains("12.5%"));
        assert!(row.contains("4.0 MiB"));
        assert!(row.contains("claude"));
        assert!(row.contains("thread title"));
        assert!(row.contains("workspace:missing›7"));
    }

    #[test]
    fn overlay_renders_required_fields() {
        let mut app = AppState::test_new();
        app.usage.rows = vec![AgentUsageInfo {
            pane_id: "pane:1".into(),
            workspace_id: "workspace:missing".into(),
            tab_id: "tab:7".into(),
            agent: Some("claude".into()),
            title: Some("thread title".into()),
            cpu_percent: 12.5,
            mem_bytes: 4 * 1024 * 1024,
            process_count: 3,
        }];
        let mut terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();
        terminal
            .draw(|frame| render_usage_overlay(&app, frame))
            .unwrap();

        let buffer = terminal.backend().buffer();
        let text = (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("agent usage"));
        assert!(text.contains("12.5%"));
        assert!(text.contains("4.0 MiB"));
        assert!(text.contains("claude"));
        assert!(text.contains("thread title"));
        assert!(text.contains("workspace:missing›7"));
    }
}
