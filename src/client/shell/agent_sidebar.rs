use std::collections::HashMap;

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::Line,
    widgets::{Paragraph, Widget},
};

use super::*;

pub(super) struct AgentRow {
    pub(super) pane_id: String,
    pub(super) workspace_id: String,
    pub(super) tab_id: String,
    pub(super) status: crate::api::schema::AgentStatus,
    pub(super) focused: bool,
    pub(super) rows: Vec<Vec<crate::ui::ResolvedToken>>,
    /// Extra leading columns applied by the tree view, on top of the row's own
    /// one-column (first line) / three-column (continuation) indent.
    pub(super) indent: u8,
    /// Pane hosting this agent's resolved current owner.
    pub(super) owner_pane_id: Option<String>,
    /// The recorded current owner no longer resolves to a live agent.
    pub(super) orphaned: bool,
    /// Placement inside its ownership or orchestrator group.
    pub(super) group: super::tree::AgentGroupRender,
}

impl AgentRow {
    /// Drop the tokens a tree header already names, so an agent row under a
    /// space or tab header does not repeat its container's label.
    pub(super) fn strip_tokens(&mut self, drop_workspace: bool, drop_tab: bool) {
        if !drop_workspace && !drop_tab {
            return;
        }
        for row in &mut self.rows {
            row.retain(|token| match &token.kind {
                crate::ui::ResolvedTokenKind::Workspace(_) => !drop_workspace,
                crate::ui::ResolvedTokenKind::Tab(_) => !drop_tab,
                _ => true,
            });
        }
        self.rows.retain(|row| !row.is_empty());
    }
}

pub(super) fn ordered_agent_pane_ids(
    snapshot: &ClientShellSnapshot,
    sort: crate::config::AgentPanelSortConfig,
) -> Vec<String> {
    if snapshot.agent_view_label.is_some() {
        return snapshot
            .agent_order
            .iter()
            .filter(|pane_id| {
                snapshot
                    .agents
                    .iter()
                    .any(|agent| agent.pane_id == pane_id.as_str())
            })
            .cloned()
            .collect();
    }
    let mut agents = snapshot.agents.iter().collect::<Vec<_>>();
    match sort {
        crate::config::AgentPanelSortConfig::Priority => {
            agents.sort_by_key(|agent| {
                (
                    std::cmp::Reverse(status_priority(agent.agent_status)),
                    std::cmp::Reverse(agent.state_change_seq),
                )
            });
        }
        // Triage puts the most demanding agent first and, within a tier, the one
        // that has been waiting longest.
        crate::config::AgentPanelSortConfig::Triage => {
            agents.sort_by_key(|agent| {
                (
                    std::cmp::Reverse(super::tree::cycle_attention_rank(agent.agent_status)),
                    agent.state_change_seq,
                )
            });
        }
        // The tree groups this order; it does not resort it.
        crate::config::AgentPanelSortConfig::Spaces | crate::config::AgentPanelSortConfig::Tree => {
        }
    }
    agents
        .into_iter()
        .map(|agent| agent.pane_id.clone())
        .collect()
}

pub(super) fn render_agent_panel(
    buffer: &mut Buffer,
    area: Rect,
    snapshot: &ClientShellSnapshot,
    config: &ClientShellConfig,
    tree: &super::tree::ClientTreeChrome,
    agent_scroll: &mut usize,
    hits: &mut ShellHitMap,
) {
    if !render_agent_panel_header(
        buffer,
        area,
        snapshot.agent_view_label.as_deref(),
        config,
        hits,
    ) {
        return;
    }

    let rows = agent_rows(snapshot, config, None);
    let (rows, automations) = super::tree::partition_automations(snapshot, config, rows);
    let rows = super::tree::arrange_agent_hierarchy(snapshot, tree, rows);
    let mut entries =
        if super::tree::tree_view_active(config) && snapshot.agent_view_label.is_none() {
            super::tree::tree_list_entries(snapshot, tree, rows)
        } else {
            rows.into_iter()
                .map(super::tree::AgentPanelListEntry::Agent)
                .collect()
        };
    super::tree::append_automations(&mut entries, tree, automations);
    render_agent_list(
        buffer,
        area,
        &entries,
        snapshot
            .agent_view_label
            .as_ref()
            .map(|_| " no matching agents"),
        config,
        agent_scroll,
        hits,
        super::tree::AgentPanelListEntry::line_count,
        |buffer, rect, entry, hits| render_panel_list_entry(buffer, rect, entry, config, hits),
    );
}

