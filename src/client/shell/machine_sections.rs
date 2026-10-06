//! Saved machines as sections under an unchanged Local sidebar
//! (`[ui.sidebar] machines = "sections"`).
//!
//! Local keeps its own renderer and data. Each enabled saved machine adds one
//! header row (collapsed by default) and, when expanded, that machine's spaces
//! and tabs. Rows carry a dim ` · <label>` so a chat's machine is always named.
//! Selecting a row goes through `focus_or_activate`, which streams the remote
//! tab through the existing endpoint activation.

use super::render::{display_width, put_right_text, put_text, ShellRenderState};
use super::*;

/// One rendered strip row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum MachineRow {
    Rule,
    Header { endpoint: usize, collapsed: bool },
    Workspace { endpoint: usize, workspace: usize },
    Tab { endpoint: usize, tab: usize },
    More(usize),
}

/// Remote endpoints that get a section: enabled saved machines, in catalog order.
pub(super) fn section_endpoints(endpoints: &[ClientShellEndpoint]) -> Vec<usize> {
    endpoints
        .iter()
        .enumerate()
        .filter(|(_, endpoint)| {
            !endpoint.endpoint_id.is_local() && endpoint.status != ClientEndpointStatus::Disabled
        })
        .map(|(index, _)| index)
        .collect()
}

/// Every row the sections would show with unlimited height.
pub(super) fn section_rows(
    endpoints: &[ClientShellEndpoint],
    collapsed_endpoints: &HashSet<ClientEndpointId>,
) -> Vec<MachineRow> {
    let sections = section_endpoints(endpoints);
    if sections.is_empty() {
        return Vec::new();
    }
    let mut rows = vec![MachineRow::Rule];
    for index in sections {
        let endpoint = &endpoints[index];
        let collapsed = collapsed_endpoints.contains(&endpoint.endpoint_id);
        rows.push(MachineRow::Header {
            endpoint: index,
            collapsed,
        });
        if collapsed {
            continue;
        }
        let Some(snapshot) = endpoint.snapshot.as_deref() else {
            continue;
        };
        for (workspace_index, workspace) in snapshot
            .workspaces
            .iter()
            .enumerate()
            .filter(|(_, workspace)| workspace.visible_in_profile)
        {
            rows.push(MachineRow::Workspace {
                endpoint: index,
                workspace: workspace_index,
            });
            rows.extend(
                snapshot
                    .tabs
                    .iter()
                    .enumerate()
                    .filter(|(_, tab)| tab.workspace_id == workspace.workspace_id)
                    .map(|(tab_index, _)| MachineRow::Tab {
                        endpoint: index,
                        tab: tab_index,
                    }),
            );
        }
    }
    rows
}

/// Rows that fit in a sidebar of `sidebar_height`: Local keeps at least half,
/// and every header stays visible. Content an expanded section cannot show
/// folds into one "N more" row at the end of that section.
pub(super) fn fit_rows(rows: Vec<MachineRow>, sidebar_height: u16) -> Vec<MachineRow> {
    let cap = usize::from(sidebar_height / 2);
    if rows.len() <= cap {
        return rows;
    }
    let headers = rows
        .iter()
        .filter(|row| matches!(row, MachineRow::Rule | MachineRow::Header { .. }))
        .count();
    let expanded = rows
        .iter()
        .filter(|row| {
            matches!(
                row,
                MachineRow::Header {
                    collapsed: false,
                    ..
                }
            )
        })
        .count();
    // Content rows get what is left after the headers and one "more" row per
    // expanded section.
    let mut budget = cap.saturating_sub(headers).saturating_sub(expanded);
    let mut kept = Vec::with_capacity(cap);
    let mut hidden = 0usize;
    for row in rows {
        match row {
            MachineRow::Rule | MachineRow::Header { .. } => {
                if hidden > 0 {
                    kept.push(MachineRow::More(std::mem::take(&mut hidden)));
                }
                kept.push(row);
            }
            _ if budget > 0 => {
                budget -= 1;
                kept.push(row);
            }
            _ => hidden += 1,
        }
    }
    if hidden > 0 {
        kept.push(MachineRow::More(hidden));
    }
    kept.truncate(cap.max(1));
    kept
}

fn aggregate_status(snapshot: &ClientShellSnapshot) -> crate::api::schema::AgentStatus {
    snapshot
        .tabs
        .iter()
        .filter(|tab| {
            snapshot.workspaces.iter().any(|workspace| {
                workspace.visible_in_profile && workspace.workspace_id == tab.workspace_id
            })
        })
        .map(|tab| tab.agent_status)
        .max_by_key(|status| status_priority(*status))
        .unwrap_or(crate::api::schema::AgentStatus::Unknown)
}

