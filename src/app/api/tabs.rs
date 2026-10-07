use std::path::PathBuf;

use crate::api::schema::{
    EventData, EventEnvelope, EventKind, ResponseResult, TabCreateParams, TabListParams,
    TabMoveParams, TabPinMoveParams, TabRenameParams, TabSetPinnedParams, TabSetRoleParams,
    TabTarget,
};
use crate::app::{App, Mode};

use super::responses::{encode_error, encode_success};

impl App {
    pub(super) fn handle_tab_list(&mut self, id: String, params: TabListParams) -> String {
        let tabs = if let Some(workspace_id) = params.workspace_id {
            let Some(ws_idx) = self.parse_workspace_id(&workspace_id) else {
                return workspace_not_found(id, &workspace_id);
            };
            let Some(_) = self.state.workspaces.get(ws_idx) else {
                return workspace_not_found(id, &workspace_id);
            };
            self.tab_list_info(ws_idx)
        } else {
            let mut tabs = Vec::new();
            for (ws_idx, ws) in self.state.workspaces.iter().enumerate() {
                for tab_idx in 0..ws.tabs.len() {
                    if let Some(tab) = self.tab_info(ws_idx, tab_idx) {
                        tabs.push(tab);
                    }
                }
            }
            tabs
        };

        encode_success(id, ResponseResult::TabList { tabs })
    }

    pub(super) fn handle_tab_get(&mut self, id: String, target: TabTarget) -> String {
        let Some((ws_idx, tab_idx)) = self.parse_tab_id(&target.tab_id) else {
            return tab_not_found(id, &target.tab_id);
        };
        let Some(tab) = self.tab_info(ws_idx, tab_idx) else {
            return tab_not_found(id, &target.tab_id);
        };

        encode_success(id, ResponseResult::TabInfo { tab })
    }

    pub(super) fn handle_tab_create(&mut self, id: String, params: TabCreateParams) -> String {
        let TabCreateParams {
            workspace_id,
            cwd,
            focus,
            label,
            env,
        } = params;
        let ws_idx = if let Some(workspace_id) = workspace_id {
            let Some(ws_idx) = self.parse_workspace_id(&workspace_id) else {
                return workspace_not_found(id, &workspace_id);
            };
            ws_idx
        } else if let Some(active) = self.state.active {
            active
        } else {
            return encode_error(id, "workspace_not_found", "no active workspace");
        };
        let cwd = cwd.map(PathBuf::from).unwrap_or_else(|| {
            self.resolve_new_terminal_cwd(self.focused_pane_cwd_in_workspace(ws_idx))
        });
        let (rows, cols) = self.state.estimate_pane_size();
        let default_shell = self.state.default_shell.clone();
        let scrollback_limit_bytes = self.state.pane_scrollback_limit_bytes;
        let host_terminal_theme = self.state.host_terminal_theme;
        let host_terminal_appearance = self.state.host_terminal_appearance;
        let extra_env = match super::env::normalize_launch_env(env) {
            Ok(env) => env,
            Err((code, message)) => return encode_error(id, &code, message),
        };
        let result = self
            .state
            .workspaces
            .get_mut(ws_idx)
            .ok_or_else(|| std::io::Error::other("workspace disappeared"))
            .and_then(|ws| {
                ws.create_tab(
                    rows,
                    cols,
                    cwd,
                    scrollback_limit_bytes,
                    host_terminal_theme,
                    host_terminal_appearance,
                    crate::pane::PaneShellConfig::new(&default_shell, self.state.shell_mode),
                    extra_env,
                )
            });
        match result {
            Ok((tab_idx, terminal, runtime)) => {
                self.terminal_runtimes.insert(terminal.id.clone(), runtime);
                self.state.terminals.insert(terminal.id.clone(), terminal);
                self.state.remove_alias_shadowed_by_new_pane(
                    self.state.workspaces[ws_idx].tabs[tab_idx].root_pane,
                );
                if let Some(label) = label {
                    let workspace_id = self.state.workspaces[ws_idx].id.clone();
                    let tab_id = self.public_tab_id(ws_idx, tab_idx).unwrap_or_else(|| {
                        crate::workspace::public_tab_id_for_number(&workspace_id, tab_idx + 1)
                    });
                    if let Some(tab) = self
                        .state
                        .workspaces
                        .get_mut(ws_idx)
                        .and_then(|ws| ws.tabs.get_mut(tab_idx))
                    {
                        tab.set_custom_name(label);
                        crate::logging::tab_renamed(&workspace_id, &tab_id);
                    }
                }
                if focus {
                    self.state.switch_workspace_tab(ws_idx, tab_idx);
                    self.state.mode = Mode::Terminal;
                }
                self.schedule_session_save();
                self.emit_tab_created_events(ws_idx, tab_idx);
                encode_success(
                    id,
                    self.tab_created_result(ws_idx, tab_idx)
                        .expect("new tab should produce a complete create response"),
                )
            }
            Err(err) => encode_error(id, "tab_create_failed", err.to_string()),
        }
    }

