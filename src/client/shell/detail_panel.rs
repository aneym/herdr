use super::*;
use crate::factory_overlay::{self, Panel, PanelRow, RowStyle};
use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::widgets::{Block, Borders, Widget};

struct DetailLine<'a> {
    text: &'a str,
    right: Option<&'a str>,
    style: Style,
    indent: u8,
    target: Option<&'a str>,
    index: Option<usize>,
}

fn panel_rows<'a>(
    panel: &'a Panel,
    state: &DetailPanelState,
    palette: &Palette,
) -> Vec<DetailLine<'a>> {
    let mut lines = Vec::new();
    let mut index = 0;
    for section in &panel.sections {
        if !section.title.is_empty() || section.right.is_some() {
            lines.push(DetailLine {
                text: "",
                right: None,
                style: Style::default(),
                indent: 0,
                target: None,
                index: None,
            });
            lines.push(DetailLine {
                text: &section.title,
                right: section.right.as_deref(),
                style: Style::default()
                    .fg(palette.subtext0)
                    .add_modifier(Modifier::BOLD),
                indent: 0,
                target: None,
                index: None,
            });
        }
        for row in &section.rows {
            if state.exceptions_only && !matches!(row.style, RowStyle::Act | RowStyle::Warn) {
                continue;
            }
            lines.push(detail_line(row, palette, &mut index));
        }
    }
    lines
}

fn detail_line<'a>(row: &'a PanelRow, palette: &Palette, index: &mut usize) -> DetailLine<'a> {
    let selected = row.target.as_ref().map(|_| {
        let current = *index;
        *index += 1;
        current
    });
    let color = match row.style {
        RowStyle::Normal => palette.text,
        RowStyle::Dim => palette.overlay0,
        RowStyle::Accent => palette.accent,
        RowStyle::Ok => palette.green,
        RowStyle::Warn => palette.peach,
        RowStyle::Act => palette.red,
    };
    DetailLine {
        text: &row.text,
        right: row.right.as_deref(),
        style: Style::default().fg(color),
        indent: row.indent,
        target: row.target.as_deref(),
        index: selected,
    }
}

