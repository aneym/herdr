use super::render::{display_width, put_right_text, put_text, ShellRenderState};
use super::*;

fn collapsed_groups_for_endpoint<'a>(
    state: &'a ShellRenderState<'_>,
    endpoint_id: &ClientEndpointId,
) -> Option<&'a HashSet<String>> {
    if endpoint_id.is_local() {
        Some(state.collapsed_groups)
    } else {
        state.remote_collapsed_groups.get(endpoint_id)
    }
}

/// One row of the multi-machine spaces list. There is no machines section
/// (Alex, 2026-10-06): a remote workspace whose label matches a local space's
/// label joins that row, and any other remote workspace with a chat is a row
/// of its own, carrying its machine's badge and placed in its areas.json group.
pub(super) struct SpaceListRow {
    /// Index into `state.endpoints` of the endpoint that owns the row.
    pub(super) endpoint: usize,
    pub(super) entry: WorkspaceEntry,
    /// Remote workspaces folded into this row: (endpoint index, workspace index).
    pub(super) merged: Vec<(usize, usize)>,
}

fn space_key(label: &str) -> String {
    label.trim().to_lowercase()
}

/// A remote workspace travels only when it holds a chat: an agent or a pin.
fn remote_workspace_has_chat(snapshot: &ClientShellSnapshot, workspace_id: &str) -> bool {
    snapshot
        .agents
        .iter()
        .any(|agent| agent.workspace_id == workspace_id)
        || snapshot
            .pinned_tabs
            .iter()
            .any(|pin| pin.workspace_id == workspace_id)
}

pub(super) fn space_list_rows(
    state: &ShellRenderState<'_>,
    config: &ClientShellConfig,
    flat: bool,
) -> Vec<SpaceListRow> {
    space_list_rows_for(
        state.endpoints,
        state.collapsed_groups,
        state.remote_collapsed_groups,
        config.factory.enabled,
        flat,
    )
}

/// Every visible workspace in snapshot order, as the collapsed sidebar lists them.
fn flat_entries(snapshot: &ClientShellSnapshot) -> Vec<WorkspaceEntry> {
    snapshot
        .workspaces
        .iter()
        .enumerate()
        .filter(|(_, workspace)| workspace.visible_in_profile)
        .map(|(index, _)| WorkspaceEntry {
            index,
            indented: false,
            last_child: false,
        })
        .collect()
}