    /// A pinned space keeps a live tab: when a close is about to remove the
    /// workspace's last tab, grow a fresh shell tab first so the workspace
    /// (and its pinned header row) survives. Returns true when a replacement
    /// tab was created.
    pub(crate) fn respawn_tab_for_pinned_workspace(&mut self, ws_idx: usize) -> bool {
        let pinned = self
            .state
            .workspaces
            .get(ws_idx)
            .is_some_and(|ws| self.state.tree_pinned_spaces.contains(&ws.id));
        if !pinned {
            return false;
        }
        let cwd = self.resolve_new_terminal_cwd(self.focused_pane_cwd_in_workspace(ws_idx));
        let (rows, cols) = self.state.estimate_pane_size();
        let default_shell = self.state.default_shell.clone();
        let scrollback_limit_bytes = self.state.pane_scrollback_limit_bytes;
        let host_terminal_theme = self.state.host_terminal_theme;
        let host_terminal_appearance = self.state.host_terminal_appearance;
        let result = self
            .state
            .workspaces
            .get_mut(ws_idx)
            .ok_or_else(|| std::io::Error::other("workspace disappeared"))
            .and_then(|ws| {
                ws.create_tab(
                    rows,
                    cols,
                    cwd,
                    scrollback_limit_bytes,
                    host_terminal_theme,
                    host_terminal_appearance,
                    crate::pane::PaneShellConfig::new(&default_shell, self.state.shell_mode),
                    Vec::new(),
                )
            });
        match result {
            Ok((tab_idx, terminal, runtime)) => {
                self.terminal_runtimes.insert(terminal.id.clone(), runtime);
                self.state.terminals.insert(terminal.id.clone(), terminal);
                self.state.remove_alias_shadowed_by_new_pane(
                    self.state.workspaces[ws_idx].tabs[tab_idx].root_pane,
                );
                self.schedule_session_save();
                self.emit_tab_created_events(ws_idx, tab_idx);
                true
            }
            Err(err) => {
                tracing::warn!(
                    err = %err,
                    "failed to grow replacement tab for pinned space; workspace will close"
                );
                false
            }
        }
    }

    pub(super) fn handle_tab_focus(&mut self, id: String, target: TabTarget) -> String {
        let Some((ws_idx, tab_idx)) = self.parse_tab_id(&target.tab_id) else {
            return tab_not_found(id, &target.tab_id);
        };
        self.state.switch_workspace_tab(ws_idx, tab_idx);
        let Some(tab) = self.tab_info(ws_idx, tab_idx) else {
            return tab_not_found(id, &target.tab_id);
        };

        encode_success(id, ResponseResult::TabInfo { tab })
    }

    pub(super) fn handle_tab_rename(&mut self, id: String, params: TabRenameParams) -> String {
        let Some((ws_idx, tab_idx)) = self.parse_tab_id(&params.tab_id) else {
            return tab_not_found(id, &params.tab_id);
        };
        let old_rank = self.priority_tab_rank(ws_idx, tab_idx).value;
        let workspace_id = self.state.workspaces[ws_idx].id.clone();
        let tab_id = self.public_tab_id(ws_idx, tab_idx).unwrap_or_else(|| {
            crate::workspace::public_tab_id_for_number(&workspace_id, tab_idx + 1)
        });
        let Some(tab) = self
            .state
            .workspaces
            .get_mut(ws_idx)
            .and_then(|ws| ws.tabs.get_mut(tab_idx))
        else {
            return tab_not_found(id, &params.tab_id);
        };
        tab.set_custom_name(params.label.clone());
        crate::logging::tab_renamed(&workspace_id, &tab_id);
        if self.priority_tab_rank(ws_idx, tab_idx).value != old_rank {
            self.state
                .priority_renamed_pins(std::slice::from_ref(&tab_id));
        }
        self.schedule_session_save();
        self.emit_event(EventEnvelope {
            event: EventKind::TabRenamed,
            data: EventData::TabRenamed {
                tab_id: self.public_tab_id(ws_idx, tab_idx).unwrap(),
                workspace_id: self.public_workspace_id(ws_idx),
                label: params.label,
            },
        });
        let Some(tab) = self.tab_info(ws_idx, tab_idx) else {
            return tab_not_found(id, &params.tab_id);
        };

        encode_success(id, ResponseResult::TabInfo { tab })
    }

    /// Pin or unpin a chat in the shared pinned order. The pin lives in
    /// session state so every client (and Cmd+1..9) sees the same order.
    pub(super) fn handle_tab_set_pinned(
        &mut self,
        id: String,
        params: TabSetPinnedParams,
    ) -> String {
        let Some((ws_idx, tab_idx)) = self.parse_tab_id(&params.tab_id) else {
            return tab_not_found(id, &params.tab_id);
        };
        let Some(tab_id) = self.public_tab_id(ws_idx, tab_idx) else {
            return tab_not_found(id, &params.tab_id);
        };
        let before = self.state.pinned_tabs.clone();
        if params.pinned {
            let priority = params.priority.unwrap_or_else(|| {
                self.state
                    .pinned_tabs
                    .iter()
                    .find(|pin| pin.tab_id == tab_id)
                    .map(|pin| pin.priority)
                    .unwrap_or(0)
            });
            if params.priority.is_some() || !self.state.is_tab_pinned(&tab_id) {
                self.state.pin_tab(tab_id, priority);
            }
        } else {
            self.state.unpin_tab(&tab_id);
        }
        if self.state.pinned_tabs != before {
            self.state.mark_session_dirty();
            self.schedule_session_save();
            // API clients (Herdr Shell) refresh their snapshot on workspace
            // events; the pin order is carried by each tab's `pin_index`.
            let workspace = self.workspace_info(ws_idx);
            self.emit_event(EventEnvelope {
                event: EventKind::WorkspaceUpdated,
                data: EventData::WorkspaceUpdated { workspace },
            });
        }
        let Some(tab) = self.tab_info(ws_idx, tab_idx) else {
            return tab_not_found(id, &params.tab_id);
        };
        encode_success(id, ResponseResult::TabInfo { tab })
    }

