use super::*;

#[path = "../shell/overlays.rs"]
mod overlays;
#[path = "../shell/sidebar.rs"]
pub(in crate::client::shell) mod sidebar;
#[path = "../shell/tabs.rs"]
mod tabs;

pub(super) use super::agent_sidebar::{ordered_agent_pane_ids, render_agent_panel_with_overlay};
pub(super) use super::aggregate_navigation::navigator_rows as client_navigator_rows;
pub(super) use overlays::{render_client_overlay, render_context_menu, render_global_menu};
pub(super) use sidebar::{render_collapsed_sidebar, render_sidebar, workspace_entries};
pub(super) use tabs::{render_tab_bar, tab_bar_status_width};
pub(in crate::client::shell) use tabs::tab_status_glyphs;
#[cfg(test)]
pub(in crate::client::shell) use tabs::{session_badge_rect, session_badge_text};

pub(in crate::client::shell) fn render_sidebar_background(
    buffer: &mut Buffer,
    area: Rect,
    palette: &Palette,
) {
    buffer.set_style(area, Style::default().bg(palette.sidebar_bg));
    let separator_x = area.right().saturating_sub(1);
    for y in area.y..area.bottom() {
        if let Some(cell) = buffer.cell_mut((separator_x, y)) {
            cell.set_symbol("│");
            cell.set_style(Style::default().fg(palette.surface_dim));
        }
    }
}