/// Right-most two cells of a header row: the disclosure chevron.
pub(super) fn tree_header_chevron_rect(rect: Rect) -> Rect {
    let width = 2u16.min(rect.width);
    if width == 0 {
        return Rect::default();
    }
    Rect::new(rect.right().saturating_sub(width), rect.y, width, 1)
}

/// New-tab plus on a space header: the cell pair left of the chevron slot. The
/// chevron slot is reserved whether or not a chevron is drawn, so this rect
/// never moves.
pub(super) fn tree_header_plus_rect(rect: Rect) -> Rect {
    if rect.width < 4 {
        return Rect::default();
    }
    Rect::new(rect.right().saturating_sub(4), rect.y, 2, 1)
}

/// Pin toggle on a space header: the cell pair two slots left of the chevron,
/// leaving the slot between them for the new-tab plus.
pub(super) fn tree_header_pin_rect(rect: Rect) -> Rect {
    if rect.width < 6 {
        return Rect::default();
    }
    Rect::new(rect.right().saturating_sub(6), rect.y, 2, 1)
}

fn render_panel_list_entry(
    buffer: &mut Buffer,
    rect: Rect,
    entry: &super::tree::AgentPanelListEntry,
    config: &ClientShellConfig,
    hits: &mut ShellHitMap,
) {
    use super::tree::AgentPanelListEntry;
    match entry {
        AgentPanelListEntry::Agent(row) | AgentPanelListEntry::Automation(row) => {
            hits.agents.push((rect, row.pane_id.clone()));
            if let Some(key) = row.group.group_key.clone() {
                hits.agent_groups
                    .push((agent_group_chevron_rect(rect, row), key));
            }
            render_agent_row(buffer, rect, row, config);
        }
        AgentPanelListEntry::AutomationsHeader(summary) => {
            let style = Style::default()
                .fg(summary.color(&config.palette))
                .add_modifier(Modifier::BOLD);
            put_text(buffer, rect.x, rect.y, rect.width, " automations", style);
            let label = summary.label();
            let width = (display_width(&label) as u16).min(rect.width);
            put_text(
                buffer,
                rect.right().saturating_sub(width),
                rect.y,
                width,
                &label,
                style,
            );
            hits.automations_header = rect;
        }
        AgentPanelListEntry::SpaceHeader(header) => {
            render_tree_header(buffer, rect, header, true, config, hits);
        }
        AgentPanelListEntry::TabHeader(header) => {
            render_tree_header(buffer, rect, header, false, config, hits);
        }
        AgentPanelListEntry::HiddenSpacesHeader { count, collapsed } => {
            let palette = &config.palette;
            // Muted on purpose: this section names what was folded away, so it
            // must never outrank a space still meant to be seen.
            let style = Style::default()
                .fg(palette.overlay0)
                .add_modifier(Modifier::DIM);
            put_text(buffer, rect.x, rect.y, rect.width, " hidden", style);
            let trailing = format!(
                "{count} {} ",
                if *collapsed { "\u{25b8}" } else { "\u{25be}" }
            );
            let width = (display_width(&trailing) as u16).min(rect.width);
            put_text(
                buffer,
                rect.right().saturating_sub(width),
                rect.y,
                width,
                &trailing,
                style,
            );
            hits.tree_hidden_header = rect;
        }
    }
}

