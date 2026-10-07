use std::time::{Duration, Instant};

use bytes::Bytes;

use crate::api::schema::{
    AgentGroupCollapseParams, AgentGroupSetParams, AgentOwnerSetParams, AgentPromptParams,
    AgentRenameParams, AgentRestartParams, AgentResumeParams, AgentSendKeysParams,
    AgentStartParams, AgentTarget, PaneReadResult, ResponseResult,
};
use crate::app::agent_resume::InPlaceAgentResumeError;
use crate::app::App;

use super::responses::{encode_error, encode_error_body, encode_send_accepted, encode_success};

const AGENT_PROMPT_SUBMIT_DELAY: Duration = Duration::from_millis(300);

// Codex's Windows input reader does not surface bracketed paste. It detects the prompt as a
// "paste burst" and, while that burst is buffered, rewrites a following Enter into a newline
// instead of submitting. The burst only flushes after an idle timeout, so any size-based delay is
// a timing guess that fails when ConPTY delivery lags it. Codex flushes a buffered burst
// synchronously when it receives a non-character key, so appending one after the paste gives the
// submission a deterministic paste boundary regardless of prompt size or delivery speed.
#[cfg(windows)]
fn append_codex_paste_boundary(runtime: &crate::terminal::TerminalRuntime, text: &mut Vec<u8>) {
    let keys = match crate::app::api_helpers::encode_api_keys(runtime, &["right".to_string()]) {
        Ok(keys) => keys,
        Err(key) => {
            tracing::warn!(key = %key, "failed to encode Codex paste boundary key");
            return;
        }
    };
    if let Some(key) = keys.into_iter().find(|bytes| !bytes.is_empty()) {
        text.extend_from_slice(&key);
    }
}

type QueuedAgentPrompt = (
    String,
    crate::api::schema::AgentInfo,
    std::sync::mpsc::Receiver<std::io::Result<()>>,
    crate::terminal::polite_send::SendOutcome,
    Option<crate::api::schema::AgentPromptDeliveryAck>,
);

impl App {
    pub(super) fn handle_agents_list(&mut self, id: String) -> String {
        let agents =
            self.state
                .pinned_tabs
                .iter()
                .enumerate()
                .filter(|(_, pin)| pin.role == Some(crate::api::schema::TabRole::Agent))
                .filter_map(|(pin_index, pin)| {
                    let (ws_idx, tab_idx) = self.parse_tab_id(&pin.tab_id)?;
                    let info = self.tab_info(ws_idx, tab_idx)?;
                    let tab = self.state.workspaces.get(ws_idx)?.tabs.get(tab_idx)?;
                    let plugin_pane =
                        tab.layout.pane_ids().into_iter().find_map(|id| {
                            self.state.plugin_panes.get(&id).map(|record| (id, record))
                        });
                    let pane_id = plugin_pane.map(|(id, _)| id).or_else(|| {
                        tab.panes
                            .get_key_value(&tab.layout.focused())
                            .map(|(id, _)| *id)
                            .or_else(|| tab.layout.pane_ids().first().copied())
                    })?;
                    let plugin_id = plugin_pane.map(|(_, record)| record.plugin_id.clone());
                    let state_dir = plugin_id.as_deref().map(|id| {
                        crate::plugin_paths::plugin_state_dir(id)
                            .to_string_lossy()
                            .into_owned()
                    });
                    let session_id = tab
                        .panes
                        .get(&pane_id)
                        .and_then(|pane| self.state.terminals.get(&pane.attached_terminal_id))
                        .and_then(|terminal| terminal.current_agent_session())
                        .filter(|session| {
                            session.session_ref.kind == crate::agent_resume::AgentSessionRefKind::Id
                        })
                        .map(|session| session.session_ref.value);
                    Some(crate::api::schema::PinnedAgentInfo {
                        tab_id: info.tab_id,
                        workspace_id: info.workspace_id,
                        label: info.label,
                        pin_index,
                        pane_id: self.public_pane_id(ws_idx, pane_id)?,
                        agent_status: info.agent_status,
                        plugin_id,
                        session_id,
                        state_dir,
                    })
                })
                .collect();
        encode_success(id, ResponseResult::AgentsList { agents })
    }

    pub(super) fn handle_agent_list(&mut self, id: String) -> String {
        encode_success(
            id,
            ResponseResult::AgentList {
                agents: self.collect_agent_infos(),
            },
        )
    }

    pub(super) fn handle_agent_usage(&mut self, id: String) -> String {
        match self.collect_agent_usage() {
            Ok(usage) => encode_success(id, ResponseResult::AgentUsage { usage }),
            Err(err) => encode_error(
                id,
                "process_snapshot_failed",
                format!("failed to sample process usage: {err}"),
            ),
        }
    }

    pub(super) fn handle_agent_get(&mut self, id: String, target: AgentTarget) -> String {
        self.reconcile_managed_agent_target(&target.target);
        let agent = match self.agent_info_for_target(&target.target) {
            Ok(agent) => agent,
            Err(err) => return encode_error_body(id, self.agent_target_error_body(err)),
        };

        encode_success(id, ResponseResult::AgentInfo { agent })
    }

    pub(super) fn handle_agent_focus(&mut self, id: String, target: AgentTarget) -> String {
        let agent = match self.focus_agent_target(&target.target) {
            Ok(agent) => agent,
            Err(err) => return encode_error_body(id, self.agent_target_error_body(err)),
        };

        encode_success(id, ResponseResult::AgentInfo { agent })
    }

    pub(super) fn handle_agent_rename(&mut self, id: String, params: AgentRenameParams) -> String {
        let agent = match self.rename_agent_target(&params.target, params.name) {
            Ok(agent) => agent,
            Err(err) => return encode_error_body(id, self.agent_rename_error_body(err)),
        };

        encode_success(id, ResponseResult::AgentInfo { agent })
    }

    pub(super) fn handle_agent_start(&mut self, id: String, params: AgentStartParams) -> String {
        let (agent, argv) = match self.start_agent(params) {
            Ok(started) => started,
            Err(err) => return encode_error_body(id, self.agent_start_error_body(err)),
        };

        encode_success(id, ResponseResult::AgentStarted { agent, argv })
    }

    pub(super) fn handle_agent_restart(
        &mut self,
        id: String,
        params: AgentRestartParams,
    ) -> String {
        let Some((ws_idx, pane_id)) = self.parse_pane_id(&params.pane_id) else {
            return encode_error(id, "not_resumable", "pane not found");
        };
        match self.restart_agent_in_place(ws_idx, pane_id, params.force, None) {
            Ok(resumed) => encode_success(
                id,
                ResponseResult::AgentRestarted {
                    ok: true,
                    command_summary: crate::agent_resume::restart_command_summary(&resumed.argv),
                },
            ),
            Err(error) => {
                let (code, message) = match error {
                    InPlaceAgentResumeError::SessionUnknown(message) => ("no_session", message),
                    InPlaceAgentResumeError::Busy(message) => ("busy", message),
                    InPlaceAgentResumeError::NotResumable(message) => ("not_resumable", message),
                    InPlaceAgentResumeError::ArgvUnsupported(message) => ("unsupported", message),
                    InPlaceAgentResumeError::PaneNotFound | InPlaceAgentResumeError::NotRunning => {
                        ("not_resumable", "pane is not running".into())
                    }
                    InPlaceAgentResumeError::Failed(_) => {
                        ("unsupported", "could not restart agent".into())
                    }
                };
                encode_error(id, code, message)
            }
        }
    }

    pub(super) fn handle_agent_resume(&mut self, id: String, params: AgentResumeParams) -> String {
        let Some((ws_idx, pane_id)) = self.parse_pane_id(&params.pane_id) else {
            return encode_error(id, "pane_not_found", "pane not found");
        };
        let quiet = params.input_quiet_ms.map(Duration::from_millis);
        match self.resume_agent_in_place(ws_idx, pane_id, quiet) {
            Ok(resumed) => encode_success(
                id,
                ResponseResult::AgentResumed {
                    pane_id: self
                        .public_pane_id(ws_idx, pane_id)
                        .unwrap_or(params.pane_id),
                    agent: resumed.agent,
                    session_id: resumed.session_id,
                    argv: resumed.argv,
                },
            ),
            Err(InPlaceAgentResumeError::PaneNotFound) => {
                encode_error(id, "pane_not_found", "pane not found")
            }
            Err(InPlaceAgentResumeError::SessionUnknown(message)) => {
                encode_error(id, "agent_session_unknown", message)
            }
            Err(InPlaceAgentResumeError::Busy(message)) => encode_error(id, "agent_busy", message),
            Err(InPlaceAgentResumeError::NotResumable(message)) => {
                encode_error(id, "not_resumable", message)
            }
            Err(InPlaceAgentResumeError::ArgvUnsupported(message)) => {
                encode_error(id, "agent_argv_unsupported", message)
            }
            Err(InPlaceAgentResumeError::NotRunning) => encode_error(
                id,
                "agent_not_running",
                format!("pane {} has no running terminal", params.pane_id),
            ),
            Err(InPlaceAgentResumeError::Failed(message)) => {
                encode_error(id, "agent_resume_failed", message)
            }
        }
    }

    pub(super) fn handle_agent_owner_set(
        &mut self,
        id: String,
        params: AgentOwnerSetParams,
    ) -> String {
        let agent = match self.set_agent_owner_target(&params.target, &params.owner) {
            Ok(agent) => agent,
            Err(err) => return encode_error_body(id, self.agent_owner_error_body(err)),
        };

        encode_success(id, ResponseResult::AgentInfo { agent })
    }

    pub(super) fn handle_agent_owner_clear(&mut self, id: String, target: AgentTarget) -> String {
        let agent = match self.clear_agent_owner_target(&target.target) {
            Ok(agent) => agent,
            Err(err) => return encode_error_body(id, self.agent_owner_error_body(err)),
        };

        encode_success(id, ResponseResult::AgentInfo { agent })
    }

