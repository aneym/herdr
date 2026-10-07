use std::time::Instant;

use bytes::Bytes;
use ratatui::layout::Rect;

use super::App;

/// How long an `agent.resume` replacement shell keeps its pane on exit when
/// the resumed agent has not been observed yet.
const AGENT_RESUME_REPLACEMENT_WINDOW: std::time::Duration = std::time::Duration::from_secs(15);

/// The replacement runtime `agent.resume` started for a pane and when its
/// window ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AgentResumeReplacementWindow {
    pub(crate) runtime_pid: Option<u32>,
    pub(crate) until: Instant,
}

#[derive(Debug)]
pub(crate) struct AgentRestartAfterShutdown {
    pub(crate) pane_id: crate::layout::PaneId,
    pub(crate) terminal_id: crate::terminal::TerminalId,
    pub(crate) replaced_pid: Option<u32>,
    cwd: std::path::PathBuf,
    resumed: InPlaceAgentResume,
    launch_env: crate::pane::PaneLaunchEnv,
    rows: u16,
    cols: u16,
}

struct PendingAgentResumeCandidate {
    pane_id: crate::layout::PaneId,
    terminal_id: crate::terminal::TerminalId,
    cwd: std::path::PathBuf,
    plan: crate::agent_resume::AgentResumePlan,
    rows: u16,
    cols: u16,
}

/// Why `agent.resume` refused or failed for a pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum InPlaceAgentResumeError {
    PaneNotFound,
    SessionUnknown(String),
    NotResumable(String),
    Busy {
        message: String,
        reason: &'static str,
    },
    NotRunning,
    ArgvUnsupported(String),
    Failed(String),
}

/// What `agent.resume` restarted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InPlaceAgentResume {
    pub(crate) agent: String,
    pub(crate) session_id: String,
    pub(crate) argv: Vec<String>,
    pub(crate) launcher: String,
}

impl App {
    /// Restart the idle agent in a pane on the same session: end its process,
    /// then start a fresh pane shell and type the resume command into it, as
    /// deferred resume does, retaining the pane, terminal, cwd and launch
    /// environment. Never blocks: a replacement that dies early is caught by
    /// its replacement window when `PaneDied` arrives.
    pub(crate) fn resume_agent_in_place(
        &mut self,
        ws_idx: usize,
        pane_id: crate::layout::PaneId,
        input_quiet: Option<std::time::Duration>,
    ) -> Result<InPlaceAgentResume, InPlaceAgentResumeError> {
        self.restart_agent_in_place(
            ws_idx,
            pane_id,
            false,
            Some(
                input_quiet
                    .unwrap_or_default()
                    .max(std::time::Duration::from_secs(20)),
            ),
        )
    }

    pub(crate) fn restart_agent_in_place(
        &mut self,
        ws_idx: usize,
        pane_id: crate::layout::PaneId,
        force: bool,
        input_quiet: Option<std::time::Duration>,
    ) -> Result<InPlaceAgentResume, InPlaceAgentResumeError> {
        if self.pending_agent_restart_shutdowns.contains_key(&pane_id)
            || self
                .pending_agent_resume_runtime_exits
                .contains_key(&pane_id)
        {
            return Err(InPlaceAgentResumeError::Busy {
                message: "previous restart is still completing".into(),
                reason: "restart_pending",
            });
        }
        let pane = self
            .pane_info(ws_idx, pane_id)
            .ok_or(InPlaceAgentResumeError::PaneNotFound)?;
        let terminal_id = self
            .state
            .workspaces
            .get(ws_idx)
            .and_then(|workspace| workspace.terminal_id(pane_id))
            .cloned()
            .ok_or(InPlaceAgentResumeError::PaneNotFound)?;
        let terminal_cwd = self
            .state
            .terminals
            .get(&terminal_id)
            .map(|terminal| terminal.cwd.clone())
            .ok_or(InPlaceAgentResumeError::PaneNotFound)?;

        let Some(session) = pane.agent_session.clone() else {
            return Err(InPlaceAgentResumeError::SessionUnknown(format!(
                "pane {} has no reported agent session",
                pane.pane_id
            )));
        };
        let session_ref = match session.kind {
            crate::agent_resume::AgentSessionRefKind::Id => {
                crate::agent_resume::AgentSessionRef::id(session.value.clone())
            }
            crate::agent_resume::AgentSessionRefKind::Path => {
                crate::agent_resume::AgentSessionRef::path(session.value.clone())
            }
        }
        .ok_or_else(|| {
            InPlaceAgentResumeError::SessionUnknown(format!(
                "pane {} reports an invalid agent session",
                pane.pane_id
            ))
        })?;
        let Some(plan) = crate::agent_resume::plan(&session.source, &session.agent, &session_ref)
        else {
            return Err(InPlaceAgentResumeError::NotResumable(format!(
                "agent {} has no resume command",
                session.agent
            )));
        };
        if !(matches!(
            pane.agent_status,
            crate::api::schema::AgentStatus::Idle | crate::api::schema::AgentStatus::Done
        ) || force && pane.agent_status == crate::api::schema::AgentStatus::Working)
        {
            return Err(InPlaceAgentResumeError::Busy {
                message: format!("agent in pane {} is {:?}", pane.pane_id, pane.agent_status),
                reason: if pane.agent_status == crate::api::schema::AgentStatus::Working {
                    "working"
                } else {
                    "blocked"
                },
            });
        }

        let runtime = self
            .terminal_runtimes
            .get(&terminal_id)
            .ok_or(InPlaceAgentResumeError::NotRunning)?;
        if let Some(quiet) = input_quiet {
            if !runtime.human_input_quiet_for(quiet) {
                return Err(InPlaceAgentResumeError::Busy {
                    message: "pane received recent input".into(),
                    reason: "blocked",
                });
            }
        }
        let (rows, cols) = runtime.current_size();
        let shell_pid = runtime
            .child_pid()
            .ok_or(InPlaceAgentResumeError::NotRunning)?;
        let process = crate::detect::foreground_job(shell_pid)
            .and_then(|job| {
                job.processes.into_iter().find(|process| {
                    process
                        .argv
                        .as_ref()
                        .and_then(|argv| argv.first())
                        .is_some_and(|program| {
                            crate::agent_resume::same_executable(program, &plan.argv[0])
                                || crate::agent_resume::same_executable(program, "node")
                        })
                })
            })
            .ok_or_else(|| {
                InPlaceAgentResumeError::ArgvUnsupported("agent process unavailable".into())
            })?;
        let agent_argv = process.argv.unwrap_or_default();
        let terminal = self
            .state
            .terminals
            .get(&terminal_id)
            .ok_or(InPlaceAgentResumeError::PaneNotFound)?;
        let recorded = terminal.launch_argv.clone();
        let launch_env_overrides = crate::agent_resume::restart_env_overrides(
            terminal.launch_env_overrides.clone(),
            crate::platform::process_env_var(process.pid, "PATH"),
        );
        let outer = crate::platform::agent_launch_argv(shell_pid, process.pid);
        let known_outer = outer.as_ref().filter(|argv| {
            argv.iter()
                .take(2)
                .any(|arg| crate::agent_resume::same_executable(arg, "claude-lb-launch"))
        });
        let (mut argv, launcher) = crate::agent_resume::restart_launch_argv(
            &agent_argv,
            recorded
                .as_deref()
                .or_else(|| known_outer.map(Vec::as_slice)),
            self.agent_restart
                .launchers
                .get(&plan.agent)
                .map(String::as_str),
            &plan,
        )
        .map_err(InPlaceAgentResumeError::ArgvUnsupported)?;
        // Only footer evidence is used; never infer permission mode from incidental chat text.
        if plan.agent == "claude"
            && !argv.iter().any(|arg| {
                arg == "--permission-mode"
                    || arg.starts_with("--permission-mode=")
                    || arg == "--dangerously-skip-permissions"
            })
        {
            let text = runtime.detection_text();
            let mode = text.lines().rev().take(4).find_map(|line| {
                if line.contains("bypass permissions") {
                    Some("bypassPermissions")
                } else if line.contains("accept edits") {
                    Some("acceptEdits")
                } else if line.contains("plan mode") {
                    Some("plan")
                } else {
                    None
                }
            });
            if let Some(mode) = mode {
                argv.extend(["--permission-mode".into(), mode.into()]);
            }
        }
        let cwd = pane
            .cwd
            .as_deref()
            .map(std::path::PathBuf::from)
            .filter(|cwd| cwd.is_dir())
            .unwrap_or(terminal_cwd);
        let resumed = InPlaceAgentResume {
            agent: plan.agent.clone(),
            session_id: session_ref.value.clone(),
            argv: argv.clone(),
            launcher: launcher.into(),
        };
        let persisted = crate::agent_resume::PersistedAgentSession {
            source: session.source,
            agent: session.agent,
            session_ref,
        };
        let plan = crate::agent_resume::AgentResumePlan { argv, ..plan };

        let launch_env = self
            .pane_launch_env(ws_idx, pane_id, launch_env_overrides)
            .ok_or_else(|| {
                InPlaceAgentResumeError::Failed("pane launch environment unavailable".into())
            })?;

        tracing::info!(
            pane = pane_id.raw(),
            terminal = %terminal_id,
            agent = %resumed.agent,
            "restarting idle agent in place to resume its session"
        );
        let replaced_pid = runtime.child_pid();
        self.pending_agent_resume_runtime_exits
            .insert(pane_id, replaced_pid);
        let old_runtime = self
            .terminal_runtimes
            .remove(&terminal_id)
            .ok_or(InPlaceAgentResumeError::NotRunning)?;
        self.pending_agent_restart_shutdowns
            .insert(pane_id, replaced_pid);
        if let Some(mut terminal) = self.state.terminals.remove(&terminal_id) {
            terminal.cwd = cwd.clone();
            terminal.begin_in_place_agent_resume(persisted, plan);
            // The shutdown event owns launch; deferred restore must not race it.
            terminal.pending_agent_resume_plan = None;
            self.state
                .terminals
                .insert(terminal_id.clone(), terminal.with_respawn_shell_on_exit());
        }
        let restart = AgentRestartAfterShutdown {
            pane_id,
            terminal_id,
            replaced_pid,
            cwd,
            resumed: resumed.clone(),
            launch_env,
            rows,
            cols,
        };
        let event_tx = self.event_tx.clone();
        std::thread::spawn(move || {
            old_runtime.shutdown();
            let _ = event_tx.blocking_send(crate::events::AppEvent::AgentRestartShutdownFinished(
                Box::new(restart),
            ));
        });
        self.state.mark_session_dirty();
        self.emit_pane_updated(ws_idx, pane_id);
        Ok(resumed)
    }