fn render_tree_header(
    buffer: &mut Buffer,
    rect: Rect,
    header: &super::tree::TreeHeader,
    is_space: bool,
    config: &ClientShellConfig,
    hits: &mut ShellHitMap,
) {
    let palette = &config.palette;
    // The tab is the unit that gets scanned, so it carries the strongest
    // weight. The space reads as the container above it, and agent titles below
    // stay lighter than both.
    let label_style = if is_space {
        Style::default()
            .fg(palette.overlay0)
            .add_modifier(Modifier::BOLD)
    } else if header.active {
        Style::default()
            .fg(palette.text)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(palette.subtext0)
            .add_modifier(Modifier::BOLD)
    };
    if header.active {
        buffer.set_style(rect, Style::default().bg(palette.active_row_bg));
    }
    let prefix = 1 + u16::from(header.indent);
    // Space headers reserve extra cells for the pin and new-tab plus.
    let trailing_width = if is_space { 10 } else { 6 };
    put_text(
        buffer,
        rect.x.saturating_add(prefix),
        rect.y,
        rect.width
            .saturating_sub(prefix)
            .saturating_sub(trailing_width),
        &header.label,
        label_style,
    );

    let mut trailing = Vec::<(String, Style)>::new();
    if !header.child_states.is_empty() {
        // One dot per agent this header stands in for, so a collapsed tab still
        // reports every agent's state rather than a count. Past the cap the dots
        // would crowd the label, so the rest collapse into a trailing count.
        const MAX_DOTS: usize = 6;
        let shown = header.child_states.len().min(MAX_DOTS);
        for status in header.child_states.iter().take(shown) {
            trailing.push((
                resolved_status_icon(*status, config).to_owned(),
                Style::default().fg(status_color(*status, palette)),
            ));
        }
        if header.child_states.len() > shown {
            trailing.push((
                format!("+{}", header.child_states.len() - shown),
                Style::default().fg(palette.overlay0),
            ));
        }
        trailing.push((" ".to_owned(), Style::default()));
    }
    let chevron = |collapsed: bool| {
        (
            if collapsed { "\u{25b8} " } else { "\u{25be} " }.to_owned(),
            Style::default().fg(palette.overlay0),
        )
    };
    if is_space {
        // Pin toggle, one cell pair left of the plus, so the trailing strip
        // reads [pin][+][chevron]. A pinned space keeps its header row even when
        // no agents remain beneath it.
        trailing.push((
            "\u{26b2} ".to_owned(),
            if header.pinned {
                Style::default().fg(palette.accent)
            } else {
                Style::default().fg(palette.overlay0)
            },
        ));
        // New-tab plus, one cell pair left of the chevron slot so its hit region
        // stays fixed whether or not this header draws a chevron.
        trailing.push(("+ ".to_owned(), Style::default().fg(palette.overlay0)));
        trailing.push(if header.collapsible {
            chevron(header.collapsed)
        } else {
            ("  ".to_owned(), Style::default())
        });
    } else if header.collapsible {
        trailing.push(chevron(header.collapsed));
    } else if !trailing.is_empty() {
        trailing.push((" ".to_owned(), Style::default()));
    }
    let total = trailing
        .iter()
        .map(|(text, _)| display_width(text))
        .sum::<usize>()
        .min(rect.width as usize) as u16;
    let mut x = rect.right().saturating_sub(total);
    for (text, style) in &trailing {
        let width = (display_width(text) as u16).min(rect.right().saturating_sub(x));
        put_text(buffer, x, rect.y, width, text, *style);
        x = x.saturating_add(width);
    }

    hits.tree_headers.push(TreeHeaderHit {
        rect,
        chevron: if header.collapsible {
            tree_header_chevron_rect(rect)
        } else {
            Rect::default()
        },
        plus: if is_space {
            tree_header_plus_rect(rect)
        } else {
            Rect::default()
        },
        pin: if is_space {
            tree_header_pin_rect(rect)
        } else {
            Rect::default()
        },
        workspace_id: header.workspace_id.clone(),
        tab_id: header.tab_id.clone(),
        key: header.key.clone(),
        pinned: header.pinned,
    });
}