pub(super) fn render_mode_bar(
    buffer: &mut Buffer,
    pane_area: Rect,
    mode: ClientShellMode,
    copy_mode: Option<&ClientCopyModeState>,
    endpoint_error: Option<&str>,
    update_available: bool,
    keybinds: &LiveKeybindConfig,
    palette: &Palette,
) -> Option<Rect> {
    if (mode == ClientShellMode::Terminal && endpoint_error.is_none()) || pane_area.is_empty() {
        return None;
    }

    let bar = Rect::new(
        pane_area.x,
        pane_area.y + pane_area.height.saturating_sub(1),
        pane_area.width,
        1,
    );
    let base = Style::default().fg(palette.overlay0).bg(palette.panel_bg);
    for x in bar.x..bar.x + bar.width {
        buffer[(x, bar.y)].set_symbol(" ").set_style(base);
    }

    let key = Style::default()
        .fg(palette.accent)
        .bg(palette.panel_bg)
        .add_modifier(Modifier::BOLD);
    let mode_style = Style::default()
        .fg(match palette.panel_bg {
            ratatui::style::Color::Reset => palette.surface_dim,
            color => color,
        })
        .bg(if mode == ClientShellMode::Resize {
            palette.mauve
        } else {
            palette.accent
        })
        .add_modifier(Modifier::BOLD);
    let prefix = crate::config::format_key_combo(keybinds.prefix);
    let prefix_rhs = |bindings: &crate::config::ActionKeybinds| {
        bindings
            .prefix_rhs_label()
            .unwrap_or_else(|| "unset".to_owned())
    };

    let mut segments = Vec::<(String, Style)>::new();
    if let Some(error) = endpoint_error {
        segments.extend([
            (" ERROR ".to_owned(), mode_style),
            (format!(" {error}"), base),
        ]);
    } else {
        match mode {
            ClientShellMode::Prefix => {
                segments.extend([
                    (" PREFIX ".to_owned(), mode_style),
                    (" ".to_owned(), base),
                    ("esc".to_owned(), key),
                    (" cancel  ".to_owned(), base),
                    (prefix, key),
                    (" send prefix  ".to_owned(), base),
                    (prefix_rhs(&keybinds.keybinds.workspace_picker), key),
                    (" workspace nav  ".to_owned(), base),
                    (prefix_rhs(&keybinds.keybinds.help), key),
                    (" keybinds".to_owned(), base),
                ]);
            }
            ClientShellMode::Navigate => {
                segments.extend([
                    (" NAVIGATE ".to_owned(), mode_style),
                    (" esc back  ".to_owned(), base),
                    ("↑/↓".to_owned(), key),
                    (" workspace  ".to_owned(), base),
                    ("tab".to_owned(), key),
                    (" pane  ".to_owned(), base),
                    (prefix_rhs(&keybinds.keybinds.help), key),
                    (" keybinds".to_owned(), base),
                ]);
            }
            ClientShellMode::Resize => {
                segments.extend([
                    (" RESIZE ".to_owned(), mode_style),
                    ("  ".to_owned(), base),
                    ("h/l".to_owned(), key),
                    (" width  ".to_owned(), base),
                    ("j/k".to_owned(), key),
                    (" height  ".to_owned(), base),
                    ("esc".to_owned(), key),
                    (" done".to_owned(), base),
                ]);
            }
            ClientShellMode::Copy => {
                let copy_mode = copy_mode?;
                if let Some(prompt) = copy_mode.search_prompt.as_ref() {
                    let marker = match prompt.direction {
                        crate::api::schema::PaneCopySearchDirection::Forward => "/",
                        crate::api::schema::PaneCopySearchDirection::Backward => "?",
                    };
                    buffer.set_stringn(bar.x, bar.y, " COPY ", usize::from(bar.width), mode_style);
                    let prefix = 8.min(bar.width);
                    if bar.width >= 8 {
                        buffer.set_string(bar.x + 7, bar.y, marker, key);
                    }
                    let footer = "  enter search  esc cancel";
                    let footer_width = if bar.width >= 50 {
                        footer.len() as u16
                    } else {
                        0
                    };
                    let field = Rect::new(
                        bar.x + prefix,
                        bar.y,
                        bar.width.saturating_sub(prefix + footer_width),
                        1,
                    );
                    if let Some(cursor) = text_editor::render(
                        buffer,
                        field,
                        &prompt.query,
                        Style::default().fg(palette.text).bg(palette.panel_bg),
                    ) {
                        buffer[(cursor.x, cursor.y)]
                            .set_style(Style::default().fg(palette.panel_bg).bg(palette.text));
                    }
                    if footer_width > 0 {
                        buffer.set_string(bar.right() - footer_width, bar.y, footer, base);
                    }
                    return Some(bar);
                } else {
                    let select = if copy_mode.selection.is_some() {
                        "selecting"
                    } else {
                        "select"
                    };
                    let match_status = copy_mode
                        .search_current_global
                        .map(|current| format!(" {}/{}", current + 1, copy_mode.search_total))
                        .or_else(|| (!copy_mode.search_query.is_empty()).then(|| " 0/0".to_owned()))
                        .unwrap_or_default();
                    let (exit_keys, exit_label) =
                        if copy_mode.search_query.is_empty() && copy_mode.selection.is_none() {
                            ("q/esc", " exit")
                        } else {
                            ("esc", " clear  q exit")
                        };
                    segments.extend([
                        (" COPY ".to_owned(), mode_style),
                        (" ".to_owned(), base),
                        ("h/j/k/l w/b/e { }".to_owned(), key),
                        (" move  ".to_owned(), base),
                        ("/ ?".to_owned(), key),
                        (" search  ".to_owned(), base),
                        ("n/N".to_owned(), key),
                        (format!(" repeat{match_status}  "), base),
                        ("v/space".to_owned(), key),
                        (format!(" {select}  "), base),
                        ("y/enter".to_owned(), key),
                        (" copy  ".to_owned(), base),
                        (exit_keys.to_owned(), key),
                        (exit_label.to_owned(), base),
                    ]);
                }
            }
            ClientShellMode::Terminal => unreachable!(),
        }
    }

    let mut x = bar.x;
    let end = bar.x + bar.width;
    for (text, style) in segments {
        if x >= end {
            break;
        }
        let remaining = end - x;
        buffer.set_stringn(x, bar.y, &text, usize::from(remaining), style);
        x = x.saturating_add(
            u16::try_from(UnicodeWidthStr::width(text.as_str()))
                .unwrap_or(u16::MAX)
                .min(remaining),
        );
    }
    if update_available && mode == ClientShellMode::Navigate {
        let width = 13.min(bar.width);
        let area = Rect::new(bar.right().saturating_sub(width), bar.y, width, 1);
        buffer.set_style(area, Style::default().bg(palette.panel_bg));
        put_right_text(
            buffer,
            area,
            area.y,
            " update ready",
            Style::default()
                .fg(palette.accent)
                .bg(palette.panel_bg)
                .add_modifier(Modifier::BOLD),
        );
    }
    Some(bar)
}

