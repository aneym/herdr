use super::*;

impl ClientContextMenuOverlay {
    pub(super) fn items(&self) -> Vec<ClientContextMenuItem> {
        use ClientContextMenuAction as Action;

        let item = |label: &str, action| ClientContextMenuItem {
            label: label.into(),
            action,
        };
        match &self.target {
            ClientContextMenuTarget::SidebarView {
                show_spaces,
                show_tabs,
                show_agents,
                show_hidden,
            } => vec![
                item(
                    if *show_spaces {
                        "Hide spaces"
                    } else {
                        "Show spaces"
                    },
                    Action::ToggleTreeSpaces,
                ),
                item(
                    if *show_tabs { "Hide tabs" } else { "Show tabs" },
                    Action::ToggleTreeTabs,
                ),
                item(
                    if *show_agents {
                        "Hide agents"
                    } else {
                        "Show agents"
                    },
                    Action::ToggleTreeAgents,
                ),
                item(
                    if *show_hidden {
                        "Hide folded spaces"
                    } else {
                        "Reveal folded spaces"
                    },
                    Action::ToggleHiddenSpaces,
                ),
            ],
            ClientContextMenuTarget::Workspace { is_git: false, .. } => {
                vec![
                    item("Send to profile", Action::SendToProfile),
                    item("Share profiles", Action::ShareProfiles),
                    item("Rename", Action::Rename),
                    item("Close", Action::Close),
                ]
            }
            ClientContextMenuTarget::Workspace {
                is_linked_worktree: false,
                has_worktree_children: false,
                ..
            } => vec![
                item("Send to profile", Action::SendToProfile),
                item("Share profiles", Action::ShareProfiles),
                item("Rename", Action::Rename),
                item("Close", Action::Close),
                item("New worktree", Action::NewWorktree),
                item("Open worktree...", Action::OpenWorktree),
            ],
            ClientContextMenuTarget::Workspace {
                is_linked_worktree: true,
                ..
            } => vec![
                item("Send to profile", Action::SendToProfile),
                item("Share profiles", Action::ShareProfiles),
                item("Rename", Action::Rename),
                item("Close", Action::Close),
                item("Delete worktree checkout...", Action::RemoveWorktree),
            ],
            ClientContextMenuTarget::Workspace {
                has_worktree_children: true,
                collapsed,
                ..
            } => vec![
                item("Send to profile", Action::SendToProfile),
                item("Share profiles", Action::ShareProfiles),
                item("Rename", Action::Rename),
                item("Close group", Action::Close),
                item("New worktree", Action::NewWorktree),
                item("Open worktree...", Action::OpenWorktree),
                item(
                    if *collapsed { "Expand" } else { "Collapse" },
                    Action::ToggleGroup,
                ),
            ],
            ClientContextMenuTarget::Tab { .. } => vec![
                item("New tab", Action::NewTab),
                item("Rename", Action::Rename),
                item("Close", Action::Close),
            ],
            ClientContextMenuTarget::Pane {
                source_pane_id,
                has_manual_label,
                right_click_passthrough,
                ..
            } => {
                let mut items = vec![item("Rename pane", Action::RenamePane)];
                items.push(item("Send to profile", Action::SendToProfile));
                items.push(item("Share profiles", Action::ShareProfiles));
                if *has_manual_label {
                    items.push(item("Clear pane name", Action::ClearPaneName));
                }
                if source_pane_id.is_some() {
                    items.push(item("Swap with focused pane", Action::SwapWithFocusedPane));
                }
                items.extend([
                    item("Split right", Action::SplitRight),
                    item("Split down", Action::SplitDown),
                    item("Zoom", Action::Zoom),
                    item(
                        if *right_click_passthrough {
                            "Use Herdr right-click menu"
                        } else {
                            "Send right-clicks to pane"
                        },
                        Action::ToggleRightClickPassthrough,
                    ),
                    item("Close pane", Action::ClosePane),
                ]);
                items
            }
            ClientContextMenuTarget::Profile { entries, .. } => entries
                .iter()
                .enumerate()
                .map(|(index, entry)| ClientContextMenuItem {
                    label: entry.label.clone(),
                    action: Action::ProfileSelect(index),
                })
                .collect(),
        }
    }
}