pub(super) fn render_agent_panel_header(
    buffer: &mut Buffer,
    area: Rect,
    agent_view_label: Option<&str>,
    config: &ClientShellConfig,
    hits: &mut ShellHitMap,
) -> bool {
    if area.height == 0 {
        return false;
    }
    put_text(
        buffer,
        area.x,
        area.y,
        area.width,
        &"─".repeat(area.width as usize),
        Style::default().fg(config.palette.surface_dim),
    );
    if area.height < 2 {
        return false;
    }
    put_text(
        buffer,
        area.x,
        area.y + 1,
        area.width,
        " agents",
        Style::default()
            .fg(config.palette.overlay0)
            .add_modifier(Modifier::BOLD),
    );
    let sort_label = agent_view_label.unwrap_or(match config.agent_panel_sort {
        crate::config::AgentPanelSortConfig::Spaces => "grouped",
        crate::config::AgentPanelSortConfig::Priority => "priority",
        crate::config::AgentPanelSortConfig::Triage => "triage",
        crate::config::AgentPanelSortConfig::Tree => "tree",
    });
    let sort_width = display_width(sort_label).min(area.width as usize) as u16;
    let sort_rect = Rect::new(
        area.right().saturating_sub(sort_width),
        area.y + 1,
        sort_width,
        1,
    );
    let usage_width = 6u16.min(sort_rect.x.saturating_sub(area.x));
    let usage_rect = Rect::new(
        sort_rect.x.saturating_sub(usage_width),
        area.y + 1,
        usage_width,
        1,
    );
    hits.agent_usage = if config.mouse_capture {
        usage_rect
    } else {
        Rect::default()
    };
    put_text(
        buffer,
        usage_rect.x,
        usage_rect.y,
        usage_rect.width,
        " usage",
        Style::default()
            .fg(config.palette.accent)
            .add_modifier(Modifier::BOLD),
    );
    hits.agent_sort_toggle = if config.mouse_capture && agent_view_label.is_none() {
        sort_rect
    } else {
        Rect::default()
    };
    put_text(
        buffer,
        sort_rect.x,
        sort_rect.y,
        sort_rect.width,
        sort_label,
        Style::default()
            .fg(if agent_view_label.is_some() {
                config.palette.accent
            } else {
                config.palette.overlay0
            })
            .add_modifier(Modifier::BOLD),
    );
    true
}

pub(super) fn render_agent_list<T>(
    buffer: &mut Buffer,
    area: Rect,
    rows: &[T],
    empty_message: Option<&str>,
    config: &ClientShellConfig,
    agent_scroll: &mut usize,
    hits: &mut ShellHitMap,
    row_lines: impl Fn(&T) -> usize,
    mut render_row: impl FnMut(&mut Buffer, Rect, &T, &mut ShellHitMap),
) {
    let body = Rect::new(
        area.x,
        area.y.saturating_add(3),
        area.width,
        area.height.saturating_sub(3),
    );
    hits.agent_body = body;
    if body.is_empty() || rows.is_empty() {
        *agent_scroll = 0;
        if let Some(message) = empty_message.filter(|_| !body.is_empty()) {
            put_text(
                buffer,
                body.x,
                body.y,
                body.width,
                message,
                Style::default()
                    .fg(config.palette.overlay0)
                    .add_modifier(Modifier::DIM),
            );
        }
        return;
    }

    let row_heights = rows
        .iter()
        .map(|row| row_lines(row).max(1).min(u16::MAX as usize) as u16)
        .collect::<Vec<_>>();
    let gaps = rows
        .iter()
        .enumerate()
        .map(|(index, _)| {
            if index + 1 < rows.len() {
                config.agents.row_gap
            } else {
                0
            }
        })
        .collect::<Vec<_>>();
    let metrics =
        super::scroll::list_scroll_metrics(&row_heights, &gaps, body.height, *agent_scroll);
    hits.agent_max_scroll = metrics.max_offset_from_bottom;
    hits.agent_scroll_metrics = Some(metrics);
    *agent_scroll = metrics
        .max_offset_from_bottom
        .saturating_sub(metrics.offset_from_bottom);
    let show_scrollbar = metrics.max_offset_from_bottom > 0 && body.width > 1;
    let content_width = body.width.saturating_sub(u16::from(show_scrollbar));
    let mut y = body.y;
    for (index, row) in rows.iter().enumerate().skip(*agent_scroll) {
        let height = row_heights[index].min(body.height);
        if y.saturating_add(height) > body.bottom() {
            break;
        }
        let rect = Rect::new(body.x, y, content_width, height);
        render_row(buffer, rect, row, hits);
        y = y
            .saturating_add(height)
            .saturating_add(if index + 1 < rows.len() {
                config.agents.row_gap
            } else {
                0
            });
    }

    if show_scrollbar {
        let track = Rect::new(body.right().saturating_sub(1), body.y, 1, body.height);
        hits.agent_scrollbar = track;
        super::scroll::render_list_scrollbar(buffer, track, metrics, &config.palette);
    }
}