pub(super) struct ShellRenderState<'a> {
    /// Factory overlay document; `None` when `[ui.factory]` is off or no document arrived.
    pub(super) factory_overlay: Option<&'a crate::factory_overlay::FactoryOverlay>,
    pub(super) machine_diagnostics: &'a super::machine_diagnostics::MachineDiagnostics,
    pub(super) endpoints: &'a [ClientShellEndpoint],
    pub(super) active_endpoint_id: &'a ClientEndpointId,
    pub(super) collapsed_endpoints: &'a HashSet<ClientEndpointId>,
    pub(super) collapsed_groups: &'a HashSet<String>,
    pub(super) tree: &'a super::tree::ClientTreeChrome,
    pub(super) remote_collapsed_groups: &'a HashMap<ClientEndpointId, HashSet<String>>,
    pub(super) workspace_scroll: &'a mut usize,
    pub(super) agent_scroll: &'a mut usize,
    pub(super) tab_scroll: &'a mut usize,
    pub(super) reveal_focused_workspace: &'a mut bool,
    pub(super) reveal_focused_tab: &'a mut bool,
    pub(super) sidebar_collapsed: bool,
    pub(super) sidebar_section_split: f32,
    pub(super) tab_drag_insert_index: Option<usize>,
    pub(super) selected_workspace_id: Option<&'a WorkspaceNavigationTarget>,
    pub(super) reveal_navigation_workspace: &'a mut bool,
    pub(super) dragged_workspace_id: Option<&'a str>,
    pub(super) workspace_drop_indicator_row: Option<u16>,
    /// Local's own snapshot, tree chrome and overlay. With `machines = "sections"`
    /// the Local sidebar draws from these even while a machine owns the main area.
    pub(super) local_snapshot: Option<&'a ClientShellSnapshot>,
    pub(super) local_tree: &'a super::tree::ClientTreeChrome,
    pub(super) local_factory_overlay: Option<&'a crate::factory_overlay::FactoryOverlay>,
    /// Rows the tree-view sidebar leaves free above its footer for machine sections.
    pub(super) machine_rows: u16,
}

static LOCAL_ENDPOINT: ClientEndpointId = ClientEndpointId::Local;

/// Expanded or collapsed sidebar for any endpoint count.
///
/// One endpoint, or `machines = "list"`: unchanged upstream paths. Otherwise
/// Local renders exactly as it would alone, and each enabled machine adds a
/// section under it (`machines = "sections"`) or nothing (`"off"`). When Local
/// itself has no snapshot the flat machines list keeps machines reachable.
pub(super) fn render_sidebar_area(
    buffer: &mut Buffer,
    area: Rect,
    active_snapshot: Option<&ClientShellSnapshot>,
    config: &ClientShellConfig,
    state: &mut ShellRenderState<'_>,
    hits: &mut ShellHitMap,
) {
    let local_active = state.active_endpoint_id.is_local();
    let selected = state.selected_workspace_id;
    let additive = state.endpoints.len() > 1
        && config.machines != crate::config::SidebarMachinesConfig::List;
    let local_snapshot = if !additive || local_active {
        active_snapshot
    } else {
        state.local_snapshot
    };
    let Some(snapshot) = local_snapshot.filter(|_| state.endpoints.len() == 1 || additive) else {
        if state.sidebar_collapsed {
            super::endpoint_sidebar::render_collapsed(buffer, area, config, state, hits);
        } else {
            super::endpoint_sidebar::render_expanded(
                buffer,
                area,
                active_snapshot,
                config,
                state,
                hits,
            );
        }
        return;
    };
    if !local_active {
        // A machine owns the main area; the Local rows still show Local.
        state.active_endpoint_id = &LOCAL_ENDPOINT;
        state.tree = state.local_tree;
        state.factory_overlay = state.local_factory_overlay;
        state.selected_workspace_id = state
            .selected_workspace_id
            .filter(|target| target.endpoint_id.is_local());
    }
    let rows = if additive
        && config.machines == crate::config::SidebarMachinesConfig::Sections
        && !state.sidebar_collapsed
    {
        super::machine_sections::fit_rows(
            super::machine_sections::section_rows(state.endpoints, state.collapsed_endpoints),
            area.height,
        )
    } else {
        Vec::new()
    };
    if state.sidebar_collapsed {
        render_collapsed_sidebar(
            buffer,
            area,
            snapshot,
            config,
            state
                .selected_workspace_id
                .map(|target| target.workspace_id.as_str()),
            hits,
        );
    } else if rows.is_empty() {
        render_sidebar(buffer, area, snapshot, config, state, hits);
    } else {
        let strip_height = (rows.len().min(usize::from(u16::MAX)) as u16).min(area.height);
        let strip_width = area.width.saturating_sub(1);
        let (local_area, strip) = if super::tree::tree_view_active(config) {
            // The strip sits between the agents tree and the footer row.
            state.machine_rows = strip_height;
            let footer = u16::from(area.height >= 2);
            (
                area,
                Rect::new(
                    area.x,
                    area.bottom()
                        .saturating_sub(footer)
                        .saturating_sub(strip_height),
                    strip_width,
                    strip_height,
                ),
            )
        } else {
            let local_height = area.height.saturating_sub(strip_height);
            render_sidebar_background(
                buffer,
                Rect::new(area.x, area.y + local_height, area.width, strip_height),
                &config.palette,
            );
            (
                Rect::new(area.x, area.y, area.width, local_height),
                Rect::new(area.x, area.y + local_height, strip_width, strip_height),
            )
        };
        render_sidebar(buffer, local_area, snapshot, config, state, hits);
        state.machine_rows = 0;
        state.selected_workspace_id = selected;
        super::machine_sections::render(buffer, strip, &rows, config, state, hits);
        if !local_active {
            hits.local_sidebar = Rect::new(
                area.x,
                area.y,
                strip_width,
                strip.y.saturating_sub(area.y),
            );
        }
        return;
    }
    if !local_active {
        hits.local_sidebar = Rect::new(area.x, area.y, area.width.saturating_sub(1), area.height);
    }
}