/// Draw `rows` into `area` (one row each, top down) and record their hits.
pub(super) fn render(
    buffer: &mut Buffer,
    area: Rect,
    rows: &[MachineRow],
    config: &ClientShellConfig,
    state: &ShellRenderState<'_>,
    hits: &mut ShellHitMap,
) {
    let palette = &config.palette;
    for (offset, row) in rows.iter().enumerate() {
        let y = area.y.saturating_add(offset as u16);
        if y >= area.bottom() {
            break;
        }
        let rect = Rect::new(area.x, y, area.width, 1);
        match row {
            MachineRow::Rule => put_text(
                buffer,
                rect.x,
                y,
                rect.width,
                &"─".repeat(usize::from(rect.width)),
                Style::default().fg(palette.surface_dim),
            ),
            MachineRow::Header {
                endpoint,
                collapsed,
            } => {
                let endpoint = &state.endpoints[*endpoint];
                let online = endpoint.status == ClientEndpointStatus::Online;
                let (signal, signal_style) = if state.machine_diagnostics.required_for(endpoint) {
                    ("! auth".to_owned(), Style::default().fg(palette.red))
                } else if endpoint.status == ClientEndpointStatus::Attention {
                    ("! error".to_owned(), Style::default().fg(palette.red))
                } else if online {
                    let status = endpoint
                        .snapshot
                        .as_deref()
                        .map_or(crate::api::schema::AgentStatus::Unknown, aggregate_status);
                    (
                        workspace_status_icon(status, config).to_owned(),
                        Style::default().fg(status_color(status, palette)),
                    )
                } else {
                    let (_, label, _) = endpoint_status_presentation(endpoint.status, palette);
                    (
                        label.to_owned(),
                        Style::default()
                            .fg(palette.overlay0)
                            .add_modifier(Modifier::DIM),
                    )
                };
                let signal_width = display_width(&signal).min(rect.width);
                let marker = if *collapsed { "▸" } else { "▾" };
                put_text(
                    buffer,
                    rect.x,
                    y,
                    rect.width.saturating_sub(signal_width.saturating_add(1)),
                    &format!(" {marker} {}", endpoint.label),
                    Style::default()
                        .fg(if online {
                            palette.text
                        } else {
                            palette.overlay0
                        })
                        .add_modifier(Modifier::BOLD),
                );
                put_right_text(
                    buffer,
                    rect,
                    y,
                    &signal,
                    state
                        .machine_diagnostics
                        .badge_style(endpoint, palette, signal_style),
                );
                hits.machines.push(MachineHit {
                    rect,
                    status_badge: Rect::new(
                        rect.right().saturating_sub(signal_width),
                        y,
                        signal_width,
                        1,
                    ),
                    // The whole header folds and unfolds its section.
                    collapse_toggle: rect,
                    endpoint_id: endpoint.endpoint_id.clone(),
                });
            }
            MachineRow::Workspace {
                endpoint,
                workspace,
            } => {
                let endpoint = &state.endpoints[*endpoint];
                let Some(workspace) = endpoint
                    .snapshot
                    .as_deref()
                    .and_then(|snapshot| snapshot.workspaces.get(*workspace))
                else {
                    continue;
                };
                if state.selected_workspace_id.is_some_and(|target| {
                    target.matches(&endpoint.endpoint_id, &workspace.workspace_id)
                }) {
                    buffer.set_style(rect, Style::default().bg(palette.selection_bg));
                }
                put_text(
                    buffer,
                    rect.x,
                    y,
                    rect.width,
                    &format!("   {}", workspace.label),
                    Style::default().fg(palette.overlay0),
                );
                hits.workspaces.push(WorkspaceHit {
                    rect,
                    endpoint_id: endpoint.endpoint_id.clone(),
                    workspace_id: workspace.workspace_id.clone(),
                    indented: false,
                    group_toggle: None,
                });
                dim_if_offline(buffer, rect, endpoint, palette);
            }
            MachineRow::Tab { endpoint, tab } => {
                let endpoint = &state.endpoints[*endpoint];
                let Some(tab) = endpoint
                    .snapshot
                    .as_deref()
                    .and_then(|snapshot| snapshot.tabs.get(*tab))
                else {
                    continue;
                };
                let active = &endpoint.endpoint_id == state.active_endpoint_id && tab.focused;
                if active {
                    buffer.set_style(rect, Style::default().bg(palette.active_row_bg));
                }
                let machine = format!(" · {} ", endpoint.label);
                let machine_width = display_width(&machine).min(rect.width);
                let glyph = resolved_status_icon(tab.agent_status, config);
                put_text(
                    buffer,
                    rect.x.saturating_add(3),
                    y,
                    rect.width.saturating_sub(3),
                    glyph,
                    Style::default().fg(status_color(tab.agent_status, palette)),
                );
                let label_x = rect.x.saturating_add(5);
                put_text(
                    buffer,
                    label_x,
                    y,
                    rect.right()
                        .saturating_sub(label_x)
                        .saturating_sub(machine_width),
                    &crate::ui::truncate_end(
                        &tab.label,
                        usize::from(
                            rect.right()
                                .saturating_sub(label_x)
                                .saturating_sub(machine_width),
                        ),
                    ),
                    Style::default().fg(if active {
                        palette.text
                    } else {
                        palette.subtext0
                    }),
                );
                put_right_text(
                    buffer,
                    rect,
                    y,
                    &machine,
                    Style::default()
                        .fg(palette.overlay0)
                        .add_modifier(Modifier::DIM),
                );
                hits.endpoint_tabs
                    .push((rect, endpoint.endpoint_id.clone(), tab.tab_id.clone()));
                dim_if_offline(buffer, rect, endpoint, palette);
            }
            MachineRow::More(hidden) => put_text(
                buffer,
                rect.x,
                y,
                rect.width,
                &format!("     {hidden} more"),
                Style::default()
                    .fg(palette.overlay0)
                    .add_modifier(Modifier::DIM),
            ),
        }
    }
}

fn dim_if_offline(
    buffer: &mut Buffer,
    rect: Rect,
    endpoint: &ClientShellEndpoint,
    palette: &Palette,
) {
    if endpoint.status != ClientEndpointStatus::Online {
        buffer.set_style(
            rect,
            Style::default()
                .fg(palette.overlay0)
                .add_modifier(Modifier::DIM),
        );
    }
}