    pub(super) fn handle_tab_set_role(&mut self, id: String, params: TabSetRoleParams) -> String {
        let Some((ws_idx, tab_idx)) = self.parse_tab_id(&params.tab_id) else {
            return tab_not_found(id, &params.tab_id);
        };
        let Some(tab_id) = self.public_tab_id(ws_idx, tab_idx) else {
            return tab_not_found(id, &params.tab_id);
        };
        if self.state.set_tab_role(&tab_id, params.role) {
            self.state.mark_session_dirty();
            self.schedule_session_save();
            let workspace = self.workspace_info(ws_idx);
            self.emit_event(EventEnvelope {
                event: EventKind::WorkspaceUpdated,
                data: EventData::WorkspaceUpdated { workspace },
            });
            self.emit_event(EventEnvelope {
                event: EventKind::TabPinMoved,
                data: EventData::TabPinMoved {
                    tab_id: tab_id.clone(),
                    workspace_id: self.public_workspace_id(ws_idx),
                    pin_index: self.state.pinned_tab_index(&tab_id).unwrap_or(0),
                    pinned_tab_ids: self
                        .state
                        .pinned_tabs
                        .iter()
                        .map(|pin| pin.tab_id.clone())
                        .collect(),
                },
            });
        }
        let Some(tab) = self.tab_info(ws_idx, tab_idx) else {
            return tab_not_found(id, &params.tab_id);
        };
        encode_success(id, ResponseResult::TabInfo { tab })
    }

    /// Move a pinned chat within the shared pin order. Every client draws the
    /// pinned section and resolves Cmd+1..9 from this order, so a drag in any
    /// client lands here and reaches the others through the next snapshot.
    pub(super) fn handle_tab_pin_move(&mut self, id: String, params: TabPinMoveParams) -> String {
        let Some((ws_idx, tab_idx)) = self.parse_tab_id(&params.tab_id) else {
            return tab_not_found(id, &params.tab_id);
        };
        let Some(tab_id) = self.public_tab_id(ws_idx, tab_idx) else {
            return tab_not_found(id, &params.tab_id);
        };
        if !self.state.is_tab_pinned(&tab_id) {
            return encode_error(id, "tab_not_pinned", format!("tab {tab_id} is not pinned"));
        }
        if params.pin_index >= self.state.pinned_tabs.len() {
            return encode_error(
                id,
                "pin_index_out_of_bounds",
                format!(
                    "pin_index {} is out of bounds for {} pins",
                    params.pin_index,
                    self.state.pinned_tabs.len()
                ),
            );
        }
        if self
            .state
            .pin_role_range(&tab_id)
            .is_some_and(|range| !range.contains(&params.pin_index))
        {
            return encode_error(
                id,
                "pin_index_outside_role_block",
                "pin_index is outside this tab's role block",
            );
        }
        let before = self.state.pinned_tabs.clone();
        self.state.move_pinned_tab(&tab_id, params.pin_index);
        if self.state.pinned_tabs != before {
            self.state.mark_session_dirty();
            self.schedule_session_save();
            self.emit_event(EventEnvelope {
                event: EventKind::TabPinMoved,
                data: EventData::TabPinMoved {
                    tab_id: tab_id.clone(),
                    workspace_id: self.public_workspace_id(ws_idx),
                    pin_index: self.state.pinned_tab_index(&tab_id).unwrap_or(0),
                    pinned_tab_ids: self
                        .state
                        .pinned_tabs
                        .iter()
                        .map(|pin| pin.tab_id.clone())
                        .collect(),
                },
            });
            // Herdr Shell refreshes on workspace events, as for a pin toggle.
            let workspace = self.workspace_info(ws_idx);
            self.emit_event(EventEnvelope {
                event: EventKind::WorkspaceUpdated,
                data: EventData::WorkspaceUpdated { workspace },
            });
        }
        let Some(tab) = self.tab_info(ws_idx, tab_idx) else {
            return tab_not_found(id, &params.tab_id);
        };
        encode_success(id, ResponseResult::TabInfo { tab })
    }

    pub(super) fn handle_tab_move(&mut self, id: String, params: TabMoveParams) -> String {
        let Some((ws_idx, tab_idx)) = self.parse_tab_id(&params.tab_id) else {
            return tab_not_found(id, &params.tab_id);
        };
        let Some(ws) = self.state.workspaces.get(ws_idx) else {
            return tab_not_found(id, &params.tab_id);
        };
        if params.insert_index > ws.tabs.len() {
            return encode_error(
                id,
                "tab_move_failed",
                format!("insert_index {} is out of bounds", params.insert_index),
            );
        }

        let tab_id = self
            .public_tab_id(ws_idx, tab_idx)
            .unwrap_or_else(|| crate::workspace::public_tab_id_for_number(&ws.id, tab_idx + 1));
        let workspace_id = self.public_workspace_id(ws_idx);
        let insert_index = params.insert_index;
        let moved = self
            .state
            .workspaces
            .get_mut(ws_idx)
            .is_some_and(|ws| ws.move_tab(tab_idx, insert_index));
        let tabs = self.tab_list_info(ws_idx);
        if moved {
            self.schedule_session_save();
            self.emit_event(EventEnvelope {
                event: EventKind::TabMoved,
                data: EventData::TabMoved {
                    tab_id,
                    workspace_id,
                    insert_index,
                    tabs: tabs.clone(),
                },
            });
        }

        encode_success(id, ResponseResult::TabList { tabs })
    }

    /// Capture a live successor before removing a focused pin. Role-bearing
    /// pins and content pins have independent ordering, with cross-list fallback.
    pub(super) fn pinned_close_successor(&self, ws_idx: usize, tab_idx: usize) -> Option<String> {
        if self.state.active != Some(ws_idx)
            || self.state.workspaces.get(ws_idx)?.active_tab != tab_idx
        {
            return None;
        }
        let tab_id = self.public_tab_id(ws_idx, tab_idx)?;
        let position = self.state.pinned_tab_index(&tab_id)?;
        let pins = &self.state.pinned_tabs;
        let has_role = pins[position].role.is_some();
        let live = |pin: &&crate::app::state::PinnedTab| {
            pin.tab_id != tab_id && self.parse_tab_id(&pin.tab_id).is_some()
        };
        pins[position + 1..]
            .iter()
            .filter(|pin| pin.role.is_some() == has_role)
            .find(live)
            .or_else(|| {
                pins[..position]
                    .iter()
                    .rev()
                    .filter(|pin| pin.role.is_some() == has_role)
                    .find(live)
            })
            .or_else(|| {
                pins.iter()
                    .filter(|pin| pin.role.is_some() != has_role)
                    .find(live)
            })
            .map(|pin| pin.tab_id.clone())
    }