impl ClientShellState {
    pub(super) fn receive_profile_menu_roster(&mut self, generation: u64, roster: Vec<String>) {
        let Some(load) = self
            .profile_menu_load
            .as_mut()
            .filter(|load| load.generation == generation)
        else {
            return;
        };
        load.roster = Some(roster);
        self.finish_profile_menu_load(generation);
    }

    pub(super) fn receive_profile_menu_workspace_membership(
        &mut self,
        generation: u64,
        membership: Vec<String>,
    ) {
        let Some(load) = self
            .profile_menu_load
            .as_mut()
            .filter(|load| load.generation == generation)
        else {
            return;
        };
        load.workspace_membership = Some(membership);
        self.finish_profile_menu_load(generation);
    }

    pub(super) fn receive_profile_menu_pane_membership(
        &mut self,
        generation: u64,
        membership: Vec<String>,
    ) {
        let Some(load) = self
            .profile_menu_load
            .as_mut()
            .filter(|load| load.generation == generation)
        else {
            return;
        };
        load.pane_membership = Some(membership);
        self.finish_profile_menu_load(generation);
    }

    pub(super) fn fail_profile_menu_load(&mut self, generation: u64, message: String) {
        if self
            .profile_menu_load
            .as_ref()
            .is_some_and(|load| load.generation == generation)
        {
            self.profile_menu_load = None;
            self.set_endpoint_error(message);
        }
    }

    fn finish_profile_menu_load(&mut self, generation: u64) {
        let Some(load) = self
            .profile_menu_load
            .as_ref()
            .filter(|load| load.generation == generation)
        else {
            return;
        };
        if !matches!(self.overlay, Some(ClientShellOverlay::ProfileLoading { generation: visible }) if visible == generation)
        {
            return;
        }
        let Some(roster) = load.roster.as_ref() else {
            return;
        };
        let Some(workspace_membership) = load.workspace_membership.as_ref() else {
            return;
        };
        if load.pane_id.is_some() && load.pane_membership.is_none() {
            return;
        }
        let membership = load
            .pane_membership
            .as_ref()
            .filter(|profiles| !profiles.is_empty())
            .unwrap_or(workspace_membership);
        let mut entries = Vec::new();
        if load.pane_id.is_some() {
            entries.push(ClientProfileMenuEntry {
                profile: None,
                label: "Follow space".into(),
                selected: load.pane_membership.as_ref().is_some_and(Vec::is_empty),
            });
        }
        entries.extend(
            roster
                .iter()
                .cloned()
                .map(|profile| ClientProfileMenuEntry {
                    selected: if load.share {
                        membership.contains(&profile)
                    } else {
                        membership.len() == 1 && membership[0] == profile
                    },
                    label: profile.clone(),
                    profile: Some(profile),
                }),
        );
        let Some(load) = self.profile_menu_load.take() else {
            return;
        };
        self.overlay = Some(ClientShellOverlay::ContextMenu(ClientContextMenuOverlay {
            target: ClientContextMenuTarget::Profile {
                pane_id: load.pane_id,
                workspace_id: load.workspace_id,
                share: load.share,
                entries,
            },
            x: load.x,
            y: load.y,
            highlighted: 0,
        }));
    }

    pub(super) fn open_workspace_context_menu(&mut self, workspace_id: String, x: u16, y: u16) {
        let Some(snapshot) = self.snapshot.as_deref() else {
            return;
        };
        let Some(workspace) = snapshot
            .workspaces
            .iter()
            .find(|workspace| workspace.workspace_id == workspace_id)
        else {
            return;
        };
        let worktree = workspace.worktree.as_ref();
        let has_worktree_children = worktree.is_some_and(|worktree| {
            !worktree.is_linked_worktree
                && snapshot
                    .workspaces
                    .iter()
                    .filter(|candidate| {
                        candidate
                            .worktree
                            .as_ref()
                            .is_some_and(|candidate| candidate.key == worktree.key)
                    })
                    .count()
                    >= 2
        });
        let collapsed = worktree.is_some_and(|worktree| {
            self.group_is_collapsed(&self.active_endpoint_id, &worktree.key)
        });
        self.overlay = Some(ClientShellOverlay::ContextMenu(ClientContextMenuOverlay {
            target: ClientContextMenuTarget::Workspace {
                workspace_id,
                is_git: worktree.is_some() || workspace.branch.is_some(),
                is_linked_worktree: worktree.is_some_and(|worktree| worktree.is_linked_worktree),
                has_worktree_children,
                collapsed,
            },
            x,
            y,
            highlighted: 0,
        }));
    }

