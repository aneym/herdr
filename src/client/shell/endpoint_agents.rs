use super::render::put_text;
use super::*;

pub(super) fn render_collapsed(
    buffer: &mut Buffer,
    area: Rect,
    endpoints: &[ClientShellEndpoint],
    active_endpoint_id: &ClientEndpointId,
    config: &ClientShellConfig,
    hits: &mut ShellHitMap,
) {
    let rows = agent_rows(endpoints, active_endpoint_id, config);
    for (index, row) in rows.into_iter().take(area.height as usize).enumerate() {
        let rect = Rect::new(area.x, area.y + index as u16, area.width, 1);
        if row.agent.focused {
            buffer.set_style(rect, Style::default().bg(config.palette.active_row_bg));
        }
        // Only another machine's chat names its machine; local rows have no badge.
        let initial = if row.endpoint_id.is_local() {
            ' '
        } else {
            row.machine_label.chars().next().unwrap_or('?')
        };
        put_text(
            buffer,
            rect.x,
            rect.y,
            rect.width,
            &format!(
                "{initial}{}",
                resolved_status_icon(row.agent.status, config)
            ),
            Style::default()
                .fg(if row.stale {
                    config.palette.overlay0
                } else {
                    status_color(row.agent.status, &config.palette)
                })
                .add_modifier(if row.stale {
                    Modifier::DIM
                } else {
                    Modifier::empty()
                }),
        );
        hits.endpoint_agents
            .push((rect, row.endpoint_id, row.agent.pane_id));
    }
}

pub(super) fn render_expanded(
    buffer: &mut Buffer,
    area: Rect,
    agent_view_label: Option<&str>,
    endpoints: &[ClientShellEndpoint],
    active_endpoint_id: &ClientEndpointId,
    config: &ClientShellConfig,
    diagnostics: &super::machine_diagnostics::MachineDiagnostics,
    agent_scroll: &mut usize,
    hits: &mut ShellHitMap,
) {
    if !super::agent_sidebar::render_agent_panel_header(
        buffer,
        area,
        agent_view_label,
        config,
        hits,
    ) {
        return;
    }
    let rows = agent_rows(endpoints, active_endpoint_id, config);
    super::agent_sidebar::render_agent_list(
        buffer,
        area,
        &rows,
        agent_view_label.map(|_| " no matching agents"),
        config,
        agent_scroll,
        hits,
        |row| row.agent.rows.len(),
        |buffer, rect, row, hits| {
            let pin = super::agent_sidebar::chat_pin_rect(rect);
            let endpoint = endpoints
                .iter()
                .find(|endpoint| endpoint.endpoint_id == row.endpoint_id);
            // Another machine's chat carries its badge one cell before the pin column.
            let badge_area = rect.width.saturating_sub(pin.width).saturating_sub(1);
            let badge = endpoint.map_or(0, |endpoint| {
                super::endpoint_sidebar::render_machine_badge(
                    buffer,
                    Rect::new(rect.x, rect.y, badge_area, 1),
                    endpoint,
                    diagnostics,
                    &config.palette,
                    hits,
                )
            });
            let text_rect = Rect::new(
                rect.x,
                rect.y,
                if badge > 0 {
                    badge_area.saturating_sub(badge)
                } else {
                    rect.width.saturating_sub(pin.width)
                },
                rect.height,
            );
            super::agent_sidebar::render_agent_row(buffer, text_rect, &row.agent, config);
            let pinned = endpoint
                .and_then(|endpoint| endpoint.snapshot.as_deref())
                .is_some_and(|snapshot| {
                    snapshot
                        .pinned_tabs
                        .iter()
                        .any(|pin| pin.tab_id == row.agent.tab_id)
                });
            put_text(
                buffer,
                pin.x,
                pin.y,
                pin.width,
                "⚲ ",
                Style::default().fg(if pinned {
                    config.palette.accent
                } else {
                    config.palette.overlay0
                }),
            );
            // Only the toggle intercepts the normal pane-row click.
            hits.endpoint_pins
                .push((pin, pin, row.endpoint_id.clone(), row.agent.tab_id.clone()));
            if row.stale {
                buffer.set_style(
                    rect,
                    Style::default()
                        .fg(config.palette.overlay0)
                        .add_modifier(Modifier::DIM),
                );
            }
            hits.endpoint_agents
                .push((rect, row.endpoint_id.clone(), row.agent.pane_id.clone()));
        },
    );
}

impl ClientShellState {
    pub(super) fn reveal_endpoint_agent(
        &mut self,
        endpoint_id: &ClientEndpointId,
        pane_id: &str,
        body_height: u16,
    ) {
        if body_height == 0 {
            return;
        }
        let rows = agent_rows(&self.endpoints, &self.active_endpoint_id, &self.config);
        let Some(target) = rows
            .iter()
            .position(|row| &row.endpoint_id == endpoint_id && row.agent.pane_id == pane_id)
        else {
            return;
        };
        let heights = rows
            .iter()
            .map(|row| row.agent.rows.len().max(1).min(u16::MAX as usize) as u16)
            .collect::<Vec<_>>();
        let mut gaps = vec![self.config.agents.row_gap; rows.len()];
        if let Some(last) = gaps.last_mut() {
            *last = 0;
        }
        self.agent_scroll = super::scroll::list_scroll_start_to_reveal(
            &heights,
            &gaps,
            body_height,
            self.agent_scroll,
            target,
        );
    }
}

struct EndpointAgentRow {
    endpoint_id: ClientEndpointId,
    machine_label: String,
    stale: bool,
    agent: super::agent_sidebar::AgentRow,
}

fn agent_rows(
    endpoints: &[ClientShellEndpoint],
    active_endpoint_id: &ClientEndpointId,
    config: &ClientShellConfig,
) -> Vec<EndpointAgentRow> {
    let mut rendered_rows = endpoints
        .iter()
        .filter_map(|endpoint| {
            endpoint.snapshot.as_deref().map(|snapshot| {
                snapshot
                    .agents
                    .iter()
                    .filter_map(|agent| {
                        // The machine is the row's badge, not a token: a remote
                        // row names it once and a local row not at all. A pane
                        // running a remote shell still names its own host.
                        super::agent_sidebar::agent_row(snapshot, &agent.pane_id, config, None)
                    })
                    .map(|agent| ((endpoint.endpoint_id.clone(), agent.pane_id.clone()), agent))
                    .collect::<Vec<_>>()
            })
        })
        .flatten()
        .collect::<HashMap<_, _>>();

    super::aggregate_navigation::aggregate_agent_rows(
        endpoints,
        active_endpoint_id,
        config.agent_panel_sort,
    )
    .into_iter()
    .filter(|row| row.agent.visible_in_profile)
    .filter_map(|row| {
        let key = (row.endpoint.endpoint_id.clone(), row.agent.pane_id.clone());
        let mut agent = rendered_rows.remove(&key)?;
        agent.focused &= row.endpoint.endpoint_id == active_endpoint_id;
        Some(EndpointAgentRow {
            endpoint_id: row.endpoint.endpoint_id.clone(),
            machine_label: row.endpoint.label.to_owned(),
            stale: row.endpoint.stale(),
            agent,
        })
    })
    .collect()
}