/// The rows in display order; workspace navigation walks the same list. `flat`
/// is the collapsed sidebar, which lists workspaces without their worktree groups.
pub(super) fn space_list_rows_for(
    endpoints: &[ClientShellEndpoint],
    collapsed_groups: &HashSet<String>,
    remote_collapsed_groups: &HashMap<ClientEndpointId, HashSet<String>>,
    factory_enabled: bool,
    flat: bool,
) -> Vec<SpaceListRow> {
    let entries = |snapshot: &ClientShellSnapshot, collapsed: &HashSet<String>| {
        if flat {
            flat_entries(snapshot)
        } else {
            super::sidebar::workspace_entries(snapshot, collapsed)
        }
    };
    let empty = HashSet::new();
    let mut rows = Vec::new();
    let mut local_by_label = HashMap::<String, usize>::new();
    let local = endpoints
        .iter()
        .position(|endpoint| endpoint.endpoint_id.is_local());
    let local_snapshot = local.and_then(|index| endpoints[index].snapshot.as_deref());
    if let (Some(index), Some(snapshot)) = (local, local_snapshot) {
        for entry in entries(snapshot, collapsed_groups) {
            if let Some(workspace) = snapshot.workspaces.get(entry.index) {
                local_by_label
                    .entry(space_key(&workspace.label))
                    .or_insert(rows.len());
            }
            rows.push(SpaceListRow {
                endpoint: index,
                entry,
                merged: Vec::new(),
            });
        }
    }
    // areas.json groups live with the local session; a remote space of its own
    // follows the last row of its group, as a local space would sit there.
    let overlay = local
        .filter(|_| factory_enabled)
        .and_then(|index| super::ClientShellState::endpoint_factory_overlay(&endpoints[index]));
    let group_of = |endpoint: usize, workspace: usize| {
        let snapshot = endpoints[endpoint].snapshot.as_deref()?;
        let workspace = snapshot.workspaces.get(workspace)?;
        overlay
            .as_deref()?
            .space_group(&workspace.workspace_id, &workspace.label)
    };
    for (endpoint_index, endpoint) in endpoints.iter().enumerate() {
        if endpoint.endpoint_id.is_local() {
            continue;
        }
        let Some(snapshot) = endpoint.snapshot.as_deref() else {
            continue;
        };
        let collapsed = remote_collapsed_groups
            .get(&endpoint.endpoint_id)
            .unwrap_or(&empty);
        for entry in entries(snapshot, collapsed) {
            let Some(workspace) = snapshot.workspaces.get(entry.index) else {
                continue;
            };
            // A folded worktree group stands for its hidden members' chats too.
            let folded = (!flat)
                .then(|| super::sidebar::parent_group_key(snapshot, entry.index))
                .flatten()
                .filter(|key| collapsed.contains(key));
            let has_chat = match folded {
                Some(key) => snapshot.workspaces.iter().any(|member| {
                    member
                        .worktree
                        .as_ref()
                        .is_some_and(|worktree| worktree.key == key)
                        && remote_workspace_has_chat(snapshot, &member.workspace_id)
                }),
                None => remote_workspace_has_chat(snapshot, &workspace.workspace_id),
            };
            if !has_chat {
                continue;
            }
            let key = space_key(&workspace.label);
            if let Some(&home) = local_by_label.get(&key) {
                rows[home].merged.push((endpoint_index, entry.index));
                continue;
            }
            let row = SpaceListRow {
                endpoint: endpoint_index,
                entry,
                merged: Vec::new(),
            };
            let at = group_of(endpoint_index, entry.index)
                .and_then(|group| {
                    rows.iter()
                        .rposition(|row| group_of(row.endpoint, row.entry.index) == Some(group))
                        .map(|index| index + 1)
                })
                .unwrap_or(rows.len());
            // Only a local space takes remote workspaces in: two remote spaces
            // that share a label stay two rows, each its own click target.
            for index in local_by_label.values_mut() {
                if *index >= at {
                    *index += 1;
                }
            }
            rows.insert(at, row);
        }
    }
    rows
}

/// A row's status: its own, raised by any remote workspace folded into it.
fn space_row_status(
    state: &ShellRenderState<'_>,
    row: &SpaceListRow,
    empty: &HashSet<String>,
) -> Option<crate::api::schema::AgentStatus> {
    let endpoint = &state.endpoints[row.endpoint];
    let snapshot = endpoint.snapshot.as_deref()?;
    let workspace = snapshot.workspaces.get(row.entry.index)?;
    let collapsed = collapsed_groups_for_endpoint(state, &endpoint.endpoint_id).unwrap_or(empty);
    let own = super::sidebar::displayed_workspace_status(snapshot, workspace, collapsed);
    Some(
        row.merged
            .iter()
            .filter_map(|(endpoint, index)| {
                state.endpoints[*endpoint]
                    .snapshot
                    .as_deref()?
                    .workspaces
                    .get(*index)
                    .map(|workspace| workspace.agent_status)
            })
            .fold(own, |best, status| {
                if status_priority(status) > status_priority(best) {
                    status
                } else {
                    best
                }
            }),
    )
}

fn machine_badge_with(
    endpoint: &ClientShellEndpoint,
    healthy: bool,
    palette: &Palette,
) -> Option<(String, Style)> {
    if endpoint.endpoint_id.is_local() {
        return None;
    }
    let name = crate::ui::truncate_end(&endpoint.label, 8);
    Some((
        format!("{} {name}", if healthy { "◇" } else { "◌" }),
        Style::default()
            .fg(if healthy {
                palette.overlay0
            } else {
                palette.surface_dim
            })
            .add_modifier(if healthy {
                Modifier::empty()
            } else {
                Modifier::DIM
            }),
    ))
}