pub(super) fn agent_rows(
    snapshot: &ClientShellSnapshot,
    config: &ClientShellConfig,
    machine: Option<&str>,
) -> Vec<AgentRow> {
    ordered_agent_pane_ids(snapshot, config.agent_panel_sort)
        .into_iter()
        .filter_map(|pane_id| agent_row(snapshot, &pane_id, config, machine))
        .collect()
}

pub(super) fn agent_row(
    snapshot: &ClientShellSnapshot,
    pane_id: &str,
    config: &ClientShellConfig,
    machine: Option<&str>,
) -> Option<AgentRow> {
    let agent = snapshot
        .agents
        .iter()
        .find(|agent| agent.pane_id == pane_id)?;
    let workspace = snapshot
        .workspaces
        .iter()
        .find(|workspace| workspace.workspace_id == agent.workspace_id)?;
    let tab = snapshot.tabs.iter().find(|tab| tab.tab_id == agent.tab_id);
    let pane = snapshot
        .panes
        .iter()
        .find(|pane| pane.pane_id == agent.pane_id);
    let tab_count = snapshot
        .tabs
        .iter()
        .filter(|candidate| candidate.workspace_id == agent.workspace_id)
        .count();
    let tab_label = tab
        .filter(|tab| tab_count > 1 || tab.custom_label)
        .map(|tab| tab.label.as_str());
    let agent_label = agent
        .display_agent
        .as_deref()
        .or(agent.name.as_deref())
        .or(agent.agent.as_deref())
        .or(agent.title.as_deref());
    let labels = agent
        .state_labels
        .iter()
        .cloned()
        .collect::<HashMap<_, _>>();
    let tokens = agent.tokens.iter().cloned().collect::<HashMap<_, _>>();
    let state_text = labels
        .get(status_text(agent.agent_status))
        .map(String::as_str)
        .unwrap_or_else(|| sidebar_status_text(agent.agent_status));
    let canonical_agent = agent
        .agent
        .as_deref()
        .and_then(crate::detect::parse_agent_label);
    let rows = crate::ui::sidebar_agent_rows(
        &config.agents,
        crate::ui::AgentTokenContext {
            machine,
            workspace: &workspace.label,
            tab: tab_label,
            pane: agent
                .title
                .as_deref()
                .or_else(|| pane.and_then(|pane| pane.label.as_deref())),
            agent_label,
            terminal_title: agent.terminal_title.as_deref(),
            terminal_title_stripped: agent.terminal_title_stripped.as_deref(),
            canonical_agent,
            tokens: &tokens,
        },
        state_text,
    );
    Some(AgentRow {
        pane_id: agent.pane_id.clone(),
        workspace_id: agent.workspace_id.clone(),
        tab_id: agent.tab_id.clone(),
        status: agent.agent_status,
        focused: agent.focused,
        rows,
        indent: 0,
        owner_pane_id: agent.owner_pane_id.clone(),
        orphaned: agent.orphaned,
        group: super::tree::AgentGroupRender::default(),
    })
}

/// Right-aligned chevron (and collapsed-group summary) hit region on a group
/// owner's first row.
pub(super) fn agent_group_chevron_rect(rect: Rect, row: &AgentRow) -> Rect {
    let width = (agent_group_trailing_width(row) as u16).min(rect.width);
    if width == 0 {
        return Rect::default();
    }
    Rect::new(rect.right().saturating_sub(width), rect.y, width, 1)
}

fn agent_group_trailing_width(row: &AgentRow) -> usize {
    let count_width = row
        .group
        .group_count
        .map(|count| format!("[{count}] ").len())
        .unwrap_or(0);
    count_width
        + match row.group.expanded {
            None => 0,
            Some(true) => 2,
            Some(false) => {
                // "+N " summary plus the chevron cell.
                2 + if row.group.hidden_children > 0 {
                    format!("+{} ", row.group.hidden_children).len()
                } else {
                    0
                }
            }
        }
}