    pub(super) fn open_profile_context_menu(
        &mut self,
        workspace_id: String,
        pane_id: Option<String>,
        share: bool,
        x: u16,
        y: u16,
        outcome: &mut ClientShellInput,
    ) {
        self.next_profile_menu_generation = self.next_profile_menu_generation.wrapping_add(1);
        let generation = self.next_profile_menu_generation;
        self.profile_menu_load = Some(ClientProfileMenuLoad {
            generation,
            workspace_id: workspace_id.clone(),
            pane_id: pane_id.clone(),
            share,
            x,
            y,
            roster: None,
            workspace_membership: None,
            pane_membership: None,
        });
        self.overlay = Some(ClientShellOverlay::ProfileLoading { generation });
        let roster_ok = self.push_endpoint_method_with_kind(
            crate::api::schema::Method::ProfileList(crate::api::schema::EmptyParams::default()),
            PendingEndpointKind::ProfileList { generation },
            outcome,
        );
        let workspace_ok = self.push_endpoint_method_with_kind(
            crate::api::schema::Method::WorkspaceGet(crate::api::schema::WorkspaceTarget {
                workspace_id: workspace_id.clone(),
            }),
            PendingEndpointKind::ProfileWorkspaceMembership { generation },
            outcome,
        );
        let pane_ok = if let Some(pane_id) = pane_id {
            self.push_endpoint_method_with_kind(
                crate::api::schema::Method::PaneGet(crate::api::schema::PaneTarget { pane_id }),
                PendingEndpointKind::ProfilePaneMembership { generation },
                outcome,
            )
        } else {
            true
        };
        if !roster_ok || !workspace_ok || !pane_ok {
            self.profile_menu_load = None;
            self.set_endpoint_error("unable to load profiles for this item");
        }
    }

    pub(super) fn open_tab_context_menu(&mut self, tab_id: String, x: u16, y: u16) {
        let Some(tab) = self
            .snapshot
            .as_deref()
            .and_then(|snapshot| snapshot.tabs.iter().find(|tab| tab.tab_id == tab_id))
        else {
            return;
        };
        self.overlay = Some(ClientShellOverlay::ContextMenu(ClientContextMenuOverlay {
            target: ClientContextMenuTarget::Tab {
                tab_id,
                workspace_id: tab.workspace_id.clone(),
            },
            x,
            y,
            highlighted: 0,
        }));
    }

    pub(super) fn open_pane_context_menu(&mut self, pane_id: String, x: u16, y: u16) {
        let Some(snapshot) = self.snapshot.as_deref() else {
            return;
        };
        let Some(pane) = snapshot.panes.iter().find(|pane| pane.pane_id == pane_id) else {
            return;
        };
        let source_pane_id = snapshot
            .focused_pane_id
            .clone()
            .filter(|focused| focused != &pane_id);
        self.overlay = Some(ClientShellOverlay::ContextMenu(ClientContextMenuOverlay {
            target: ClientContextMenuTarget::Pane {
                pane_id,
                workspace_id: pane.workspace_id.clone(),
                source_pane_id,
                has_manual_label: pane.label.is_some(),
                right_click_passthrough: pane.right_click_passthrough,
            },
            x,
            y,
            highlighted: 0,
        }));
    }

    pub(super) fn move_context_menu_selection(&mut self, delta: isize) {
        let Some(ClientShellOverlay::ContextMenu(menu)) = self.overlay.as_mut() else {
            return;
        };
        let item_count = menu.items().len();
        if item_count == 0 {
            return;
        }
        menu.highlighted = (menu.highlighted as isize + delta)
            .clamp(0, item_count.saturating_sub(1) as isize) as usize;
    }