    pub(super) fn focus_after_pinned_close(&mut self, successor: Option<String>) {
        if let Some((ws_idx, tab_idx)) = successor.as_deref().and_then(|id| self.parse_tab_id(id)) {
            self.state.switch_workspace_tab(ws_idx, tab_idx);
        }
    }

    pub(super) fn handle_tab_close(&mut self, id: String, target: TabTarget) -> String {
        let Some((ws_idx, tab_idx)) = self.parse_tab_id(&target.tab_id) else {
            return tab_not_found(id, &target.tab_id);
        };
        let Some(tab_id) = self.public_tab_id(ws_idx, tab_idx) else {
            return tab_not_found(id, &target.tab_id);
        };
        let workspace_id = self.public_workspace_id(ws_idx);
        let successor = self.pinned_close_successor(ws_idx, tab_idx);
        if self
            .state
            .workspaces
            .get(ws_idx)
            .is_some_and(|ws| ws.tabs.len() <= 1)
        {
            // A pinned space keeps a live tab instead of closing with its
            // last one.
            self.respawn_tab_for_pinned_workspace(ws_idx);
        }
        let Some(ws) = self.state.workspaces.get(ws_idx) else {
            return tab_not_found(id, &target.tab_id);
        };
        let closes_workspace = ws.tabs.len() <= 1;
        let terminal_ids = self.state.terminal_ids_for_tab(ws_idx, tab_idx);
        let pane_ids = ws
            .tabs
            .get(tab_idx)
            .map(|tab| tab.layout.pane_ids())
            .unwrap_or_default();

        if closes_workspace {
            if self.state.confirm_implicit_worktree_group_close(ws_idx) {
                return encode_error(
                    id,
                    "confirmation_required",
                    "closing this tab would close a worktree group",
                );
            }
            self.state.unpin_tab(&tab_id);
            let workspace = self.workspace_info(ws_idx);
            self.state.selected = ws_idx;
            self.state.close_selected_workspace();
            self.focus_after_pinned_close(successor);
            self.state.remove_plugin_pane_records(pane_ids);
            self.shutdown_detached_terminal_runtimes();
            self.emit_event(EventEnvelope {
                event: EventKind::TabClosed,
                data: EventData::TabClosed {
                    tab_id,
                    workspace_id: workspace_id.clone(),
                },
            });
            self.emit_event(EventEnvelope {
                event: EventKind::WorkspaceClosed,
                data: EventData::WorkspaceClosed {
                    workspace_id,
                    workspace: Some(workspace),
                },
            });
            return encode_success(id, ResponseResult::Ok {});
        }

        let Some(ws) = self.state.workspaces.get_mut(ws_idx) else {
            return tab_not_found(id, &target.tab_id);
        };
        if !ws.close_tab(tab_idx) {
            return encode_error(
                id,
                "tab_close_failed",
                format!("tab {} could not be closed", target.tab_id),
            );
        }
        self.state.unpin_tab(&tab_id);
        self.focus_after_pinned_close(successor);
        self.state.prune_desks();
        self.state.mark_session_dirty();
        self.state.remove_plugin_pane_records(pane_ids);
        self.state.remove_unattached_terminal_ids(terminal_ids);
        self.shutdown_detached_terminal_runtimes();
        self.schedule_session_save();
        self.emit_event(EventEnvelope {
            event: EventKind::TabClosed,
            data: EventData::TabClosed {
                tab_id,
                workspace_id,
            },
        });

        encode_success(id, ResponseResult::Ok {})
    }

    fn tab_list_info(&self, ws_idx: usize) -> Vec<crate::api::schema::TabInfo> {
        self.state
            .workspaces
            .get(ws_idx)
            .map(|ws| {
                (0..ws.tabs.len())
                    .filter_map(|idx| self.tab_info(ws_idx, idx))
                    .collect()
            })
            .unwrap_or_default()
    }
}

fn workspace_not_found(id: String, workspace_id: &str) -> String {
    encode_error(
        id,
        "workspace_not_found",
        format!("workspace {workspace_id} not found"),
    )
}