    pub(crate) fn finish_agent_restart_shutdown(&mut self, restart: AgentRestartAfterShutdown) {
        let AgentRestartAfterShutdown {
            pane_id,
            terminal_id,
            replaced_pid,
            cwd,
            resumed,
            launch_env,
            rows,
            cols,
        } = restart;
        if self.pending_agent_restart_shutdowns.get(&pane_id) != Some(&replaced_pid) {
            return;
        }
        self.pending_agent_restart_shutdowns.remove(&pane_id);
        let Some((ws_idx, pane)) = self.find_pane(pane_id) else {
            return;
        };
        if pane.attached_terminal_id != terminal_id
            || self.terminal_runtimes.get(&terminal_id).is_some()
        {
            return;
        }
        if replaced_pid.is_some_and(crate::platform::process_exists) {
            if let Some(terminal) = self.state.terminals.get_mut(&terminal_id) {
                terminal.restore_error =
                    Some("start_failed: old agent runtime did not exit".into());
                terminal.revision = terminal.revision.saturating_add(1);
            }
            self.emit_pane_updated(ws_idx, pane_id);
            return;
        }
        let expected_agent = resumed.agent.clone();
        // Direct argv execution avoids terminal canonical-input limits and shell quoting.
        let runtime = crate::terminal::TerminalRuntime::spawn_argv_command(
            pane_id,
            rows,
            cols,
            cwd,
            &resumed.argv,
            &launch_env,
            crate::pane::AgentDetection::Enabled,
            self.state.pane_scrollback_limit_bytes,
            self.state.host_terminal_theme,
            self.state.host_terminal_appearance,
            self.event_tx.clone(),
            self.render_notify.clone(),
            self.render_dirty.clone(),
        );
        let runtime = match runtime {
            Ok(runtime) => runtime,
            Err(err) => {
                if let Some(terminal) = self.state.terminals.get_mut(&terminal_id) {
                    terminal.restore_error =
                        Some(format!("start_failed: could not resume agent: {err}"));
                    terminal.revision = terminal.revision.saturating_add(1);
                }
                self.emit_pane_updated(ws_idx, pane_id);
                return;
            }
        };
        let runtime_pid = runtime.child_pid();
        let event_tx = self.event_tx.clone();
        let launcher = resumed.launcher.clone();
        let startup_argv = resumed.argv.clone();
        // Process discovery can be slow; only the worker polls, never the server loop.
        std::thread::spawn(move || {
            let started = runtime_pid.is_some_and(|pid| {
                let deadline = Instant::now() + std::time::Duration::from_secs(5);
                let settle = Instant::now() + std::time::Duration::from_millis(150);
                while Instant::now() < deadline {
                    let is_agent = |argv: &[String]| {
                        argv.first().is_some_and(|program| {
                            crate::agent_resume::same_executable(program, &expected_agent)
                        })
                    };
                    let process_started = crate::platform::process_launch_argv(pid)
                        .is_some_and(|argv| is_agent(&argv));
                    let descendant_started =
                        crate::detect::foreground_job(pid).is_some_and(|job| {
                            job.processes.iter().any(|process| {
                                process.argv.as_ref().is_some_and(|argv| is_agent(argv))
                            })
                        });
                    let recorded_launcher_started = launcher == "recorded"
                        && !startup_argv.iter().take(2).any(|program| {
                            crate::agent_resume::same_executable(program, "claude-lb-launch")
                        })
                        && crate::platform::process_launch_argv(pid).is_some_and(|argv| {
                            argv.first().is_some_and(|program| {
                                crate::agent_resume::same_executable(program, &startup_argv[0])
                            })
                        });
                    if Instant::now() >= settle
                        && (process_started || descendant_started || recorded_launcher_started)
                    {
                        return true;
                    }
                    if !crate::platform::process_exists(pid) {
                        return false;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(25));
                }
                false
            });
            let _ = event_tx.blocking_send(crate::events::AppEvent::AgentRestartStartupFinished {
                pane_id,
                runtime_pid,
                started,
            });
        });
        self.retained_agent_resume_panes.insert(
            pane_id,
            AgentResumeReplacementWindow {
                runtime_pid: runtime.child_pid(),
                until: Instant::now() + AGENT_RESUME_REPLACEMENT_WINDOW,
            },
        );
        // Keep the replacement runtime attached even when startup times out.
        self.terminal_runtimes.insert(terminal_id.clone(), runtime);
        if let Some(terminal) = self.state.terminals.get_mut(&terminal_id) {
            terminal.pending_agent_resume_plan = None;
            terminal.respawn_shell_on_exit = true;
        }
        self.state.mark_session_dirty();
        self.emit_pane_updated(ws_idx, pane_id);
    }

    /// Whether `PaneDied` for `runtime_pid` is the replacement runtime dying
    /// inside its `agent.resume` window. Claiming closes the window; an
    /// expired or mismatched window is dropped and claims nothing.
    pub(crate) fn claim_agent_resume_replacement_exit(
        &mut self,
        pane_id: crate::layout::PaneId,
        runtime_pid: Option<u32>,
        now: Instant,
    ) -> bool {
        let Some(window) = self.retained_agent_resume_panes.get(&pane_id).copied() else {
            return false;
        };
        if now >= window.until {
            self.retained_agent_resume_panes.remove(&pane_id);
            return false;
        }
        if window.runtime_pid.is_none() || window.runtime_pid != runtime_pid {
            return false;
        }
        self.retained_agent_resume_panes.remove(&pane_id);
        true
    }

    /// The resumed agent was observed under the replacement runtime: the
    /// replacement window is over and later exits are ordinary pane exits.
    /// Only a process detection tagged with the replacement's own pid counts;
    /// observations queued by the replaced runtime (or untagged screen and
    /// hook reports, which cannot say which runtime they came from) leave the
    /// window open.
    pub(crate) fn close_agent_resume_window_on_ready(&mut self, event: &crate::events::AppEvent) {
        if self.retained_agent_resume_panes.is_empty() {
            return;
        }
        let crate::events::AppEvent::AgentProcessDetected {
            pane_id,
            runtime_pid: Some(runtime_pid),
            ..
        } = event
        else {
            return;
        };
        if self
            .retained_agent_resume_panes
            .get(pane_id)
            .is_some_and(|window| window.runtime_pid == Some(*runtime_pid))
        {
            self.retained_agent_resume_panes.remove(pane_id);
        }
    }

    /// Whether `event` is a detection from a runtime the pane no longer runs
    /// (one `agent.resume` or a respawn replaced). Its agent or state must
    /// not overwrite what the replacement reports.
    pub(crate) fn is_superseded_runtime_detection(&self, event: &crate::events::AppEvent) -> bool {
        let (pane_id, Some(event_pid)) = (match event {
            crate::events::AppEvent::AgentProcessDetected {
                pane_id,
                runtime_pid,
                ..
            }
            | crate::events::AppEvent::StateChanged {
                pane_id,
                runtime_pid,
                ..
            } => (*pane_id, *runtime_pid),
            _ => return false,
        }) else {
            return false;
        };
        let Some(ws_idx) = self
            .state
            .workspaces
            .iter()
            .position(|workspace| workspace.pane_state(pane_id).is_some())
        else {
            return false;
        };
        // With no runtime at all (the replacement failed to spawn), every
        // tagged detection comes from a runtime that is gone.
        match self.lookup_runtime_sender(ws_idx, pane_id) {
            None => true,
            Some(runtime) => runtime
                .child_pid()
                .is_some_and(|current| current != event_pid),
        }
    }

    /// Consume one exit owed by a runtime `agent.resume` replaced.
    pub(crate) fn take_agent_resume_runtime_exit(
        &mut self,
        pane_id: crate::layout::PaneId,
        runtime_pid: Option<u32>,
    ) -> bool {
        if self.pending_agent_resume_runtime_exits.get(&pane_id) != Some(&runtime_pid) {
            return false;
        }
        self.pending_agent_resume_runtime_exits.remove(&pane_id);
        true
    }

    pub(crate) fn has_pending_agent_resumes(&self) -> bool {
        self.state
            .terminals
            .values()
            .any(|terminal| terminal.pending_agent_resume_plan.is_some())
    }

    pub(crate) fn sync_pending_agent_resume_deadline(&mut self, now: Instant) {
        if !self.has_pending_agent_resumes() {
            self.pending_agent_resume_deadline = None;
            self.next_agent_resume_at = None;
            return;
        }
        if self.pending_agent_resume_candidates().is_empty() {
            self.pending_agent_resume_deadline = None;
            return;
        }
        if let Some(next) = self.next_agent_resume_at {
            self.pending_agent_resume_deadline = Some(next);
        } else {
            self.pending_agent_resume_deadline
                .get_or_insert(now + super::PENDING_AGENT_RESUME_THEME_WAIT);
        }
    }

    pub(crate) fn pending_agent_resume_due(&self, now: Instant) -> bool {
        self.pending_agent_resume_deadline
            .is_some_and(|deadline| now >= deadline)
    }

    pub(crate) fn start_pending_agent_resumes(
        &mut self,
        now: Instant,
        allow_empty_theme: bool,
    ) -> bool {
        // Geometry/theme events can also enter here; they must not bypass spacing.
        if self.next_agent_resume_at.is_some_and(|next| now < next) {
            return false;
        }
        let pending = self.pending_agent_resume_candidates();
        let mut changed = false;
        for PendingAgentResumeCandidate {
            pane_id,
            terminal_id,
            cwd,
            plan,
            rows,
            cols,
        } in pending
        {
            if self.terminal_runtimes.get(&terminal_id).is_some() {
                continue;
            }
            changed |= self.start_pending_agent_resume(
                pane_id,
                terminal_id,
                cwd,
                plan,
                rows,
                cols,
                allow_empty_theme,
            );
            if changed && !self.startup_per_agent_delay.is_zero() {
                self.next_agent_resume_at = Some(now + self.startup_per_agent_delay);
                self.pending_agent_resume_deadline = self.next_agent_resume_at;
                break;
            }
        }

        if changed {
            self.schedule_session_save();
        }
        if !self.has_pending_agent_resumes() || self.pending_agent_resume_candidates().is_empty() {
            self.pending_agent_resume_deadline = None;
        }
        if !self.has_pending_agent_resumes() {
            self.next_agent_resume_at = None;
        }
        changed
    }