    pub(super) fn activate_context_menu_item(
        &mut self,
        index: usize,
        outcome: &mut ClientShellInput,
    ) {
        let Some(ClientShellOverlay::ContextMenu(menu)) = self.overlay.take() else {
            return;
        };
        let Some(action) = menu.items().get(index).map(|item| item.action) else {
            outcome.repaint = true;
            return;
        };
        let menu_position = (menu.x, menu.y);
        match menu.target {
            ClientContextMenuTarget::SidebarView { .. } => {
                self.activate_sidebar_view_action(action, outcome)
            }
            ClientContextMenuTarget::Workspace { workspace_id, .. } => {
                self.activate_workspace_context_action(workspace_id, action, menu_position, outcome)
            }
            ClientContextMenuTarget::Tab {
                tab_id,
                workspace_id,
            } => self.activate_tab_context_action(tab_id, workspace_id, action, outcome),
            ClientContextMenuTarget::Pane {
                pane_id,
                workspace_id,
                source_pane_id,
                right_click_passthrough,
                ..
            } => self.activate_pane_context_action(
                pane_id,
                workspace_id,
                source_pane_id,
                right_click_passthrough,
                action,
                menu_position,
                outcome,
            ),
            ClientContextMenuTarget::Profile {
                pane_id,
                workspace_id,
                share,
                entries,
            } => self.activate_profile_context_action(
                pane_id,
                workspace_id,
                share,
                entries,
                action,
                outcome,
            ),
        }
        outcome.repaint = true;
    }

    pub(super) fn activate_profile_context_action(
        &mut self,
        pane_id: Option<String>,
        workspace_id: String,
        share: bool,
        entries: Vec<ClientProfileMenuEntry>,
        action: ClientContextMenuAction,
        outcome: &mut ClientShellInput,
    ) {
        let ClientContextMenuAction::ProfileSelect(index) = action else {
            return;
        };
        let Some(entry) = entries.get(index) else {
            return;
        };
        let profile = entry.profile.as_ref();
        if let Some(pane_id) = pane_id {
            let selected = if profile.is_none() {
                Vec::new()
            } else if share {
                let mut values = entries
                    .iter()
                    .filter_map(|entry| entry.selected.then(|| entry.profile.clone()).flatten())
                    .collect::<Vec<_>>();
                let Some(profile) = profile else {
                    return;
                };
                if values.contains(profile) {
                    values.retain(|value| value != profile);
                } else {
                    values.push(profile.clone());
                }
                values
            } else {
                match profile {
                    Some(profile) => vec![profile.clone()],
                    None => return,
                }
            };
            self.push_endpoint_method(
                crate::api::schema::Method::PaneSetProfiles(
                    crate::api::schema::PaneSetProfilesParams {
                        pane_id,
                        profiles: selected,
                    },
                ),
                outcome,
            );
        } else {
            let selected = if share {
                let mut values = entries
                    .iter()
                    .filter_map(|entry| entry.selected.then(|| entry.profile.clone()).flatten())
                    .collect::<Vec<_>>();
                let Some(profile) = profile else {
                    return;
                };
                if values.contains(profile) {
                    values.retain(|value| value != profile);
                } else {
                    values.push(profile.clone());
                }
                values
            } else {
                let Some(profile) = profile else {
                    return;
                };
                vec![profile.clone()]
            };
            self.push_endpoint_method(
                crate::api::schema::Method::WorkspaceSetProfiles(
                    crate::api::schema::WorkspaceSetProfilesParams {
                        workspace_id,
                        profiles: selected,
                    },
                ),
                outcome,
            );
        }
    }

    /// Open the agents-panel view control. The fork put the tree layer toggles
    /// behind the sort label; upstream's sort label already cycles the sort, so
    /// the toggles live on its right-click instead.
    pub(super) fn open_sidebar_view_context_menu(&mut self, x: u16, y: u16) {
        let tree = self
            .tree_chrome
            .get(&self.active_endpoint_id)
            .unwrap_or(&self.tree_chrome_default);
        self.overlay = Some(ClientShellOverlay::ContextMenu(ClientContextMenuOverlay {
            target: ClientContextMenuTarget::SidebarView {
                show_spaces: tree.show_spaces,
                show_tabs: tree.show_tabs,
                show_agents: tree.show_agents,
                show_hidden: tree.show_hidden_spaces,
            },
            x,
            y,
            highlighted: 0,
        }));
    }

    fn activate_sidebar_view_action(
        &mut self,
        action: ClientContextMenuAction,
        outcome: &mut ClientShellInput,
    ) {
        let tree = self.tree_chrome_mut();
        match action {
            ClientContextMenuAction::ToggleTreeSpaces => tree.show_spaces = !tree.show_spaces,
            ClientContextMenuAction::ToggleTreeTabs => tree.show_tabs = !tree.show_tabs,
            ClientContextMenuAction::ToggleTreeAgents => tree.show_agents = !tree.show_agents,
            ClientContextMenuAction::ToggleHiddenSpaces => {
                tree.show_hidden_spaces = !tree.show_hidden_spaces;
                // Turning it off closes the section too, so switching it back on
                // starts compact rather than resuming a stale expansion.
                tree.hidden_spaces_expanded = false;
            }
            _ => return,
        }
        self.agent_scroll = 0;
        self.persist_chrome_preferences(outcome);
    }