fn tab_not_found(id: String, tab_id: &str) -> String {
    encode_error(id, "tab_not_found", format!("tab {tab_id} not found"))
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{exiting_test_command, shutdown_test_runtimes};
    use super::*;
    use crate::{
        api::schema::SuccessResponse,
        config::{Config, ShellModeConfig},
        workspace::Workspace,
    };

    /// The API close boundary runs without PTYs; the table covers pin-order
    /// selection across spaces, role lists, stale pins, and legacy fallback.
    #[test]
    fn api_close_focused_pin_selects_live_neighbor() {
        use crate::api::schema::{PaneTarget, TabRole};
        use crate::app::state::PinnedTab;

        for pane_close in [false, true] {
            for (closing, pins, expected) in [
                (1, vec![(0, false), (1, false), (2, false)], 2),
                (2, vec![(0, false), (1, false), (2, false)], 1),
                (1, vec![(0, true), (1, true), (2, true)], 2),
                (1, vec![(0, false), (1, true), (2, false)], 0),
                (1, vec![(0, false), (1, false), (99, false), (2, false)], 2),
                (1, vec![(0, false), (2, false)], 4),
                (1, vec![(1, false)], 4),
            ] {
                for last_tab in [false, true] {
                    let (_api_tx, api_rx) = tokio::sync::mpsc::unbounded_channel();
                    let mut app = App::new(
                        &Config::default(),
                        crate::app::AppPolicy::TEST,
                        None,
                        api_rx,
                        crate::api::EventHub::default(),
                    );
                    app.state.workspaces = (0..3)
                        .map(|i| Workspace::test_new(&format!("space-{i}")))
                        .collect();
                    if !last_tab {
                        for ws in &mut app.state.workspaces {
                            ws.test_add_tab(Some("unpinned"));
                            ws.active_tab = 1;
                        }
                    }
                    let ids: Vec<_> = (0..3).map(|i| app.public_tab_id(i, 0).unwrap()).collect();
                    // Legacy selection: the same workspace's preceding tab,
                    // or the following workspace if its last tab is closed.
                    let legacy = if last_tab {
                        ids[(closing + 1).min(2)].clone()
                    } else {
                        app.public_tab_id(closing, 1).unwrap()
                    };
                    app.state.pinned_tabs = pins
                        .iter()
                        .copied()
                        .map(|(i, agent)| PinnedTab {
                            tab_id: ids.get(i).cloned().unwrap_or_else(|| "missing:t1".into()),
                            priority: 0,
                            role: agent.then_some(TabRole::Agent),
                        })
                        .collect();
                    app.state.switch_workspace_tab(closing, 0);
                    let response = if pane_close {
                        let pane = app.state.workspaces[closing].tabs[0].root_pane;
                        app.handle_pane_close(
                            "close".into(),
                            PaneTarget {
                                pane_id: app.public_pane_id(closing, pane).unwrap(),
                            },
                        )
                    } else {
                        app.handle_tab_close(
                            "close".into(),
                            TabTarget {
                                tab_id: ids[closing].clone(),
                            },
                        )
                    };
                    let success: SuccessResponse = serde_json::from_str(&response).unwrap();
                    assert_eq!(success.result, ResponseResult::Ok {});
                    let active = app.state.active.unwrap();
                    let actual = app
                        .public_tab_id(active, app.state.workspaces[active].active_tab)
                        .unwrap();
                    let expected = if expected == 4 {
                        &legacy
                    } else {
                        &ids[expected]
                    };
                    assert_eq!(
                        &actual, expected,
                        "pane_close={pane_close}, last_tab={last_tab}, closing={closing}"
                    );
                }
            }
        }
    }

    #[test]
    fn api_tab_close_last_tab_closes_workspace_and_emits_both_events() {
        let event_hub = crate::api::EventHub::default();
        let (_api_tx, api_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut app = App::new(
            &Config::default(),
            crate::app::AppPolicy::TEST,
            None,
            api_rx,
            event_hub.clone(),
        );
        app.state.workspaces = vec![Workspace::test_new("tabs")];
        app.state.active = Some(0);
        app.state.selected = 0;
        let tab_id = app.public_tab_id(0, 0).unwrap();
        let workspace_id = app.public_workspace_id(0);

        let response = app.handle_tab_close(
            "req".into(),
            TabTarget {
                tab_id: tab_id.clone(),
            },
        );

        let success: SuccessResponse = serde_json::from_str(&response).unwrap();
        assert_eq!(success.result, ResponseResult::Ok {});
        assert!(app.state.workspaces.is_empty());
        assert!(app.state.active.is_none());
        let events = event_hub.events_after(0);
        assert_eq!(
            events
                .iter()
                .map(|(_, event)| event.event)
                .collect::<Vec<_>>(),
            [EventKind::TabClosed, EventKind::WorkspaceClosed]
        );
        assert!(matches!(
            &events[0].1.data,
            EventData::TabClosed {
                tab_id: closed_tab_id,
                workspace_id: closed_workspace_id,
            } if closed_tab_id == &tab_id && closed_workspace_id == &workspace_id
        ));
        assert!(matches!(
            &events[1].1.data,
            EventData::WorkspaceClosed {
                workspace_id: closed_workspace_id,
                workspace: Some(workspace),
            } if closed_workspace_id == &workspace_id
                && workspace.workspace_id == workspace_id
        ));
    }

    #[tokio::test]
    async fn api_tab_close_last_tab_of_pinned_workspace_respawns_tab() {
        let event_hub = crate::api::EventHub::default();
        let (_api_tx, api_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut app = App::new(
            &Config::default(),
            crate::app::AppPolicy::TEST,
            None,
            api_rx,
            event_hub.clone(),
        );
        app.state.workspaces = vec![Workspace::test_new("tabs")];
        app.state.active = Some(0);
        app.state.selected = 0;
        let workspace_id = app.state.workspaces[0].id.clone();
        app.state.tree_pinned_spaces.insert(workspace_id.clone());
        let tab_id = app.public_tab_id(0, 0).unwrap();

        let response = app.handle_tab_close(
            "req".into(),
            TabTarget {
                tab_id: tab_id.clone(),
            },
        );

        let success: SuccessResponse = serde_json::from_str(&response).unwrap();
        assert_eq!(success.result, ResponseResult::Ok {});
        assert_eq!(app.state.workspaces.len(), 1);
        assert_eq!(app.state.workspaces[0].id, workspace_id);
        assert_eq!(app.state.workspaces[0].tabs.len(), 1);
        assert_ne!(app.public_tab_id(0, 0).unwrap(), tab_id);
        assert!(app.state.tree_pinned_spaces.contains(&workspace_id));
        let kinds: Vec<_> = event_hub
            .events_after(0)
            .iter()
            .map(|(_, event)| event.event)
            .collect();
        assert!(kinds.contains(&EventKind::TabCreated));
        assert!(kinds.contains(&EventKind::TabClosed));
        assert!(!kinds.contains(&EventKind::WorkspaceClosed));
        shutdown_test_runtimes(&mut app);
    }

    #[test]
    fn api_tab_move_reorders_tabs_in_target_workspace() {
        let event_hub = crate::api::EventHub::default();
        let (_api_tx, api_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut app = App::new(
            &Config::default(),
            crate::app::AppPolicy::TEST,
            None,
            api_rx,
            event_hub.clone(),
        );
        let mut workspace = Workspace::test_new("tabs");
        workspace.test_add_tab(Some("two"));
        workspace.test_add_tab(Some("three"));
        app.state.workspaces = vec![workspace];
        app.state.active = Some(0);
        app.state.selected = 0;
        let moved_root = app.state.workspaces[0].tabs[0].root_pane;
        let moved_id = app.public_tab_id(0, 0).unwrap();

        let response = app.handle_tab_move(
            "req".into(),
            TabMoveParams {
                tab_id: moved_id.clone(),
                insert_index: 3,
            },
        );

        let success: SuccessResponse = serde_json::from_str(&response).unwrap();
        let ResponseResult::TabList { tabs } = success.result else {
            panic!("expected tab list");
        };
        assert_eq!(app.state.workspaces[0].tabs[2].root_pane, moved_root);
        assert_eq!(tabs[2].tab_id, app.public_tab_id(0, 2).unwrap());
        let events = event_hub.events_after(0);
        assert!(events.iter().any(|(_, event)| {
            matches!(
                &event.data,
                EventData::TabMoved {
                    tab_id,
                    workspace_id,
                    insert_index: 3,
                    tabs,
                } if tab_id == &moved_id
                    && workspace_id == &app.public_workspace_id(0)
                    && tabs[2].tab_id == moved_id
            )
        }));
    }

    #[tokio::test]
    async fn tab_create_follows_cached_focused_pane_cwd_without_runtime() {
        let event_hub = crate::api::EventHub::default();
        let (_api_tx, api_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut app = App::new(
            &Config::default(),
            crate::app::AppPolicy::TEST,
            None,
            api_rx,
            event_hub,
        );
        app.state.default_shell = exiting_test_command().into();
        app.state.shell_mode = ShellModeConfig::NonLogin;
        let workspace = Workspace::test_new("tabs");
        let focused_pane = workspace.tabs[0].root_pane;
        app.state.workspaces = vec![workspace];
        app.state.active = Some(0);
        app.state.selected = 0;
        app.state.ensure_test_terminals();
        let cached_cwd = std::env::temp_dir();
        let terminal_id = app.state.workspaces[0]
            .terminal_id(focused_pane)
            .cloned()
            .unwrap();
        app.state.terminals.get_mut(&terminal_id).unwrap().cwd = cached_cwd.clone();

        let response = app.handle_tab_create(
            "req".into(),
            TabCreateParams {
                workspace_id: None,
                cwd: None,
                focus: false,
                label: None,
                env: Default::default(),
            },
        );

        let success: SuccessResponse = serde_json::from_str(&response).unwrap();
        assert!(matches!(success.result, ResponseResult::TabCreated { .. }));
        let created = &app.state.workspaces[0].tabs[1];
        let created_terminal_id = created.terminal_id(created.root_pane).unwrap();
        let created_cwd = &app.state.terminals.get(created_terminal_id).unwrap().cwd;
        assert_eq!(
            crate::worktree::canonical_or_original(created_cwd),
            crate::worktree::canonical_or_original(&cached_cwd)
        );
        shutdown_test_runtimes(&mut app);
    }

    fn pin_test_app() -> (App, Vec<String>, crate::api::EventHub) {
        let event_hub = crate::api::EventHub::default();
        let (_api_tx, api_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut app = App::new(
            &Config::default(),
            crate::app::AppPolicy::TEST,
            None,
            api_rx,
            event_hub.clone(),
        );
        // Four chats over two spaces, so moves cross workspace boundaries.
        let mut other = Workspace::test_new("other");
        other.id = "w9".to_string();
        app.state.workspaces = vec![Workspace::test_new("tabs"), other];
        app.state.workspaces[0].id = "w1".to_string();
        app.state.workspaces[0].test_add_tab(Some("two"));
        app.state.workspaces[0].test_add_tab(Some("three"));
        app.state.active = Some(0);
        app.state.selected = 0;
        let tabs = vec![
            app.public_tab_id(0, 0).unwrap(),
            app.public_tab_id(0, 1).unwrap(),
            app.public_tab_id(0, 2).unwrap(),
            app.public_tab_id(1, 0).unwrap(),
        ];
        (app, tabs, event_hub)
    }

    fn pin_order(app: &App) -> Vec<String> {
        app.state
            .pinned_tabs
            .iter()
            .map(|pin| pin.tab_id.clone())
            .collect()
    }

    fn set_pinned(app: &mut App, tab_id: &str, pinned: bool, priority: Option<i64>) {
        let response = app.handle_tab_set_pinned(
            "req".into(),
            TabSetPinnedParams {
                tab_id: tab_id.to_string(),
                pinned,
                priority,
            },
        );
        assert!(
            serde_json::from_str::<SuccessResponse>(&response).is_ok(),
            "{response}"
        );
    }

    fn pin_move(app: &mut App, tab_id: &str, pin_index: usize) -> serde_json::Value {
        let response = app.handle_tab_pin_move(
            "req".into(),
            TabPinMoveParams {
                tab_id: tab_id.to_string(),
                pin_index,
            },
        );
        serde_json::from_str(&response).unwrap()
    }

    #[test]
    fn api_tab_pin_move_reorders_the_shared_pin_order() {
        // (moved pin, target index, order after) from pins [a, b, c, d].
        let cases: [(usize, usize, [usize; 4]); 6] = [
            (2, 1, [0, 2, 1, 3]), // up one
            (0, 2, [1, 2, 0, 3]), // down two
            (3, 0, [3, 0, 1, 2]), // to the top
            (0, 3, [1, 2, 3, 0]), // to the bottom
            (1, 1, [0, 1, 2, 3]), // onto itself
            (3, 3, [0, 1, 2, 3]), // already last
        ];
        for (moved, index, expected) in cases {
            let (mut app, tabs, events) = pin_test_app();
            for tab in &tabs {
                set_pinned(&mut app, tab, true, None);
            }
            let seen = events.current_sequence();
            let response = pin_move(&mut app, &tabs[moved], index);
            assert_eq!(response["result"]["tab"]["pin_index"], index, "{response}");
            let expected: Vec<String> = expected.iter().map(|i| tabs[*i].clone()).collect();
            assert_eq!(pin_order(&app), expected, "move {moved} to {index}");
            let moved_event = events
                .events_after(seen)
                .into_iter()
                .find_map(|(_, event)| match event.data {
                    EventData::TabPinMoved {
                        tab_id,
                        pin_index,
                        pinned_tab_ids,
                        ..
                    } => Some((tab_id, pin_index, pinned_tab_ids)),
                    _ => None,
                });
            if moved == index {
                assert_eq!(moved_event, None, "a no-op move emits nothing");
            } else {
                assert_eq!(moved_event, Some((tabs[moved].clone(), index, expected)));
            }
            shutdown_test_runtimes(&mut app);
        }
    }

    #[test]
    fn api_tab_pin_move_rejects_bad_targets_and_keeps_priority_order() {
        let (mut app, tabs, _events) = pin_test_app();
        set_pinned(&mut app, &tabs[0], true, None);
        set_pinned(&mut app, &tabs[1], true, None);
        set_pinned(&mut app, &tabs[2], true, Some(5));
        assert_eq!(
            pin_order(&app),
            vec![tabs[2].clone(), tabs[0].clone(), tabs[1].clone()]
        );

        let response = pin_move(&mut app, &tabs[0], 3);
        assert_eq!(
            response["error"]["code"], "pin_index_out_of_bounds",
            "{response}"
        );
        let response = pin_move(&mut app, &tabs[3], 0);
        assert_eq!(response["error"]["code"], "tab_not_pinned", "{response}");
        let response = pin_move(&mut app, "w1:t99", 0);
        assert_eq!(response["error"]["code"], "tab_not_found", "{response}");
        assert_eq!(
            pin_order(&app),
            vec![tabs[2].clone(), tabs[0].clone(), tabs[1].clone()]
        );

        // A priority-0 chat dragged above the priority-5 one stays there when
        // the next prioritized pin lands by priority.
        pin_move(&mut app, &tabs[1], 0);
        set_pinned(&mut app, &tabs[3], true, Some(5));
        assert_eq!(
            pin_order(&app),
            vec![
                tabs[1].clone(),
                tabs[2].clone(),
                tabs[3].clone(),
                tabs[0].clone()
            ]
        );

        // An unpin between a client's read and its move: the move applies to
        // the order that is left, and a now-unpinned chat is refused.
        set_pinned(&mut app, &tabs[2], false, None);
        let response = pin_move(&mut app, &tabs[2], 0);
        assert_eq!(response["error"]["code"], "tab_not_pinned", "{response}");
        pin_move(&mut app, &tabs[0], 0);
        assert_eq!(
            pin_order(&app),
            vec![tabs[0].clone(), tabs[1].clone(), tabs[3].clone()]
        );
        // A pin arriving between them lands by priority and the next move
        // counts it.
        set_pinned(&mut app, &tabs[2], true, None);
        pin_move(&mut app, &tabs[2], 1);
        assert_eq!(
            pin_order(&app),
            vec![
                tabs[0].clone(),
                tabs[2].clone(),
                tabs[1].clone(),
                tabs[3].clone()
            ]
        );
        shutdown_test_runtimes(&mut app);
    }

    #[test]
    fn api_tab_set_pinned_orders_by_priority_and_drops_closed_tabs() {
        let event_hub = crate::api::EventHub::default();
        let (_api_tx, api_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut app = App::new(
            &Config::default(),
            crate::app::AppPolicy::TEST,
            None,
            api_rx,
            event_hub,
        );
        // Two spaces so the pin order crosses workspace boundaries.
        let mut second = Workspace::test_new("other");
        second.id = "w9".to_string();
        app.state.workspaces = vec![Workspace::test_new("tabs"), second];
        app.state.workspaces[0].id = "w1".to_string();
        app.state.workspaces[0].test_add_tab(Some("logs"));
        app.state.active = Some(0);
        app.state.selected = 0;
        let first = app.public_tab_id(0, 0).unwrap();
        let second_tab = app.public_tab_id(0, 1).unwrap();
        let cross_space = app.public_tab_id(1, 0).unwrap();

        let pin_order = |app: &App| -> Vec<String> {
            app.state
                .pinned_tabs
                .iter()
                .map(|pin| pin.tab_id.clone())
                .collect()
        };
        let pin = |app: &mut App, tab_id: &str, priority: Option<i64>| {
            let response = app.handle_tab_set_pinned(
                "req".into(),
                TabSetPinnedParams {
                    tab_id: tab_id.to_string(),
                    pinned: true,
                    priority,
                },
            );
            let success: SuccessResponse = serde_json::from_str(&response).unwrap();
            assert!(matches!(success.result, ResponseResult::TabInfo { .. }));
        };
        pin(&mut app, &first, None);
        pin(&mut app, &cross_space, None);
        pin(&mut app, &second_tab, Some(5));
        // Priority 5 jumps the tie order; equal priorities keep pin order.
        assert_eq!(
            pin_order(&app),
            vec![second_tab.clone(), first.clone(), cross_space.clone()]
        );

        let response = app.handle_tab_set_pinned(
            "req".into(),
            TabSetPinnedParams {
                tab_id: cross_space.clone(),
                pinned: false,
                priority: None,
            },
        );
        let success: SuccessResponse = serde_json::from_str(&response).unwrap();
        assert!(matches!(success.result, ResponseResult::TabInfo { .. }));
        assert_eq!(pin_order(&app), vec![second_tab.clone(), first.clone()]);

        // Closing a pinned chat drops it from the pinned order.
        let response = app.handle_tab_close(
            "req".into(),
            TabTarget {
                tab_id: second_tab.clone(),
            },
        );
        let success: SuccessResponse = serde_json::from_str(&response).unwrap();
        assert!(matches!(success.result, ResponseResult::Ok {}));
        assert_eq!(pin_order(&app), vec![first.clone()]);

        // Pin state survives a session snapshot round-trip.
        let ui = app.state.snapshot_ui_prefs();
        assert_eq!(ui.pinned_tabs.len(), 1);
        assert_eq!(ui.pinned_tabs[0].tab_id, first);
        shutdown_test_runtimes(&mut app);
    }
    /// Public API response/event contract, exercised through JSON request decoding
    /// and the real dispatch path rather than a mocked handler.
    #[test]
    fn api_tab_set_role_round_trip_and_pin_move_boundary() {
        let (mut app, tabs, events) = pin_test_app();
        set_pinned(&mut app, &tabs[0], true, Some(10));
        let request: crate::api::schema::Request = serde_json::from_value(serde_json::json!({
            "id": "role", "method": "tab.set_role", "params": {"tab_id": tabs[1], "role": "agent"}
        }))
        .unwrap();
        let seen = events.current_sequence();
        let response: serde_json::Value =
            serde_json::from_str(&app.handle_api_request(request)).unwrap();
        assert_eq!(response["result"]["tab"]["role"], "agent");
        assert_eq!(response["result"]["tab"]["pin_index"], 0);
        assert_eq!(pin_order(&app), [tabs[1].clone(), tabs[0].clone()]);
        let emitted = events.events_after(seen);
        assert_eq!(
            emitted
                .iter()
                .map(|(_, event)| event.event)
                .collect::<Vec<_>>(),
            [EventKind::WorkspaceUpdated, EventKind::TabPinMoved]
        );
        assert!(
            matches!(&emitted[1].1.data, EventData::TabPinMoved { pin_index: 0, pinned_tab_ids, .. } if pinned_tab_ids == &pin_order(&app))
        );
        assert_eq!(
            pin_move(&mut app, &tabs[0], 0)["error"]["code"],
            "pin_index_outside_role_block"
        );
        assert_eq!(
            pin_move(&mut app, &tabs[1], 1)["error"]["code"],
            "pin_index_outside_role_block"
        );
        set_pinned(&mut app, &tabs[1], true, Some(-50));
        assert_eq!(
            app.tab_info(0, 1).unwrap().role,
            Some(crate::api::schema::TabRole::Agent)
        );
        let request = serde_json::from_value(
            serde_json::json!({"id":"clear", "method":"tab.set_role", "params":{"tab_id":tabs[1]}}),
        )
        .unwrap();
        let response: serde_json::Value =
            serde_json::from_str(&app.handle_api_request(request)).unwrap();
        assert!(response["result"]["tab"].get("role").is_none());
        assert_eq!(response["result"]["tab"]["pin_index"], 0);
        assert!(app.state.pinned_tabs[0].priority >= app.state.pinned_tabs[1].priority);
        // Workspace-close retain must preserve the partition too.
        app.state.ensure_test_terminals();
        app.state
            .set_tab_role(&tabs[0], Some(crate::api::schema::TabRole::Agent));
        app.state
            .set_tab_role(&tabs[3], Some(crate::api::schema::TabRole::Agent));
        app.handle_tab_close(
            "close".into(),
            TabTarget {
                tab_id: tabs[3].clone(),
            },
        );
        app.state.assert_invariants_for_test();
        shutdown_test_runtimes(&mut app);
    }
    /// Real JSON API boundary: ranks and parked status survive serialization,
    /// and rename repositions a plain pin without changing workspace/tab identity.
    #[test]
    fn api_priority_sort_rank_and_parked() {
        let (mut app, tabs, _) = pin_test_app();
        app.state.ensure_test_terminals();
        app.state.sidebar_priority.order = vec!["tab:two".into()];
        app.state.sidebar_priority.last = vec!["tabs".into()];
        set_pinned(&mut app, &tabs[0], true, None);
        set_pinned(&mut app, &tabs[1], true, None);
        assert_eq!(pin_order(&app), [tabs[1].clone(), tabs[0].clone()]);
        let workspace = serde_json::to_value(app.workspace_info(0)).unwrap();
        assert_eq!(workspace["sort_rank"], 2);
        assert_eq!(workspace["parked"], true);
        let tab = serde_json::to_value(app.tab_info(0, 1).unwrap()).unwrap();
        assert_eq!(tab["sort_rank"], 0);
        // Leaving the "two" group ties both pins on the parked workspace rank;
        // the renamed pin joins the end of that group, so the order flips.
        let response = app.handle_tab_rename(
            "rename".into(),
            TabRenameParams {
                tab_id: tabs[1].clone(),
                label: "plain".into(),
            },
        );
        let response: serde_json::Value = serde_json::from_str(&response).unwrap();
        assert_eq!(response["result"]["tab"]["sort_rank"], 2);
        assert_eq!(pin_order(&app), [tabs[0].clone(), tabs[1].clone()]);
        app.state.assert_invariants_for_test();
        // Joining the "two" group moves the renamed pin ahead of its old peer.
        let response = app.handle_tab_rename(
            "rename-back".into(),
            TabRenameParams {
                tab_id: tabs[1].clone(),
                label: "two".into(),
            },
        );
        let response: serde_json::Value = serde_json::from_str(&response).unwrap();
        assert_eq!(response["result"]["tab"]["sort_rank"], 0);
        assert_eq!(pin_order(&app), [tabs[1].clone(), tabs[0].clone()]);
        app.state.assert_invariants_for_test();
        // A label change within the same rank must not reshuffle tied pins.
        let before = pin_order(&app);
        let response = app.handle_tab_rename(
            "same-group".into(),
            TabRenameParams {
                tab_id: tabs[1].clone(),
                label: "TWO".into(),
            },
        );
        assert!(serde_json::from_str::<SuccessResponse>(&response).is_ok());
        assert_eq!(pin_order(&app), before);
        app.state.assert_invariants_for_test();
    }
}