fn panel_for(
    overlay: &factory_overlay::FactoryOverlay,
    key: &str,
    snapshot: &ClientShellSnapshot,
) -> Panel {
    overlay.panel(key).cloned().unwrap_or_else(|| {
        let label = key
            .strip_prefix("tab:")
            .and_then(|id| snapshot.tabs.iter().find(|tab| tab.tab_id == id))
            .map(|tab| tab.label.as_str())
            .unwrap_or("Factory overview");
        Panel {
            title: label.to_owned(),
            sections: vec![factory_overlay::PanelSection {
                rows: vec![PanelRow {
                    text: "no details yet".into(),
                    style: RowStyle::Dim,
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        }
    })
}

fn target_count(panel: &Panel, state: &DetailPanelState) -> usize {
    let rows = panel
        .sections
        .iter()
        .flat_map(|section| &section.rows)
        .filter(|row| !state.exceptions_only || matches!(row.style, RowStyle::Warn | RowStyle::Act))
        .filter(|row| row.target.is_some())
        .count();
    rows + panel
        .actions
        .iter()
        .filter(|row| row.target.is_some())
        .count()
}

fn target_at<'a>(panel: &'a Panel, state: &DetailPanelState, selected: usize) -> Option<&'a str> {
    panel
        .sections
        .iter()
        .flat_map(|section| &section.rows)
        .filter(|row| !state.exceptions_only || matches!(row.style, RowStyle::Warn | RowStyle::Act))
        .chain(panel.actions.iter())
        .filter_map(|row| row.target.as_deref())
        .nth(selected)
}

fn print_line(
    buffer: &mut Buffer,
    area: Rect,
    y: u16,
    line: &DetailLine<'_>,
    selected: bool,
    palette: &Palette,
) {
    if area.width == 0 || y >= area.bottom() {
        return;
    }
    let base = if selected {
        line.style.bg(palette.active_row_bg)
    } else {
        line.style.bg(palette.panel_bg)
    };
    buffer.set_style(Rect::new(area.x, y, area.width, 1), base);
    let indent = u16::from(line.indent).saturating_mul(2).min(area.width);
    let x = area.x + indent;
    let width = area.width.saturating_sub(indent);
    let right = line.right.unwrap_or("");
    let right_width = render::display_width(right).min(width.saturating_sub(1));
    let left_width = width.saturating_sub(if right_width > 0 { right_width + 1 } else { 0 });
    let text = crate::ui::truncate_end(line.text, usize::from(left_width));
    render::put_text(buffer, x, y, left_width, &text, base);
    if right_width > 0 {
        let right = crate::ui::truncate_end(right, usize::from(right_width));
        render::put_text(
            buffer,
            area.right() - right_width,
            y,
            right_width,
            &right,
            base.fg(palette.overlay0),
        );
    }
}

pub(super) fn render_panel(
    buffer: &mut Buffer,
    area: Rect,
    overlay: &factory_overlay::FactoryOverlay,
    snapshot: &ClientShellSnapshot,
    state: &mut DetailPanelState,
    palette: &Palette,
    hits: &mut ShellHitMap,
) {
    if area.is_empty() {
        return;
    }
    hits.detail_rows.clear();
    let panel = panel_for(overlay, &state.key, snapshot);
    let border = if state.focused {
        palette.accent
    } else {
        palette.surface_dim
    };
    Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border))
        .style(Style::default().bg(palette.panel_bg))
        .render(area, buffer);
    let inner = Rect::new(
        area.x + 1,
        area.y + 1,
        area.width.saturating_sub(2),
        area.height.saturating_sub(2),
    );
    if inner.is_empty() {
        return;
    }
    let title_width = inner.width.saturating_sub(4);
    let title = crate::ui::truncate_end(&panel.title, usize::from(title_width));
    render::put_text(
        buffer,
        inner.x,
        inner.y,
        title_width,
        &title,
        Style::default()
            .fg(palette.text)
            .bg(palette.panel_bg)
            .add_modifier(Modifier::BOLD),
    );
    if inner.width >= 3 {
        render::put_text(
            buffer,
            inner.right() - 3,
            inner.y,
            3,
            "esc",
            Style::default().fg(palette.overlay0).bg(palette.panel_bg),
        );
    }
    if inner.height > 1 {
        if let Some(subtitle) = &panel.subtitle {
            let subtitle = crate::ui::truncate_end(subtitle, usize::from(inner.width));
            render::put_text(
                buffer,
                inner.x,
                inner.y + 1,
                inner.width,
                &subtitle,
                Style::default().fg(palette.overlay0).bg(palette.panel_bg),
            );
        }
    }
    let mut lines = panel_rows(&panel, state, palette);
    let mut index = lines.iter().filter(|line| line.index.is_some()).count();
    let actions: Vec<_> = panel
        .actions
        .iter()
        .map(|row| {
            let mut line = detail_line(row, palette, &mut index);
            line.style = Style::default().fg(palette.accent);
            line
        })
        .collect();
    let header_rows = if panel.subtitle.is_some() { 2 } else { 1 };
    let action_count = actions
        .len()
        .min(usize::from(inner.height.saturating_sub(header_rows)));
    let action_start = actions.len().saturating_sub(action_count);
    let content_y = inner.y + inner.height.min(header_rows);
    let available = usize::from(inner.height.saturating_sub(header_rows)) - action_count;
    if let Some(selected) = state.selected.filter(|_| state.focused) {
        if let Some(position) = lines.iter().position(|line| line.index == Some(selected)) {
            if position < usize::from(state.scroll) {
                state.scroll = position as u16;
            }
            if available > 0 && position >= usize::from(state.scroll) + available {
                state.scroll = (position + 1 - available).min(usize::from(u16::MAX)) as u16;
            }
        }
    }
    let max_scroll = lines.len().saturating_sub(available);
    state.scroll = state
        .scroll
        .min(max_scroll.min(usize::from(u16::MAX)) as u16);
    for (offset, line) in lines
        .drain(usize::from(state.scroll)..)
        .take(available)
        .enumerate()
    {
        let y = content_y + offset as u16;
        print_line(
            buffer,
            inner,
            y,
            &line,
            state.focused && state.selected == line.index && line.index.is_some(),
            palette,
        );
        if let Some(target) = line.target {
            hits.detail_rows
                .push((Rect::new(inner.x, y, inner.width, 1), target.to_owned()));
        }
    }
    for (i, line) in actions.iter().skip(action_start).enumerate() {
        let y = inner.bottom() - action_count as u16 + i as u16;
        let text = format!("[ {} ↵ ]", line.text);
        let line = DetailLine {
            text: &text,
            right: None,
            style: line.style,
            indent: 0,
            target: line.target,
            index: line.index,
        };
        print_line(
            buffer,
            inner,
            y,
            &line,
            state.focused && state.selected == line.index && line.index.is_some(),
            palette,
        );
        if let Some(target) = line.target {
            hits.detail_rows
                .push((Rect::new(inner.x, y, inner.width, 1), target.to_owned()));
        }
    }
}