pub(super) fn render_agent_row(
    buffer: &mut Buffer,
    rect: Rect,
    row: &AgentRow,
    config: &ClientShellConfig,
) {
    let palette = &config.palette;
    let row_style = if row.focused {
        Style::default().bg(palette.active_row_bg)
    } else {
        Style::default()
    };
    let name_style = if row.focused {
        Style::default()
            .fg(palette.text)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(palette.subtext0)
            .add_modifier(Modifier::BOLD)
    };
    let status_style = Style::default().fg(status_color(row.status, palette));
    let secondary = Style::default().fg(palette.overlay0);
    let icon = (
        resolved_status_icon(row.status, config),
        Style::default().fg(status_color(row.status, palette)),
    );
    let rows = if row.rows.is_empty() {
        vec![vec![crate::ui::ResolvedToken {
            kind: crate::ui::ResolvedTokenKind::StateIcon,
            style: Default::default(),
        }]]
    } else {
        row.rows.clone()
    };
    let depth = usize::from(row.group.depth);
    let group_trailing = agent_group_trailing_width(row);
    for (index, tokens) in rows.iter().take(rect.height as usize).enumerate() {
        let mut spans = vec![ratatui::text::Span::raw(
            " ".repeat(1 + usize::from(row.indent)),
        )];
        let mut prefix = 1 + usize::from(row.indent);
        if depth > 0 {
            if depth > 1 {
                spans.push(ratatui::text::Span::raw("   ".repeat(depth - 1)));
                prefix += 3 * (depth - 1);
            }
            let guide = if index == 0 {
                if row.group.last_in_group {
                    "\u{2514}\u{2500} "
                } else {
                    "\u{251c}\u{2500} "
                }
            } else if row.group.last_in_group {
                "   "
            } else {
                "\u{2502}  "
            };
            spans.push(ratatui::text::Span::styled(
                guide,
                Style::default().fg(palette.overlay0),
            ));
            prefix += 3;
        } else if index != 0 {
            spans.push(ratatui::text::Span::raw("  "));
            prefix += 2;
        }
        if index == 0 && row.orphaned {
            // The recorded owner is gone; say so rather than silently
            // flattening the row into the roots.
            spans.push(ratatui::text::Span::styled(
                "\u{25cc} ",
                Style::default()
                    .fg(palette.mauve)
                    .add_modifier(Modifier::BOLD),
            ));
            prefix += 2;
        }
        let trailing = if index == 0 { group_trailing } else { 0 };
        spans.extend(crate::ui::resolved_token_spans(
            tokens,
            icon,
            status_style,
            name_style,
            secondary,
            secondary,
            palette,
            (rect.width as usize).saturating_sub(prefix + trailing),
        ));
        Paragraph::new(Line::from(spans)).style(row_style).render(
            Rect::new(rect.x, rect.y + index as u16, rect.width, 1),
            buffer,
        );
    }

    if row.group.expanded.is_none() && row.group.group_count.is_none() {
        return;
    }
    let mut trailing = Vec::<(String, Style)>::new();
    if let Some(count) = row.group.group_count {
        trailing.push((format!("[{count}] "), Style::default().fg(palette.overlay0)));
    }
    if let Some(expanded) = row.group.expanded {
        if !expanded && row.group.hidden_children > 0 {
            trailing.push((
                format!("+{} ", row.group.hidden_children),
                Style::default()
                    .fg(status_color(row.status, palette))
                    .add_modifier(Modifier::BOLD),
            ));
        }
        trailing.push((
            if expanded { "\u{25be}" } else { "\u{25b8}" }.to_owned(),
            Style::default().fg(palette.accent),
        ));
    }
    let chevron = agent_group_chevron_rect(rect, row);
    let mut x = chevron.x;
    for (text, style) in &trailing {
        let width = (display_width(text) as u16).min(chevron.right().saturating_sub(x));
        put_text(buffer, x, chevron.y, width, text, *style);
        x = x.saturating_add(width);
    }
}

fn put_text(buffer: &mut Buffer, x: u16, y: u16, width: u16, text: &str, style: Style) {
    for (offset, character) in text.chars().take(width as usize).enumerate() {
        if let Some(cell) = buffer.cell_mut((x + offset as u16, y)) {
            cell.set_char(character).set_style(style);
        }
    }
}

fn display_width(text: &str) -> usize {
    unicode_width::UnicodeWidthStr::width(text)
}

fn sidebar_status_text(status: crate::api::schema::AgentStatus) -> &'static str {
    use crate::api::schema::AgentStatus;
    match status {
        AgentStatus::Blocked => "blocked",
        AgentStatus::Done => "done",
        AgentStatus::Working => "working",
        AgentStatus::Idle | AgentStatus::Unknown => "idle",
    }
}