pub(super) fn render_shell(
    buffer: &mut Buffer,
    layout: ClientShellLayout,
    snapshot: &ClientShellSnapshot,
    config: &ClientShellConfig,
    mut state: ShellRenderState<'_>,
) -> ShellHitMap {
    let mut hits = ShellHitMap::default();
    if layout.mobile_header.height > 0 {
        super::mobile::render_mobile_header(
            buffer,
            layout.mobile_header,
            snapshot,
            config,
            state.active_endpoint_id,
            &mut hits,
        );
    }
    if layout.sidebar.width > 0 {
        render_sidebar_area(
            buffer,
            layout.sidebar,
            Some(snapshot),
            config,
            &mut state,
            &mut hits,
        );
    }
    if layout.tab_bar.height > 0 {
        render_tab_bar(
            buffer,
            layout.tab_bar,
            snapshot,
            config,
            state.factory_overlay,
            state.tab_scroll,
            state.reveal_focused_tab,
            state.tab_drag_insert_index,
            &mut hits,
        );
    }
    if !config.mouse_capture {
        hits.sidebar_divider = Rect::default();
        hits.sidebar_section_divider = Rect::default();
        hits.workspace_scrollbar = Rect::default();
        hits.agent_scrollbar = Rect::default();
        hits.agent_sort_toggle = Rect::default();
        hits.new_workspace = Rect::default();
        hits.machines.clear();
        hits.workspaces.clear();
        hits.agents.clear();
        hits.endpoint_agents.clear();
        hits.tab_scroll_left = Rect::default();
        hits.tab_scroll_right = Rect::default();
        hits.new_tab = Rect::default();
        hits.pane_splits.clear();
    }
    hits
}

pub(super) fn put_right_text(buffer: &mut Buffer, area: Rect, y: u16, text: &str, style: Style) {
    let width = display_width(text).min(area.width);
    put_text(
        buffer,
        area.right().saturating_sub(width),
        y,
        width,
        text,
        style,
    );
}

pub(super) fn put_segment(
    buffer: &mut Buffer,
    x: u16,
    y: u16,
    right: u16,
    text: &str,
    style: Style,
) -> u16 {
    let width = display_width(text).min(right.saturating_sub(x));
    put_text(buffer, x, y, width, text, style);
    x.saturating_add(width)
}

pub(super) fn put_text(buffer: &mut Buffer, x: u16, y: u16, width: u16, text: &str, style: Style) {
    if width == 0 || y >= buffer.area.bottom() || x >= buffer.area.right() {
        return;
    }
    buffer.set_stringn(x, y, text, width as usize, style);
}

pub(super) fn display_width(text: &str) -> u16 {
    UnicodeWidthStr::width(text).min(u16::MAX as usize) as u16
}