/// Draws the badge right-aligned on `rect`'s first line and returns the cells
/// it took, gap included. The badge keeps the machine's diagnostics hover and
/// click (its auth or error popup) without a machine row.
pub(super) fn render_machine_badge(
    buffer: &mut Buffer,
    rect: Rect,
    endpoint: &ClientShellEndpoint,
    diagnostics: &super::machine_diagnostics::MachineDiagnostics,
    palette: &Palette,
    hits: &mut ShellHitMap,
) -> u16 {
    // A machine waiting on sign-in is not healthy either; its badge opens the prompt.
    let healthy =
        endpoint.status == ClientEndpointStatus::Online && !diagnostics.required_for(endpoint);
    let Some((text, style)) = machine_badge_with(endpoint, healthy, palette) else {
        return 0;
    };
    // The row's own text keeps two thirds of the line; in less room the badge
    // shortens the machine name and, below three cells, keeps only its glyph.
    let width = display_width(&text).min(rect.width / 3);
    if width == 0 {
        return 0;
    }
    let text = if width < 3 {
        text.chars().take(1).collect::<String>()
    } else {
        crate::ui::truncate_end(&text, width as usize)
    };
    let width = display_width(&text).min(width);
    let badge = Rect::new(rect.right().saturating_sub(width), rect.y, width, 1);
    put_text(
        buffer,
        badge.x,
        badge.y,
        badge.width,
        &text,
        diagnostics.badge_style(endpoint, palette, style),
    );
    hits.machines.push(MachineHit {
        status_badge: badge,
        endpoint_id: endpoint.endpoint_id.clone(),
    });
    width.saturating_add(1)
}