    fn activate_workspace_context_action(
        &mut self,
        workspace_id: String,
        action: ClientContextMenuAction,
        menu_position: (u16, u16),
        outcome: &mut ClientShellInput,
    ) {
        use crate::input::KeybindAction;

        match action {
            ClientContextMenuAction::SendToProfile | ClientContextMenuAction::ShareProfiles => {
                self.open_profile_context_menu(
                    workspace_id,
                    None,
                    action == ClientContextMenuAction::ShareProfiles,
                    menu_position.0,
                    menu_position.1,
                    outcome,
                );
            }
            ClientContextMenuAction::Rename => {
                let label = self
                    .snapshot
                    .as_deref()
                    .and_then(|snapshot| {
                        snapshot
                            .workspaces
                            .iter()
                            .find(|workspace| workspace.workspace_id == workspace_id)
                    })
                    .map(|workspace| workspace.label.clone());
                if let Some(label) = label {
                    self.overlay = Some(ClientShellOverlay::Rename(ClientRenameOverlay {
                        title: "rename workspace",
                        input: TextEditor::new(&label, false),
                        target: ClientRenameTarget::Workspace { workspace_id },
                    }));
                }
            }
            ClientContextMenuAction::Close => {
                if self.config.confirm_close {
                    self.open_confirm_close_overlay(workspace_id);
                } else {
                    self.push_endpoint_method(
                        crate::api::schema::Method::WorkspaceClose(
                            crate::api::schema::WorkspaceCloseParams {
                                workspace_id,
                                close_group: true,
                            },
                        ),
                        outcome,
                    );
                }
            }
            ClientContextMenuAction::NewWorktree => {
                self.begin_worktree_action_for(KeybindAction::NewWorktree, workspace_id, outcome)
            }
            ClientContextMenuAction::OpenWorktree => {
                self.begin_worktree_action_for(KeybindAction::OpenWorktree, workspace_id, outcome)
            }
            ClientContextMenuAction::RemoveWorktree => {
                self.begin_worktree_action_for(KeybindAction::RemoveWorktree, workspace_id, outcome)
            }
            ClientContextMenuAction::ToggleGroup => {
                let key = self.snapshot.as_deref().and_then(|snapshot| {
                    snapshot
                        .workspaces
                        .iter()
                        .find(|workspace| workspace.workspace_id == workspace_id)
                        .and_then(|workspace| workspace.worktree.as_ref())
                        .map(|worktree| worktree.key.clone())
                });
                if let Some(key) = key {
                    let endpoint_id = self.active_endpoint_id.clone();
                    self.toggle_collapsed_group(&endpoint_id, key);
                    self.persist_chrome_preferences(outcome);
                }
            }
            _ => {}
        }
    }

    fn activate_tab_context_action(
        &mut self,
        tab_id: String,
        workspace_id: String,
        action: ClientContextMenuAction,
        outcome: &mut ClientShellInput,
    ) {
        use crate::api::schema::{Method, TabTarget};

        self.push_endpoint_method(
            Method::TabFocus(TabTarget {
                tab_id: tab_id.clone(),
            }),
            outcome,
        );
        match action {
            ClientContextMenuAction::NewTab => {
                if self.config.prompt_new_tab_name {
                    let default_name = (self
                        .snapshot
                        .as_deref()
                        .map(|snapshot| {
                            snapshot
                                .tabs
                                .iter()
                                .filter(|tab| tab.workspace_id == workspace_id)
                                .count()
                        })
                        .unwrap_or(0)
                        + 1)
                    .to_string();
                    self.overlay = Some(ClientShellOverlay::Rename(ClientRenameOverlay {
                        title: "new tab",
                        input: TextEditor::new(&default_name, true),
                        target: ClientRenameTarget::NewTab {
                            workspace_id,
                            default_name,
                        },
                    }));
                } else {
                    self.push_endpoint_method(
                        Method::TabCreate(crate::api::schema::TabCreateParams {
                            workspace_id: Some(workspace_id),
                            cwd: None,
                            focus: true,
                            label: None,
                            env: Default::default(),
                        }),
                        outcome,
                    );
                }
            }
            ClientContextMenuAction::Rename => {
                let tab = self
                    .snapshot
                    .as_deref()
                    .and_then(|snapshot| snapshot.tabs.iter().find(|tab| tab.tab_id == tab_id));
                if let Some(tab) = tab {
                    self.overlay = Some(ClientShellOverlay::Rename(ClientRenameOverlay {
                        title: "rename tab",
                        input: TextEditor::new(&tab.label, false),
                        target: ClientRenameTarget::Tab {
                            tab_id,
                            auto_name: !tab.custom_label,
                            original_name: tab.label.clone(),
                        },
                    }));
                }
            }
            ClientContextMenuAction::Close => {
                self.request_tab_close(tab_id, outcome);
            }
            _ => {}
        }
    }

