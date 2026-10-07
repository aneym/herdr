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
    Busy(String),
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
            return Err(InPlaceAgentResumeError::SessionUnknown(format!(
                "agent {} has no resume command",
                session.agent
            )));
        };
        if !matches!(
            pane.agent_status,
            crate::api::schema::AgentStatus::Idle | crate::api::schema::AgentStatus::Done
        ) {
            return Err(InPlaceAgentResumeError::Busy(format!(
                "agent in pane {} is {:?}",
                pane.pane_id, pane.agent_status
            )));
        }

        let runtime = self
            .terminal_runtimes
            .get(&terminal_id)
            .ok_or(InPlaceAgentResumeError::NotRunning)?;
        let quiet = input_quiet
            .unwrap_or_default()
            .max(std::time::Duration::from_secs(20));
        {
            if !runtime.human_input_quiet_for(quiet) {
                return Err(InPlaceAgentResumeError::Busy(format!(
                    "pane {} had user input within the last {} ms",
                    pane.pane_id,
                    quiet.as_millis()
                )));
            }
        }
        let (rows, cols) = runtime.current_size();
        let foreground_argv = runtime
            .child_pid()
            .and_then(crate::detect::foreground_job)
            .and_then(|job| {
                job.processes
                    .into_iter()
                    .filter_map(|process| process.argv)
                    .find(|argv| {
                        argv.first().is_some_and(|program| {
                            crate::agent_resume::same_executable(program, &plan.argv[0])
                                || crate::agent_resume::same_executable(program, "node")
                        })
                    })
            })
            .unwrap_or_default();
        let argv = crate::agent_resume::resume_argv_preserving_flags(&foreground_argv, &plan)
            .map_err(InPlaceAgentResumeError::ArgvUnsupported)?;
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
        };
        let persisted = crate::agent_resume::PersistedAgentSession {
            source: session.source,
            agent: session.agent,
            session_ref,
        };
        let plan = crate::agent_resume::AgentResumePlan { argv, ..plan };

        let shell = crate::pane::pane_shell(&self.state.default_shell);
        let resume_command = resume_shell_command(&resumed.argv, &shell)
            .map_err(InPlaceAgentResumeError::ArgvUnsupported)?;
        let launch_env = self
            .pane_launch_env(ws_idx, pane_id, Vec::new())
            .ok_or_else(|| {
                InPlaceAgentResumeError::Failed("pane launch environment unavailable".into())
            })?;

        tracing::info!(
            pane = pane_id.raw(),
            terminal = %terminal_id,
            agent = %resumed.agent,
            "restarting idle agent in place to resume its session"
        );
        self.pending_agent_resume_runtime_exits
            .insert(pane_id, runtime.child_pid());
        self.shutdown_terminal_runtime(terminal_id.clone());
        if let Some(terminal) = self.state.terminals.get_mut(&terminal_id) {
            terminal.cwd = cwd.clone();
            terminal.begin_in_place_agent_resume(persisted, plan);
        }
        // Launch the way deferred resume does: a fresh pane shell (so the
        // login-shell environment the agent's provider and auth come from is
        // present) with the resume command typed into it.
        let runtime = crate::terminal::TerminalRuntime::spawn(
            pane_id,
            rows,
            cols,
            cwd,
            self.state.pane_scrollback_limit_bytes,
            self.state.host_terminal_theme,
            self.state.host_terminal_appearance,
            crate::pane::PaneShellConfig::new(&self.state.default_shell, self.state.shell_mode),
            &launch_env,
            self.event_tx.clone(),
            self.render_notify.clone(),
            self.render_dirty.clone(),
        )
        .map_err(|err| {
            if let Some(terminal) = self.state.terminals.get_mut(&terminal_id) {
                terminal.pending_agent_resume_plan = None;
                terminal.restore_error = Some(format!(
                    "Could not start the pane shell to resume the agent: {err}"
                ));
                terminal.revision = terminal.revision.saturating_add(1);
            }
            InPlaceAgentResumeError::Failed(err.to_string())
        })?;
        let mut input = resume_command;
        input.push('\r');
        let typed = runtime.try_send_bytes(Bytes::from(input));
        self.retained_agent_resume_panes.insert(
            pane_id,
            AgentResumeReplacementWindow {
                runtime_pid: runtime.child_pid(),
                until: Instant::now() + AGENT_RESUME_REPLACEMENT_WINDOW,
            },
        );
        // The shell stays in the pane even if typing failed, so the pane is
        // never left without a runtime.
        self.terminal_runtimes.insert(terminal_id.clone(), runtime);
        if let Some(terminal) = self.state.terminals.get_mut(&terminal_id) {
            terminal.pending_agent_resume_plan = None;
            terminal.respawn_shell_on_exit = false;
            if let Err(err) = &typed {
                terminal.restore_error = Some(format!("Could not type the resume command: {err}"));
            }
        }
        self.state.mark_session_dirty();
        self.emit_pane_updated(ws_idx, pane_id);
        if let Err(err) = typed {
            self.retained_agent_resume_panes.remove(&pane_id);
            return Err(InPlaceAgentResumeError::Failed(format!(
                "could not type the resume command: {err}"
            )));
        }
        Ok(resumed)
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
    crate::platform::interactive_shell_command(argv, shell)
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
            Ok("'/opt/my tools/claude' --settings '{\"viewMode\":\"focus\"}' --resume sess-1")
        );
        assert_eq!(
            resume_shell_command(&argv, "pwsh").as_deref(),
            Ok("& '/opt/my tools/claude' '--settings' '{\"viewMode\":\"focus\"}' '--resume' sess-1")
        );
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
        let cmd = resume_shell_command(&argv, r"C:\Windows\System32\cmd.exe").unwrap();
        assert!(cmd.starts_with("powershell.exe -NoLogo -NoProfile -EncodedCommand "));
    }

    #[cfg(unix)]
    fn claude_session(id: &str) -> crate::agent_resume::PersistedAgentSession {
        crate::agent_resume::PersistedAgentSession {
            source: "herdr:claude".into(),
            agent: "claude".into(),
            session_ref: crate::agent_resume::AgentSessionRef::id(id).unwrap(),
        }
    }

    #[cfg(unix)]
    /// One workspace, one pane whose terminal looks like a detected claude in
    /// `state`, optionally with a reported session.
    fn app_with_claude_pane(
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
                Err(InPlaceAgentResumeError::Busy(_))
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
    async fn agent_resume_immediately_exited_replacement_keeps_pane() {
        exercise_agent_resume(ResumeScenario::AgentExits).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn agent_resume_refuses_unquotable_shell_before_stopping_the_agent() {
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
        assert!(app.start_pending_agent_resume_for_terminal(&terminal_id, 24, 80, true));
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
        assert!(running, "stand-in agent never became the foreground job");
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
            // Refused before the old runtime is touched.
            app.state.default_shell = "fish".into();
            let response = resume_pane(&mut app, &public_id);
            assert_eq!(
                response["error"]["code"], "agent_argv_unsupported",
                "{response}"
            );
            assert!(crate::platform::process_exists(old_pid.unwrap()));
            assert_eq!(
                app.terminal_runtimes.get(&terminal_id).unwrap().child_pid(),
                old_pid
            );
            assert!(app.pending_agent_resume_runtime_exits.is_empty());
            assert!(app.retained_agent_resume_panes.is_empty());
            for (_, runtime) in app.terminal_runtimes.drain() {
                runtime.shutdown();
            }
            std::env::remove_var("ENV");
            let _ = std::fs::remove_dir_all(fake_bin);
            return;
        }

        let response = resume_pane(&mut app, &public_id);
        assert!(
            !crate::platform::process_exists(old_pid.unwrap()),
            "old shell must be gone"
        );
        assert_eq!(response["result"]["type"], "agent_resumed", "{response}");
        assert_eq!(response["result"]["pane_id"], public_id.as_str());
        assert_eq!(response["result"]["agent"], "claude");
        assert_eq!(response["result"]["session_id"], "sess-1");
        assert_eq!(
            response["result"]["argv"],
            serde_json::json!([fake_claude, script, "--resume", "sess-1"])
        );
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
        assert_eq!(
            app.pending_agent_resume_runtime_exits.get(&pane_id),
            Some(&old_pid)
        );

        // The replaced process's exit is owed, not a pane death.
        app.handle_internal_event(crate::events::AppEvent::PaneDied {
            pane_id,
            runtime_pid: old_pid,
            exit_reason: crate::platform::ChildExitReason::Exited,
        });
        assert!(app.find_pane(pane_id).is_some());
        assert!(app.terminal_runtimes.get(&terminal_id).is_some());
        assert!(app.pending_agent_resume_runtime_exits.is_empty());

        match scenario {
            ResumeScenario::Resumed => {
                let typed = "resumed-args: --resume sess-1";
                let history = wait_for_history(&app, &terminal_id, typed).await;
                assert!(
                    history.contains(typed),
                    "resumed shell should receive the preserved argv: {history}"
                );
                // The agent is a child of the pane shell, so it inherits the
                // shell's environment instead of a rebuilt one.
                let parent = format!("agent-parent: {}", new_pid.unwrap());
                let history = wait_for_history(&app, &terminal_id, &parent).await;
                assert!(
                    history.contains(&parent),
                    "agent must run under the pane shell {new_pid:?}: {history}"
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
                    // The agent exiting leaves the pane shell running.
                    assert!(crate::platform::process_exists(new_pid.unwrap()));
                } else {
                    // The replaced runtime's detector reports late, after the
                    // replacement started; it must not end the window.
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
                    });
                    assert!(app.retained_agent_resume_panes.contains_key(&pane_id));
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
            ResumeScenario::UnquotableShell => unreachable!("refused before resume"),
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