    pub(super) fn handle_agent_group_set(
        &mut self,
        id: String,
        params: AgentGroupSetParams,
    ) -> String {
        let agent = match self.set_agent_group_target(
            &params.target,
            params.placement,
            params.parent.as_deref(),
        ) {
            Ok(agent) => agent,
            Err(err) => return encode_error_body(id, self.agent_owner_error_body(err)),
        };

        encode_success(id, ResponseResult::AgentInfo { agent })
    }

    pub(super) fn handle_agent_group_collapse(
        &mut self,
        id: String,
        params: AgentGroupCollapseParams,
    ) -> String {
        let agent = match self.set_agent_group_collapsed_target(&params.target, params.collapsed) {
            Ok(agent) => agent,
            Err(err) => return encode_error_body(id, self.agent_owner_error_body(err)),
        };

        encode_success(id, ResponseResult::AgentInfo { agent })
    }

    pub(crate) fn handle_deferred_agent_api_request(
        &mut self,
        request: crate::api::schema::Request,
        respond_to: std::sync::mpsc::Sender<String>,
    ) -> bool {
        let crate::api::schema::Method::AgentPrompt(params) = request.method else {
            return false;
        };
        match self.queue_agent_prompt(request.id, params) {
            Ok((id, agent, completion, position, delivery)) => {
                let dropped = matches!(
                    position.state,
                    crate::api::schema::PaneSendState::Dropped
                        | crate::api::schema::PaneSendState::StaleSession
                );
                // A guarded prompt answers at once with its receipt (possibly
                // `handed_to_writer`); the caller polls it, so a writer held
                // up by backpressure never blocks the request.
                if position.position.is_some() || dropped || delivery.is_some() {
                    let (state, reason) =
                        crate::terminal::polite_send::recent_sends(None, Some(&position.id))
                            .into_iter()
                            .next()
                            .map_or((position.state, None), |item| (item.state, item.reason));
                    let _ = respond_to.send(encode_success(
                        id,
                        ResponseResult::AgentPrompted {
                            agent,
                            queued: position.position.is_some(),
                            dropped,
                            queue_position: position.position,
                            id: position.id,
                            state,
                            reason,
                            delivery,
                        },
                    ));
                    return true;
                }
                std::thread::spawn(move || {
                    let response = match completion.recv() {
                        Ok(Ok(())) => encode_success(
                            id,
                            ResponseResult::AgentPrompted {
                                agent,
                                queued: false,
                                dropped: false,
                                queue_position: None,
                                id: position.id,
                                state: crate::api::schema::PaneSendState::Delivered,
                                reason: None,
                                delivery: None,
                            },
                        ),
                        Ok(Err(err)) if err.kind() == std::io::ErrorKind::TimedOut => {
                            encode_error(id, "timeout", err.to_string())
                        }
                        Ok(Err(err)) => encode_error(id, "agent_prompt_failed", err.to_string()),
                        Err(_) => encode_error(id, "agent_prompt_failed", "pty actor closed"),
                    };
                    let _ = respond_to.send(response);
                });
            }
            Err(response) => {
                let _ = respond_to.send(response);
            }
        }
        true
    }

    fn queue_agent_prompt(
        &mut self,
        id: String,
        params: AgentPromptParams,
    ) -> Result<QueuedAgentPrompt, String> {
        if params.text.is_empty() {
            return Err(encode_error(
                id,
                "empty_agent_prompt",
                "agent prompt must not be empty",
            ));
        }
        let resolved = match self.resolve_agent_target(&params.target) {
            Ok(resolved) => resolved,
            Err(err) => return Err(encode_error_body(id, self.agent_target_error_body(err))),
        };
        let Some(terminal_id) = self
            .state
            .workspaces
            .get(resolved.ws_idx)
            .and_then(|workspace| workspace.terminal_id(resolved.pane_id))
            .cloned()
        else {
            return Err(agent_not_found(id, &params.target));
        };
        let Some(terminal) = self.state.terminals.get(&terminal_id) else {
            return Err(agent_not_found(id, &params.target));
        };
        if terminal.state == crate::detect::AgentState::Blocked {
            return Err(encode_error(
                id,
                "agent_blocked",
                format!(
                    "agent {} is blocked and requires interactive input",
                    params.target
                ),
            ));
        }
        let Some(expected_agent) = terminal.effective_known_agent() else {
            return Err(agent_not_ready(id, &params.target));
        };
        if terminal.managed_agent_launch_pending() {
            return Err(agent_not_ready(id, &params.target));
        }
        let Some(runtime) = self.lookup_runtime_sender(resolved.ws_idx, resolved.pane_id) else {
            return Err(agent_not_found(id, &params.target));
        };
        if !super::super::agents::runtime_hosts_agent(runtime, expected_agent) {
            return Err(encode_error(
                id,
                "agent_not_ready",
                format!(
                    "agent {} is no longer the pane foreground process",
                    params.target
                ),
            ));
        }
        #[cfg(windows)]
        let submit_deadline = params
            .wait
            .as_ref()
            .and_then(|wait| wait.submission_deadline);
        #[cfg(not(windows))]
        let submit_deadline = None;
        let mut focus_bytes = Vec::new();
        if expected_agent == crate::detect::Agent::GithubCopilot {
            // Copilot ignores synthetic Enter after focus loss until it receives focus gained.
            let focus = match crate::ghostty::encode_focus(crate::ghostty::FocusEvent::Gained) {
                Ok(focus) => focus,
                Err(err) => {
                    return Err(encode_error(id, "agent_prompt_failed", err.to_string()));
                }
            };
            focus_bytes = focus;
        }
        let (text, enter) =
            crate::app::api_helpers::encode_api_submission_parts(runtime, &params.text);
        #[cfg(windows)]
        let text = if expected_agent == crate::detect::Agent::Codex {
            let mut text = text;
            append_codex_paste_boundary(runtime, &mut text);
            text
        } else {
            text
        };
        let Some(agent) = self.agent_info(resolved.ws_idx, resolved.pane_id) else {
            return Err(agent_not_found(id, &params.target));
        };
        let ack =
            params
                .delivery
                .as_ref()
                .map(|delivery| crate::api::schema::AgentPromptDeliveryAck {
                    session_id: delivery.session_id.clone(),
                    runtime_id: delivery.runtime_id.clone(),
                });
        let guard = params.delivery.as_ref().map(|delivery| {
            let guard = crate::terminal::polite_send::DeliveryGuard {
                session_id: delivery.session_id.clone(),
                runtime_id: delivery.runtime_id.clone(),
                agent: expected_agent,
                input_quiet: Duration::from_millis(delivery.input_quiet_ms),
                expires_at: Instant::now() + Duration::from_millis(delivery.expires_ms),
            };
            let verdict = self.agent_delivery_verdict(resolved.ws_idx, resolved.pane_id, &guard);
            (guard, verdict)
        });
        let (completion_tx, completion) = std::sync::mpsc::channel();
        let position = runtime
            .polite_send_with_guard(
                self.polite_guarded(resolved.ws_idx, resolved.pane_id),
                self.polite_send_quiet,
                "agent.prompt",
                crate::terminal::polite_send::Payload::Submission {
                    focus: Bytes::from(focus_bytes),
                    text: Bytes::from(text),
                    enter: Bytes::from(enter),
                    delay: AGENT_PROMPT_SUBMIT_DELAY,
                    deadline: submit_deadline,
                    completion: completion_tx,
                },
                self.polite_options(resolved.ws_idx, resolved.pane_id, params.if_idle, false),
                guard,
            )
            .map_err(|err| encode_error(id.clone(), "agent_prompt_failed", err.to_string()))?;
        Ok((id, agent, completion, position, ack))
    }

    pub(super) fn handle_agent_read(
        &mut self,
        id: String,
        params: crate::api::schema::AgentReadParams,
    ) -> String {
        let resolved = match self.resolve_agent_target(&params.target) {
            Ok(resolved) => resolved,
            Err(err) => return encode_error_body(id, self.agent_target_error_body(err)),
        };
        let Some((pane, workspace_id)) = self.lookup_runtime(resolved.ws_idx, resolved.pane_id)
        else {
            return agent_not_found(id, &params.target);
        };
        let snapshot = crate::app::api_helpers::read_terminal_snapshot(
            pane,
            params.source,
            params.format,
            params.lines,
        );

        encode_success(
            id,
            ResponseResult::PaneRead {
                read: PaneReadResult {
                    pane_id: self
                        .public_pane_id(resolved.ws_idx, resolved.pane_id)
                        .unwrap_or_else(|| params.target.clone()),
                    workspace_id,
                    tab_id: self
                        .public_tab_id(resolved.ws_idx, resolved.tab_idx)
                        .unwrap(),
                    source: params.source,
                    format: params.format,
                    text: snapshot.text,
                    revision: 0,
                    truncated: snapshot.truncated,
                },
            },
        )
    }