impl ClientShellState {
    pub(super) fn change_detail_panel(
        &mut self,
        key: String,
        focused: bool,
        outcome: &mut ClientShellInput,
    ) {
        if self.factory_overlay().is_none() {
            return;
        }
        if self
            .detail_panel
            .as_ref()
            .is_some_and(|panel| panel.key == key)
        {
            self.detail_panel = None;
        } else {
            let mut panel = DetailPanelState {
                key,
                selected: None,
                focused,
                scroll: 0,
                exceptions_only: false,
            };
            if focused {
                if let (Some(overlay), Some(snapshot)) =
                    (self.factory_overlay(), self.snapshot.as_deref())
                {
                    let doc = panel_for(overlay, &panel.key, snapshot);
                    if target_count(&doc, &panel) > 0 {
                        panel.selected = Some(0);
                    }
                }
            }
            self.detail_panel = Some(panel);
        }
        self.invalidate_pane_surface();
        outcome.resize = true;
        outcome.repaint = true;
    }

    pub(super) fn toggle_factory_overview(&mut self, outcome: &mut ClientShellInput) {
        self.change_detail_panel(
            factory_overlay::OVERVIEW_PANEL_KEY.to_owned(),
            true,
            outcome,
        );
    }

    pub(super) fn focus_detail_target(&mut self, target: &str, outcome: &mut ClientShellInput) {
        let method = if target.contains(":p") {
            crate::api::schema::Method::PaneFocus(crate::api::schema::PaneTarget {
                pane_id: target.to_owned(),
            })
        } else if target.contains(":t") {
            crate::api::schema::Method::TabFocus(crate::api::schema::TabTarget {
                tab_id: target.to_owned(),
            })
        } else {
            return;
        };
        self.push_endpoint_method(method, outcome);
        self.detail_panel = None;
        self.invalidate_pane_surface();
        outcome.resize = true;
        outcome.repaint = true;
    }

    pub(super) fn handle_detail_key(
        &mut self,
        key: &crate::input::TerminalKey,
        outcome: &mut ClientShellInput,
    ) -> bool {
        if self.detail_panel.is_none() {
            return false;
        }
        if self.factory_overlay().is_none() {
            self.detail_panel = None;
            self.invalidate_pane_surface();
            outcome.resize = true;
            outcome.repaint = true;
            return false;
        }
        if !self
            .detail_panel
            .as_ref()
            .is_some_and(|panel| panel.focused)
        {
            return false;
        }
        if matches!(
            crate::input::resolve_direct_binding(&self.config.keybinds.keybinds, key),
            Some(crate::input::KeybindMatch::Action(
                crate::input::KeybindAction::ToggleFactoryOverview
            ))
        ) {
            self.toggle_factory_overview(outcome);
            return true;
        }
        if key.code == KeyCode::Esc {
            self.detail_panel = None;
            self.invalidate_pane_surface();
            outcome.resize = true;
            outcome.repaint = true;
            return true;
        }
        let doc =
            self.factory_overlay()
                .zip(self.snapshot.as_deref())
                .map(|(overlay, snapshot)| {
                    panel_for(overlay, &self.detail_panel.as_ref().unwrap().key, snapshot)
                });
        let Some(panel) = self.detail_panel.as_mut() else {
            return true;
        };
        if key.modifiers != KeyModifiers::NONE {
            return true;
        }
        match key.code {
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Up | KeyCode::Char('k') => {
                if let Some(doc) = &doc {
                    let count = target_count(doc, panel);
                    if count > 0 {
                        let current = panel.selected.unwrap_or(0).min(count - 1);
                        panel.selected =
                            Some(if matches!(key.code, KeyCode::Down | KeyCode::Char('j')) {
                                (current + 1).min(count - 1)
                            } else {
                                current.saturating_sub(1)
                            });
                    }
                }
                outcome.repaint = true;
            }
            KeyCode::Char('e') => {
                panel.exceptions_only = !panel.exceptions_only;
                panel.scroll = 0;
                panel.selected = doc
                    .as_ref()
                    .and_then(|doc| (target_count(doc, panel) > 0).then_some(0));
                outcome.repaint = true;
            }
            KeyCode::Enter => {
                let target = doc
                    .as_ref()
                    .and_then(|doc| {
                        panel
                            .selected
                            .and_then(|index| target_at(doc, panel, index))
                    })
                    .map(str::to_owned);
                if let Some(target) = target {
                    self.focus_detail_target(&target, outcome);
                }
            }
            _ => {}
        }
        true
    }
}