pub(super) fn render_collapsed(
    buffer: &mut Buffer,
    area: Rect,
    config: &ClientShellConfig,
    state: &mut ShellRenderState<'_>,
    hits: &mut ShellHitMap,
) {
    let palette = &config.palette;
    super::render::render_sidebar_background(buffer, area, palette);
    let (workspace_area, divider_y, detail_area) =
        super::sidebar::ordered_collapsed_sidebar_sections(area, config);
    let rows = space_list_rows(state, config, true);
    let reveal = std::mem::take(state.reveal_navigation_workspace);
    let selected_row = reveal
        .then(|| {
            rows.iter().position(|row| {
                let endpoint = &state.endpoints[row.endpoint];
                endpoint
                    .snapshot
                    .as_deref()
                    .and_then(|snapshot| snapshot.workspaces.get(row.entry.index))
                    .is_some_and(|workspace| {
                        state.selected_workspace_id.is_some_and(|target| {
                            target.matches(&endpoint.endpoint_id, &workspace.workspace_id)
                        })
                    })
            })
        })
        .flatten();
    let height = usize::from(workspace_area.height);
    let max_scroll = rows.len().saturating_sub(height);
    *state.workspace_scroll = (*state.workspace_scroll).min(max_scroll);
    if let Some(row) = selected_row {
        if row < *state.workspace_scroll {
            *state.workspace_scroll = row;
        } else if row >= state.workspace_scroll.saturating_add(height) {
            *state.workspace_scroll = row.saturating_add(1).saturating_sub(height).min(max_scroll);
        }
    }
    hits.workspace_max_scroll = max_scroll;
    let empty = HashSet::new();
    let mut y = workspace_area.y;
    for row in rows.iter().skip(*state.workspace_scroll) {
        if y >= workspace_area.bottom() {
            break;
        }
        let endpoint = &state.endpoints[row.endpoint];
        let Some(workspace) = endpoint
            .snapshot
            .as_deref()
            .and_then(|snapshot| snapshot.workspaces.get(row.entry.index))
        else {
            continue;
        };
        let status = space_row_status(state, row, &empty).unwrap_or(workspace.agent_status);
        let rect = Rect::new(workspace_area.x, y, workspace_area.width, 1);
        let active = &endpoint.endpoint_id == state.active_endpoint_id;
        let focused = active && workspace.focused;
        let selected = state
            .selected_workspace_id
            .is_some_and(|target| target.matches(&endpoint.endpoint_id, &workspace.workspace_id));
        let selection_background = if palette.selection_bg == ratatui::style::Color::Reset {
            palette.active_row_bg
        } else {
            palette.selection_bg
        };
        if selected {
            buffer.set_style(rect, Style::default().bg(selection_background));
        } else if focused {
            buffer.set_style(
                rect,
                Style::default().bg(super::sidebar::workspace_active_background(
                    palette,
                    state.selected_workspace_id.is_some(),
                )),
            );
        }
        let stale = endpoint.status != ClientEndpointStatus::Online;
        // A remote space of its own keeps the badge's glyph in place of a number.
        let number = if endpoint.endpoint_id.is_local() {
            format!(" {}", workspace.number)
        } else {
            let healthy = endpoint.status == ClientEndpointStatus::Online
                && !state.machine_diagnostics.required_for(endpoint);
            format!(" {}", if healthy { "◇" } else { "◌" })
        };
        let number_width = super::render::display_width(&number).min(rect.width);
        let dim = if stale {
            Modifier::DIM
        } else {
            Modifier::empty()
        };
        let number_style = Style::default()
            .fg(if focused && !stale {
                palette.text
            } else {
                palette.overlay0
            })
            .add_modifier(dim);
        let number_style = if endpoint.endpoint_id.is_local() {
            number_style
        } else {
            let badge = Rect::new(
                rect.x.saturating_add(1),
                rect.y,
                number_width.saturating_sub(1),
                1,
            );
            if badge.width > 0 {
                hits.machines.push(MachineHit {
                    status_badge: badge,
                    endpoint_id: endpoint.endpoint_id.clone(),
                });
            }
            state
                .machine_diagnostics
                .badge_style(endpoint, palette, number_style)
        };
        put_text(buffer, rect.x, rect.y, number_width, &number, number_style);
        put_text(
            buffer,
            rect.x.saturating_add(number_width),
            rect.y,
            rect.width.saturating_sub(number_width),
            workspace_status_icon(status, config),
            Style::default()
                .fg(if stale {
                    palette.overlay0
                } else {
                    status_color(status, palette)
                })
                .add_modifier(dim),
        );
        hits.workspaces.push(WorkspaceHit {
            rect,
            endpoint_id: endpoint.endpoint_id.clone(),
            workspace_id: workspace.workspace_id.clone(),
            indented: false,
            group_toggle: None,
        });
        y = y.saturating_add(1);
    }
    if let Some(divider_y) = divider_y {
        put_text(
            buffer,
            workspace_area.x,
            divider_y,
            workspace_area.width,
            &"─".repeat(workspace_area.width as usize),
            Style::default().fg(palette.surface_dim),
        );
    }
    super::endpoint_agents::render_collapsed(
        buffer,
        detail_area,
        state.endpoints,
        state.active_endpoint_id,
        config,
        hits,
    );
    hits.sidebar_toggle = if area.is_empty() || workspace_area.width == 0 {
        Rect::default()
    } else {
        Rect::new(
            workspace_area.x + workspace_area.width / 2,
            area.bottom().saturating_sub(1),
            1,
            1,
        )
    };
    put_text(
        buffer,
        hits.sidebar_toggle.x,
        hits.sidebar_toggle.y,
        hits.sidebar_toggle.width,
        "»",
        Style::default().fg(palette.overlay0),
    );
}

