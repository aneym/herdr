use std::collections::HashMap;

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::Line,
    widgets::{Paragraph, Widget},
};

use super::*;

#[derive(Clone)]
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
    /// The recorded current owner, or the explicit sidebar parent, no longer
    /// resolves to a live agent.
    pub(super) orphaned: bool,
    /// Explicit sidebar placement published by the server: the hands-on pin,
    /// the explicit parent and the server-held group fold.
    pub(super) placement: crate::protocol::ClientShellAgentGroup,
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
    ordered_agent_pane_ids_with_visibility(snapshot, sort, false)
}

pub(super) fn ordered_agent_pane_ids_with_visibility(
    snapshot: &ClientShellSnapshot,
    sort: crate::config::AgentPanelSortConfig,
    all_profiles: bool,
) -> Vec<String> {
    let visible = |agent: &crate::protocol::ClientShellAgent| all_profiles || agent.visible_in_profile;
    if snapshot.agent_view_label.is_some() {
        return snapshot
            .agent_order
            .iter()
            .filter(|pane_id| {
                snapshot
                    .agents
                    .iter()
                    .any(|agent| agent.pane_id == pane_id.as_str() && visible(agent))
            })
            .cloned()
            .collect();
    }
    let mut agents = snapshot
        .agents
        .iter()
        .filter(|agent| visible(agent))
        .collect::<Vec<_>>();
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

/// Sidebar entry point; `overlay` is the factory document when `[ui.factory]` is on.
#[allow(clippy::too_many_arguments)]
pub(super) fn render_agent_panel_with_overlay(
    buffer: &mut Buffer,
    area: Rect,
    snapshot: &ClientShellSnapshot,
    config: &ClientShellConfig,
    tree: &super::tree::ClientTreeChrome,
    overlay: Option<&crate::factory_overlay::FactoryOverlay>,
    agent_scroll: &mut usize,
    hits: &mut ShellHitMap,
) {
    if !render_agent_panel_header_with_factory(
        buffer, area, snapshot.agent_view_label.as_deref(), config, hits,
        overlay.is_some_and(|overlay| snapshot.workspaces.iter().any(|space| {
            overlay.space_is_tagged(snapshot.tabs.iter()
                .filter(|tab| tab.workspace_id == space.workspace_id)
                .map(|tab| tab.tab_id.as_str()))
        })),
    ) {
        return;
    }
    super::sidebar_report::begin();
    let rows = agent_rows(snapshot, config, None);
    let (rows, automations) = super::tree::partition_automations(snapshot, config, rows);
    let tree_tabs = super::tree::tree_view_active(config)
        && snapshot.agent_view_label.is_none()
        && tree.show_tabs;
    let rows = super::tree::arrange_agent_hierarchy_with(snapshot, tree, rows, !tree_tabs);
    let mut entries =
        if super::tree::tree_view_active(config) && snapshot.agent_view_label.is_none() {
            super::tree::tree_list_entries_with_overlay(snapshot, tree, rows, overlay)
        } else {
            rows.into_iter()
                .map(super::tree::AgentPanelListEntry::Agent)
                .collect()
        };
    super::tree::append_automations(&mut entries, tree, automations);
    // Usage and hosts are a fixed footer, not part of the scrolling spaces list.
    let footer_overlay = overlay.filter(|overlay| snapshot.workspaces.iter().any(|space| {
        overlay.space_is_tagged(snapshot.tabs.iter()
            .filter(|tab| tab.workspace_id == space.workspace_id)
            .map(|tab| tab.tab_id.as_str()))
    }));
    let hosts = footer_overlay.map(|overlay| overlay.hosts.as_slice()).unwrap_or(&[]);
    let usage = footer_overlay.map(|overlay| overlay.usage.as_slice()).unwrap_or(&[]);
    let usage = compact_footer_rows(usage);
    let hosts = compact_footer_rows(hosts);
    let footer_rows = footer_line_count(&usage, area.width) + footer_line_count(&hosts, area.width);
    let footer_height = footer_rows.min(area.height.saturating_sub(3) as usize) as u16;
    let list_area = Rect::new(area.x, area.y, area.width, area.height.saturating_sub(footer_height));
    render_agent_list_with_gaps(
        buffer,
        list_area,
        &entries,
        snapshot
            .agent_view_label
            .as_ref()
            .map(|_| " no matching agents"),
        config,
        agent_scroll,
        hits,
        super::tree::AgentPanelListEntry::line_count,
        entries.first().is_some_and(is_factory_entry),
        |index| {
            use super::tree::{AgentPanelListEntry as Entry, FactoryGroupKind};
            if overlay.is_some() && matches!(&entries[index + 1],
                Entry::SpaceHeader(header) if header.factory_space)
                || overlay.is_some() && matches!(&entries[index + 1],
                    Entry::FactoryBackground { kind: FactoryGroupKind::Background, .. }) {
                return config.agents.row_gap.max(1);
            }
            if overlay.is_some() && (matches!(entries[index], Entry::FactorySection { .. })
                || matches!(&entries[index], Entry::SpaceHeader(header) if header.factory_space)) {
                0
            } else {
                config.agents.row_gap
            }
        },
        |buffer, rect, entry, hits| {
            super::sidebar_report::drawn(entry, rect.y.saturating_sub(list_area.y));
            render_panel_list_entry(buffer, rect, entry, config, hits);
        },
        |entry| match entry {
            super::tree::AgentPanelListEntry::FactoryTab(row) => (row.header.active, row.header.active),
            super::tree::AgentPanelListEntry::Agent(row) => (row.focused, false),
            _ => (false, false),
        },
    );
    super::sidebar_report::end();
    let mut y = area.bottom() - footer_height;
    for (rows, clickable) in [(&usage, true), (&hosts, false)] {
        if rows.is_empty() || y >= area.bottom() {
            continue;
        }
        if let Some((separator, shorten_names)) = footer_layout(rows, area.width) {
            let mut x = area.x + 1;
            for (index, row) in rows.iter().enumerate() {
                if index > 0 {
                    let width = display_width(separator) as u16;
                    put_text(buffer, x, y, width, separator, Style::default().fg(config.palette.overlay0));
                    x += width;
                }
                let start = x;
                let name = footer_name(row, shorten_names);
                let name_width = display_width(name) as u16;
                put_text(buffer, x, y, name_width, name, Style::default().fg(config.palette.subtext0));
                x += name_width;
                if let Some(value) = row.summary.as_deref().filter(|value| !value.is_empty()) {
                    x += 1;
                    let width = display_width(value) as u16;
                    put_text(buffer, x, y, width, value, Style::default().fg(footer_value_color(row.attention, config)));
                    x += width;
                }
                if clickable {
                    if let Some(url) = &row.url {
                        hits.factory_usage_urls.push((Rect::new(start, y, x - start, 1), url.clone()));
                    }
                }
            }
            y += 1;
        } else {
            for row in rows.iter().take(area.bottom().saturating_sub(y) as usize) {
                let rect = Rect::new(area.x, y, area.width, 1);
                render_panel_list_entry(buffer, rect,
                    &super::tree::AgentPanelListEntry::FactoryHost {
                        name: row.name.clone(), summary: row.summary.clone(),
                        attention: row.attention, indent: 0,
                    }, config, hits);
                if clickable {
                    if let Some(url) = &row.url {
                        hits.factory_usage_urls.push((rect, url.clone()));
                    }
                }
                y += 1;
            }
        }
    }
}

fn compact_footer_rows(rows: &[crate::factory_overlay::HostRow]) -> Vec<crate::factory_overlay::HostRow> {
    rows.iter().cloned().map(|mut row| {
        row.summary = row.summary.map(|summary| {
            summary.strip_prefix("load ").unwrap_or(&summary)
                .replace(" live", "").replace(" · ", " ")
        });
        row
    }).collect()
}

fn footer_line_count(rows: &[crate::factory_overlay::HostRow], width: u16) -> usize {
    if rows.is_empty() { 0 } else if footer_layout(rows, width).is_some() { 1 } else { rows.len() }
}

fn footer_name(row: &crate::factory_overlay::HostRow, shorten: bool) -> &str {
    if shorten {
        let mut words = row.name.split_whitespace();
        if let (Some(first), Some(_)) = (words.next(), words.next()) {
            return first;
        }
    }
    &row.name
}

fn footer_layout(rows: &[crate::factory_overlay::HostRow], width: u16) -> Option<(&'static str, bool)> {
    [(" · ", false), ("  ", false), ("  ", true)].into_iter().find(|(separator, shorten)| {
        let joined_width = rows.iter().map(|row| {
            display_width(footer_name(row, *shorten)) + row.summary.as_deref().filter(|value| !value.is_empty())
                .map_or(0, |value| 1 + display_width(value))
        }).sum::<usize>() + display_width(separator) * rows.len().saturating_sub(1);
        joined_width <= usize::from(width.saturating_sub(1))
    })
}

fn footer_value_color(attention: crate::factory_overlay::Attention, config: &ClientShellConfig) -> ratatui::style::Color {
    match attention {
        crate::factory_overlay::Attention::Act => config.palette.red,
        crate::factory_overlay::Attention::Warn => config.palette.peach,
        crate::factory_overlay::Attention::None => config.palette.subtext0,
    }
}

fn is_factory_entry(entry: &super::tree::AgentPanelListEntry) -> bool {
    use super::tree::AgentPanelListEntry as Entry;
    matches!(entry, Entry::FactorySection { .. } | Entry::FactoryTab(_)
        | Entry::FactoryBackground { .. } | Entry::FactoryHost { .. })
        || matches!(entry, Entry::SpaceHeader(row) if row.factory_space)
}

/// Right-most two cells of a header row: the disclosure chevron.
pub(super) fn tree_header_chevron_rect(rect: Rect) -> Rect {
    let width = 2u16.min(rect.width);
    if width == 0 {
        return Rect::default();
    }
    Rect::new(rect.right().saturating_sub(width), rect.y, width, 1)
}

/// Hit region of the agent-group control on a header that carries one. On a
/// tab header it spans the `+N` summary and the chevron; on a space header the
/// pin and plus sit between them, so only the chevron slot toggles.
fn tree_header_group_rect(
    rect: Rect,
    group: &super::tree::TreeHeaderGroup,
    is_space: bool,
) -> Rect {
    let width = if is_space {
        2
    } else {
        2 + group.summary_width() as u16
    }
    .min(rect.width);
    if width == 0 {
        return Rect::default();
    }
    Rect::new(rect.right().saturating_sub(width), rect.y, width, 1)
}

/// The folded `+N` summary takes the color of the most demanding agent it
/// hides, so blocked or finished work still shows through the fold.
fn hidden_summary_style(
    status: Option<crate::api::schema::AgentStatus>,
    config: &ClientShellConfig,
) -> Style {
    Style::default()
        .fg(status.map_or(config.palette.overlay0, |status| {
            status_color(status, &config.palette)
        }))
        .add_modifier(Modifier::BOLD)
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
            if let (Some(key), Some(expanded)) = (row.group.group_key.clone(), row.group.expanded) {
                hits.agent_groups.push(AgentGroupHit {
                    rect: agent_group_chevron_rect(rect, row),
                    key,
                    owner_pane_id: row.pane_id.clone(),
                    expanded,
                    server_collapsed: row.group.server_collapsed,
                });
            }
            render_agent_row(buffer, rect, row, config);
        }
        AgentPanelListEntry::QuietSections { hidden, automations } => {
            let quiet = Style::default().fg(config.palette.overlay0).add_modifier(Modifier::DIM);
            let hidden_label = format!("{hidden} hidden");
            let automation_label = format!("{} automations ▸", automations.total);
            let start = rect.x.saturating_add(1);
            let hidden_width = (display_width(&hidden_label) as u16).min(rect.right().saturating_sub(start));
            put_text(buffer, start, rect.y, hidden_width, &hidden_label, quiet);
            hits.tree_hidden_header = Rect::new(start, rect.y, hidden_width, 1);
            let separator = start + hidden_width;
            put_text(buffer, separator, rect.y, 3.min(rect.right().saturating_sub(separator)), " · ", quiet);
            let start = separator.saturating_add(3).min(rect.right());
            let width = (display_width(&automation_label) as u16).min(rect.right().saturating_sub(start));
            put_text(buffer, start, rect.y, width, &automation_label, quiet.fg(automations.color(&config.palette)));
            hits.automations_header = Rect::new(start, rect.y, width, 1);
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
        AgentPanelListEntry::FactoryGoalPicker { filter, choices } => {
            put_text(buffer, rect.x, rect.y, rect.width, " goal  ",
                Style::default().fg(config.palette.overlay0).add_modifier(Modifier::DIM));
            let clear = if filter.is_some() { Rect::new(rect.right().saturating_sub(2), rect.y, 2.min(rect.width), 1) } else { Rect::default() };
            let value = filter.as_ref().map_or("All".to_owned(), |value| value.replace(':', " · "));
            put_text(buffer, rect.x + 7, rect.y, rect.width.saturating_sub(7 + clear.width), &format!("{value} ▾"),
                Style::default().fg(if filter.is_some() { config.palette.blue } else { config.palette.subtext0 }));
            if filter.is_some() {
                put_text(buffer, clear.x, clear.y, clear.width, " ✕", Style::default().fg(config.palette.blue));
            }
            hits.factory_goal_picker = Some((rect, clear, choices.clone()));
        }
        AgentPanelListEntry::FactorySection { label, right, indent, controls } => {
            let style = Style::default().fg(config.palette.subtext0).add_modifier(Modifier::DIM);
            if controls.is_none() {
                let start = rect.x.saturating_add(1 + u16::from(*indent));
                put_text(buffer, start, rect.y, rect.right().saturating_sub(start), label, style);
                let width = display_width(right) as u16;
                if width > 0 && rect.right().saturating_sub(width) > start.saturating_add(display_width(label) as u16) {
                    put_text(buffer, rect.right() - width, rect.y, width, right, style);
                }
                return;
            }
            let button = if controls.is_some() { tree_header_chevron_rect(rect) } else { Rect::default() };
            let end = if controls.is_some() { button.x } else { rect.right() };
            let width = (display_width(right) as u16).min(end.saturating_sub(rect.x));
            let right_x = end.saturating_sub(width);
            let label_start = rect.x.saturating_add(1 + u16::from(*indent));
            let label_width = right_x.saturating_sub(label_start);
            let display_label = if *label == "READY FOR REVIEW" && label_width < display_width(label) as u16 + 2 {
                "READY"
            } else { label };
            let label_text = if let Some(controls) = controls {
                format!("{} {display_label}", if controls.collapsed { "▸" } else { "▾" })
            } else { (*label).to_owned() };
            put_text(buffer, label_start, rect.y, label_width, &label_text, style);
            put_text(buffer, right_x, rect.y, width, right,
                if controls.as_ref().is_some_and(|controls| controls.alert) { style.fg(config.palette.red) } else { style });
            if let Some(controls) = controls {
                put_text(buffer, button.x, rect.y, button.width, if controls.focused { "✕" } else { "◎" },
                    Style::default().fg(config.palette.overlay0));
                hits.factory_sections.push(FactorySectionHit { rect,
                    label_rect: Rect::new(rect.x, rect.y, right_x.saturating_sub(rect.x), 1), button,
                    workspace_id: controls.workspace_id.clone(), label,
                    collapsed: controls.collapsed, focused: controls.focused });
            }
        }
        AgentPanelListEntry::FactoryShowAll { workspace_id, count, alert, indent } => {
            put_text(buffer, rect.x + u16::from(*indent), rect.y, rect.width.saturating_sub(u16::from(*indent)),
                &format!(" show all · {count} more{}", if *alert { " !" } else { "" }),
                Style::default().fg(if *alert { config.palette.red } else { config.palette.overlay0 }));
            hits.factory_show_all.push((rect, workspace_id.clone()));
        }
        AgentPanelListEntry::FactoryTab(row) => render_factory_tab(buffer, rect, row, config, hits),
        AgentPanelListEntry::FactoryHost {
            name,
            summary,
            attention,
            indent,
        } => {
            let start = rect.x.saturating_add(1 + u16::from(*indent));
            let right = summary.as_deref().unwrap_or("");
            let right_width = (display_width(right) as u16).min(rect.right().saturating_sub(start));
            let name_width = rect.right().saturating_sub(start + right_width + u16::from(right_width > 0));
            let name = crate::ui::truncate_end(name, name_width as usize);
            put_text(buffer, start, rect.y, name_width, &name, Style::default().fg(config.palette.subtext0));
            let color = match attention {
                crate::factory_overlay::Attention::Act => config.palette.red,
                crate::factory_overlay::Attention::Warn => config.palette.peach,
                crate::factory_overlay::Attention::None => config.palette.subtext0,
            };
            put_text(buffer, rect.right().saturating_sub(right_width), rect.y, right_width, right, Style::default().fg(color));
        }
        AgentPanelListEntry::FactoryBackground { kind, workspace_id, count, collapsed, indent, alert, working, shortcut } => {
            render_factory_group(buffer, rect, config, hits, workspace_id, kind.label(), *count, *collapsed, *indent, *alert, *working, *shortcut);
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

#[allow(clippy::too_many_arguments)]
fn render_factory_group(
    buffer: &mut Buffer, rect: Rect, config: &ClientShellConfig, hits: &mut ShellHitMap,
    workspace_id: &str, label: &str, count: usize, collapsed: bool, indent: u8,
    alert: bool, working: bool, shortcut: bool,
) {
    let style = Style::default().fg(config.palette.overlay0).add_modifier(Modifier::DIM);
    let start = rect.x.saturating_add(1 + u16::from(indent));
    let chevron = Rect::new(start, rect.y, 2.min(rect.right().saturating_sub(start)), 1);
    put_text(buffer, chevron.x, rect.y, 2.min(chevron.width), if collapsed { "▸ " } else { "▾ " }, style);
    let label_x = start.saturating_add(2);
    let right_edge = rect.right().saturating_sub(if shortcut { 5 } else { u16::from(alert) });
    let marker = if working { " ●" } else { "" };
    let text = crate::ui::truncate_end(&format!("{label} {count}{marker}"), right_edge.saturating_sub(label_x) as usize);
    put_text(buffer, label_x, rect.y, right_edge.saturating_sub(label_x), &text, style);
    if working && text.ends_with('●') {
        let dot_x = label_x + display_width(&text).saturating_sub(1) as u16;
        put_text(buffer, dot_x, rect.y, 1, "●", Style::default().fg(config.palette.peach));
    }
    if shortcut {
        put_text(buffer, rect.right().saturating_sub(5), rect.y, 5, "⌘1..9", style);
    }
    if alert {
        let x = rect.right().saturating_sub(if shortcut { 6 } else { 1 });
        put_text(buffer, x, rect.y, 1, "!", Style::default().fg(config.palette.red));
    }
    hits.tree_headers.push(TreeHeaderHit {
        rect, chevron, plus: Rect::default(), pin: Rect::default(), group: None,
        workspace_id: workspace_id.to_owned(), tab_id: None,
        key: if label == "background" { format!("factory-background:{workspace_id}") }
            else { format!("factory-background:{label}:{workspace_id}") },
        pinned: false, collapsed,
    });
}

fn render_factory_tab(
    buffer: &mut Buffer,
    rect: Rect,
    row: &super::tree::FactoryTabRow,
    config: &ClientShellConfig,
    hits: &mut ShellHitMap,
) {
    let palette = &config.palette;
    let header = &row.header;
    if header.active {
        buffer.set_style(rect, Style::default().bg(palette.active_row_bg));
    }
    let start = rect.x.saturating_add(1 + u16::from(header.indent));
    let chevron = Rect::new(start, rect.y, 2.min(rect.right().saturating_sub(start)), 1);
    if header.collapsible {
        put_text(buffer, chevron.x, rect.y, 2.min(chevron.width),
            if header.collapsed { "▸ " } else { "▾ " }, Style::default().fg(palette.accent));
    }
    let icon_x = start.saturating_add(2);
    let failed = row.workflow && row.done && row.attention == crate::factory_overlay::Attention::Act;
    let icon = if row.workflow {
        if failed { "✗" } else if row.done { "✓" } else { "◐" }
    } else if row.idle {
        "○"
    } else {
        let key = match row.status {
            crate::api::schema::AgentStatus::Blocked => "blocked",
            crate::api::schema::AgentStatus::Working => "working",
            crate::api::schema::AgentStatus::Done => "idle_unseen",
            crate::api::schema::AgentStatus::Idle => "idle",
            crate::api::schema::AgentStatus::Unknown => "unknown",
        };
        match config.agents.state_icons.get(key) {
            Some(icon) if !icon.trim().is_empty() => icon.as_str(),
            Some(_) if row.status != crate::api::schema::AgentStatus::Working => "■",
            _ => "●",
        }
    };
    let color = if failed { palette.red } else if row.idle { palette.overlay0 } else { status_color(row.status, palette) };
    put_text(buffer, icon_x, rect.y, 1.min(rect.right().saturating_sub(icon_x)), icon,
        Style::default().fg(color));
    let attention_color = match row.attention {
        crate::factory_overlay::Attention::Act => Some(palette.red),
        crate::factory_overlay::Attention::Warn => Some(palette.peach),
        crate::factory_overlay::Attention::None => None,
    };
    let content_right = rect.right().saturating_sub(if attention_color.is_some() { 2 } else { 0 });
    let name_x = icon_x.saturating_add(2);
    let available = content_right.saturating_sub(name_x);
    let metadata = if row.reviewing {
        if row.review_url.is_some() { "review ↗" } else { "no link" }.to_owned()
    } else if row.scoping && row.scope_url.is_some() {
        "scope ↗".to_owned()
    } else if let Some(badge) = row.badge.as_deref() {
        badge.to_owned()
    } else if let Some(summary) = row.summary.as_deref().filter(|value| !value.is_empty()) {
        summary.to_owned()
    } else if row.idle {
        row.idle_reason.as_deref().map(|reason| crate::ui::truncate_end(reason, 10))
            .unwrap_or_else(|| "idle".to_owned())
    } else { String::new() };
    let metadata = if !row.reviewing && !row.workflow && !row.background && !row.idle && row.summary.is_some()
        && row.badge.is_none() && available > 0 && display_width(&header.label) + display_width(&metadata) + 1 > available as usize {
        metadata.split(" · ").next().unwrap_or("").to_owned()
    } else { metadata };
    // A workflow name gets first claim on the row; its host can move below.
    let badge_below = row.workflow && row.badge.as_deref().is_some_and(|badge| {
        display_width(&header.label) + 1 + display_width(badge) > available as usize
    });
    let right_label = if badge_below { "" } else { metadata.as_str() };
    let right_width = (display_width(right_label) as u16).min(available.saturating_sub(1));
    let label_room = available.saturating_sub(right_width + u16::from(right_width > 0));
    let marker_width = if row.devloop && label_room >= 2 { 2 } else { 0 };
    let name_width = label_room.saturating_sub(marker_width);
    let name = crate::ui::truncate_end(&header.label, name_width as usize);
    let style = if row.idle || row.background {
        Style::default().fg(palette.overlay0).add_modifier(Modifier::DIM)
    } else if header.active {
        Style::default().fg(palette.text).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(palette.subtext0).add_modifier(Modifier::BOLD)
    };
    put_text(buffer, name_x, rect.y, name_width, &name, style);
    if marker_width > 0 {
        put_text(buffer, name_x + display_width(&name) as u16, rect.y, marker_width, " ⟳", style);
    }
    if right_width > 0 {
        let right_x = content_right.saturating_sub(right_width);
        if row.reviewing {
            let style = if let Some(url) = &row.review_url {
                hits.factory_review_urls.push((Rect::new(right_x, rect.y, right_width, 1), url.clone()));
                Style::default().fg(palette.blue)
            } else {
                Style::default().fg(palette.overlay0).add_modifier(Modifier::DIM)
            };
            put_text(buffer, right_x, rect.y, right_width, right_label, style);
        } else if let Some(url) = row.scope_url.as_ref().filter(|_| row.scoping) {
            hits.factory_scope_urls.push((Rect::new(right_x, rect.y, right_width, 1), url.clone()));
            put_text(buffer, right_x, rect.y, right_width, right_label, Style::default().fg(palette.blue));
        } else if let Some(badge) = row.badge.as_deref() {
            let badge_width = display_width(badge) as u16;
            let badge_color = attention_color.unwrap_or(color);
            put_text(buffer, content_right.saturating_sub(badge_width), rect.y, badge_width,
                badge, Style::default().fg(badge_color));
        } else {
            put_text(buffer, right_x, rect.y, right_width, right_label,
                Style::default().fg(if row.idle && row.idle_reason.as_deref() == Some("stalled") { palette.peach } else { palette.overlay0 }).add_modifier(Modifier::DIM));
        }
    }
    if let Some(attention_color) = attention_color {
        let mark_x = rect.right().saturating_sub(1);
        put_text(buffer, mark_x, rect.y, 1.min(rect.right().saturating_sub(mark_x)), "!",
            Style::default().fg(attention_color));
    }
    if rect.height > 1 && row.workflow && !row.done {
        let phase = row.phase.as_deref().unwrap_or("").trim().to_lowercase();
        let age = row.started.map(|started| {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
            workflow_age(now.saturating_sub(started.max(0) as u64))
        });
        let x = name_x.saturating_add(1);
        let room = rect.right().saturating_sub(x) as usize;
        let mut progress = workflow_progress(&phase, age.as_deref(), room);
        if let Some(badge) = row.badge.as_deref().filter(|_| badge_below) {
            let reserved = display_width(badge) + usize::from(!progress.is_empty());
            if let Some(remaining) = room.checked_sub(reserved) {
                let shorter = workflow_progress(&phase, age.as_deref(), remaining);
                // A moved badge must not trade away an age the unbadged row can show.
                let age_preserved = age.as_deref().is_none_or(|age| {
                    !progress.ends_with(&format!(" · {age}")) || shorter.ends_with(&format!(" · {age}"))
                });
                if age_preserved {
                    progress = shorter;
                    if !progress.is_empty() { progress.push(' '); }
                    progress.push_str(badge);
                }
            }
        }
        put_text(buffer, x, rect.y + 1, room as u16, &progress,
            Style::default().fg(palette.overlay0).add_modifier(Modifier::DIM));
    }
    hits.tree_headers.push(TreeHeaderHit {
        rect,
        chevron: if header.collapsible { chevron } else { Rect::default() },
        plus: Rect::default(),
        pin: Rect::default(),
        group: None,
        workspace_id: header.workspace_id.clone(),
        tab_id: header.tab_id.clone(),
        key: header.key.clone(),
        pinned: false,
        collapsed: header.collapsed,
    });
}

fn workflow_progress(phase: &str, age: Option<&str>, room: usize) -> String {
    if phase.is_empty() { return String::new(); }
    let fraction = phase.rsplit_once(' ').filter(|(word, count)| {
        !word.is_empty() && count.split_once('/').is_some_and(|(numerator, denominator)| {
            !numerator.is_empty() && !denominator.is_empty()
                && numerator.bytes().all(|byte| byte.is_ascii_digit())
                && denominator.bytes().all(|byte| byte.is_ascii_digit())
        })
    });
    let fit_phase = |width: usize| -> Option<String> {
        let (word, count) = fraction.unwrap_or((phase, ""));
        let suffix_width = if count.is_empty() { 0 } else { 1 + display_width(count) };
        if display_width(phase) <= width { return Some(phase.to_owned()); }
        // Abbreviation keeps four letters plus its ellipsis and the entire fraction.
        let word_width = width.checked_sub(suffix_width)?;
        if word_width < 5 { return None; }
        let shortened = crate::ui::truncate_end(word, word_width);
        (display_width(&shortened) >= 5).then(|| format!("{shortened}{}", if count.is_empty() {
            String::new()
        } else {
            format!(" {count}")
        }))
    };
    if let Some(age) = age {
        let segment = format!(" · {age}");
        if let Some(width) = room.checked_sub(display_width(&segment)) {
            if let Some(phase) = fit_phase(width) { return format!("{phase}{segment}"); }
        }
        // Hours may omit minutes, but never the unit.
        let hours = age.split_once('h').map(|(hours, _)| format!("{hours}h"));
        if let Some(hours) = &hours {
            let segment = format!(" · {hours}");
            if let Some(width) = room.checked_sub(display_width(&segment)) {
                if let Some(phase) = fit_phase(width) { return format!("{phase}{segment}"); }
            }
        }
        if let Some((_, count)) = fraction {
            for age in std::iter::once(age).chain(hours.as_deref()) {
                let progress = format!("{count} · {age}");
                if display_width(&progress) <= room { return progress; }
            }
        }
    }
    fit_phase(room).unwrap_or_default()
}

fn workflow_age(seconds: u64) -> String {
    let minutes = seconds / 60;
    if minutes == 0 { "<1m".to_owned() }
    else if minutes < 60 { format!("{minutes}m") }
    else { format!("{}h{}m", minutes / 60, minutes % 60) }
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
        paint_half_pads(buffer, rect, palette.active_row_bg,
            if is_space && header.factory_space { 0 } else { config.agents.row_gap });
    }
    let prefix = 1 + u16::from(header.indent);
    // Factory spaces keep their pin, plus and chevron even under a scrollbar.
    // The attention count yields first; the name truncates only after that.
    let summary_width = header.space_attention.as_ref().map_or(0, |(_, summary)| 2 + display_width(summary) as u16);
    let factory_controls = is_space && header.factory_space && rect.width >= 6;
    let compact_space = is_space && header.factory_space && !factory_controls
        && prefix + display_width(&header.label) as u16 + 10 + summary_width > rect.width;
    // A header standing in for a group owner takes that group's chevron (and
    // its folded `+N`), since with agent rows hidden it is the only place the
    // group can be opened from.
    let group = header.group_chevron();
    // Space headers reserve extra cells for the pin and new-tab plus.
    let control_width = if compact_space { 2 } else if is_space { 10 } else { 6 };
    let factory_summary_width = 3 + header.space_attention.as_ref().map_or(0, |(_, summary)| display_width(summary) as u16);
    let summary_fits = if factory_controls {
        header.space_attention.is_some() && prefix + display_width(&header.label) as u16
            + 1 + factory_summary_width + 6 <= rect.width
    } else {
        !compact_space || prefix + display_width(&header.label) as u16 + 2 + summary_width <= rect.width
    };
    let reserve_summary = if factory_controls && summary_fits {
        factory_summary_width
    } else if summary_fits { summary_width } else { 0 };
    let trailing_width = if factory_controls { 6 } else { control_width }
        + group.map(|group| group.summary_width() as u16).unwrap_or(0)
        + reserve_summary;
    let label_width = rect.width.saturating_sub(prefix).saturating_sub(trailing_width)
        .saturating_sub(u16::from(factory_controls));
    put_text(
        buffer,
        rect.x.saturating_add(prefix),
        rect.y,
        label_width,
        &crate::ui::truncate_end(&header.label, label_width as usize),
        label_style,
    );

    let mut trailing = Vec::<(String, Style)>::new();
    if !(header.child_states.is_empty() || is_space && header.factory_space) {
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
    if let Some(group) = group.filter(|group| group.summary_width() > 0) {
        trailing.push((
            format!("+{} ", group.hidden_children),
            hidden_summary_style(group.hidden_status, config),
        ));
    }
    let group_chevron = group.map(|group| {
        (
            if group.expanded {
                "\u{25be} "
            } else {
                "\u{25b8} "
            }
            .to_owned(),
            if group.expanded {
                Style::default().fg(palette.overlay0)
            } else {
                Style::default().fg(group
                    .hidden_status
                    .map_or(palette.overlay0, |status| status_color(status, palette)))
            },
        )
    });
    if is_space && !compact_space {
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
        } else if let Some(group_chevron) = group_chevron {
            group_chevron
        } else {
            ("  ".to_owned(), Style::default())
        });
    } else if header.collapsible {
        trailing.push(chevron(header.collapsed));
    } else if let Some(group_chevron) = group_chevron {
        trailing.push(group_chevron);
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

    if is_space {
        if let Some((attention, summary)) = header.space_attention.as_ref().filter(|_| summary_fits) {
            let color = match attention {
                crate::factory_overlay::Attention::Act => palette.red,
                crate::factory_overlay::Attention::Warn => palette.peach,
                crate::factory_overlay::Attention::None => palette.overlay0,
            };
            let x = if factory_controls {
                rect.right().saturating_sub(6 + factory_summary_width)
            } else {
                rect.right().saturating_sub(2 + display_width(summary) as u16 + if compact_space { 3 } else { 10 })
            };
            put_text(buffer, x, rect.y, 2, if header.factory_space { "  " } else { "● " }, Style::default().fg(color));
            put_text(
                buffer,
                x.saturating_add(2),
                rect.y,
                display_width(summary) as u16,
                summary,
                Style::default()
                    .fg(palette.overlay0)
                    .add_modifier(Modifier::DIM),
            );
            if compact_space && header.collapsible {
                put_text(buffer, x.saturating_add(2 + display_width(summary) as u16), rect.y, 1,
                    " ", Style::default());
            }
        }
    }
    hits.tree_headers.push(TreeHeaderHit {
        rect,
        chevron: if header.collapsible {
            tree_header_chevron_rect(rect)
        } else {
            Rect::default()
        },
        plus: if is_space && !compact_space {
            tree_header_plus_rect(rect)
        } else {
            Rect::default()
        },
        pin: if is_space && !compact_space {
            tree_header_pin_rect(rect)
        } else {
            Rect::default()
        },
        group: group.map(|group| AgentGroupHit {
            rect: tree_header_group_rect(rect, group, is_space),
            key: group.key.clone(),
            owner_pane_id: group.owner_pane_id.clone(),
            expanded: group.expanded,
            server_collapsed: group.server_collapsed,
        }),
        workspace_id: header.workspace_id.clone(),
        tab_id: header.tab_id.clone(),
        key: header.key.clone(),
        pinned: header.pinned,
        collapsed: header.collapsed,
    });
}

pub(super) fn render_agent_panel_header(
    buffer: &mut Buffer,
    area: Rect,
    agent_view_label: Option<&str>,
    config: &ClientShellConfig,
    hits: &mut ShellHitMap,
) -> bool {
    render_agent_panel_header_with_factory(buffer, area, agent_view_label, config, hits, false)
}

fn render_agent_panel_header_with_factory(
    buffer: &mut Buffer,
    area: Rect,
    agent_view_label: Option<&str>,
    config: &ClientShellConfig,
    hits: &mut ShellHitMap,
    factory: bool,
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
    let sort_label = if factory { "" } else { sort_label };
    let sort_width = display_width(sort_label).min(area.width as usize) as u16;
    let sort_rect = Rect::new(
        area.right().saturating_sub(sort_width),
        area.y + 1,
        sort_width,
        1,
    );
    let available = sort_rect.x.saturating_sub(area.x);
    let gap = available.min(1);
    let usage_width = 5u16.min(available.saturating_sub(gap));
    let usage_rect = Rect::new(
        sort_rect.x.saturating_sub(usage_width + gap),
        area.y + 1,
        usage_width,
        1,
    );
    hits.agent_usage = if config.mouse_capture && !factory {
        usage_rect
    } else {
        Rect::default()
    };
    put_text(
        buffer,
        usage_rect.x,
        usage_rect.y,
        usage_rect.width,
        if factory { "" } else { "usage" },
        Style::default()
            .fg(config.palette.accent)
            .add_modifier(Modifier::BOLD),
    );
    hits.agent_sort_toggle = if config.mouse_capture && !factory && agent_view_label.is_none() {
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
    buffer: &mut Buffer, area: Rect, rows: &[T], empty_message: Option<&str>,
    config: &ClientShellConfig, agent_scroll: &mut usize, hits: &mut ShellHitMap,
    row_lines: impl Fn(&T) -> usize,
    render_row: impl FnMut(&mut Buffer, Rect, &T, &mut ShellHitMap),
) {
    render_agent_list_with_gaps(buffer, area, rows, empty_message, config, agent_scroll,
        hits, row_lines, false, |_| config.agents.row_gap, render_row, |_| (false, false));
}

#[allow(clippy::too_many_arguments)]
fn render_agent_list_with_gaps<T>(
    buffer: &mut Buffer,
    area: Rect,
    rows: &[T],
    empty_message: Option<&str>,
    config: &ClientShellConfig,
    agent_scroll: &mut usize,
    hits: &mut ShellHitMap,
    row_lines: impl Fn(&T) -> usize,
    compact_header: bool,
    row_gap: impl Fn(usize) -> u16,
    mut render_row: impl FnMut(&mut Buffer, Rect, &T, &mut ShellHitMap),
    highlighted: impl Fn(&T) -> (bool, bool),
) {
    let header_height = if compact_header { 2 } else { 3 };
    let body = Rect::new(
        area.x,
        area.y.saturating_add(header_height),
        area.width,
        area.height.saturating_sub(header_height),
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
                row_gap(index)
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
        // A half-pad may only occupy a real spacer, never the next space's header.
        if highlighted(row).0 && gaps[index] == 0 && rect.bottom() < body.bottom() {
            for x in rect.x..rect.right() {
                if let Some(cell) = buffer.cell_mut((x, rect.bottom())) {
                    if matches!(cell.symbol(), "▀" | "▄") && cell.fg == config.palette.active_row_bg {
                        cell.set_symbol(" ").set_fg(config.palette.text);
                    }
                }
            }
        }
        let gap = gaps[index];
        // A spacer belongs to the row above, including for mouse hit testing.
        if gap > 0 {
            let bottom = y.saturating_add(height);
            let extension = gap.min(body.bottom().saturating_sub(bottom));
            if extension > 0 {
                if highlighted(row).1 {
                    buffer.set_style(Rect::new(rect.x, bottom, rect.width, extension),
                        Style::default().bg(config.palette.active_row_bg));
                }
                let extended = Rect::new(rect.x, rect.y, rect.width, height + extension);
                if let Some((hit, _)) = hits.agents.iter_mut().find(|(hit, _)| *hit == rect) { *hit = extended; }
                if let Some(hit) = hits.tree_headers.iter_mut().find(|hit| hit.rect == rect) { hit.rect = extended; }
            }
        }
        y = y
            .saturating_add(height)
            .saturating_add(gap);
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
        placement: agent.group.clone(),
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

/// Half a cell of selection colour above and below a lit row, drawn in the gap
/// rows with half-block glyphs, so the highlight reads as padded rather than a
/// thin strip (0.8.2 fork parity: `paint_half_pad`). Only when rows are gapped,
/// so the glyphs never land on a neighbouring row's text.
fn paint_half_pads(buffer: &mut Buffer, rect: Rect, bg: ratatui::style::Color, row_gap: u16) {
    if row_gap == 0 || rect.width == 0 {
        return;
    }
    let area = buffer.area;
    let glyph_row = |buffer: &mut Buffer, y: u16, glyph: &str| {
        for x in rect.x..rect.right().min(area.right()) {
            if let Some(cell) = buffer.cell_mut((x, y)) {
                if cell.symbol().trim().is_empty() {
                    cell.set_symbol(glyph);
                    cell.set_fg(bg);
                }
            }
        }
    };
    if rect.y > area.y {
        glyph_row(buffer, rect.y - 1, "\u{2584}");
    }
    if rect.bottom() < area.bottom() {
        glyph_row(buffer, rect.bottom(), "\u{2580}");
    }
}

pub(super) fn render_agent_row(
    buffer: &mut Buffer,
    rect: Rect,
    row: &AgentRow,
    config: &ClientShellConfig,
) {
    let palette = &config.palette;
    if row.focused {
        paint_half_pads(buffer, rect, palette.active_row_bg, config.agents.row_gap);
    }
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
        if index == 0 && row.placement.hands_on {
            // Same pin glyph as a pinned space: this row stays put, outside
            // every group.
            spans.push(ratatui::text::Span::styled(
                "\u{26b2} ",
                Style::default()
                    .fg(palette.accent)
                    .add_modifier(Modifier::BOLD),
            ));
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
            name_style,
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
                hidden_summary_style(row.group.hidden_status.or(Some(row.status)), config),
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
