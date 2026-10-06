use crate::api::schema::{ResponseResult, SessionSnapshot};
use crate::app::App;

use super::responses::encode_success;

impl App {
    pub(super) fn handle_session_snapshot(&mut self, id: String) -> String {
        encode_success(
            id,
            ResponseResult::SessionSnapshot {
                snapshot: Box::new(self.session_snapshot()),
            },
        )
    }

    pub(crate) fn session_snapshot(&self) -> SessionSnapshot {
        let focused_workspace_id = self
            .state
            .active
            .map(|ws_idx| self.public_workspace_id(ws_idx));
        let focused_tab_id = self.state.active.and_then(|ws_idx| {
            let ws = self.state.workspaces.get(ws_idx)?;
            self.public_tab_id(ws_idx, ws.active_tab)
        });
        let focused_pane_id = self.state.active.and_then(|ws_idx| {
            let ws = self.state.workspaces.get(ws_idx)?;
            self.public_pane_id(ws_idx, ws.focused_pane_id()?)
        });

        let mut workspaces = Vec::new();
        let mut tabs = Vec::new();
        let mut layouts = Vec::new();
        for (ws_idx, ws) in self.state.workspaces.iter().enumerate() {
            let workspace = self.workspace_info(ws_idx);
            let rank = super::super::priority::Rank {
                value: workspace.sort_rank,
                parked: workspace.parked,
            };
            workspaces.push(workspace);
            for tab_idx in 0..ws.tabs.len() {
                if let Some(tab) = self.tab_info_with_rank(ws_idx, tab_idx, rank) {
                    tabs.push(tab);
                }
                if let Some(layout) = self.pane_layout_snapshot(ws_idx, tab_idx) {
                    layouts.push(layout);
                }
            }
        }

        SessionSnapshot {
            version: crate::build_info::version(),
            protocol: crate::protocol::PROTOCOL_VERSION,
            focused_workspace_id,
            focused_tab_id,
            focused_pane_id,
            workspaces,
            tabs,
            panes: {
                let started = crate::render_prof::timer();
                let panes = self.collect_panes_for_workspace(None).unwrap_or_default();
                crate::render_prof::duration_since("snapshot.panes", started);
                panes
            },
            layouts,
            agents: {
                let started = crate::render_prof::timer();
                let agents = self.collect_agent_infos();
                crate::render_prof::duration_since("snapshot.agents", started);
                agents
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::api::schema::{EmptyParams, Method, ResponseResult, SuccessResponse};
    use crate::{config::Config, workspace::Workspace};

    fn app_with_two_tabs() -> crate::app::App {
        let (_api_tx, api_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut app = crate::app::App::new(
            &Config::default(),
            crate::app::AppPolicy::TEST,
            None,
            api_rx,
            crate::api::EventHub::default(),
        );
        let mut workspace = Workspace::test_new("snapshot");
        workspace.test_add_tab(None);
        app.state.workspaces = vec![workspace];
        app.state.ensure_test_terminals();
        app.state.active = Some(0);
        app
    }

    #[test]
    fn session_snapshot_bootstraps_runtime_resources() {
        let mut app = app_with_two_tabs();
        let response = app.handle_api_request(crate::api::schema::Request {
            id: "req_snapshot".into(),
            method: Method::SessionSnapshot(EmptyParams::default()),
        });

        let success: SuccessResponse = serde_json::from_str(&response).unwrap();
        let ResponseResult::SessionSnapshot { snapshot } = success.result else {
            panic!("expected session snapshot response");
        };
        assert_eq!(success.id, "req_snapshot");
        assert_eq!(snapshot.workspaces.len(), 1);
        assert_eq!(snapshot.tabs.len(), 2);
        assert_eq!(snapshot.panes.len(), 2);
        assert_eq!(snapshot.layouts.len(), 2);
        assert_eq!(
            snapshot.focused_workspace_id.as_deref(),
            Some(snapshot.workspaces[0].workspace_id.as_str())
        );
        assert_eq!(
            snapshot.focused_tab_id.as_deref(),
            Some(snapshot.tabs[0].tab_id.as_str())
        );
        assert_eq!(
            snapshot.focused_pane_id.as_deref(),
            Some(snapshot.panes[0].pane_id.as_str())
        );
    }

    /// 2026-10-06: one chat's status in every surface comes from this
    /// snapshot. "submissions" sat idle at its prompt while the overlay still
    /// listed a finished teammate as busy; it must read done. A lane whose
    /// workflow run is live reads working although its own turn has ended.
    #[test]
    fn session_snapshot_reports_one_chat_status_per_tab_agent_and_space() {
        use crate::api::schema::AgentStatus;
        use crate::factory_overlay::{FactoryOverlay, RunTag, TabKind, TabTag};
        let mut app = app_with_two_tabs();
        let mut public = Vec::new();
        for (tab_idx, seen) in [(0, false), (1, true)] {
            let tab = &mut app.state.workspaces[0].tabs[tab_idx];
            let pane_id = tab.root_pane;
            tab.panes.get_mut(&pane_id).unwrap().seen = seen;
            let terminal_id = tab.panes[&pane_id].attached_terminal_id.clone();
            let terminal = app.state.terminals.get_mut(&terminal_id).unwrap();
            terminal.agent_name = Some(format!("claude-{tab_idx}"));
            terminal.state = crate::detect::AgentState::Idle;
            public.push(app.public_tab_id(0, tab_idx).unwrap());
        }
        let lane = |runs: Vec<RunTag>, busy: bool| TabTag {
            kind: TabKind::Lane,
            runs,
            busy,
            ..TabTag::default()
        };
        let overlay = FactoryOverlay {
            version: 1,
            tabs: [
                (
                    public[0].clone(),
                    lane(
                        vec![RunTag {
                            id: "agent:agent-atrial-chase-lists-c98d7b9ae81a05d0".into(),
                            ..RunTag::default()
                        }],
                        true,
                    ),
                ),
                (
                    public[1].clone(),
                    lane(
                        vec![RunTag {
                            id: "wf_a6b8e106-bba".into(),
                            ..RunTag::default()
                        }],
                        true,
                    ),
                ),
            ]
            .into_iter()
            .collect(),
            ..FactoryOverlay::default()
        };
        let now = std::time::SystemTime::now();
        app.set_live_factory_tabs(crate::app::live_factory_tabs(
            Some(&overlay),
            Some(now),
            now,
        ));

        let response = app.handle_api_request(crate::api::schema::Request {
            id: "req_snapshot".into(),
            method: Method::SessionSnapshot(EmptyParams::default()),
        });
        let success: SuccessResponse = serde_json::from_str(&response).unwrap();
        let ResponseResult::SessionSnapshot { snapshot } = success.result else {
            panic!("expected session snapshot response");
        };
        let tab = |id: &str| {
            snapshot
                .tabs
                .iter()
                .find(|tab| tab.tab_id == id)
                .unwrap()
                .work_status
        };
        let agent = |id: &str| {
            snapshot
                .agents
                .iter()
                .find(|agent| agent.tab_id == id)
                .unwrap()
                .work_status
        };
        assert_eq!(
            (tab(&public[0]), agent(&public[0])),
            (Some(AgentStatus::Done), Some(AgentStatus::Done))
        );
        assert_eq!(
            (tab(&public[1]), agent(&public[1])),
            (Some(AgentStatus::Working), Some(AgentStatus::Working))
        );
        assert_eq!(
            snapshot.workspaces[0].work_status,
            Some(AgentStatus::Working)
        );

        // A writer that stops leaves a stale overlay, which holds nothing working.
        let stale = now
            - crate::app::work_status::FACTORY_OVERLAY_FRESH
            - std::time::Duration::from_secs(1);
        app.set_live_factory_tabs(crate::app::live_factory_tabs(
            Some(&overlay),
            Some(stale),
            now,
        ));
        assert_eq!(
            app.session_snapshot().tabs[1].work_status,
            Some(AgentStatus::Idle)
        );
    }
}