pub(super) fn render_expanded(
    buffer: &mut Buffer,
    area: Rect,
    active_snapshot: Option<&ClientShellSnapshot>,
    config: &ClientShellConfig,
    state: &mut ShellRenderState<'_>,
    hits: &mut ShellHitMap,
) {
    let palette = &config.palette;
    super::render::render_sidebar_background(buffer, area, palette);
    hits.sidebar_divider = if area.is_empty() {
        Rect::default()
    } else {
        Rect::new(area.right().saturating_sub(1), area.y, 1, area.height)
    };
    // Shared pins remain above the machine/space list, independent of which
    // endpoint is active. Each hit retains its endpoint identity.
    let mut pins = Vec::new();
    for endpoint in state.endpoints {
        let Some(snapshot) = endpoint.snapshot.as_deref() else {
            continue;
        };
        // Each machine's pins roll up from that machine's own validated
        // overlay, whether or not it holds the surface.
        let overlay = config
            .factory
            .enabled
            .then(|| super::ClientShellState::endpoint_factory_overlay(endpoint))
            .flatten();
        for entry in super::tree::pinned_tab_entries(snapshot, overlay.as_deref()) {
            let super::tree::AgentPanelListEntry::PinnedTab(mut row) = entry else {
                continue;
            };

            row.active &= &endpoint.endpoint_id == state.active_endpoint_id;
            row.machine = machine_badge_with(
                endpoint,
                endpoint.status == ClientEndpointStatus::Online
                    && !state.machine_diagnostics.required_for(endpoint),
                palette,
            )
            .map(|(text, style)| {
                (
                    text,
                    state
                        .machine_diagnostics
                        .badge_style(endpoint, palette, style),
                )
            });
            pins.push((endpoint.endpoint_id.clone(), row));
        }
    }
    pins.sort_by_key(|(endpoint, row)| (!row.agent, row.hidden, !endpoint.is_local()));
    let mut numbered = 0;
    for (_, row) in &mut pins {
        if !row.hidden {
            numbered += 1;
        }
        row.shortcut = if !row.hidden && numbered <= 9 {
            numbered
        } else {
            0
        };
    }
    let hidden_count = pins.iter().filter(|(_, row)| row.hidden).count();
    let hidden_alert = pins.iter().any(|(_, row)| {
        row.hidden
            && (row.request.is_some() || row.status == crate::api::schema::AgentStatus::Blocked)
    });
    let has_agents = pins.iter().any(|(_, row)| row.agent);
    let expanded = state.tree.hidden_agents_expanded;
    pins.retain(|(_, row)| !row.hidden || expanded);
    let mut y = area.y;
    hits.endpoint_pin_body = Rect::default();
    hits.endpoint_pin_max_scroll = 0;
    if (!pins.is_empty() || hidden_count > 0) && area.height > 1 {
        // The section takes at most a third of the column and scrolls past
        // that, so the spaces list below always keeps its rows.
        let headers = usize::from(has_agents)
            + usize::from(hidden_count > 0)
            + usize::from(pins.iter().any(|(_, row)| !row.agent));
        let visible = pins
            .len()
            .min(usize::from((area.height / 3).max(1)))
            .min(usize::from(area.height).saturating_sub(headers));
        let max_scroll = pins.len() - visible;
        *state.endpoint_pin_scroll = (*state.endpoint_pin_scroll).min(max_scroll);
        let first = *state.endpoint_pin_scroll;
        let width = area.width.saturating_sub(1);
        let pin_top = y;
        hits.endpoint_pin_max_scroll = max_scroll;
        let mut block = None;
        let mut hidden_header_drawn = false;
        for (endpoint_id, row) in pins.into_iter().skip(first).take(visible) {
            if hidden_count > 0 && !hidden_header_drawn && (row.hidden || !row.agent) {
                if block.is_none() && has_agents {
                    put_text(
                        buffer,
                        area.x,
                        y,
                        width,
                        " agents",
                        Style::default()
                            .fg(palette.overlay0)
                            .add_modifier(Modifier::BOLD),
                    );
                    y += 1;
                    block = Some(true);
                }
                render_hidden_agents_header(
                    buffer,
                    Rect::new(area.x, y, width, 1),
                    hidden_count,
                    expanded,
                    hidden_alert,
                    config,
                    hits,
                );
                y += 1;
                hidden_header_drawn = true;
            }
            if block != Some(row.agent) {
                put_text(
                    buffer,
                    area.x,
                    y,
                    width,
                    if row.agent { " agents" } else { " pinned" },
                    Style::default()
                        .fg(palette.overlay0)
                        .add_modifier(Modifier::BOLD),
                );
                if block.is_none() && max_scroll > 0 {
                    put_right_text(
                        buffer,
                        Rect::new(area.x, y, width, 1),
                        y,
                        &format!("+{max_scroll} more "),
                        Style::default().fg(palette.overlay0),
                    );
                }
                block = Some(row.agent);
                y += 1;
            }
            let rect = Rect::new(area.x, y, width, 1);
            let hit_start = hits.tree_headers.len();
            if let Some(status_badge) =
                super::agent_sidebar::render_pinned_tab_row(buffer, rect, &row, config, hits)
            {
                hits.machines.push(MachineHit {
                    status_badge,
                    endpoint_id: endpoint_id.clone(),
                });
            }
            hits.tree_headers.truncate(hit_start);
            if let Some(hit) = hits
                .pinned_rows
                .last_mut()
                .filter(|hit| hit.tab_id == row.tab_id)
            {
                hit.endpoint_id = Some(endpoint_id.clone());
            }
            // No pin toggle on pinned rows: unpinning is in the row's context menu.
            hits.endpoint_pins
                .push((rect, Rect::default(), endpoint_id, row.tab_id));
            y += 1;
        }
        if hidden_count > 0 && !hidden_header_drawn {
            if block.is_none() {
                put_text(
                    buffer,
                    area.x,
                    y,
                    width,
                    " agents",
                    Style::default()
                        .fg(palette.overlay0)
                        .add_modifier(Modifier::BOLD),
                );
                y += 1;
            }
            render_hidden_agents_header(
                buffer,
                Rect::new(area.x, y, width, 1),
                hidden_count,
                expanded,
                hidden_alert,
                config,
                hits,
            );
            y += 1;
        }
        hits.endpoint_pin_body = Rect::new(area.x, pin_top, width, y.saturating_sub(pin_top));
    }
    let remaining = Rect::new(area.x, y, area.width, area.bottom().saturating_sub(y));
    let (workspace_area, detail_area) =
        crate::ui::expanded_sidebar_sections(remaining, state.sidebar_section_split);
    // The divider and its drag follow the sections below the pinned rows.
    hits.sidebar_section_divider =
        crate::ui::sidebar_section_divider_rect(remaining, state.sidebar_section_split);
    hits.sidebar_section_track = remaining;
    hits.sidebar_section_inverted = false;
    put_text(
        buffer,
        workspace_area.x,
        workspace_area.y,
        workspace_area.width,
        " spaces",
        Style::default()
            .fg(palette.overlay0)
            .add_modifier(Modifier::BOLD),
    );

    let empty_collapsed_groups = HashSet::new();
    let rows = space_list_rows(state, config, false);
    let body = Rect::new(
        workspace_area.x,
        workspace_area.y.saturating_add(WORKSPACE_HEADER_ROWS),
        workspace_area.width,
        workspace_area
            .height
            .saturating_sub(WORKSPACE_HEADER_ROWS + 1),
    );
    hits.workspace_body = body;
    let row_heights = rows
        .iter()
        .map(|row| {
            let endpoint = &state.endpoints[row.endpoint];
            let collapsed_groups = collapsed_groups_for_endpoint(state, &endpoint.endpoint_id)
                .unwrap_or(&empty_collapsed_groups);
            endpoint
                .snapshot
                .as_deref()
                .and_then(|snapshot| {
                    let workspace = snapshot.workspaces.get(row.entry.index)?;
                    Some(
                        super::sidebar::workspace_rows(
                            workspace,
                            super::sidebar::displayed_workspace_status(
                                snapshot,
                                workspace,
                                collapsed_groups,
                            ),
                            row.entry.indented,
                            &config.spaces,
                        )
                        .len()
                        .max(1)
                        .min(u16::MAX as usize) as u16,
                    )
                })
                .unwrap_or(1)
        })
        .collect::<Vec<_>>();
    let gaps = (0..rows.len())
        .map(|index| {
            rows.get(index + 1).map_or(0, |next| {
                u16::from(!next.entry.indented) * config.spaces.row_gap
            })
        })
        .collect::<Vec<_>>();
    let reveal_navigation = !body.is_empty() && std::mem::take(state.reveal_navigation_workspace);
    let reveal_focus = !body.is_empty() && std::mem::take(state.reveal_focused_workspace);
    if reveal_navigation || reveal_focus {
        let selected_row = rows.iter().position(|row| {
            let endpoint = &state.endpoints[row.endpoint];
            endpoint
                .snapshot
                .as_deref()
                .and_then(|snapshot| snapshot.workspaces.get(row.entry.index))
                .is_some_and(|workspace| {
                    if reveal_navigation {
                        state.selected_workspace_id.is_some_and(|target| {
                            target.matches(&endpoint.endpoint_id, &workspace.workspace_id)
                        })
                    } else {
                        &endpoint.endpoint_id == state.active_endpoint_id
                            && active_snapshot.is_some_and(|snapshot| {
                                snapshot.focused_workspace_id.as_deref()
                                    == Some(workspace.workspace_id.as_str())
                            })
                    }
                })
        });
        if let Some(selected_row) = selected_row {
            *state.workspace_scroll = super::scroll::list_scroll_start_to_reveal(
                &row_heights,
                &gaps,
                body.height,
                *state.workspace_scroll,
                selected_row,
            );
        }
    }
    let metrics = super::scroll::list_scroll_metrics(
        &row_heights,
        &gaps,
        body.height,
        *state.workspace_scroll,
    );
    hits.workspace_max_scroll = metrics.max_offset_from_bottom;
    hits.workspace_scroll_metrics = Some(metrics);
    *state.workspace_scroll = metrics
        .max_offset_from_bottom
        .saturating_sub(metrics.offset_from_bottom);
    let show_scrollbar = metrics.max_offset_from_bottom > 0 && body.width > 1;
    let content_width = body.width.saturating_sub(u16::from(show_scrollbar));
    let mut y = body.y;
    for (row_index, row) in rows.iter().enumerate().skip(*state.workspace_scroll) {
        let endpoint = &state.endpoints[row.endpoint];
        let Some(snapshot) = endpoint.snapshot.as_deref() else {
            continue;
        };
        let Some(workspace) = snapshot.workspaces.get(row.entry.index) else {
            continue;
        };
        let collapsed_groups = collapsed_groups_for_endpoint(state, &endpoint.endpoint_id)
            .unwrap_or(&empty_collapsed_groups);
        let status =
            space_row_status(state, row, &empty_collapsed_groups).unwrap_or(workspace.agent_status);
        let tokens =
            super::sidebar::workspace_rows(workspace, status, row.entry.indented, &config.spaces);
        let height = (tokens.len().max(1).min(u16::MAX as usize) as u16).min(body.height);
        if y.saturating_add(height) > body.bottom() {
            break;
        }
        let rect = Rect::new(body.x, y, content_width, height);
        let badge = render_machine_badge(
            buffer,
            Rect::new(rect.x, rect.y, rect.width.saturating_sub(1), 1),
            endpoint,
            state.machine_diagnostics,
            palette,
            hits,
        );
        let text = Rect::new(
            rect.x,
            rect.y,
            rect.width.saturating_sub(badge),
            rect.height,
        );
        let endpoint_active = &endpoint.endpoint_id == state.active_endpoint_id;
        let selected = state
            .selected_workspace_id
            .is_some_and(|target| target.matches(&endpoint.endpoint_id, &workspace.workspace_id));
        super::sidebar::render_workspace_rows(
            buffer,
            text,
            status,
            config,
            &row.entry,
            tokens,
            endpoint_active && workspace.focused,
            selected,
            state.selected_workspace_id.is_some(),
            false,
            palette,
        );
        if endpoint.status != ClientEndpointStatus::Online {
            buffer.set_style(
                text,
                Style::default()
                    .fg(palette.overlay0)
                    .add_modifier(Modifier::DIM),
            );
        }
        let group_toggle = super::sidebar::render_parent_group_toggle(
            buffer,
            rect,
            snapshot,
            row.entry.index,
            collapsed_groups,
            palette,
        );
        hits.workspaces.push(WorkspaceHit {
            rect,
            endpoint_id: endpoint.endpoint_id.clone(),
            workspace_id: workspace.workspace_id.clone(),
            indented: row.entry.indented,
            group_toggle,
        });
        y = y
            .saturating_add(height)
            .saturating_add(gaps.get(row_index).copied().unwrap_or(0));
    }
    if show_scrollbar {
        let track = Rect::new(body.right().saturating_sub(1), body.y, 1, body.height);
        hits.workspace_scrollbar = track;
        super::scroll::render_list_scrollbar(buffer, track, metrics, palette);
    }

    let footer_y = workspace_area.bottom().saturating_sub(1);
    if config.mouse_capture {
        let label = format!(" new · {}", active_endpoint_label(state));
        hits.new_workspace = Rect::new(
            workspace_area.x,
            footer_y,
            display_width(&label).min(workspace_area.width),
            u16::from(workspace_area.height > 0),
        );
        put_text(
            buffer,
            workspace_area.x,
            footer_y,
            workspace_area.width,
            &label,
            Style::default().fg(palette.overlay0),
        );
        let attention = active_snapshot.is_some_and(super::global_menu::global_menu_attention);
        let width = if attention { 8 } else { 6 }.min(workspace_area.width);
        hits.global_launcher = Rect::new(
            workspace_area.right().saturating_sub(width),
            footer_y,
            width,
            1,
        );
        put_right_text(
            buffer,
            workspace_area,
            footer_y,
            if attention { "● menu" } else { "menu" },
            Style::default().fg(if attention {
                palette.accent
            } else {
                palette.overlay0
            }),
        );
    }
    super::endpoint_agents::render_expanded(
        buffer,
        detail_area,
        active_snapshot.and_then(|snapshot| snapshot.agent_view_label.as_deref()),
        state.endpoints,
        state.active_endpoint_id,
        config,
        state.machine_diagnostics,
        state.agent_scroll,
        hits,
    );
    hits.sidebar_toggle = Rect::new(
        area.right().saturating_sub(2),
        area.bottom().saturating_sub(1),
        u16::from(area.width > 1),
        u16::from(area.height > 0),
    );
    put_text(
        buffer,
        hits.sidebar_toggle.x,
        hits.sidebar_toggle.y,
        hits.sidebar_toggle.width,
        "«",
        Style::default().fg(palette.overlay0),
    );
}