    fn activate_pane_context_action(
        &mut self,
        pane_id: String,
        workspace_id: String,
        source_pane_id: Option<String>,
        right_click_passthrough: bool,
        action: ClientContextMenuAction,
        menu_position: (u16, u16),
        outcome: &mut ClientShellInput,
    ) {
        use crate::api::schema::{
            Method, PaneInputSetParams, PaneRenameParams, PaneRightClickTarget, PaneSplitParams,
            PaneSwapParams, PaneTarget, PaneZoomMode, PaneZoomParams, SplitDirection,
        };

        match action {
            ClientContextMenuAction::SendToProfile | ClientContextMenuAction::ShareProfiles => {
                self.open_profile_context_menu(
                    workspace_id,
                    Some(pane_id),
                    action == ClientContextMenuAction::ShareProfiles,
                    menu_position.0,
                    menu_position.1,
                    outcome,
                );
            }
            ClientContextMenuAction::RenamePane => {
                let label = self.snapshot.as_deref().and_then(|snapshot| {
                    snapshot
                        .panes
                        .iter()
                        .find(|pane| pane.pane_id == pane_id)
                        .and_then(|pane| pane.label.clone())
                });
                self.overlay = Some(ClientShellOverlay::Rename(ClientRenameOverlay {
                    title: "rename pane",
                    input: TextEditor::new(label.as_deref().unwrap_or_default(), label.is_none()),
                    target: ClientRenameTarget::Pane { pane_id },
                }));
            }
            ClientContextMenuAction::ClearPaneName => self.push_endpoint_method(
                Method::PaneRename(PaneRenameParams {
                    pane_id,
                    label: None,
                }),
                outcome,
            ),
            ClientContextMenuAction::SwapWithFocusedPane => {
                if let Some(source_pane_id) = source_pane_id {
                    self.push_endpoint_method(
                        Method::PaneSwap(PaneSwapParams {
                            pane_id: None,
                            direction: None,
                            source_pane_id: Some(source_pane_id.clone()),
                            target_pane_id: Some(pane_id),
                        }),
                        outcome,
                    );
                    self.push_endpoint_method(
                        Method::PaneFocus(PaneTarget {
                            pane_id: source_pane_id,
                        }),
                        outcome,
                    );
                }
            }
            ClientContextMenuAction::SplitRight | ClientContextMenuAction::SplitDown => {
                self.push_endpoint_method(
                    Method::PaneSplit(PaneSplitParams {
                        workspace_id: Some(workspace_id),
                        target_pane_id: Some(pane_id),
                        direction: if action == ClientContextMenuAction::SplitRight {
                            SplitDirection::Right
                        } else {
                            SplitDirection::Down
                        },
                        ratio: None,
                        cwd: None,
                        focus: true,
                        right_click: Default::default(),
                        env: Default::default(),
                    }),
                    outcome,
                );
            }
            ClientContextMenuAction::Zoom => self.push_endpoint_method(
                Method::PaneZoom(PaneZoomParams {
                    pane_id: Some(pane_id),
                    mode: PaneZoomMode::Toggle,
                }),
                outcome,
            ),
            ClientContextMenuAction::ToggleRightClickPassthrough => self.push_endpoint_method(
                Method::PaneInputSet(PaneInputSetParams {
                    pane_id,
                    right_click: if right_click_passthrough {
                        PaneRightClickTarget::Herdr
                    } else {
                        PaneRightClickTarget::Pane
                    },
                }),
                outcome,
            ),
            ClientContextMenuAction::ClosePane => {
                self.push_endpoint_method(Method::PaneClose(PaneTarget { pane_id }), outcome)
            }
            _ => {}
        }
    }
}