    pub(super) fn handle_agent_explain(&mut self, id: String, target: AgentTarget) -> String {
        let resolved = match self.resolve_agent_target(&target.target) {
            Ok(resolved) => resolved,
            Err(err) => return encode_error_body(id, self.agent_target_error_body(err)),
        };
        let Some((pane, _workspace_id)) = self.lookup_runtime(resolved.ws_idx, resolved.pane_id)
        else {
            return agent_not_found(id, &target.target);
        };
        let Some(terminal_id) = self
            .state
            .workspaces
            .get(resolved.ws_idx)
            .and_then(|workspace| workspace.terminal_id(resolved.pane_id))
        else {
            return agent_not_found(id, &target.target);
        };
        let Some(terminal) = self.state.terminals.get(terminal_id) else {
            return agent_not_found(id, &target.target);
        };
        if terminal.full_lifecycle_hook_authority_active() {
            let explain = serde_json::json!({
                "agent": terminal.effective_agent_label().unwrap_or("unknown"),
                "state": crate::detect::manifest::agent_state_label(terminal.state),
                "manifest_source": null,
                "manifest_version": null,
                "cached_remote_version": null,
                "local_override_shadowing_remote": false,
                "remote_update_status": null,
                "remote_update_error": null,
                "matched_rule": null,
                "visible_idle": false,
                "visible_blocker": false,
                "visible_working": false,
                "screen_detection_skipped": true,
                "screen_detection_skip_reason": "full_lifecycle_hook_authority",
                "skip_state_update": false,
                "skipped_update_reason": null,
                "fallback_reason": null,
                "warning": null,
                "evaluated_rules": [],
            });
            return encode_success(id, ResponseResult::AgentExplain { explain });
        }
        let Some(agent) = terminal.effective_known_agent().or(terminal.detected_agent) else {
            return encode_error(
                id,
                "agent_explain_unavailable",
                format!(
                    "agent target {} does not have a detected agent label",
                    target.target
                ),
            );
        };

        let screen = pane.detection_text();
        let osc_title = pane.agent_osc_title();
        let osc_progress = pane.agent_osc_progress();
        let explain = crate::detect::manifest::explain_with_input(
            agent,
            crate::detect::manifest::DetectionInput {
                screen: &screen,
                osc_title: &osc_title,
                osc_progress: &osc_progress,
            },
        );
        let value = crate::detect::manifest::explain_to_json_value(&explain);

        encode_success(id, ResponseResult::AgentExplain { explain: value })
    }

    pub(super) fn handle_agent_send_keys(
        &mut self,
        id: String,
        params: AgentSendKeysParams,
    ) -> String {
        let resolved = match self.resolve_agent_target(&params.target) {
            Ok(resolved) => resolved,
            Err(err) => return encode_error_body(id, self.agent_target_error_body(err)),
        };
        let Some(terminal_id) = self
            .state
            .workspaces
            .get(resolved.ws_idx)
            .and_then(|workspace| workspace.terminal_id(resolved.pane_id))
        else {
            return agent_not_found(id, &params.target);
        };
        let Some(expected_agent) = self
            .state
            .terminals
            .get(terminal_id)
            .and_then(|terminal| terminal.effective_known_agent())
        else {
            return agent_not_ready(id, &params.target);
        };
        let Some(runtime) = self.lookup_runtime_sender(resolved.ws_idx, resolved.pane_id) else {
            return agent_not_found(id, &params.target);
        };
        if !super::super::agents::runtime_hosts_agent(runtime, expected_agent) {
            return agent_not_ready(id, &params.target);
        }
        let encoded = match super::super::api_helpers::encode_api_keys(runtime, &params.keys) {
            Ok(encoded) => encoded,
            Err(key) => {
                return encode_error(id, "invalid_key", format!("unsupported key {key}"));
            }
        };
        let bytes: Vec<u8> = encoded.into_iter().flatten().collect();
        match self.send_polite_bytes(
            resolved.ws_idx,
            resolved.pane_id,
            "agent.send_keys",
            Bytes::from(bytes),
            false,
            false,
        ) {
            Ok(position) => encode_send_accepted(id, position),
            Err(err) => encode_error(id, "agent_send_keys_failed", err.to_string()),
        }
    }
}

fn agent_not_ready(id: String, target: &str) -> String {
    encode_error(
        id,
        "agent_not_ready",
        format!("agent {target} is not an active named agent"),
    )
}