fn active_endpoint_label<'a>(state: &'a ShellRenderState<'_>) -> &'a str {
    state
        .endpoints
        .iter()
        .find(|endpoint| &endpoint.endpoint_id == state.active_endpoint_id)
        .map_or("Local", |endpoint| endpoint.label.as_str())
}

fn render_hidden_agents_header(
    buffer: &mut Buffer,
    rect: Rect,
    count: usize,
    expanded: bool,
    alert: bool,
    config: &ClientShellConfig,
    hits: &mut ShellHitMap,
) {
    let text = format!(" Hidden {count}");
    put_text(
        buffer,
        rect.x,
        rect.y,
        rect.width,
        &text,
        Style::default().fg(config.palette.overlay0),
    );
    if !expanded && alert {
        let x = rect.x.saturating_add(display_width(&text) as u16 + 1);
        put_text(
            buffer,
            x,
            rect.y,
            rect.right().saturating_sub(x),
            "•",
            Style::default().fg(config.palette.accent),
        );
    }
    let x = rect
        .x
        .saturating_add(display_width(&text) as u16 + if !expanded && alert { 3 } else { 1 });
    put_text(
        buffer,
        x,
        rect.y,
        rect.right().saturating_sub(x),
        if expanded { "▾" } else { "▸" },
        Style::default().fg(config.palette.overlay0),
    );
    hits.hidden_agents_header = rect;
}