    fn pending_agent_resume_candidates(&self) -> Vec<PendingAgentResumeCandidate> {
        let terminal_area = self.state.view.terminal_area;
        if terminal_area.width == 0 || terminal_area.height == 0 {
            return Vec::new();
        };

        let mut pending = Vec::new();
        for (ws_idx, ws) in self.state.workspaces.iter().enumerate() {
            for (tab_idx, tab) in ws.tabs.iter().enumerate() {
                for info in
                    self.pending_agent_resume_pane_infos(ws_idx, tab_idx, tab, terminal_area)
                {
                    let Some(pane) = tab.panes.get(&info.id) else {
                        continue;
                    };
                    if self
                        .terminal_runtimes
                        .get(&pane.attached_terminal_id)
                        .is_some()
                    {
                        continue;
                    }
                    let Some(terminal) = self.state.terminals.get(&pane.attached_terminal_id)
                    else {
                        continue;
                    };
                    let Some(plan) = terminal.pending_agent_resume_plan.clone() else {
                        continue;
                    };
                    pending.push(PendingAgentResumeCandidate {
                        pane_id: info.id,
                        terminal_id: pane.attached_terminal_id.clone(),
                        cwd: terminal.cwd.clone(),
                        plan,
                        rows: info.inner_rect.height,
                        cols: info.inner_rect.width,
                    });
                }
            }
        }
        pending
    }

    fn pending_agent_resume_pane_infos(
        &self,
        ws_idx: usize,
        tab_idx: usize,
        tab: &crate::workspace::Tab,
        terminal_area: Rect,
    ) -> Vec<crate::layout::PaneInfo> {
        let mut pane_infos = derived_pending_agent_resume_pane_infos(
            tab,
            terminal_area,
            self.state.pane_borders,
            self.state.pane_gaps,
            self.state.pane_outer_borders,
        );

        if self.state.active == Some(ws_idx)
            && self
                .state
                .workspaces
                .get(ws_idx)
                .is_some_and(|ws| tab_idx == ws.active_tab_index())
        {
            for visible_info in &self.state.view.pane_infos {
                if let Some(info) = pane_infos
                    .iter_mut()
                    .find(|info| info.id == visible_info.id)
                {
                    *info = visible_info.clone();
                } else {
                    pane_infos.push(visible_info.clone());
                }
            }
        }

        pane_infos
    }

    pub(crate) fn start_pending_agent_resume_for_terminal(
        &mut self,
        terminal_id: &crate::terminal::TerminalId,
        rows: u16,
        cols: u16,
        allow_empty_theme: bool,
    ) -> bool {
        if self.terminal_runtimes.get(terminal_id).is_some() {
            return false;
        }
        let Some((pane_id, cwd, plan)) = self.state.workspaces.iter().find_map(|ws| {
            ws.tabs.iter().find_map(|tab| {
                tab.layout.pane_ids().into_iter().find_map(|pane_id| {
                    let pane = tab.panes.get(&pane_id)?;
                    if &pane.attached_terminal_id != terminal_id {
                        return None;
                    }
                    let terminal = self.state.terminals.get(terminal_id)?;
                    Some((
                        pane_id,
                        terminal.cwd.clone(),
                        terminal.pending_agent_resume_plan.clone()?,
                    ))
                })
            })
        }) else {
            return false;
        };

        let changed = self.start_pending_agent_resume(
            pane_id,
            terminal_id.clone(),
            cwd,
            plan,
            rows,
            cols,
            allow_empty_theme,
        );
        if changed {
            self.schedule_session_save();
        }
        if !self.has_pending_agent_resumes() {
            self.pending_agent_resume_deadline = None;
        }
        changed
    }

    fn start_pending_agent_resume(
        &mut self,
        pane_id: crate::layout::PaneId,
        terminal_id: crate::terminal::TerminalId,
        cwd: std::path::PathBuf,
        plan: crate::agent_resume::AgentResumePlan,
        rows: u16,
        cols: u16,
        allow_empty_theme: bool,
    ) -> bool {
        let host_terminal_theme = self.state.host_terminal_theme;
        if host_terminal_theme.is_empty() && !allow_empty_theme {
            return false;
        }

        let Some(resume_command) = shell_command_from_argv(&plan.argv) else {
            tracing::warn!(
                pane = pane_id.raw(),
                terminal = %terminal_id,
                agent = %plan.agent,
                "failed to start deferred agent resume with empty argv"
            );
            return false;
        };
        let Some(launch_env) = self
            .find_pane(pane_id)
            .and_then(|(ws_idx, _)| self.pane_launch_env(ws_idx, pane_id, Vec::new()))
        else {
            return false;
        };

        if !cwd.is_dir() {
            if let Some(terminal) = self.state.terminals.get_mut(&terminal_id) {
                terminal.pending_agent_resume_plan = None;
                terminal.restore_error = Some("Saved directory is unavailable. Restore the directory and restart this session.".into());
                terminal.revision = terminal.revision.saturating_add(1);
            }
            return true;
        }

        let runtime = match crate::terminal::TerminalRuntime::spawn(
            pane_id,
            rows,
            cols,
            cwd,
            self.state.pane_scrollback_limit_bytes,
            host_terminal_theme,
            self.state.host_terminal_appearance,
            crate::pane::PaneShellConfig::new(&self.state.default_shell, self.state.shell_mode),
            &launch_env,
            self.event_tx.clone(),
            self.render_notify.clone(),
            self.render_dirty.clone(),
        ) {
            Ok(runtime) => runtime,
            Err(err) => {
                tracing::warn!(
                    pane = pane_id.raw(),
                    terminal = %terminal_id,
                    agent = %plan.agent,
                    err = %err,
                    "failed to start shell for deferred agent resume"
                );
                if let Some(terminal) = self.state.terminals.get_mut(&terminal_id) {
                    terminal.pending_agent_resume_plan = None;
                    terminal.restore_error = Some(format!(
                        "Could not start the saved shell: {err}. Fix the shell configuration and restart this session."
                    ));
                    terminal.revision = terminal.revision.saturating_add(1);
                }
                return true;
            }
        };

        let mut input = resume_command;
        input.push('\r');
        if let Err(err) = runtime.try_send_bytes(Bytes::from(input)) {
            tracing::warn!(
                pane = pane_id.raw(),
                terminal = %terminal_id,
                agent = %plan.agent,
                err = %err,
                "failed to send deferred agent resume command to shell"
            );
            runtime.shutdown();
            return false;
        }

        self.terminal_runtimes.insert(terminal_id.clone(), runtime);
        if let Some(terminal) = self.state.terminals.get_mut(&terminal_id) {
            terminal.pending_agent_resume_plan = None;
            terminal.respawn_shell_on_exit = false;
        }
        true
    }
}

fn derived_pending_agent_resume_pane_infos(
    tab: &crate::workspace::Tab,
    terminal_area: Rect,
    pane_borders: crate::config::PaneBordersConfig,
    pane_gaps: bool,
    pane_outer_borders: bool,
) -> Vec<crate::layout::PaneInfo> {
    crate::ui::apply_pane_chrome(
        tab.layout.panes(terminal_area),
        pane_borders,
        pane_gaps,
        pane_outer_borders,
    )
    .into_iter()
    .map(|mut info| {
        let pane_inner = crate::ui::pane_inner_rect(info.rect, info.borders);
        info.inner_rect = stable_terminal_inner_rect(pane_inner);
        info
    })
    .collect()
}

fn stable_terminal_inner_rect(pane_inner: Rect) -> Rect {
    if pane_inner.width <= 4 {
        return pane_inner;
    }

    Rect::new(
        pane_inner.x,
        pane_inner.y,
        pane_inner.width.saturating_sub(1),
        pane_inner.height,
    )
}

/// The line typed into the replacement pane shell for `agent.resume`,
/// serialized for that shell. Refuses argv that the terminal's line editor
/// could reinterpret (any control character, even inside quotes) and shells
/// herdr cannot quote for, so nothing is typed that differs from the argv.
#[cfg(test)]
fn resume_shell_command(argv: &[String], shell: &str) -> Result<String, String> {
    if argv.is_empty() {
        return Err("resume command is empty".into());
    }
    if argv.iter().any(|arg| arg.chars().any(|ch| ch.is_control())) {
        return Err("resume argv contains terminal control characters".into());
    }
    if !crate::platform::is_quotable_interactive_shell(shell) {
        return Err(format!("cannot type a resume command into shell {shell:?}"));
    }
    crate::platform::resume_shell_command(argv, shell)
        .ok_or_else(|| format!("cannot type a resume command into shell {shell:?}"))
}

fn shell_command_from_argv(argv: &[String]) -> Option<String> {
    let mut parts = argv.iter();
    let first = shell_quote(parts.next()?);
    let mut command = first;
    for part in parts {
        command.push(' ');
        command.push_str(&shell_quote(part));
    }
    Some(command)
}