fn agent_not_found(id: String, target: &str) -> String {
    encode_error(
        id,
        "agent_not_found",
        format!("agent target {target} not found"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        api::schema::{AgentGroupPlacementKind, AgentStatus, SuccessResponse},
        app::Mode,
        config::Config,
        detect::{Agent, AgentState},
        workspace::Workspace,
    };

    #[test]
    fn agents_list_returns_only_pinned_agents_in_pin_order() {
        let mut app = app_with_agent();
        let ws = &mut app.state.workspaces[0];
        ws.test_add_tab(Some("bound"));
        ws.test_add_tab(Some("plain"));
        ws.test_add_tab(Some("unpinned"));
        let first = app.public_tab_id(0, 0).unwrap();
        let bound = app.public_tab_id(0, 1).unwrap();
        let plain = app.public_tab_id(0, 2).unwrap();
        app.state.pinned_tabs = vec![
            crate::app::state::PinnedTab {
                tab_id: bound.clone(),
                priority: 0,
                role: Some(crate::api::schema::TabRole::Agent),
            },
            crate::app::state::PinnedTab {
                tab_id: plain,
                priority: 0,
                role: None,
            },
            crate::app::state::PinnedTab {
                tab_id: first.clone(),
                priority: 0,
                role: Some(crate::api::schema::TabRole::Agent),
            },
        ];
        let pane = app.state.workspaces[0].tabs[1].root_pane;
        app.state.workspaces[0].active_tab = 1;
        let focused = app.state.workspaces[0].test_split(ratatui::layout::Direction::Horizontal);
        assert_ne!(focused, pane);
        app.state.ensure_test_terminals();
        for (id, session_id) in [(pane, "plugin-session"), (focused, "focused-session")] {
            let terminal_id = app.state.workspaces[0].terminal_id(id).unwrap().clone();
            app.state
                .terminals
                .get_mut(&terminal_id)
                .unwrap()
                .persisted_agent_session = Some(crate::agent_resume::PersistedAgentSession {
                source: "herdr:claude".into(),
                agent: "claude".into(),
                session_ref: crate::agent_resume::AgentSessionRef::id(session_id).unwrap(),
            });
        }
        app.state.plugin_panes.insert(
            pane,
            crate::app::state::PluginPaneRecord {
                plugin_id: "test-agent".into(),
                entrypoint: "main".into(),
            },
        );
        let request = crate::cli::pinned_agents_request();
        assert_eq!(
            serde_json::to_value(&request).unwrap()["method"],
            "agents.list"
        );
        let response = app.handle_api_request(request);
        let value: serde_json::Value = serde_json::from_str(&response).unwrap();
        let agents = value["result"]["agents"].as_array().unwrap();
        assert_eq!(agents.len(), 2);
        assert_eq!(agents[0]["tab_id"], bound);
        assert_eq!(agents[0]["pin_index"], 0);
        assert_eq!(agents[0]["pane_id"], app.public_pane_id(0, pane).unwrap());
        assert_eq!(agents[0]["plugin_id"], "test-agent");
        assert_eq!(agents[0]["session_id"], "plugin-session");
        assert_eq!(
            agents[0]["state_dir"],
            crate::plugin_paths::plugin_state_dir("test-agent")
                .to_string_lossy()
                .as_ref()
        );
        assert_eq!(agents[1]["tab_id"], first);
        assert_eq!(agents[1]["pin_index"], 2);
        assert!(agents[1]["plugin_id"].is_null());
        assert!(agents[1]["state_dir"].is_null());
        assert!(agents[1]["session_id"].is_null());

        let terminal = app.state.terminals.values_mut().next().unwrap();
        terminal.set_detected_state(Some(Agent::Codex), AgentState::Idle);
        let legacy = app.handle_api_request(crate::api::schema::Request {
            id: "cli:agent:list".into(),
            method: crate::api::schema::Method::AgentList(Default::default()),
        });
        let legacy: serde_json::Value = serde_json::from_str(&legacy).unwrap();
        let legacy_agents = legacy["result"]["agents"].as_array().unwrap();
        assert_eq!(legacy_agents.len(), 1);
        assert!(legacy_agents[0]["terminal_id"].is_string());
        assert!(legacy_agents[0].get("pin_index").is_none());
        assert!(legacy_agents[0].get("plugin_id").is_none());
    }

    fn app_with_agent() -> App {
        let (_api_tx, api_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut app = App::new(
            &Config::default(),
            crate::app::AppPolicy::TEST,
            None,
            api_rx,
            crate::api::EventHub::default(),
        );
        app.state.workspaces = vec![Workspace::test_new("agent")];
        app.state.ensure_test_terminals();
        app.state.active = Some(0);
        app.state.selected = 0;
        app.state.mode = Mode::Terminal;
        app
    }

    fn start_deferred_agent_prompt(
        app: &mut App,
        id: &str,
        params: AgentPromptParams,
    ) -> std::sync::mpsc::Receiver<String> {
        let (respond_to, response_rx) = std::sync::mpsc::channel();
        assert!(app.handle_deferred_agent_api_request(
            crate::api::schema::Request {
                id: id.into(),
                method: crate::api::schema::Method::AgentPrompt(params),
            },
            respond_to,
        ));
        response_rx
    }

    fn run_deferred_agent_prompt(app: &mut App, id: &str, params: AgentPromptParams) -> String {
        start_deferred_agent_prompt(app, id, params)
            .recv_timeout(Duration::from_secs(1))
            .expect("agent prompt responds after submission")
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn windows_codex_prompt_flushes_paste_burst_before_enter() {
        let mut app = app_with_agent();
        let pane_id = app.state.workspaces[0].tabs[0].root_pane;
        let terminal_id = app.state.workspaces[0].tabs[0].panes[&pane_id]
            .attached_terminal_id
            .clone();
        let terminal = app.state.terminals.get_mut(&terminal_id).unwrap();
        terminal.set_agent_name("reviewer".into());
        terminal.set_detected_state(Some(Agent::Codex), AgentState::Idle);
        let (runtime, mut rx) = crate::terminal::TerminalRuntime::test_with_channel(80, 24);
        app.state.insert_test_runtime(pane_id, runtime);

        let response = run_deferred_agent_prompt(
            &mut app,
            "req",
            AgentPromptParams {
                if_idle: false,
                target: "reviewer".into(),
                text: "A != B".into(),
                wait: None,
                delivery: None,
            },
        );
        let success: SuccessResponse = serde_json::from_str(&response).unwrap();
        assert!(matches!(
            success.result,
            ResponseResult::AgentPrompted { .. }
        ));
        // The non-character key must precede Enter so Codex commits the paste burst first.
        assert_eq!(rx.try_recv().unwrap(), Bytes::from_static(b"A != B\x1b[C"));
        assert_eq!(rx.try_recv().unwrap(), Bytes::from_static(b"\r"));
    }

    #[tokio::test]
    async fn a_false_process_exit_makes_a_named_live_agent_unreachable_by_name() {
        // Reproduces the registration loss reported on #3225 by rszrszrsz:
        // a live agent pane with an assigned name stops resolving by that name
        // while its process keeps running, and renaming is the only recovery.
        let mut app = app_with_agent();
        let pane_id = app.state.workspaces[0].tabs[0].root_pane;
        let terminal_id = app.state.workspaces[0].tabs[0].panes[&pane_id]
            .attached_terminal_id
            .clone();
        let observed_at = std::time::Instant::now();
        let terminal = app.state.terminals.get_mut(&terminal_id).unwrap();
        terminal.set_detected_state(Some(Agent::Pi), AgentState::Working);
        terminal.set_agent_name("reviewer".into());

        let found = app.handle_agent_get(
            "req:before".into(),
            AgentTarget {
                target: "reviewer".into(),
            },
        );
        assert!(
            serde_json::from_str::<SuccessResponse>(&found).is_ok(),
            "the assigned name must resolve while the agent is running: {found}"
        );

        // One process-exit observation, then the same agent is observed alive
        // again on the next probe - the process never actually went away.
        app.handle_internal_event(crate::events::AppEvent::StateChanged {
            pane_id,
            agent: Some(Agent::Pi),
            state: AgentState::Idle,
            visible_blocker: false,
            visible_working: false,
            process_exited: true,
            observed_at,
            runtime_pid: None,
        });
        app.handle_internal_event(crate::events::AppEvent::AgentProcessDetected {
            pane_id,
            agent: Agent::Pi,
            observed_at: observed_at + std::time::Duration::from_secs(1),
            runtime_pid: None,
        });

        let terminal = &app.state.terminals[&terminal_id];
        assert_eq!(
            terminal.detected_agent,
            Some(Agent::Pi),
            "the agent process is still there"
        );

        let after = app.handle_agent_get(
            "req:after".into(),
            AgentTarget {
                target: "reviewer".into(),
            },
        );
        assert!(
            serde_json::from_str::<SuccessResponse>(&after).is_ok(),
            "a live agent must stay reachable by its assigned name: {after}"
        );
    }

    #[tokio::test]
    async fn polite_queue_id_api_returns_receipt_without_payload() {
        let mut app = app_with_agent();
        app.polite_send_mode = crate::config::PoliteSendConfig::All;
        let pane = app.state.workspaces[0].tabs[0].root_pane;
        let (runtime, _rx) = crate::terminal::TerminalRuntime::test_with_channel(80, 24);
        runtime.record_human_text();
        app.state.insert_test_runtime(pane, runtime);
        let public = app.public_pane_id(0, pane).unwrap();
        let response = app.handle_api_request(crate::api::schema::Request {
            id: "send".into(),
            method: crate::api::schema::Method::PaneSendText(
                crate::api::schema::PaneSendTextParams {
                    pane_id: public.clone(),
                    text: "never-store-this-payload".into(),
                    if_idle: false,
                    human: false,
                },
            ),
        });
        let response: serde_json::Value = serde_json::from_str(&response).unwrap();
        let qid = response["result"]["id"].as_str().unwrap();
        assert_eq!(response["result"]["state"], "queued");
        let response = app.handle_api_request(crate::api::schema::Request {
            id: "query".into(),
            method: crate::api::schema::Method::PaneQueue(crate::api::schema::PaneQueueParams {
                pane_id: String::new(),
                id: Some(qid.into()),
                flush: false,
                cancel: false,
            }),
        });
        assert!(!response.contains("never-store-this-payload"));
        let response: serde_json::Value = serde_json::from_str(&response).unwrap();
        assert_eq!(response["result"]["recent"].as_array().unwrap().len(), 1);
        assert_eq!(response["result"]["recent"][0]["id"], qid);
        assert_eq!(response["result"]["recent"][0]["byte_length"], 24);
    }

    /// A session-bound agent.prompt goes through the real polite-send queue:
    /// held while the human typed recently (even input that parses to no
    /// key) or the agent works, held through a forced flush, delivered once
    /// idle and quiet, refused or dropped when the session changes, and
    /// expired at its deadline.
    #[tokio::test]
    async fn session_bound_agent_prompt_waits_for_idle_quiet_and_same_session() {
        let mut app = app_with_agent();
        let pane = app.state.workspaces[0].tabs[0].root_pane;
        let terminal_id = app.state.workspaces[0].terminal_id(pane).unwrap().clone();
        let set_session = |app: &mut App, session: &str| {
            app.state
                .terminals
                .get_mut(&terminal_id)
                .unwrap()
                .persisted_agent_session = Some(crate::agent_resume::PersistedAgentSession {
                source: "herdr:claude".into(),
                agent: "claude".into(),
                session_ref: crate::agent_resume::AgentSessionRef::id(session).unwrap(),
            });
        };
        let set_state = |app: &mut App, state: AgentState| {
            app.state
                .terminals
                .get_mut(&terminal_id)
                .unwrap()
                .set_detected_state(Some(Agent::Claude), state);
        };
        set_state(&mut app, AgentState::Idle);
        set_session(&mut app, "sess-1");
        let (runtime, mut rx) = crate::terminal::TerminalRuntime::test_with_channel(80, 24);
        // F13: complete, but parses to no key.
        runtime.record_human_bytes(b"\x1b[25~");
        app.state.insert_test_runtime(pane, runtime);
        let public = app.public_pane_id(0, pane).unwrap();
        let prompt = |text: &str, session: &str, expires_ms: u64| AgentPromptParams {
            if_idle: false,
            target: public.clone(),
            text: text.into(),
            wait: None,
            delivery: Some(crate::api::schema::AgentPromptDelivery {
                session_id: session.into(),
                runtime_id: None,
                input_quiet_ms: 300,
                expires_ms,
            }),
        };
        let force_flush = |app: &mut App| {
            app.handle_api_request(crate::api::schema::Request {
                id: "flush".into(),
                method: crate::api::schema::Method::PaneQueue(
                    crate::api::schema::PaneQueueParams {
                        pane_id: public.clone(),
                        id: None,
                        flush: true,
                        cancel: false,
                    },
                ),
            });
        };
        let state_of = |id: &str| {
            crate::terminal::polite_send::recent_sends(None, Some(id))
                .into_iter()
                .next()
                .map(|item| (item.state, item.reason))
                .unwrap()
        };
        let queued = |response: String| {
            let response: serde_json::Value = serde_json::from_str(&response).unwrap();
            assert_eq!(response["result"]["state"], "queued", "{response}");
            response["result"]["id"].as_str().unwrap().to_string()
        };

        let first = queued(run_deferred_agent_prompt(
            &mut app,
            "p1",
            prompt("reload-line", "sess-1", 10_000),
        ));
        // A human submit starts a turn: still inside the quiet interval,
        // then working, so even a forced flush holds the line.
        app.lookup_runtime_sender(0, pane)
            .unwrap()
            .record_human_bytes(b"\r");
        set_state(&mut app, AgentState::Working);
        force_flush(&mut app);
        tokio::time::sleep(Duration::from_millis(400)).await;
        force_flush(&mut app);
        assert!(rx.try_recv().is_err(), "held line reached a working agent");
        assert_eq!(
            state_of(&first).0,
            crate::api::schema::PaneSendState::Queued
        );
        set_state(&mut app, AgentState::Idle);
        force_flush(&mut app);
        let deadline = Instant::now() + Duration::from_secs(3);
        while state_of(&first).0 == crate::api::schema::PaneSendState::Queued
            && Instant::now() < deadline
        {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let written = rx.try_recv().expect("idle, quiet agent receives the line");
        assert!(
            String::from_utf8_lossy(&written).contains("reload-line"),
            "{written:?}"
        );
        while rx.try_recv().is_ok() {}

        // The session changes while the line is held: it is never delivered.
        set_state(&mut app, AgentState::Working);
        let second = queued(run_deferred_agent_prompt(
            &mut app,
            "p2",
            prompt("stale-line", "sess-1", 10_000),
        ));
        set_session(&mut app, "sess-2");
        set_state(&mut app, AgentState::Idle);
        tokio::time::sleep(Duration::from_millis(400)).await;
        force_flush(&mut app);
        assert_eq!(
            state_of(&second),
            (
                crate::api::schema::PaneSendState::StaleSession,
                Some("agent_session_changed".into())
            )
        );
        // Bound to a session the pane no longer runs: refused at once.
        let refused = run_deferred_agent_prompt(&mut app, "p3", prompt("old", "sess-1", 10_000));
        let refused: serde_json::Value = serde_json::from_str(&refused).unwrap();
        assert_eq!(refused["result"]["state"], "stale_session", "{refused}");
        assert_eq!(refused["result"]["dropped"], true);

        // Held past its deadline: dropped as expired, never delivered.
        set_state(&mut app, AgentState::Working);
        let third = queued(run_deferred_agent_prompt(
            &mut app,
            "p4",
            prompt("late-line", "sess-2", 50),
        ));
        tokio::time::sleep(Duration::from_millis(100)).await;
        set_state(&mut app, AgentState::Idle);
        force_flush(&mut app);
        assert_eq!(
            state_of(&third),
            (
                crate::api::schema::PaneSendState::Dropped,
                Some("expired".into())
            )
        );
        assert!(rx.try_recv().is_err(), "a dropped line was written");
    }

    /// A session-bound agent.prompt through the real queue is bound to the
    /// runtime the caller saw (a replacement runtime on the same session is
    /// refused), never written once expired even when it could go out at
    /// once, acknowledged in the response, and cancellable while held.
    #[tokio::test]
    async fn session_bound_agent_prompt_binds_the_runtime_expiry_ack_and_cancel() {
        let mut app = app_with_agent();
        let pane = app.state.workspaces[0].tabs[0].root_pane;
        let terminal_id = app.state.workspaces[0].terminal_id(pane).unwrap().clone();
        let set_state = |app: &mut App, state: AgentState| {
            app.state
                .terminals
                .get_mut(&terminal_id)
                .unwrap()
                .set_detected_state(Some(Agent::Claude), state);
        };
        set_state(&mut app, AgentState::Idle);
        app.state
            .terminals
            .get_mut(&terminal_id)
            .unwrap()
            .persisted_agent_session = Some(crate::agent_resume::PersistedAgentSession {
            source: "herdr:claude".into(),
            agent: "claude".into(),
            session_ref: crate::agent_resume::AgentSessionRef::id("sess-1").unwrap(),
        });
        let (runtime, mut rx_a) = crate::terminal::TerminalRuntime::test_with_channel(80, 24);
        app.state.insert_test_runtime(pane, runtime);
        let public = app.public_pane_id(0, pane).unwrap();
        let runtime_id = |app: &mut App| {
            let response = app.handle_api_request(crate::api::schema::Request {
                id: "get".into(),
                method: crate::api::schema::Method::AgentGet(AgentTarget {
                    target: public.clone(),
                }),
            });
            let response: serde_json::Value = serde_json::from_str(&response).unwrap();
            response["result"]["agent"]["runtime_id"]
                .as_str()
                .expect("agent.get reports the runtime id")
                .to_string()
        };
        let prompt = |text: &str, runtime_id: &str, expires_ms: u64| AgentPromptParams {
            if_idle: false,
            target: public.clone(),
            text: text.into(),
            wait: None,
            delivery: Some(crate::api::schema::AgentPromptDelivery {
                session_id: "sess-1".into(),
                runtime_id: Some(runtime_id.into()),
                input_quiet_ms: 300,
                expires_ms,
            }),
        };
        let json =
            |response: String| -> serde_json::Value { serde_json::from_str(&response).unwrap() };
        let state_of = |id: &str| {
            crate::terminal::polite_send::recent_sends(None, Some(id))
                .into_iter()
                .next()
                .map(|item| (item.state, item.reason))
                .unwrap()
        };
        let runtime_a = runtime_id(&mut app);

        // Idle and quiet, so it could go out at once, but already expired.
        let expired = json(run_deferred_agent_prompt(
            &mut app,
            "p1",
            prompt("expired-line", &runtime_a, 0),
        ));
        assert_eq!(expired["result"]["state"], "dropped", "{expired}");
        assert_eq!(expired["result"]["reason"], "expired", "{expired}");
        assert!(rx_a.try_recv().is_err(), "an expired line was written");

        // Delivered on the bound runtime, with the binding echoed back.
        let delivered = json(run_deferred_agent_prompt(
            &mut app,
            "p2",
            prompt("live-line", &runtime_a, 10_000),
        ));
        assert_eq!(delivered["result"]["delivery"]["session_id"], "sess-1");
        assert_eq!(
            delivered["result"]["delivery"]["runtime_id"],
            runtime_a.as_str()
        );
        let id = delivered["result"]["id"].as_str().unwrap().to_string();
        let deadline = Instant::now() + Duration::from_secs(3);
        while state_of(&id).0 == crate::api::schema::PaneSendState::Queued
            && Instant::now() < deadline
        {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(
            state_of(&id).0,
            crate::api::schema::PaneSendState::Delivered
        );
        let written = rx_a
            .try_recv()
            .expect("the bound runtime receives the line");
        assert!(String::from_utf8_lossy(&written).contains("live-line"));

        // A second resume: a new runtime on the same session and agent.
        let (runtime, mut rx_b) = crate::terminal::TerminalRuntime::test_with_channel(80, 24);
        app.state.insert_test_runtime(pane, runtime);
        let runtime_b = runtime_id(&mut app);
        assert_ne!(runtime_a, runtime_b);
        let refused = json(run_deferred_agent_prompt(
            &mut app,
            "p3",
            prompt("old-runtime-line", &runtime_a, 10_000),
        ));
        assert_eq!(refused["result"]["state"], "stale_session", "{refused}");
        assert_eq!(refused["result"]["reason"], "runtime_changed", "{refused}");

        // Held on the new runtime, then cancelled: never written.
        set_state(&mut app, AgentState::Working);
        let held = json(run_deferred_agent_prompt(
            &mut app,
            "p4",
            prompt("cancelled-line", &runtime_b, 10_000),
        ));
        assert_eq!(held["result"]["state"], "queued", "{held}");
        let held_id = held["result"]["id"].as_str().unwrap().to_string();
        let cancel = json(app.handle_api_request(crate::api::schema::Request {
            id: "cancel".into(),
            method: crate::api::schema::Method::PaneQueue(crate::api::schema::PaneQueueParams {
                pane_id: public.clone(),
                id: Some(held_id.clone()),
                flush: false,
                cancel: true,
            }),
        }));
        assert_eq!(
            cancel["result"]["recent"][0]["state"], "dropped",
            "{cancel}"
        );
        assert_eq!(cancel["result"]["recent"][0]["reason"], "cancelled");
        set_state(&mut app, AgentState::Idle);
        tokio::time::sleep(Duration::from_millis(350)).await;
        app.handle_api_request(crate::api::schema::Request {
            id: "flush".into(),
            method: crate::api::schema::Method::PaneQueue(crate::api::schema::PaneQueueParams {
                pane_id: public.clone(),
                id: None,
                flush: true,
                cancel: false,
            }),
        });
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(
            rx_b.try_recv().is_err(),
            "a refused or cancelled line was written"
        );
    }

    #[tokio::test]
    async fn polite_send_agent_prompt_if_idle_drops_without_waiting_for_completion() {
        let mut app = app_with_agent();
        let pane = app.state.workspaces[0].tabs[0].root_pane;
        let terminal_id = app.state.workspaces[0].terminal_id(pane).unwrap().clone();
        let terminal = app.state.terminals.get_mut(&terminal_id).unwrap();
        terminal.set_agent_name("reviewer".into());
        terminal.set_detected_state(Some(Agent::OpenCode), AgentState::Idle);
        let (runtime, mut rx) = crate::terminal::TerminalRuntime::test_with_channel(80, 24);
        runtime.record_human_text();
        app.state.insert_test_runtime(pane, runtime);
        let response = run_deferred_agent_prompt(
            &mut app,
            "polite",
            AgentPromptParams {
                target: "reviewer".into(),
                text: "wake".into(),
                wait: None,
                if_idle: true,
                delivery: None,
            },
        );
        let response: serde_json::Value = serde_json::from_str(&response).unwrap();
        assert_eq!(response["result"]["dropped"], true);
        assert_eq!(response["result"]["queued"], false);
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn agent_prompt_sends_text_then_delays_enter() {
        let mut app = app_with_agent();
        let pane_id = app.state.workspaces[0].tabs[0].root_pane;
        let terminal_id = app.state.workspaces[0].tabs[0].panes[&pane_id]
            .attached_terminal_id
            .clone();
        let terminal = app.state.terminals.get_mut(&terminal_id).unwrap();
        terminal.set_agent_name("reviewer".into());
        terminal.set_detected_state(Some(Agent::OpenCode), AgentState::Working);
        let (runtime, mut rx) =
            crate::terminal::TerminalRuntime::test_with_channel_and_scrollback_bytes(
                80, 24, 0, b"", 2,
            );
        runtime.test_process_pty_bytes(b"\x1b[?2004h");
        app.state.insert_test_runtime(pane_id, runtime);

        let public_pane_id = app.public_pane_id(0, pane_id).unwrap();
        let bracketed_started = std::time::Instant::now();
        let response_rx = start_deferred_agent_prompt(
            &mut app,
            "req",
            AgentPromptParams {
                if_idle: false,
                target: public_pane_id,
                text: "A != B".into(),
                wait: None,
                delivery: None,
            },
        );
        assert!(response_rx.try_recv().is_err());
        let response = response_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("agent prompt responds after submission");
        let success: SuccessResponse = serde_json::from_str(&response).unwrap();
        let ResponseResult::AgentPrompted { agent, .. } = success.result else {
            panic!("expected prompted response");
        };
        assert_eq!(agent.name.as_deref(), Some("reviewer"));
        assert_eq!(
            rx.try_recv().unwrap(),
            Bytes::from_static(b"\x1b[200~A != B\x1b[201~")
        );
        assert_eq!(rx.try_recv().unwrap(), Bytes::from_static(b"\r"));
        assert!(bracketed_started.elapsed() >= AGENT_PROMPT_SUBMIT_DELAY);

        app.lookup_runtime_sender(0, pane_id)
            .unwrap()
            .test_process_pty_bytes(b"\x1b[?2004l");
        let raw_started = std::time::Instant::now();
        let raw = run_deferred_agent_prompt(
            &mut app,
            "req-raw",
            AgentPromptParams {
                if_idle: false,
                target: "reviewer".into(),
                text: "A != B".into(),
                wait: None,
                delivery: None,
            },
        );
        let raw: SuccessResponse = serde_json::from_str(&raw).unwrap();
        assert!(matches!(raw.result, ResponseResult::AgentPrompted { .. }));
        assert_eq!(rx.try_recv().unwrap(), Bytes::from_static(b"A != B"));
        assert_eq!(rx.try_recv().unwrap(), Bytes::from_static(b"\r"));
        assert!(raw_started.elapsed() >= AGENT_PROMPT_SUBMIT_DELAY);

        let rejected = run_deferred_agent_prompt(
            &mut app,
            "req-label",
            AgentPromptParams {
                if_idle: false,
                target: "opencode".into(),
                text: "wrong target".into(),
                wait: None,
                delivery: None,
            },
        );
        let error: crate::api::schema::ErrorResponse = serde_json::from_str(&rejected).unwrap();
        assert_eq!(error.error.code, "agent_not_found");
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn agent_prompt_rejects_blocked_agent_without_writing() {
        let mut app = app_with_agent();
        let pane_id = app.state.workspaces[0].tabs[0].root_pane;
        let terminal_id = app.state.workspaces[0].tabs[0].panes[&pane_id]
            .attached_terminal_id
            .clone();
        let terminal = app.state.terminals.get_mut(&terminal_id).unwrap();
        terminal.set_agent_name("reviewer".into());
        terminal.set_detected_state(Some(Agent::GithubCopilot), AgentState::Blocked);
        let (runtime, mut rx) = crate::terminal::TerminalRuntime::test_with_channel(80, 24);
        app.state.insert_test_runtime(pane_id, runtime);

        let response = run_deferred_agent_prompt(
            &mut app,
            "req",
            AgentPromptParams {
                if_idle: false,
                target: "reviewer".into(),
                text: "unrelated prompt".into(),
                wait: None,
                delivery: None,
            },
        );

        let error: crate::api::schema::ErrorResponse = serde_json::from_str(&response).unwrap();
        assert_eq!(error.error.code, "agent_blocked");
        assert!(
            tokio::time::timeout(
                AGENT_PROMPT_SUBMIT_DELAY + Duration::from_millis(100),
                rx.recv()
            )
            .await
            .is_err(),
            "blocked prompt wrote or scheduled terminal input"
        );
    }

    #[tokio::test]
    async fn agent_prompt_focuses_copilot_before_submitting() {
        let mut app = app_with_agent();
        let pane_id = app.state.workspaces[0].tabs[0].root_pane;
        let terminal_id = app.state.workspaces[0].tabs[0].panes[&pane_id]
            .attached_terminal_id
            .clone();
        let terminal = app.state.terminals.get_mut(&terminal_id).unwrap();
        terminal.set_agent_name("reviewer".into());
        terminal.set_detected_state(Some(Agent::GithubCopilot), AgentState::Idle);
        let (runtime, mut rx) =
            crate::terminal::TerminalRuntime::test_with_channel_and_scrollback_bytes(
                80, 24, 0, b"", 3,
            );
        runtime.test_process_pty_bytes(b"\x1b[?2004h");
        app.state.insert_test_runtime(pane_id, runtime);

        let response = run_deferred_agent_prompt(
            &mut app,
            "req",
            AgentPromptParams {
                if_idle: false,
                target: "reviewer".into(),
                text: "A != B".into(),
                wait: None,
                delivery: None,
            },
        );
        let success: SuccessResponse = serde_json::from_str(&response).unwrap();
        assert!(matches!(
            success.result,
            ResponseResult::AgentPrompted { .. }
        ));
        assert_eq!(rx.try_recv().unwrap(), Bytes::from_static(b"\x1b[I"));
        assert_eq!(
            rx.try_recv().unwrap(),
            Bytes::from_static(b"\x1b[200~A != B\x1b[201~")
        );
        assert_eq!(rx.try_recv().unwrap(), Bytes::from_static(b"\r"));
    }

    #[tokio::test]
    async fn agent_send_keys_validates_every_key_before_writing() {
        let mut app = app_with_agent();
        let pane_id = app.state.workspaces[0].tabs[0].root_pane;
        let terminal_id = app.state.workspaces[0].tabs[0].panes[&pane_id]
            .attached_terminal_id
            .clone();
        let terminal = app.state.terminals.get_mut(&terminal_id).unwrap();
        terminal.set_agent_name("reviewer".into());
        terminal.set_detected_state(Some(Agent::Pi), AgentState::Idle);
        let (runtime, mut rx) = crate::terminal::TerminalRuntime::test_with_channel(80, 24);
        app.state.insert_test_runtime(pane_id, runtime);

        let rejected = app.handle_agent_send_keys(
            "req-invalid".into(),
            AgentSendKeysParams {
                target: "reviewer".into(),
                keys: vec!["enter".into(), "not-a-key".into()],
            },
        );
        let error: crate::api::schema::ErrorResponse = serde_json::from_str(&rejected).unwrap();
        assert_eq!(error.error.code, "invalid_key");
        assert!(rx.try_recv().is_err());

        let sent = app.handle_agent_send_keys(
            "req-valid".into(),
            AgentSendKeysParams {
                target: "reviewer".into(),
                keys: vec!["up".into(), "enter".into()],
            },
        );
        let success: SuccessResponse = serde_json::from_str(&sent).unwrap();
        assert!(matches!(success.result, ResponseResult::Ok {}));
        assert_eq!(rx.try_recv().unwrap(), Bytes::from_static(b"\x1b[A\r"));
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn agent_prompt_rejects_managed_agent_while_startup_is_pending() {
        let mut app = app_with_agent();
        let pane_id = app.state.workspaces[0].tabs[0].root_pane;
        let terminal_id = app.state.workspaces[0].tabs[0].panes[&pane_id]
            .attached_terminal_id
            .clone();
        let terminal = app.state.terminals.get_mut(&terminal_id).unwrap();
        let now = std::time::Instant::now();
        terminal.begin_managed_agent(
            "reviewer".into(),
            Agent::OpenCode,
            now,
            std::time::Duration::from_secs(3),
            std::time::Duration::from_secs(10),
        );
        terminal.set_detected_state(Some(Agent::OpenCode), AgentState::Idle);
        let (runtime, mut rx) = crate::terminal::TerminalRuntime::test_with_channel(80, 24);
        app.state.insert_test_runtime(pane_id, runtime);

        let response = run_deferred_agent_prompt(
            &mut app,
            "req-pending",
            AgentPromptParams {
                if_idle: false,
                target: "reviewer".into(),
                text: "A != B".into(),
                wait: None,
                delivery: None,
            },
        );
        let error: crate::api::schema::ErrorResponse = serde_json::from_str(&response).unwrap();
        assert_eq!(error.error.code, "agent_not_ready");
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn agent_focus_marks_already_focused_done_agent_seen() {
        let mut app = app_with_agent();
        app.state.outer_terminal_focus = Some(false);

        let pane_id = app.state.workspaces[0].tabs[0].root_pane;
        let terminal_id = app.state.workspaces[0].tabs[0].panes[&pane_id]
            .attached_terminal_id
            .clone();
        app.state
            .terminals
            .get_mut(&terminal_id)
            .unwrap()
            .set_detected_state(Some(Agent::Pi), AgentState::Idle);
        app.state.workspaces[0].tabs[0]
            .panes
            .get_mut(&pane_id)
            .unwrap()
            .seen = false;
        app.state.workspaces[0].tabs[0].layout.focus_pane(pane_id);

        let response = app.handle_agent_focus(
            "req".into(),
            AgentTarget {
                target: app.public_pane_id(0, pane_id).unwrap(),
            },
        );

        let success: SuccessResponse = serde_json::from_str(&response).unwrap();
        let ResponseResult::AgentInfo { agent } = success.result else {
            panic!("expected agent info response");
        };
        assert_eq!(agent.agent_status, AgentStatus::Idle);
    }

    fn app_with_named_agents(names: &[&str]) -> App {
        let (_api_tx, api_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut app = App::new(
            &Config::default(),
            crate::app::AppPolicy::TEST,
            None,
            api_rx,
            crate::api::EventHub::default(),
        );
        app.state.workspaces = names.iter().map(|name| Workspace::test_new(name)).collect();
        app.state.ensure_test_terminals();
        if !names.is_empty() {
            app.state.active = Some(0);
            app.state.selected = 0;
            app.state.mode = Mode::Terminal;
        }
        for (ws_idx, name) in names.iter().enumerate() {
            let pane_id = app.state.workspaces[ws_idx].tabs[0].root_pane;
            let terminal_id = app.state.workspaces[ws_idx].tabs[0].panes[&pane_id]
                .attached_terminal_id
                .clone();
            let terminal = app.state.terminals.get_mut(&terminal_id).unwrap();
            terminal.set_agent_name((*name).to_string());
            terminal.set_detected_state(Some(Agent::Pi), AgentState::Idle);
        }
        app
    }

    fn agent_json(app: &mut App, target: &str) -> serde_json::Value {
        let response = app.handle_agent_get(
            "req-get".into(),
            AgentTarget {
                target: target.into(),
            },
        );
        let response: serde_json::Value = serde_json::from_str(&response).unwrap();
        response["result"]["agent"].clone()
    }

    fn exit_agent_process(app: &mut App, target_ws: usize) {
        let pane_id = app.state.workspaces[target_ws].tabs[0].root_pane;
        let terminal_id = app.state.workspaces[target_ws].tabs[0].panes[&pane_id]
            .attached_terminal_id
            .clone();
        let terminal = app.state.terminals.get_mut(&terminal_id).unwrap();
        // Upstream 0.9 frees the pane on the first no-agent observation *after* a
        // recorded exit, so drive both steps here.
        terminal.set_detected_state_with_visible_blocker(
            terminal.effective_known_agent(),
            AgentState::Unknown,
            false,
            false,
            true,
        );
        terminal.set_detected_state_with_visible_blocker(
            None,
            AgentState::Unknown,
            false,
            false,
            true,
        );
    }

    #[tokio::test]
    async fn agent_start_records_caller_agent_as_owner() {
        let mut app = app_with_named_agents(&["lead"]);
        app.state
            .workspaces
            .push(Workspace::test_new("shell-space"));
        app.state.ensure_test_terminals();
        let shell_pane = app.state.workspaces[1].tabs[0].root_pane;
        let shell_terminal_id = app.state.workspaces[1].tabs[0].panes[&shell_pane]
            .attached_terminal_id
            .clone();
        let (runtime, _rx) = crate::terminal::TerminalRuntime::test_with_channel(80, 24);
        app.terminal_runtimes
            .insert(shell_terminal_id.clone(), runtime);
        let caller_pane_id = app.public_pane_id(0, app.state.workspaces[0].tabs[0].root_pane);
        let target_pane_id = app.public_pane_id(1, shell_pane).unwrap();

        let response = app.handle_agent_start(
            "req-start".into(),
            AgentStartParams {
                name: "worker".into(),
                kind: "pi".into(),
                pane_id: target_pane_id,
                args: Vec::new(),
                timeout_ms: Some(4_000),
                owner: None,
                caller_pane_id,
            },
        );
        let response: serde_json::Value = serde_json::from_str(&response).unwrap();
        let agent = &response["result"]["agent"];
        assert_eq!(agent["name"], "worker");
        assert!(agent["agent_id"].as_str().unwrap().starts_with("agent_"));
        let ownership = &agent["ownership"];
        let lead_identity = agent_json(&mut app, "lead")["agent_id"]
            .as_str()
            .unwrap()
            .to_string();
        assert_eq!(ownership["origin"]["agent_id"], lead_identity.as_str());
        assert_eq!(ownership["current"]["agent_id"], lead_identity.as_str());
        assert_eq!(ownership["current"]["name"], "lead");
        assert_eq!(ownership["current"]["resolved"], true);
        assert!(ownership.get("orphaned").is_none_or(|v| v == false));
    }

    #[tokio::test]
    async fn agent_start_without_agent_caller_stays_root_and_bad_owner_errors() {
        let mut app = app_with_named_agents(&[]);
        app.state.workspaces = vec![
            Workspace::test_new("caller-shell"),
            Workspace::test_new("target-shell"),
        ];
        app.state.ensure_test_terminals();
        app.state.active = Some(0);
        let target_pane = app.state.workspaces[1].tabs[0].root_pane;
        let target_terminal_id = app.state.workspaces[1].tabs[0].panes[&target_pane]
            .attached_terminal_id
            .clone();
        let (runtime, _rx) = crate::terminal::TerminalRuntime::test_with_channel(80, 24);
        app.terminal_runtimes.insert(target_terminal_id, runtime);
        let caller_pane_id = app.public_pane_id(0, app.state.workspaces[0].tabs[0].root_pane);
        let target_pane_id = app.public_pane_id(1, target_pane).unwrap();

        let response = app.handle_agent_start(
            "req-root".into(),
            AgentStartParams {
                name: "worker".into(),
                kind: "pi".into(),
                pane_id: target_pane_id.clone(),
                args: Vec::new(),
                timeout_ms: Some(4_000),
                owner: None,
                caller_pane_id,
            },
        );
        let response: serde_json::Value = serde_json::from_str(&response).unwrap();
        let agent = &response["result"]["agent"];
        assert_eq!(agent["name"], "worker");
        assert!(agent.get("ownership").is_none());

        let rejected = app.handle_agent_start(
            "req-bad-owner".into(),
            AgentStartParams {
                name: "worker2".into(),
                kind: "pi".into(),
                pane_id: target_pane_id,
                args: Vec::new(),
                timeout_ms: Some(4_000),
                owner: Some("missing".into()),
                caller_pane_id: None,
            },
        );
        let rejected: serde_json::Value = serde_json::from_str(&rejected).unwrap();
        assert_eq!(rejected["error"]["code"], "agent_owner_not_agent");
    }

    #[tokio::test]
    async fn agent_owner_set_transfer_and_clear_keep_origin() {
        let mut app = app_with_named_agents(&["lead", "peer", "worker"]);

        let adopted = app.handle_agent_owner_set(
            "req-adopt".into(),
            AgentOwnerSetParams {
                target: "worker".into(),
                owner: "lead".into(),
            },
        );
        let adopted: serde_json::Value = serde_json::from_str(&adopted).unwrap();
        let lead_identity = agent_json(&mut app, "lead")["agent_id"]
            .as_str()
            .unwrap()
            .to_string();
        let ownership = &adopted["result"]["agent"]["ownership"];
        assert_eq!(ownership["origin"]["agent_id"], lead_identity.as_str());
        assert_eq!(ownership["current"]["agent_id"], lead_identity.as_str());

        let transferred = app.handle_agent_owner_set(
            "req-transfer".into(),
            AgentOwnerSetParams {
                target: "worker".into(),
                owner: "peer".into(),
            },
        );
        let transferred: serde_json::Value = serde_json::from_str(&transferred).unwrap();
        let peer_identity = agent_json(&mut app, "peer")["agent_id"]
            .as_str()
            .unwrap()
            .to_string();
        let ownership = &transferred["result"]["agent"]["ownership"];
        // Origin lineage is immutable; current ownership transfers.
        assert_eq!(ownership["origin"]["agent_id"], lead_identity.as_str());
        assert_eq!(ownership["current"]["agent_id"], peer_identity.as_str());

        let released = app.handle_agent_owner_clear(
            "req-release".into(),
            AgentTarget {
                target: "worker".into(),
            },
        );
        let released: serde_json::Value = serde_json::from_str(&released).unwrap();
        let ownership = &released["result"]["agent"]["ownership"];
        assert_eq!(ownership["origin"]["agent_id"], lead_identity.as_str());
        assert!(ownership.get("current").is_none());
        assert!(ownership.get("orphaned").is_none_or(|v| v == false));
    }

    #[tokio::test]
    async fn agent_owner_set_rejects_self_ownership_and_cycles() {
        let mut app = app_with_named_agents(&["lead", "worker"]);

        let self_owned = app.handle_agent_owner_set(
            "req-self".into(),
            AgentOwnerSetParams {
                target: "lead".into(),
                owner: "lead".into(),
            },
        );
        let self_owned: serde_json::Value = serde_json::from_str(&self_owned).unwrap();
        assert_eq!(self_owned["error"]["code"], "agent_owner_invalid");

        let adopted = app.handle_agent_owner_set(
            "req-adopt".into(),
            AgentOwnerSetParams {
                target: "worker".into(),
                owner: "lead".into(),
            },
        );
        assert!(adopted.contains("\"result\""));
        let cycle = app.handle_agent_owner_set(
            "req-cycle".into(),
            AgentOwnerSetParams {
                target: "lead".into(),
                owner: "worker".into(),
            },
        );
        let cycle: serde_json::Value = serde_json::from_str(&cycle).unwrap();
        assert_eq!(cycle["error"]["code"], "agent_owner_cycle");
    }

    #[tokio::test]
    async fn agent_group_set_places_without_touching_ownership() {
        let mut app = app_with_named_agents(&["lead", "worker"]);
        let adopted = app.handle_agent_owner_set(
            "req-adopt".into(),
            AgentOwnerSetParams {
                target: "worker".into(),
                owner: "lead".into(),
            },
        );
        assert!(adopted.contains("\"result\""));
        assert!(agent_json(&mut app, "worker").get("group").is_none());

        let pinned = app.handle_agent_group_set(
            "req-pin".into(),
            AgentGroupSetParams {
                target: "worker".into(),
                placement: AgentGroupPlacementKind::HandsOn,
                parent: None,
            },
        );
        let pinned: serde_json::Value = serde_json::from_str(&pinned).unwrap();
        let agent = &pinned["result"]["agent"];
        assert_eq!(agent["group"]["placement"], "hands_on");
        assert!(agent["group"].get("parent").is_none());
        assert_eq!(
            agent["ownership"]["current"]["name"], "lead",
            "placement leaves ownership alone"
        );
        assert!(app.state.session_dirty);

        let nested = app.handle_agent_group_set(
            "req-nest".into(),
            AgentGroupSetParams {
                target: "worker".into(),
                placement: AgentGroupPlacementKind::Under,
                parent: Some("lead".into()),
            },
        );
        let nested: serde_json::Value = serde_json::from_str(&nested).unwrap();
        let group = &nested["result"]["agent"]["group"];
        assert_eq!(group["placement"], "under");
        assert_eq!(group["parent"]["name"], "lead");
        assert_eq!(group["parent"]["resolved"], true);
        assert!(group.get("orphaned").is_none_or(|v| v == false));

        // The parent leaving marks the placement orphaned instead of erasing it.
        exit_agent_process(&mut app, 0);
        let orphaned = agent_json(&mut app, "worker");
        assert_eq!(orphaned["group"]["orphaned"], true);
        assert_eq!(orphaned["group"]["parent"]["resolved"], false);

        let cleared = app.handle_agent_group_set(
            "req-auto".into(),
            AgentGroupSetParams {
                target: "worker".into(),
                placement: AgentGroupPlacementKind::Auto,
                parent: None,
            },
        );
        let cleared: serde_json::Value = serde_json::from_str(&cleared).unwrap();
        assert!(cleared["result"]["agent"].get("group").is_none());
    }

    #[tokio::test]
    async fn agent_group_under_rejects_bad_parents() {
        let mut app = app_with_named_agents(&["lead", "worker"]);
        let missing = app.handle_agent_group_set(
            "req-missing".into(),
            AgentGroupSetParams {
                target: "worker".into(),
                placement: AgentGroupPlacementKind::Under,
                parent: None,
            },
        );
        let missing: serde_json::Value = serde_json::from_str(&missing).unwrap();
        assert_eq!(missing["error"]["code"], "agent_group_parent_required");

        let self_nested = app.handle_agent_group_set(
            "req-self".into(),
            AgentGroupSetParams {
                target: "worker".into(),
                placement: AgentGroupPlacementKind::Under,
                parent: Some("worker".into()),
            },
        );
        let self_nested: serde_json::Value = serde_json::from_str(&self_nested).unwrap();
        assert_eq!(self_nested["error"]["code"], "agent_owner_invalid");

        let nested = app.handle_agent_group_set(
            "req-nest".into(),
            AgentGroupSetParams {
                target: "worker".into(),
                placement: AgentGroupPlacementKind::Under,
                parent: Some("lead".into()),
            },
        );
        assert!(nested.contains("\"result\""));
        let cycle = app.handle_agent_group_set(
            "req-cycle".into(),
            AgentGroupSetParams {
                target: "lead".into(),
                placement: AgentGroupPlacementKind::Under,
                parent: Some("worker".into()),
            },
        );
        let cycle: serde_json::Value = serde_json::from_str(&cycle).unwrap();
        assert_eq!(cycle["error"]["code"], "agent_owner_cycle");

        let unknown = app.handle_agent_group_set(
            "req-unknown".into(),
            AgentGroupSetParams {
                target: "worker".into(),
                placement: AgentGroupPlacementKind::Under,
                parent: Some("nobody".into()),
            },
        );
        assert!(unknown.contains("\"error\""));
    }

    #[tokio::test]
    async fn agent_group_collapse_records_the_owner_key() {
        let mut app = app_with_named_agents(&["lead", "worker"]);
        let collapsed = app.handle_agent_group_collapse(
            "req-collapse".into(),
            AgentGroupCollapseParams {
                target: "lead".into(),
                collapsed: true,
            },
        );
        let collapsed: serde_json::Value = serde_json::from_str(&collapsed).unwrap();
        let agent = &collapsed["result"]["agent"];
        assert_eq!(agent["group"]["collapsed"], true);
        assert_eq!(agent["group"]["placement"], "auto");
        let lead_identity = agent["agent_id"].as_str().unwrap().to_string();
        assert!(app
            .state
            .collapsed_agent_group_keys
            .contains(&lead_identity));

        let expanded = app.handle_agent_group_collapse(
            "req-expand".into(),
            AgentGroupCollapseParams {
                target: "lead".into(),
                collapsed: false,
            },
        );
        let expanded: serde_json::Value = serde_json::from_str(&expanded).unwrap();
        assert!(expanded["result"]["agent"].get("group").is_none());
        assert!(app.state.collapsed_agent_group_keys.is_empty());

        // An orchestrator workspace's first-tab agent collapses by the
        // synthetic orchestrator key so the CLI and the chevron agree.
        app.state.workspaces[0].orchestrator_mode = true;
        let orch = app.handle_agent_group_collapse(
            "req-orch".into(),
            AgentGroupCollapseParams {
                target: "lead".into(),
                collapsed: true,
            },
        );
        assert!(orch.contains("\"result\""));
        let key = format!("orch:{}", app.state.workspaces[0].id);
        assert!(app.state.collapsed_agent_group_keys.contains(&key));
    }

    #[tokio::test]
    async fn agent_rename_clear_does_not_sever_ownership() {
        let mut app = app_with_named_agents(&["lead", "worker"]);
        let response = app.handle_agent_owner_set(
            "req-adopt".into(),
            AgentOwnerSetParams {
                target: "worker".into(),
                owner: "lead".into(),
            },
        );
        assert!(response.contains("\"result\""));
        let worker_pane_id = agent_json(&mut app, "worker")["pane_id"]
            .as_str()
            .unwrap()
            .to_string();

        let renamed = app.handle_agent_rename(
            "req-unname".into(),
            AgentRenameParams {
                target: "worker".into(),
                name: None,
            },
        );
        assert!(renamed.contains("\"result\""));

        let agent = agent_json(&mut app, &worker_pane_id);
        assert!(agent.get("name").is_none());
        assert!(agent["ownership"]["current"]["agent_id"]
            .as_str()
            .unwrap()
            .starts_with("agent_"));
    }

    #[tokio::test]
    async fn owner_exit_orphans_worker_and_owner_clear_recovers_it() {
        let mut app = app_with_named_agents(&["lead", "worker"]);
        let response = app.handle_agent_owner_set(
            "req-adopt".into(),
            AgentOwnerSetParams {
                target: "worker".into(),
                owner: "lead".into(),
            },
        );
        assert!(response.contains("\"result\""));

        exit_agent_process(&mut app, 0);

        let orphaned = agent_json(&mut app, "worker");
        assert_eq!(orphaned["ownership"]["orphaned"], true);
        assert_eq!(orphaned["ownership"]["current"]["resolved"], false);
        // The snapshot of who owned it is preserved for the user to act on.
        assert_eq!(orphaned["ownership"]["current"]["name"], "lead");

        let released = app.handle_agent_owner_clear(
            "req-release".into(),
            AgentTarget {
                target: "worker".into(),
            },
        );
        let released: serde_json::Value = serde_json::from_str(&released).unwrap();
        let ownership = &released["result"]["agent"]["ownership"];
        assert!(ownership.get("current").is_none());
        assert!(ownership.get("orphaned").is_none_or(|v| v == false));
    }

    #[tokio::test]
    async fn owner_resolution_reconciles_via_resumed_session() {
        let mut app = app_with_named_agents(&["lead", "worker", "revived"]);
        let lead_pane = app.state.workspaces[0].tabs[0].root_pane;
        let lead_terminal_id = app.state.workspaces[0].tabs[0].panes[&lead_pane]
            .attached_terminal_id
            .clone();
        app.state
            .terminals
            .get_mut(&lead_terminal_id)
            .unwrap()
            .set_persisted_agent_session(crate::agent_resume::PersistedAgentSession {
                source: "herdr:pi".into(),
                agent: "pi".into(),
                session_ref: crate::agent_resume::AgentSessionRef::id("lead-session").unwrap(),
            });
        let response = app.handle_agent_owner_set(
            "req-adopt".into(),
            AgentOwnerSetParams {
                target: "worker".into(),
                owner: "lead".into(),
            },
        );
        assert!(response.contains("\"result\""));

        exit_agent_process(&mut app, 0);
        assert_eq!(
            agent_json(&mut app, "worker")["ownership"]["orphaned"],
            true
        );

        // The owner's session comes back in a different pane: the durable
        // reference reconciles through the recorded session identity.
        let revived_pane = app.state.workspaces[2].tabs[0].root_pane;
        let revived_terminal_id = app.state.workspaces[2].tabs[0].panes[&revived_pane]
            .attached_terminal_id
            .clone();
        app.state
            .terminals
            .get_mut(&revived_terminal_id)
            .unwrap()
            .set_persisted_agent_session(crate::agent_resume::PersistedAgentSession {
                source: "herdr:pi".into(),
                agent: "pi".into(),
                session_ref: crate::agent_resume::AgentSessionRef::id("lead-session").unwrap(),
            });

        let agent = agent_json(&mut app, "worker");
        assert!(agent["ownership"]
            .get("orphaned")
            .is_none_or(|v| v == false));
        assert_eq!(agent["ownership"]["current"]["resolved"], true);
        assert_eq!(
            agent["ownership"]["current"]["pane_id"],
            app.public_pane_id(2, revived_pane).unwrap().as_str()
        );
        assert_eq!(agent["ownership"]["current"]["name"], "revived");
    }

    #[tokio::test]
    async fn ownership_survives_pane_move_across_tabs() {
        let mut app = app_with_named_agents(&["lead", "worker"]);
        let response = app.handle_agent_owner_set(
            "req-adopt".into(),
            AgentOwnerSetParams {
                target: "worker".into(),
                owner: "lead".into(),
            },
        );
        assert!(response.contains("\"result\""));

        let worker_pane = app.state.workspaces[1].tabs[0].root_pane;
        let target_tab = app.state.workspaces[0].test_add_tab(Some("target"));
        let target_pane = app.state.workspaces[0].tabs[target_tab].root_pane;
        app.state.ensure_test_terminals();
        let worker_public = app.public_pane_id(1, worker_pane).unwrap();
        let target_tab_public = app.public_tab_id(0, target_tab).unwrap();
        let target_public = app.public_pane_id(0, target_pane).unwrap();

        let moved = app.handle_pane_move(
            "req-move".into(),
            crate::api::schema::PaneMoveParams {
                pane_id: worker_public,
                destination: crate::api::schema::PaneMoveDestination::Tab {
                    tab_id: target_tab_public,
                    target_pane_id: Some(target_public),
                    split: crate::api::schema::SplitDirection::Right,
                    ratio: None,
                },
                focus: false,
            },
        );
        let moved: serde_json::Value = serde_json::from_str(&moved).unwrap();
        let moved_pane_id = moved["result"]["move_result"]["pane"]["pane_id"]
            .as_str()
            .expect("pane move should succeed")
            .to_string();

        let agent = agent_json(&mut app, &moved_pane_id);
        assert_eq!(agent["name"], "worker");
        let lead_identity = agent_json(&mut app, "lead")["agent_id"]
            .as_str()
            .unwrap()
            .to_string();
        assert_eq!(
            agent["ownership"]["current"]["agent_id"],
            lead_identity.as_str()
        );
        assert!(agent["ownership"]
            .get("orphaned")
            .is_none_or(|v| v == false));
    }

    #[test]
    fn agent_rename_does_not_replace_the_pane_label() {
        let mut app = app_with_agent();
        let pane_id = app.state.workspaces[0].tabs[0].root_pane;
        let terminal_id = app.state.workspaces[0].tabs[0].panes[&pane_id]
            .attached_terminal_id
            .clone();
        let terminal = app.state.terminals.get_mut(&terminal_id).unwrap();
        terminal.set_manual_label("shell-pane".into());
        terminal.set_detected_state(Some(Agent::Pi), AgentState::Idle);
        let target = app.public_pane_id(0, pane_id).unwrap();

        for name in [Some("reviewer".to_string()), None] {
            let response = app.handle_agent_rename(
                "req".into(),
                AgentRenameParams {
                    target: target.clone(),
                    name,
                },
            );
            let success: SuccessResponse = serde_json::from_str(&response).unwrap();
            assert!(matches!(success.result, ResponseResult::AgentInfo { .. }));
            assert_eq!(
                app.state.terminals[&terminal_id].manual_label.as_deref(),
                Some("shell-pane")
            );
        }
    }
}