fn shell_quote(value: &str) -> String {
    if value.is_empty() {
        return "''".to_string();
    }
    if value.bytes().all(|byte| {
        byte.is_ascii_alphanumeric()
            || matches!(
                byte,
                b'_' | b'-' | b'.' | b'/' | b':' | b'@' | b'%' | b'+' | b'='
            )
    }) {
        return value.to_string();
    }
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn test_app() -> App {
        let (_api_tx, api_rx) = tokio::sync::mpsc::unbounded_channel();
        App::new(
            &crate::config::Config::default(),
            crate::app::AppPolicy::TEST,
            None,
            api_rx,
            crate::api::EventHub::default(),
        )
    }

    #[tokio::test]
    async fn pending_agent_resume_spacing_survives_events_and_failed_restores() {
        for delay_ms in [100, 250, 0] {
            let config: crate::config::Config = toml::from_str(&format!(
                "[session]\nstartup_per_agent_delay_ms = {delay_ms}"
            ))
            .unwrap();
            let (_api_tx, api_rx) = tokio::sync::mpsc::unbounded_channel();
            let mut app = App::new(
                &config,
                crate::app::AppPolicy::TEST,
                None,
                api_rx,
                crate::api::EventHub::default(),
            );
            app.state.workspaces = (0..4)
                .map(|_| crate::workspace::Workspace::test_new("restore"))
                .collect();
            app.state.active = Some(0);
            app.state.view.terminal_area = Rect::new(0, 0, 100, 30);
            app.state.ensure_test_terminals();
            let missing = std::env::current_dir()
                .unwrap()
                .join("__missing_resume_cwd__");
            assert!(!missing.exists());
            for terminal in app.state.terminals.values_mut() {
                terminal.cwd = missing.clone();
                terminal.pending_agent_resume_plan = Some(crate::agent_resume::AgentResumePlan {
                    agent: "codex".into(),
                    argv: vec!["codex".into()],
                    dedupe_key: terminal.id.to_string(),
                });
            }
            let now = Instant::now();
            app.sync_pending_agent_resume_deadline(now);
            assert!(!app.start_pending_agent_resumes(now, false));
            assert!(app.start_pending_agent_resumes(now, true));
            if delay_ms != 0 {
                let next = now + std::time::Duration::from_millis(delay_ms);
                assert_eq!(
                    app.state
                        .terminals
                        .values()
                        .filter(|t| t.restore_error.is_some())
                        .count(),
                    1
                );
                // Geometry changes clear the wakeup, but must preserve the launch gap.
                app.pending_agent_resume_deadline = None;
                app.sync_pending_agent_resume_deadline(now);
                assert_eq!(app.pending_agent_resume_deadline, Some(next));
                assert!(!app
                    .start_pending_agent_resumes(next - std::time::Duration::from_millis(1), true));
                // A late wakeup must not release every overdue agent in a burst.
                for processed in 2..=4 {
                    let late = now + std::time::Duration::from_secs(processed * 10);
                    assert!(app.start_pending_agent_resumes(late, true));
                    assert_eq!(
                        app.state
                            .terminals
                            .values()
                            .filter(|t| t.restore_error.is_some())
                            .count(),
                        processed as usize
                    );
                }
            }
            assert!(!app.has_pending_agent_resumes());
            assert!(app.pending_agent_resume_deadline.is_none());
            assert!(app.next_agent_resume_at.is_none());
            assert_eq!(
                app.state
                    .terminals
                    .values()
                    .filter(|t| t.restore_error.is_some())
                    .count(),
                4
            );
        }
    }

    #[cfg(unix)]
    fn long_running_test_argv() -> Vec<String> {
        vec!["/bin/sh".into(), "-c".into(), "sleep 5".into()]
    }

    #[cfg(unix)]
    fn marker_resume_test_argv() -> Vec<String> {
        vec![
            "/bin/sh".into(),
            "-c".into(),
            "printf '%s' 'restored agent: shell quoted | marker'; sleep 5".into(),
        ]
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn failed_deferred_restore_keeps_session_reference_without_retrying_elsewhere() {
        for missing_shell in [false, true] {
            let mut app = test_app();
            let workspace = crate::workspace::Workspace::test_new("unavailable");
            let pane_id = workspace.tabs[0].root_pane;
            let terminal_id = workspace.terminal_id(pane_id).unwrap().clone();
            app.state.workspaces = vec![workspace];
            app.state.active = Some(0);
            app.state.ensure_test_terminals();
            if missing_shell {
                app.state.default_shell = "__herdr_missing_resume_shell__".into();
            }
            let terminal = app.state.terminals.get_mut(&terminal_id).unwrap();
            if !missing_shell {
                terminal.cwd = std::env::current_dir()
                    .unwrap()
                    .join("__herdr_missing_resume_cwd__");
                assert!(!terminal.cwd.exists());
            }
            let session = crate::agent_resume::PersistedAgentSession {
                source: "herdr:codex".into(),
                agent: "codex".into(),
                session_ref: crate::agent_resume::AgentSessionRef::id("resume-test").unwrap(),
            };
            terminal.persisted_agent_session = Some(session.clone());
            terminal.pending_agent_resume_plan = Some(crate::agent_resume::AgentResumePlan {
                agent: "codex".into(),
                argv: long_running_test_argv(),
                dedupe_key: "resume-test".into(),
            });
            app.start_pending_agent_resume_for_terminal(&terminal_id, 24, 80, true);
            assert!(app.terminal_runtimes.get(&terminal_id).is_none());
            let terminal = &app.state.terminals[&terminal_id];
            assert!(terminal.pending_agent_resume_plan.is_none());
            assert_eq!(terminal.persisted_agent_session.as_ref(), Some(&session));
            assert!(terminal.restore_error.is_some());
            assert!(!app.has_pending_agent_resumes());
            assert!(!app.start_pending_agent_resume_for_terminal(&terminal_id, 24, 80, true));
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn pending_agent_resume_waits_for_host_theme_before_launch() {
        let mut app = test_app();
        let workspace = crate::workspace::Workspace::test_new("restored");
        let pane_id = workspace.tabs[0].root_pane;
        let terminal_id = workspace.terminal_id(pane_id).cloned().unwrap();
        let pane_infos = workspace.tabs[0]
            .layout
            .panes(ratatui::layout::Rect::new(0, 0, 100, 30));
        app.state.workspaces = vec![workspace];
        app.state.active = Some(0);
        app.state.ensure_test_terminals();
        app.state.view.terminal_area = ratatui::layout::Rect::new(0, 0, 100, 30);
        app.state.view.pane_infos = pane_infos;
        let terminal = app
            .state
            .terminals
            .get_mut(&terminal_id)
            .expect("test terminal should exist");
        terminal.pending_agent_resume_plan = Some(crate::agent_resume::AgentResumePlan {
            agent: "codex".into(),
            argv: marker_resume_test_argv(),
            dedupe_key: "herdr:codex\0codex\0Id\0codex-session".into(),
        });

        assert!(!app.start_pending_agent_resumes(Instant::now(), false));
        assert!(app.terminal_runtimes.get(&terminal_id).is_none());

        app.state.host_terminal_theme = crate::terminal_theme::TerminalTheme {
            foreground: Some(crate::terminal_theme::RgbColor {
                r: 220,
                g: 220,
                b: 220,
            }),
            background: Some(crate::terminal_theme::RgbColor {
                r: 20,
                g: 20,
                b: 20,
            }),
            ..Default::default()
        };

        assert!(app.start_pending_agent_resumes(Instant::now(), false));
        assert!(app.terminal_runtimes.get(&terminal_id).is_some());
        let terminal = app
            .state
            .terminals
            .get(&terminal_id)
            .expect("terminal should survive launch");
        assert!(terminal.pending_agent_resume_plan.is_none());
        assert!(!terminal.respawn_shell_on_exit);

        let runtime = app
            .terminal_runtimes
            .get(&terminal_id)
            .expect("pending resume should leave a shell runtime");
        let marker = "restored agent: shell quoted | marker";
        for _ in 0..20 {
            if runtime
                .snapshot_history()
                .is_some_and(|text| text.contains(marker))
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        assert!(
            runtime
                .snapshot_history()
                .expect("runtime should expose terminal history")
                .contains(marker),
            "deferred restore should inject the resume argv into the restored shell"
        );

        for (_, runtime) in app.terminal_runtimes.drain() {
            runtime.shutdown();
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn pending_agent_resume_can_launch_after_theme_wait_expires() {
        let mut app = test_app();
        let workspace = crate::workspace::Workspace::test_new("restored");
        let pane_id = workspace.tabs[0].root_pane;
        let terminal_id = workspace.terminal_id(pane_id).cloned().unwrap();
        app.state.view.pane_infos = workspace.tabs[0]
            .layout
            .panes(ratatui::layout::Rect::new(0, 0, 100, 30));
        app.state.view.terminal_area = ratatui::layout::Rect::new(0, 0, 100, 30);
        app.state.workspaces = vec![workspace];
        app.state.active = Some(0);
        app.state.ensure_test_terminals();
        app.state
            .terminals
            .get_mut(&terminal_id)
            .expect("test terminal should exist")
            .pending_agent_resume_plan = Some(crate::agent_resume::AgentResumePlan {
            agent: "codex".into(),
            argv: long_running_test_argv(),
            dedupe_key: "herdr:codex\0codex\0Id\0codex-session".into(),
        });

        app.sync_pending_agent_resume_deadline(std::time::Instant::now());
        assert!(!app.start_pending_agent_resumes(Instant::now(), false));
        assert!(app.start_pending_agent_resumes(Instant::now(), true));
        assert!(app.terminal_runtimes.get(&terminal_id).is_some());

        for (_, runtime) in app.terminal_runtimes.drain() {
            runtime.shutdown();
        }
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn pending_agent_resume_launches_hidden_panes_with_current_terminal_area() {
        let mut app = test_app();
        let active_workspace = crate::workspace::Workspace::test_new("active");
        let active_pane = active_workspace.tabs[0].root_pane;
        let active_terminal = active_workspace.terminal_id(active_pane).cloned().unwrap();
        let hidden_workspace = crate::workspace::Workspace::test_new("hidden");
        let hidden_pane = hidden_workspace.tabs[0].root_pane;
        let hidden_terminal = hidden_workspace.terminal_id(hidden_pane).cloned().unwrap();
        app.state.view.pane_infos = active_workspace.tabs[0]
            .layout
            .panes(ratatui::layout::Rect::new(0, 0, 100, 30));
        app.state.view.terminal_area = ratatui::layout::Rect::new(0, 0, 100, 30);
        app.state.workspaces = vec![active_workspace, hidden_workspace];
        app.state.active = Some(0);
        app.state.ensure_test_terminals();
        app.state.host_terminal_theme = crate::terminal_theme::TerminalTheme {
            foreground: Some(crate::terminal_theme::RgbColor {
                r: 220,
                g: 220,
                b: 220,
            }),
            background: Some(crate::terminal_theme::RgbColor {
                r: 20,
                g: 20,
                b: 20,
            }),
            ..Default::default()
        };
        for terminal_id in [&active_terminal, &hidden_terminal] {
            app.state
                .terminals
                .get_mut(terminal_id)
                .expect("test terminal should exist")
                .pending_agent_resume_plan = Some(crate::agent_resume::AgentResumePlan {
                agent: "codex".into(),
                argv: long_running_test_argv(),
                dedupe_key: format!("herdr:codex\0codex\0Id\0{terminal_id}"),
            });
        }
        app.pending_agent_resume_deadline =
            Some(std::time::Instant::now() - std::time::Duration::from_millis(1));

        let now = Instant::now();
        assert!(app.start_pending_agent_resumes(now, false));
        assert!(app.terminal_runtimes.get(&active_terminal).is_some());
        assert!(app.terminal_runtimes.get(&hidden_terminal).is_none());
        assert!(!app.start_pending_agent_resumes(now, true));
        assert!(
            app.start_pending_agent_resumes(now + std::time::Duration::from_millis(100), false,)
        );
        assert!(app.terminal_runtimes.get(&hidden_terminal).is_some());
        assert!(
            app.pending_agent_resume_deadline.is_none(),
            "launched pending resumes should clear the wakeup deadline"
        );

        for (_, runtime) in app.terminal_runtimes.drain() {
            runtime.shutdown();
        }
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn pending_agent_resume_launches_inactive_tab_panes_with_current_terminal_area() {
        let mut app = test_app();
        let mut workspace = crate::workspace::Workspace::test_new("tabs");
        let active_pane = workspace.tabs[0].root_pane;
        let inactive_tab = workspace.test_add_tab(Some("agents"));
        let inactive_pane = workspace.tabs[inactive_tab].root_pane;
        let inactive_terminal = workspace.tabs[inactive_tab]
            .terminal_id(inactive_pane)
            .cloned()
            .unwrap();
        app.state.view.pane_infos = workspace.tabs[0]
            .layout
            .panes(ratatui::layout::Rect::new(0, 0, 100, 30));
        app.state.view.terminal_area = ratatui::layout::Rect::new(0, 0, 100, 30);
        app.state.workspaces = vec![workspace];
        app.state.active = Some(0);
        app.state.ensure_test_terminals();
        assert!(app
            .state
            .workspaces
            .first()
            .and_then(|ws| ws.tabs[0].terminal_id(active_pane))
            .is_some());
        app.state.host_terminal_theme = crate::terminal_theme::TerminalTheme {
            foreground: Some(crate::terminal_theme::RgbColor {
                r: 220,
                g: 220,
                b: 220,
            }),
            background: Some(crate::terminal_theme::RgbColor {
                r: 20,
                g: 20,
                b: 20,
            }),
            ..Default::default()
        };
        app.state
            .terminals
            .get_mut(&inactive_terminal)
            .expect("inactive tab terminal should exist")
            .pending_agent_resume_plan = Some(crate::agent_resume::AgentResumePlan {
            agent: "codex".into(),
            argv: long_running_test_argv(),
            dedupe_key: "herdr:codex\0codex\0Id\0inactive-tab-session".into(),
        });

        assert!(app.start_pending_agent_resumes(Instant::now(), false));
        assert!(app.terminal_runtimes.get(&inactive_terminal).is_some());
        assert!(
            app.state
                .terminals
                .get(&inactive_terminal)
                .expect("inactive tab terminal should still exist")
                .pending_agent_resume_plan
                .is_none(),
            "inactive tab restored panes should not wait for tab focus"
        );

        for (_, runtime) in app.terminal_runtimes.drain() {
            runtime.shutdown();
        }
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn pending_agent_resume_launches_zoom_hidden_active_tab_panes() {
        let mut app = test_app();
        let mut workspace = crate::workspace::Workspace::test_new("zoomed");
        let hidden_pane = workspace.tabs[0].root_pane;
        let visible_pane = workspace.test_split(ratatui::layout::Direction::Horizontal);
        workspace.tabs[0].zoomed = true;
        let hidden_terminal = workspace.terminal_id(hidden_pane).cloned().unwrap();
        app.state.view.pane_infos = vec![crate::layout::PaneInfo {
            id: visible_pane,
            rect: ratatui::layout::Rect::new(0, 0, 100, 30),
            inner_rect: ratatui::layout::Rect::new(1, 1, 98, 28),
            scrollbar_rect: None,
            borders: ratatui::widgets::Borders::ALL,
            is_focused: true,
        }];
        app.state.view.terminal_area = ratatui::layout::Rect::new(0, 0, 100, 30);
        app.state.workspaces = vec![workspace];
        app.state.active = Some(0);
        app.state.ensure_test_terminals();
        app.state.host_terminal_theme = crate::terminal_theme::TerminalTheme {
            foreground: Some(crate::terminal_theme::RgbColor {
                r: 220,
                g: 220,
                b: 220,
            }),
            background: Some(crate::terminal_theme::RgbColor {
                r: 20,
                g: 20,
                b: 20,
            }),
            ..Default::default()
        };
        app.state
            .terminals
            .get_mut(&hidden_terminal)
            .expect("hidden zoom pane terminal should exist")
            .pending_agent_resume_plan = Some(crate::agent_resume::AgentResumePlan {
            agent: "codex".into(),
            argv: long_running_test_argv(),
            dedupe_key: "herdr:codex\0codex\0Id\0zoom-hidden-session".into(),
        });

        assert!(app.start_pending_agent_resumes(Instant::now(), false));
        assert!(app.terminal_runtimes.get(&hidden_terminal).is_some());
        assert!(
            app.state
                .terminals
                .get(&hidden_terminal)
                .expect("hidden zoom pane terminal should still exist")
                .pending_agent_resume_plan
                .is_none(),
            "zoom-hidden restored panes should not wait for pane focus"
        );

        for (_, runtime) in app.terminal_runtimes.drain() {
            runtime.shutdown();
        }
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn pending_agent_resume_uses_current_terminal_area_for_background_panes() {
        let mut app = test_app();
        let previous_workspace = crate::workspace::Workspace::test_new("previous");
        let previous_pane = previous_workspace.tabs[0].root_pane;
        let previous_terminal = previous_workspace
            .terminal_id(previous_pane)
            .cloned()
            .unwrap();
        let current_workspace = crate::workspace::Workspace::test_new("current");
        app.state.view.pane_infos = previous_workspace.tabs[0]
            .layout
            .panes(ratatui::layout::Rect::new(0, 0, 100, 30));
        app.state.view.terminal_area = ratatui::layout::Rect::new(0, 0, 80, 24);
        app.state.workspaces = vec![previous_workspace, current_workspace];
        app.state.active = Some(1);
        app.state.ensure_test_terminals();
        app.state.host_terminal_theme = crate::terminal_theme::TerminalTheme {
            foreground: Some(crate::terminal_theme::RgbColor {
                r: 220,
                g: 220,
                b: 220,
            }),
            background: Some(crate::terminal_theme::RgbColor {
                r: 20,
                g: 20,
                b: 20,
            }),
            ..Default::default()
        };
        app.state
            .terminals
            .get_mut(&previous_terminal)
            .expect("test terminal should exist")
            .pending_agent_resume_plan = Some(crate::agent_resume::AgentResumePlan {
            agent: "codex".into(),
            argv: long_running_test_argv(),
            dedupe_key: "herdr:codex\0codex\0Id\0codex-session".into(),
        });

        app.sync_pending_agent_resume_deadline(std::time::Instant::now());
        assert!(app.pending_agent_resume_deadline.is_some());
        assert!(app.start_pending_agent_resumes(Instant::now(), false));
        assert!(app.terminal_runtimes.get(&previous_terminal).is_some());
        assert!(
            app.state
                .terminals
                .get(&previous_terminal)
                .expect("previous terminal should still exist")
                .pending_agent_resume_plan
                .is_none(),
            "background restored panes should not wait for focus once terminal area is known"
        );

        for (_, runtime) in app.terminal_runtimes.drain() {
            runtime.shutdown();
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn pending_agent_resume_launches_with_inner_rect_size() {
        let mut app = test_app();
        let mut workspace = crate::workspace::Workspace::test_new("split");
        let pane_id = workspace.test_split(ratatui::layout::Direction::Horizontal);
        let terminal_id = workspace.terminal_id(pane_id).cloned().unwrap();
        app.state.view.pane_infos = vec![crate::layout::PaneInfo {
            id: pane_id,
            rect: ratatui::layout::Rect::new(0, 0, 100, 30),
            inner_rect: ratatui::layout::Rect::new(1, 1, 98, 28),
            scrollbar_rect: None,
            borders: ratatui::widgets::Borders::ALL,
            is_focused: true,
        }];
        app.state.view.terminal_area = ratatui::layout::Rect::new(0, 0, 100, 30);
        app.state.workspaces = vec![workspace];
        app.state.active = Some(0);
        app.state.ensure_test_terminals();
        app.state.host_terminal_theme = crate::terminal_theme::TerminalTheme {
            foreground: Some(crate::terminal_theme::RgbColor {
                r: 220,
                g: 220,
                b: 220,
            }),
            background: Some(crate::terminal_theme::RgbColor {
                r: 20,
                g: 20,
                b: 20,
            }),
            ..Default::default()
        };
        app.state
            .terminals
            .get_mut(&terminal_id)
            .expect("test terminal should exist")
            .pending_agent_resume_plan = Some(crate::agent_resume::AgentResumePlan {
            agent: "codex".into(),
            argv: long_running_test_argv(),
            dedupe_key: "herdr:codex\0codex\0Id\0codex-session".into(),
        });

        assert!(app.start_pending_agent_resumes(Instant::now(), false));
        assert_eq!(
            app.terminal_runtimes
                .get(&terminal_id)
                .expect("pending resume should launch")
                .current_size(),
            (28, 98)
        );

        for (_, runtime) in app.terminal_runtimes.drain() {
            runtime.shutdown();
        }
    }

    #[test]
    fn shell_command_from_argv_quotes_resume_arguments() {
        let argv = vec![
            "claude".to_string(),
            "--resume".to_string(),
            "session with ' quote".to_string(),
        ];

        assert_eq!(
            shell_command_from_argv(&argv).as_deref(),
            Some("claude --resume 'session with '\\'' quote'")
        );
        assert_eq!(shell_command_from_argv(&[]), None);
    }

    #[test]
    fn resume_shell_command_rejects_terminal_control_characters() {
        for bad in [
            "\u{15}echo INJECTED\r",
            "line\nbreak",
            "tab\there",
            "esc\u{1b}[A",
            "del\u{7f}",
            "c1\u{9b}",
        ] {
            let argv = vec!["claude".to_string(), "--resume".into(), bad.into()];
            for shell in ["bash", "/bin/zsh", "pwsh", "cmd.exe", "powershell.exe"] {
                assert!(
                    resume_shell_command(&argv, shell)
                        .unwrap_err()
                        .contains("control characters"),
                    "{bad:?} in {shell}"
                );
            }
        }
    }

    #[test]
    fn resume_shell_command_refuses_shells_it_cannot_quote_for() {
        let argv = vec!["claude".to_string(), "--resume".into(), "s 1".into()];
        for shell in ["fish", "/usr/bin/nu", "tcsh", "xonsh", ""] {
            assert!(resume_shell_command(&argv, shell).is_err(), "{shell}");
        }
        assert!(resume_shell_command(&[], "bash").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn resume_shell_command_serializes_for_the_pane_shell() {
        let argv = vec![
            "/opt/my tools/claude".to_string(),
            "--settings".into(),
            "{\"viewMode\":\"focus\"}".into(),
            "--resume".into(),
            "sess-1".into(),
        ];
        assert_eq!(
            resume_shell_command(&argv, "/bin/zsh").as_deref(),
            Ok("'/opt/my tools/claude' '--settings' '{\"viewMode\":\"focus\"}' '--resume' 'sess-1'")
        );
        assert_eq!(
            resume_shell_command(&argv, "pwsh").as_deref(),
            Ok("& '/opt/my tools/claude' '--settings' '{\"viewMode\":\"focus\"}' '--resume' 'sess-1'")
        );
        assert_eq!(
            resume_shell_command(&["claude".into(), "it\u{2019}s".into()], "pwsh").as_deref(),
            Ok("& 'claude' 'it\u{2019}\u{2019}s'")
        );
    }

    /// Tokens the shell would otherwise expand (zsh `=cmd`, `~`, globs,
    /// parameters) reach the program verbatim.
    #[cfg(unix)]
    #[test]
    fn resume_shell_command_quotes_every_token_so_the_shell_expands_nothing() {
        let args = ["=ls", "~", "~/x", "*", "a'b", "$HOME", "x=~", "!!"];
        let mut argv = vec!["printf".to_string(), "%s\\n".into()];
        argv.extend(args.iter().map(|arg| arg.to_string()));
        assert_eq!(
            resume_shell_command(&argv[..3], "zsh").as_deref(),
            Ok("'printf' '%s\\n' '=ls'")
        );
        for shell in ["/bin/sh", "/bin/zsh", "/bin/bash"] {
            if !std::path::Path::new(shell).exists() {
                continue;
            }
            let command = resume_shell_command(&argv, shell).unwrap();
            let output = std::process::Command::new(shell)
                .arg("-c")
                .arg(&command)
                .output()
                .unwrap();
            assert_eq!(
                String::from_utf8_lossy(&output.stdout),
                args.iter()
                    .map(|arg| format!("{arg}\n"))
                    .collect::<String>(),
                "{shell}: {command}"
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn resume_shell_command_serializes_for_windows_shells() {
        let argv = vec![
            r"C:\Program Files\claude\claude.exe".to_string(),
            "--resume".into(),
            "sess-1".into(),
        ];
        let powershell = resume_shell_command(&argv, "powershell.exe").unwrap();
        assert!(
            powershell.contains(r"& 'C:\Program Files\claude\claude.exe'"),
            "{powershell}"
        );
        assert!(
            powershell.contains("& 'C:\\Program Files\\claude\\claude.exe' '--resume' 'sess-1'"),
            "{powershell}"
        );
        let cmd = resume_shell_command(&argv, r"C:\Windows\System32\cmd.exe").unwrap();
        assert!(cmd.starts_with("powershell.exe -NoLogo -NoProfile -EncodedCommand "));
    }

    #[cfg(unix)]
    pub(super) fn claude_session(id: &str) -> crate::agent_resume::PersistedAgentSession {
        crate::agent_resume::PersistedAgentSession {
            source: "herdr:claude".into(),
            agent: "claude".into(),
            session_ref: crate::agent_resume::AgentSessionRef::id(id).unwrap(),
        }
    }

    #[cfg(unix)]
    /// One workspace, one pane whose terminal looks like a detected claude in
    /// `state`, optionally with a reported session.
    pub(super) fn app_with_claude_pane(
        state: crate::detect::AgentState,
        session: Option<crate::agent_resume::PersistedAgentSession>,
    ) -> (
        App,
        crate::layout::PaneId,
        crate::terminal::TerminalId,
        String,
    ) {
        let mut app = test_app();
        let workspace = crate::workspace::Workspace::test_new("resume");
        let pane_id = workspace.tabs[0].root_pane;
        let terminal_id = workspace.terminal_id(pane_id).cloned().unwrap();
        app.state.workspaces = vec![workspace];
        app.state.active = Some(0);
        app.state.ensure_test_terminals();
        let terminal = app.state.terminals.get_mut(&terminal_id).unwrap();
        terminal.cwd = std::env::temp_dir();
        terminal.set_detected_state(Some(crate::detect::Agent::Claude), state);
        terminal.persisted_agent_session = session;
        let public_id = app.public_pane_id(0, pane_id).unwrap();
        (app, pane_id, terminal_id, public_id)
    }

    #[cfg(unix)]
    fn resume_pane(app: &mut App, pane_id: &str) -> serde_json::Value {
        let response = app.handle_api_request(crate::api::schema::Request {
            id: "resume".into(),
            method: crate::api::schema::Method::AgentResume(
                crate::api::schema::AgentResumeParams {
                    pane_id: pane_id.into(),
                    input_quiet_ms: Some(20_000),
                },
            ),
        });
        serde_json::from_str(&response).unwrap()
    }

    #[cfg(unix)]
    #[test]
    fn agent_resume_old_and_new_runtime_exits_do_not_remove_pane() {
        let (mut app, pane_id, terminal_id, public_id) = app_with_claude_pane(
            crate::detect::AgentState::Idle,
            Some(claude_session("sess-1")),
        );
        app.pending_agent_resume_runtime_exits
            .insert(pane_id, Some(123));
        app.retained_agent_resume_panes.insert(
            pane_id,
            AgentResumeReplacementWindow {
                runtime_pid: Some(456),
                until: Instant::now() + std::time::Duration::from_secs(60),
            },
        );
        // New death can arrive before the old runtime's delayed event.
        app.handle_internal_event(crate::events::AppEvent::PaneDied {
            pane_id,
            runtime_pid: Some(456),
            exit_reason: crate::platform::ChildExitReason::Exited,
        });
        assert_eq!(
            app.pending_agent_resume_runtime_exits.get(&pane_id),
            Some(&Some(123))
        );
        assert!(app.state.terminals.contains_key(&terminal_id));
        assert!(app.state.terminals[&terminal_id].restore_error.is_some());
        // The window closes on that failure; it does not linger.
        assert!(app.retained_agent_resume_panes.is_empty());
        app.handle_internal_event(crate::events::AppEvent::PaneDied {
            pane_id,
            runtime_pid: Some(123),
            exit_reason: crate::platform::ChildExitReason::Exited,
        });
        assert!(app.pending_agent_resume_runtime_exits.is_empty());
        assert_eq!(
            app.public_pane_id(0, pane_id).as_deref(),
            Some(public_id.as_str())
        );
    }

    #[cfg(unix)]
    #[test]
    fn agent_resume_window_closes_on_readiness_and_expiry() {
        let (mut app, pane_id, _, _) = app_with_claude_pane(
            crate::detect::AgentState::Idle,
            Some(claude_session("sess-1")),
        );
        let now = Instant::now();
        let window = AgentResumeReplacementWindow {
            runtime_pid: Some(456),
            until: now + std::time::Duration::from_secs(60),
        };
        app.retained_agent_resume_panes.insert(pane_id, window);
        // Observations from the replaced runtime, or untagged ones, do not
        // count as the replacement's readiness.
        for stale in [
            crate::events::AppEvent::AgentProcessDetected {
                pane_id,
                agent: crate::detect::Agent::Claude,
                observed_at: now,
                runtime_pid: Some(123),
            },
            crate::events::AppEvent::AgentProcessDetected {
                pane_id,
                agent: crate::detect::Agent::Claude,
                observed_at: now,
                runtime_pid: None,
            },
            crate::events::AppEvent::StateChanged {
                pane_id,
                agent: Some(crate::detect::Agent::Claude),
                state: crate::detect::AgentState::Idle,
                visible_blocker: false,
                visible_working: false,
                process_exited: false,
                observed_at: now,
                runtime_pid: None,
            },
        ] {
            app.handle_internal_event(stale);
            assert_eq!(app.retained_agent_resume_panes.get(&pane_id), Some(&window));
        }
        app.handle_internal_event(crate::events::AppEvent::AgentProcessDetected {
            pane_id,
            agent: crate::detect::Agent::Claude,
            observed_at: now,
            runtime_pid: Some(456),
        });
        assert!(app.retained_agent_resume_panes.is_empty());

        app.retained_agent_resume_panes.insert(pane_id, window);
        assert!(!app.claim_agent_resume_replacement_exit(pane_id, Some(789), now));
        assert!(!app.claim_agent_resume_replacement_exit(pane_id, Some(456), window.until));
        assert!(app.retained_agent_resume_panes.is_empty());
    }

    /// After a failed replacement spawn the pane has no runtime; queued
    /// detections from the runtime it replaced must not revive its state.
    #[cfg(unix)]
    #[test]
    fn superseded_detections_are_ignored_when_the_pane_has_no_runtime() {
        let (mut app, pane_id, terminal_id, _) = app_with_claude_pane(
            crate::detect::AgentState::Idle,
            Some(claude_session("sess-1")),
        );
        assert!(app.terminal_runtimes.get(&terminal_id).is_none());
        let working = |runtime_pid| crate::events::AppEvent::StateChanged {
            pane_id,
            agent: Some(crate::detect::Agent::Claude),
            state: crate::detect::AgentState::Working,
            visible_blocker: false,
            visible_working: true,
            process_exited: false,
            observed_at: Instant::now(),
            runtime_pid,
        };
        app.handle_internal_event(working(Some(123)));
        assert_eq!(
            app.state.terminals[&terminal_id].state,
            crate::detect::AgentState::Idle
        );
        // An untagged observation still applies.
        app.handle_internal_event(working(None));
        assert_eq!(
            app.state.terminals[&terminal_id].state,
            crate::detect::AgentState::Working
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn agent_resume_enforces_minimum_quiet_even_without_caller_value() {
        let (mut app, pane_id, terminal_id, _) = app_with_claude_pane(
            crate::detect::AgentState::Idle,
            Some(claude_session("sess-1")),
        );
        app.state
            .terminals
            .get_mut(&terminal_id)
            .unwrap()
            .pending_agent_resume_plan = Some(crate::agent_resume::AgentResumePlan {
            agent: "claude".into(),
            argv: long_running_test_argv(),
            dedupe_key: "quiet".into(),
        });
        assert!(app.start_pending_agent_resume_for_terminal(&terminal_id, 24, 80, true));
        let old_pid = app.terminal_runtimes.get(&terminal_id).unwrap().child_pid();
        assert!(matches!(
            app.resume_agent_in_place(0, pane_id, None),
            Err(InPlaceAgentResumeError::ArgvUnsupported(_))
        ));
        assert_eq!(
            app.terminal_runtimes.get(&terminal_id).unwrap().child_pid(),
            old_pid
        );
        app.terminal_runtimes
            .get(&terminal_id)
            .unwrap()
            .record_human_text();
        for quiet in [None, Some(std::time::Duration::ZERO)] {
            assert!(matches!(
                app.resume_agent_in_place(0, pane_id, quiet),
                Err(InPlaceAgentResumeError::Busy { .. })
            ));
        }
        for (_, runtime) in app.terminal_runtimes.drain() {
            runtime.shutdown();
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn agent_resume_refuses_a_pane_without_a_session_reference() {
        let (mut app, _, terminal_id, public_id) =
            app_with_claude_pane(crate::detect::AgentState::Idle, None);

        let response = resume_pane(&mut app, &public_id);

        assert_eq!(response["error"]["code"], "agent_session_unknown");
        assert!(app.pending_agent_resume_runtime_exits.is_empty());
        assert!(app.state.terminals[&terminal_id]
            .pending_agent_resume_plan
            .is_none());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn agent_resume_refuses_a_working_agent() {
        let (mut app, _, terminal_id, public_id) = app_with_claude_pane(
            crate::detect::AgentState::Working,
            Some(claude_session("busy-session")),
        );

        let response = resume_pane(&mut app, &public_id);

        assert_eq!(response["error"]["code"], "agent_busy");
        assert!(app.pending_agent_resume_runtime_exits.is_empty());
        assert!(app.state.terminals[&terminal_id]
            .pending_agent_resume_plan
            .is_none());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn agent_resume_restarts_an_idle_agent_under_the_pane_shell_with_its_flags() {
        exercise_agent_resume(ResumeScenario::Resumed).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn agent_resume_restart_force_restarts_working_agent() {
        exercise_agent_resume(ResumeScenario::ForcedRestart).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn agent_resume_restart_launches_long_argv_intact() {
        exercise_agent_resume(ResumeScenario::LongArgv).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn agent_resume_immediately_exited_replacement_keeps_pane() {
        exercise_agent_resume(ResumeScenario::AgentExits).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn agent_resume_direct_launch_is_independent_of_pane_shell() {
        exercise_agent_resume(ResumeScenario::UnquotableShell).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn agent_resume_stale_readiness_then_replacement_death_keeps_pane() {
        exercise_agent_resume(ResumeScenario::StaleReadinessThenDeath).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn agent_resume_then_worktree_remove_failure_restores_pane_shell() {
        exercise_agent_resume(ResumeScenario::WorktreeRemoveFails).await;
    }

    #[cfg(unix)]
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum ResumeScenario {
        Resumed,
        ForcedRestart,
        LongArgv,
        AgentExits,
        StaleReadinessThenDeath,
        WorktreeRemoveFails,
        UnquotableShell,
    }

    #[cfg(unix)]
    async fn wait_for_history(
        app: &App,
        terminal_id: &crate::terminal::TerminalId,
        needle: &str,
    ) -> String {
        let mut history = String::new();
        for _ in 0..120 {
            history = app
                .terminal_runtimes
                .get(terminal_id)
                .and_then(|runtime| runtime.snapshot_history())
                .unwrap_or_default();
            if history.contains(needle) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        history
    }

    #[cfg(unix)]
    async fn exercise_agent_resume(scenario: ResumeScenario) {
        // A process whose argv[0] basename is `claude` stands in for the agent.
        let fake_bin = std::env::temp_dir().join(format!(
            "herdr-agent-resume-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&fake_bin).unwrap();
        let sleep = ["/bin/sh", "/usr/bin/sh"]
            .into_iter()
            .find(|path| std::path::Path::new(path).exists())
            .expect("shell binary");
        let fake_claude = fake_bin.join("claude");
        std::os::unix::fs::symlink(sleep, &fake_claude).unwrap();
        let fake_claude = fake_claude.display().to_string();
        let script = fake_bin.join("stand-in.sh");
        std::fs::write(
            &script,
            if scenario == ResumeScenario::AgentExits {
                "if [ \"$1\" = --resume ]; then echo stand-in-exited; exit 1; fi; sleep 30\n"
            } else {
                "echo resumed-args: \"$@\"; echo agent-parent: $PPID; \
                 echo agent-provider: \"$HERDR_RESUME_TEST_PROVIDER\"; sleep 30\n"
            },
        )
        .unwrap();
        let script = script.display().to_string();
        // A provider/auth-style variable that only the interactive shell's rc
        // file sets. nextest runs each test in its own process, so pointing
        // the POSIX `ENV` rc at it does not leak into other tests.
        let shell_rc = fake_bin.join("shell-rc");
        std::fs::write(
            &shell_rc,
            "export HERDR_RESUME_TEST_PROVIDER=from-shell-rc\n",
        )
        .unwrap();
        std::env::set_var("ENV", &shell_rc);

        let (mut app, pane_id, terminal_id, public_id) = app_with_claude_pane(
            crate::detect::AgentState::Idle,
            Some(claude_session("sess-1")),
        );
        app.state.default_shell = sleep.into();
        app.state.shell_mode = crate::config::ShellModeConfig::NonLogin;
        app.state
            .terminals
            .get_mut(&terminal_id)
            .unwrap()
            .agent_name = Some("frank".into());
        // Launch the stand-in agent through the same deferred path, then make
        // the terminal look like the idle claude it hosts.
        let launch = crate::agent_resume::AgentResumePlan {
            agent: "claude".into(),
            argv: vec![fake_claude.clone(), script.clone()],
            dedupe_key: "launch".into(),
        };
        app.state
            .terminals
            .get_mut(&terminal_id)
            .unwrap()
            .pending_agent_resume_plan = Some(launch);
        if scenario == ResumeScenario::ForcedRestart {
            std::env::set_var("HERDR_RESUME_TEST_PROVIDER", "from-shell-rc");
        }
        assert!(app.start_pending_agent_resume_for_terminal(&terminal_id, 24, 80, true));
        if scenario == ResumeScenario::ForcedRestart {
            std::env::remove_var("HERDR_RESUME_TEST_PROVIDER");
        }
        let terminal = app.state.terminals.get_mut(&terminal_id).unwrap();
        terminal.set_detected_state(
            Some(crate::detect::Agent::Claude),
            crate::detect::AgentState::Idle,
        );
        let old_pid = app.terminal_runtimes.get(&terminal_id).unwrap().child_pid();
        let mut running = false;
        for _ in 0..200 {
            running = app
                .terminal_runtimes
                .get(&terminal_id)
                .unwrap()
                .child_pid()
                .and_then(crate::detect::foreground_job)
                .is_some_and(|job| {
                    job.processes.iter().any(|process| {
                        process
                            .argv
                            .as_ref()
                            .and_then(|argv| argv.first())
                            .is_some_and(|argv0| argv0 == &fake_claude)
                    })
                });
            if running {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        assert!(
            running,
            "stand-in agent never became the foreground job: {}",
            app.terminal_runtimes
                .get(&terminal_id)
                .and_then(|runtime| runtime.snapshot_history())
                .unwrap_or_default()
        );
        // Process acquisition can supersede screen detection with Unknown.
        // Report idle through the same hook authority a real agent uses.
        app.state
            .terminals
            .get_mut(&terminal_id)
            .unwrap()
            .set_hook_authority_with_session_ref(
                "herdr:claude".into(),
                "claude".into(),
                crate::detect::AgentState::Idle,
                None,
                crate::agent_resume::AgentSessionRef::id("sess-1"),
                Some(1),
            );

        if scenario == ResumeScenario::UnquotableShell {
            app.state.default_shell = "fish".into();
        }
        // Explicitly recorded shell launcher; no process environment is captured.
        let terminal = app.state.terminals.get_mut(&terminal_id).unwrap();
        terminal.launch_argv = Some(vec![sleep.into(), script.clone()]);
        terminal.launch_env_overrides =
            vec![("HERDR_RESUME_TEST_PROVIDER".into(), "from-shell-rc".into())];
        if scenario == ResumeScenario::ForcedRestart {
            let delayed = fake_bin.join("claude-lb-launch");
            std::fs::write(
                &delayed,
                format!("sleep 2; exec '{}' '{}' \"$@\"\n", fake_claude, script),
            )
            .unwrap();
            app.state
                .terminals
                .get_mut(&terminal_id)
                .unwrap()
                .launch_argv = Some(vec![sleep.into(), delayed.display().to_string()]);
        }
        let long_value = "x".repeat(4096);
        if scenario == ResumeScenario::LongArgv {
            app.state
                .terminals
                .get_mut(&terminal_id)
                .unwrap()
                .launch_argv
                .as_mut()
                .unwrap()
                .extend(["--settings".into(), long_value.clone()]);
        }
        let expected_argv = if scenario == ResumeScenario::LongArgv {
            serde_json::json!([
                sleep,
                script,
                "--settings",
                long_value,
                "--resume",
                "sess-1"
            ])
        } else {
            let launch_script = if scenario == ResumeScenario::ForcedRestart {
                fake_bin.join("claude-lb-launch").display().to_string()
            } else {
                script.clone()
            };
            serde_json::json!([sleep, launch_script, "--resume", "sess-1"])
        };
        let response = if scenario == ResumeScenario::ForcedRestart {
            app.state
                .terminals
                .get_mut(&terminal_id)
                .unwrap()
                .set_hook_authority_with_session_ref(
                    "herdr:claude".into(),
                    "claude".into(),
                    crate::detect::AgentState::Working,
                    None,
                    crate::agent_resume::AgentSessionRef::id("sess-1"),
                    Some(2),
                );
            let request_started = Instant::now();
            let response: serde_json::Value =
                serde_json::from_str(&app.handle_api_request(crate::api::schema::Request {
                    id: "force-restart".into(),
                    method: crate::api::schema::Method::AgentRestart(
                        crate::api::schema::AgentRestartParams {
                            pane_id: public_id.clone(),
                            force: true,
                        },
                    ),
                }))
                .unwrap();
            assert!(
                request_started.elapsed() < std::time::Duration::from_secs(1),
                "restart handler must not wait for startup: {:?}",
                request_started.elapsed()
            );
            assert_eq!(response["result"]["ok"], true, "{response}");
            assert_eq!(response["result"]["type"], "agent_restarted");
            // The shared assertions below describe the internal resume contract.
            serde_json::json!({"result": {"type": "agent_resumed", "pane_id": public_id, "agent": "claude", "session_id": "sess-1", "argv": expected_argv}})
        } else {
            resume_pane(&mut app, &public_id)
        };
        assert!(
            app.terminal_runtimes.get(&terminal_id).is_none(),
            "replacement must not spawn before shutdown completion"
        );
        let second: serde_json::Value = serde_json::from_str(
            &app.handle_api_request_after_internal_events_drained(crate::api::schema::Request {
                id: "second-restart".into(),
                method: crate::api::schema::Method::AgentRestart(
                    crate::api::schema::AgentRestartParams {
                        pane_id: public_id.clone(),
                        force: true,
                    },
                ),
            }),
        )
        .unwrap();
        assert_eq!(second["error"]["code"], "busy", "{second}");
        assert_eq!(second["error"]["reason"], "restart_pending", "{second}");
        for _ in 0..200 {
            let mut completed = false;
            while let Ok(event) = app.event_rx.try_recv() {
                if matches!(
                    &event,
                    crate::events::AppEvent::AgentRestartShutdownFinished(_)
                ) {
                    assert!(
                        !crate::platform::process_exists(old_pid.unwrap()),
                        "old runtime must exit before replacement spawn"
                    );
                    app.handle_internal_event(event);
                    completed = true;
                    break;
                }
                app.handle_internal_event(event);
            }
            if completed {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        for _ in 0..200 {
            if !crate::platform::process_exists(old_pid.unwrap()) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        assert!(
            !crate::platform::process_exists(old_pid.unwrap()),
            "old shell must be gone: pid={old_pid:?}, argv={:?}",
            old_pid.and_then(crate::platform::process_launch_argv)
        );
        assert_eq!(response["result"]["type"], "agent_resumed", "{response}");
        assert_eq!(response["result"]["pane_id"], public_id.as_str());
        assert_eq!(response["result"]["agent"], "claude");
        assert_eq!(response["result"]["session_id"], "sess-1");
        assert_eq!(response["result"]["argv"], expected_argv);
        assert_eq!(
            app.public_pane_id(0, pane_id).as_deref(),
            Some(public_id.as_str())
        );
        let new_pid = app
            .terminal_runtimes
            .get(&terminal_id)
            .expect("resumed pane keeps a runtime on the same terminal")
            .child_pid();
        assert!(new_pid.is_some());
        assert_ne!(new_pid, old_pid, "the pane shell was replaced");
        assert_eq!(
            app.retained_agent_resume_panes
                .get(&pane_id)
                .map(|window| window.runtime_pid),
            Some(new_pid)
        );
        let terminal = &app.state.terminals[&terminal_id];
        assert!(terminal.pending_agent_resume_plan.is_none());
        assert_eq!(terminal.agent_name.as_deref(), Some("frank"));
        assert_eq!(
            terminal.persisted_agent_session.as_ref(),
            Some(&claude_session("sess-1"))
        );
        // Even if shutdown-complete raced ahead of PaneDied, consume the
        // original exit exactly once before any subsequent restart.
        if app
            .pending_agent_resume_runtime_exits
            .contains_key(&pane_id)
        {
            for _ in 0..200 {
                if let Ok(event) = app.event_rx.try_recv() {
                    app.handle_internal_event(event);
                }
                if !app
                    .pending_agent_resume_runtime_exits
                    .contains_key(&pane_id)
                {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(25)).await;
            }
        }

        match scenario {
            ResumeScenario::Resumed
            | ResumeScenario::ForcedRestart
            | ResumeScenario::UnquotableShell
            | ResumeScenario::LongArgv => {
                let expected_text = if scenario == ResumeScenario::LongArgv {
                    format!("resumed-args: --settings {long_value} --resume sess-1")
                } else {
                    "resumed-args: --resume sess-1".into()
                };
                let typed = expected_text.as_str();
                let history = wait_for_history(&app, &terminal_id, typed).await;
                assert!(
                    history.contains(typed),
                    "resumed shell should receive the preserved argv: {history}"
                );
                let provider = "agent-provider: from-shell-rc";
                let history = wait_for_history(&app, &terminal_id, provider).await;
                assert!(
                    history.contains(provider),
                    "resumed agent must inherit the shell rc environment: {history}"
                );
            }
            ResumeScenario::AgentExits | ResumeScenario::StaleReadinessThenDeath => {
                if scenario == ResumeScenario::AgentExits {
                    let history = wait_for_history(&app, &terminal_id, "stand-in-exited").await;
                    assert!(history.contains("stand-in-exited"), "{history}");
                    for _ in 0..200 {
                        while let Ok(event) = app.event_rx.try_recv() {
                            app.handle_internal_event(event);
                        }
                        if app.terminal_runtimes.get(&terminal_id).unwrap().child_pid() != new_pid {
                            break;
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
                    }
                    let shell_pid = app.terminal_runtimes.get(&terminal_id).unwrap().child_pid();
                    assert_ne!(shell_pid, new_pid, "agent exit must respawn the shell");
                    assert!(shell_pid.is_some_and(crate::platform::process_exists));
                    assert!(app.find_pane(pane_id).is_some());
                    for (_, runtime) in app.terminal_runtimes.drain() {
                        runtime.shutdown();
                    }
                    std::env::remove_var("ENV");
                    let _ = std::fs::remove_dir_all(fake_bin);
                    return;
                } else {
                    // The replaced runtime's detector reports late, after the
                    // replacement started; it must not end the window or
                    // overwrite the replacement's agent and state.
                    app.handle_internal_event(crate::events::AppEvent::AgentProcessDetected {
                        pane_id,
                        agent: crate::detect::Agent::Claude,
                        observed_at: Instant::now(),
                        runtime_pid: old_pid,
                    });
                    app.handle_internal_event(crate::events::AppEvent::StateChanged {
                        pane_id,
                        agent: Some(crate::detect::Agent::Claude),
                        state: crate::detect::AgentState::Idle,
                        visible_blocker: false,
                        visible_working: false,
                        process_exited: false,
                        observed_at: Instant::now(),
                        runtime_pid: old_pid,
                    });
                    assert!(app.retained_agent_resume_panes.contains_key(&pane_id));
                    let terminal = &app.state.terminals[&terminal_id];
                    assert_eq!(terminal.state, crate::detect::AgentState::Working);
                    assert_eq!(terminal.effective_agent_label(), Some("claude"));
                }
                // The shell itself dying inside the window keeps the pane.
                app.handle_internal_event(crate::events::AppEvent::PaneDied {
                    pane_id,
                    runtime_pid: new_pid,
                    exit_reason: crate::platform::ChildExitReason::Exited,
                });
                assert_eq!(
                    app.public_pane_id(0, pane_id).as_deref(),
                    Some(public_id.as_str())
                );
                assert!(app.state.terminals[&terminal_id].restore_error.is_some());
                assert!(app.retained_agent_resume_panes.is_empty());
                // The failure is visible through the API the CLI polls. The
                // test already delivered the old runtime's exit by hand, so do
                // not drain the real copy of it still queued in the channel.
                let pane: serde_json::Value =
                    serde_json::from_str(&app.handle_api_request_after_internal_events_drained(
                        crate::api::schema::Request {
                            id: "pane".into(),
                            method: crate::api::schema::Method::PaneGet(
                                crate::api::schema::PaneTarget {
                                    pane_id: public_id.clone(),
                                },
                            ),
                        },
                    ))
                    .unwrap();
                assert!(
                    pane["result"]["pane"]["restore_error"].is_string(),
                    "{pane}"
                );
            }
            ResumeScenario::WorktreeRemoveFails => {
                let checkout = fake_bin.clone();
                let workspace_id = app.state.workspaces[0].id.clone();
                let shutdown_panes =
                    app.shutdown_workspace_terminal_runtimes_for_worktree_remove(0);
                assert_eq!(shutdown_panes, vec![pane_id]);
                assert!(app.terminal_runtimes.get(&terminal_id).is_none());
                let checkout_key = crate::worktree::canonical_or_original(&checkout);
                app.pending_api_worktree_removes
                    .insert(workspace_id.clone(), 7);
                app.pending_api_worktree_remove_paths
                    .insert(checkout_key.clone(), 7);
                let (respond_to, response_rx) = std::sync::mpsc::channel();
                let _ =
                    app.handle_api_worktree_remove_finished(crate::events::WorktreeRemoveResult {
                        workspace_id,
                        path: checkout,
                        workspace: None,
                        worktree: None,
                        forced: false,
                        api_request: Some(crate::events::ApiWorktreeRemoveRequest {
                            id: "req".into(),
                            operation_id: 7,
                            checkout_key,
                            shutdown_panes,
                            respond_to,
                        }),
                        result: Err("simulated remove failure".into()),
                    });
                let response: serde_json::Value = serde_json::from_str(
                    &response_rx
                        .recv_timeout(std::time::Duration::from_secs(2))
                        .unwrap(),
                )
                .unwrap();
                assert_eq!(response["error"]["code"], "worktree_remove_failed");
                // The shut-down replacement shell's own exit drives recovery.
                app.handle_internal_event(crate::events::AppEvent::PaneDied {
                    pane_id,
                    runtime_pid: new_pid,
                    exit_reason: crate::platform::ChildExitReason::Exited,
                });
                let restored = app
                    .terminal_runtimes
                    .get(&terminal_id)
                    .expect("worktree recovery restores the pane shell");
                assert_ne!(restored.child_pid(), new_pid);
                assert!(app.find_pane(pane_id).is_some());
                assert!(app.pending_worktree_remove_runtime_exits.is_empty());
                assert!(app.pending_worktree_remove_runtime_restores.is_empty());
                assert!(app.retained_agent_resume_panes.is_empty());
            }
        }

        for (_, runtime) in app.terminal_runtimes.drain() {
            runtime.shutdown();
        }
        std::env::remove_var("ENV");
        let _ = std::fs::remove_dir_all(fake_bin);
    }
}

#[cfg(all(test, unix))]
mod restart_api_tests {
    use super::tests::{app_with_claude_pane, claude_session};
    #[tokio::test]
    async fn agent_resume_restart_api_checks_session_and_force() {
        for (state, session, force, expected, reason) in [
            (
                crate::detect::AgentState::Working,
                Some(claude_session("session")),
                false,
                "busy",
                Some("working"),
            ),
            (
                crate::detect::AgentState::Blocked,
                Some(claude_session("session")),
                false,
                "busy",
                Some("blocked"),
            ),
            (
                crate::detect::AgentState::Working,
                Some(claude_session("session")),
                true,
                "not_resumable",
                None,
            ),
            (
                crate::detect::AgentState::Idle,
                None,
                false,
                "no_session",
                None,
            ),
        ] {
            let (mut app, _, _, public_id) = app_with_claude_pane(state, session);
            let response = app.handle_api_request(crate::api::schema::Request {
                id: "restart-test".into(),
                method: crate::api::schema::Method::AgentRestart(
                    crate::api::schema::AgentRestartParams {
                        pane_id: public_id,
                        force,
                    },
                ),
            });
            let value: serde_json::Value = serde_json::from_str(&response).unwrap();
            // Force passes the busy gate and reaches the real missing-runtime boundary.
            assert_eq!(value["error"]["code"], expected);
            assert_eq!(value["error"]["reason"].as_str(), reason);
        }
    }
}
