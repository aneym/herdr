//! One answer to "is this chat working", owned by the server so every surface
//! (both sidebars, pinned rows, pane caps, the native chat view) draws the same
//! fact instead of each combining agent and overlay state on its own.
//!
//! The rule (Alex, 2026-10-06: "the status on the sidebar doesnt match the
//! workflow status and other status on the actual chats"):
//! - blocked when any agent pane in the chat is blocked;
//! - working when any agent pane is working: Claude's own turn is running, or
//!   its screen shows it waiting on a background agent, teammate or workflow
//!   it launched (screen detection reads the title spinner and the wait line);
//! - working when a factory run the chat owns is live by a fresh overlay: a
//!   workflow or fold run not done, a workflow tab it parents that is not
//!   done, or the tab itself being a workflow that is not done;
//! - otherwise the panes' own done, idle or unknown.
//!
//! The overlay's `busy` flag and its `agent:` runs never promote a chat. They
//! are the writer's copy of the chat's own Claude subagents and teammates,
//! which the chat's screen reports first hand; the copy reads a teammate that
//! ended on a tool result as running for up to 30 minutes (2026-10-06,
//! "submissions" idle in its pane, busy in the overlay).
//! Lanes grouped under a chat are separate chats with their own state; they
//! do not make their parent working.

use std::collections::HashSet;
use std::time::{Duration, SystemTime};

use crate::api::schema::AgentStatus;
use crate::factory_overlay::{FactoryOverlay, TabKind};

use super::App;

/// How long the writer's mtime heartbeat stays a fresh signal. The agent-rails
/// writer refreshes it at least every 60 seconds, even when content is unchanged;
/// a dead writer must not hold a chat working forever.
pub(crate) const FACTORY_OVERLAY_FRESH: Duration = Duration::from_secs(120);

/// Overlay run ids for the chat's own Claude subagents and teammates.
pub(crate) const AGENT_RUN_PREFIX: &str = "agent:";

/// A blocked pane outranks a working one, a working one outranks a finished one.
pub(crate) fn rank(status: AgentStatus) -> u8 {
    match status {
        AgentStatus::Blocked => 4,
        AgentStatus::Working => 3,
        AgentStatus::Done => 2,
        AgentStatus::Idle => 1,
        AgentStatus::Unknown => 0,
    }
}

/// The chat's status from its panes' statuses and whether it owns live factory work.
pub(crate) fn work_status(
    panes: impl IntoIterator<Item = AgentStatus>,
    live_factory_work: bool,
) -> AgentStatus {
    let status = panes
        .into_iter()
        .max_by_key(|status| rank(*status))
        .unwrap_or(AgentStatus::Unknown);
    if live_factory_work && rank(status) < rank(AgentStatus::Working) {
        AgentStatus::Working
    } else {
        status
    }
}

/// Tab ids whose factory work is live in `overlay`, or none when the overlay
/// was last written longer ago than [`FACTORY_OVERLAY_FRESH`].
pub(crate) fn live_factory_tabs(
    overlay: Option<&FactoryOverlay>,
    written: Option<SystemTime>,
    now: SystemTime,
) -> HashSet<String> {
    let fresh = written.is_some_and(|written| {
        now.duration_since(written)
            .map_or(true, |age| age <= FACTORY_OVERLAY_FRESH)
    });
    let Some(overlay) = overlay.filter(|_| fresh) else {
        return HashSet::new();
    };
    let mut live = HashSet::new();
    for (tab_id, tag) in &overlay.tabs {
        let live_workflow = tag.kind == TabKind::Workflow && !tag.done;
        let live_run = tag
            .runs
            .iter()
            .any(|run| !run.done && !run.id.starts_with(AGENT_RUN_PREFIX));
        if live_run || live_workflow {
            live.insert(tab_id.clone());
        }
        if live_workflow {
            if let Some(parent) = &tag.parent {
                live.insert(parent.clone());
            }
        }
    }
    live
}

impl App {
    /// Replace the set of tabs with live factory work. Each workspace holding a
    /// tab that entered or left the set gets a `workspace.updated` event so API
    /// clients refetch, and client shells get a fresh snapshot.
    pub(crate) fn set_live_factory_tabs(&mut self, live: HashSet<String>) {
        if live == self.live_factory_tabs {
            return;
        }
        let mut workspaces = live
            .symmetric_difference(&self.live_factory_tabs)
            .filter_map(|tab_id| self.parse_tab_id(tab_id).map(|(ws_idx, _)| ws_idx))
            .collect::<Vec<_>>();
        workspaces.sort_unstable();
        workspaces.dedup();
        self.live_factory_tabs = live;
        for ws_idx in workspaces {
            let workspace = self.workspace_info(ws_idx);
            self.emit_event(crate::api::schema::EventEnvelope {
                event: crate::api::schema::EventKind::WorkspaceUpdated,
                data: crate::api::schema::EventData::WorkspaceUpdated { workspace },
            });
        }
        self.render_dirty.request_generic();
    }

    /// Whether the chat in this tab owns live factory work. Allocates the
    /// public tab id only while some tab has live work.
    fn owns_live_factory_work(&self, ws_idx: usize, tab_idx: usize) -> bool {
        !self.live_factory_tabs.is_empty()
            && self
                .public_tab_id(ws_idx, tab_idx)
                .is_some_and(|tab_id| self.live_factory_tabs.contains(&tab_id))
    }

    /// The chat status of one tab: its agent panes' statuses under [`work_status`].
    pub(crate) fn tab_work_status(&self, ws_idx: usize, tab_idx: usize) -> AgentStatus {
        self.tab_panes_work_status(
            ws_idx,
            tab_idx,
            self.owns_live_factory_work(ws_idx, tab_idx),
        )
    }

    /// [`Self::tab_work_status`] for a caller that already holds the public tab id.
    pub(crate) fn tab_work_status_for(
        &self,
        ws_idx: usize,
        tab_idx: usize,
        tab_id: &str,
    ) -> AgentStatus {
        self.tab_panes_work_status(ws_idx, tab_idx, self.live_factory_tabs.contains(tab_id))
    }

    fn tab_panes_work_status(
        &self,
        ws_idx: usize,
        tab_idx: usize,
        live_factory_work: bool,
    ) -> AgentStatus {
        let Some(tab) = self
            .state
            .workspaces
            .get(ws_idx)
            .and_then(|ws| ws.tabs.get(tab_idx))
        else {
            return AgentStatus::Unknown;
        };
        let panes = tab.panes.values().filter_map(|pane| {
            let terminal = self.state.terminals.get(&pane.attached_terminal_id)?;
            terminal
                .is_agent_terminal()
                .then(|| super::api_helpers::pane_agent_status(terminal.state, pane.seen))
        });
        work_status(panes, live_factory_work)
    }

    /// One agent pane's chat status: its own status, working while its tab
    /// owns live factory work.
    pub(crate) fn pane_work_status(
        &self,
        ws_idx: usize,
        tab_idx: usize,
        status: AgentStatus,
    ) -> AgentStatus {
        work_status([status], self.owns_live_factory_work(ws_idx, tab_idx))
    }

    /// The space rollup: the highest chat status among its tabs.
    pub(crate) fn workspace_work_status(&self, ws_idx: usize) -> AgentStatus {
        let tabs = self
            .state
            .workspaces
            .get(ws_idx)
            .map_or(0, |ws| ws.tabs.len());
        (0..tabs)
            .map(|tab_idx| self.tab_work_status(ws_idx, tab_idx))
            .max_by_key(|status| rank(*status))
            .unwrap_or(AgentStatus::Unknown)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::factory_overlay::{RunTag, TabTag};

    fn run(id: &str, done: bool) -> RunTag {
        RunTag {
            id: id.into(),
            done,
            ..RunTag::default()
        }
    }

    /// Pure table over the overlay shapes the writer emits; the render and API
    /// boundaries own the end-to-end cases.
    #[test]
    fn only_fresh_workflow_and_fold_runs_and_live_child_workflows_count_as_live_work() {
        let now = SystemTime::now();
        let lane = |runs: Vec<RunTag>, busy: bool| TabTag {
            kind: TabKind::Lane,
            runs,
            busy,
            ..TabTag::default()
        };
        let doc = FactoryOverlay {
            version: 1,
            tabs: [
                // 2026-10-06 w5P:t7 "submissions": a teammate that ended on its
                // final SendMessage, still listed as running and busy.
                (
                    "w:submissions",
                    lane(vec![run("agent:agent-atrial-chase-lists", false)], true),
                ),
                ("w:workflow", lane(vec![run("wf_a6b8e106", false)], true)),
                ("w:fold", lane(vec![run("fold:42", false)], false)),
                ("w:finished", lane(vec![run("wf_317ad08a", true)], false)),
                ("w:parent", lane(Vec::new(), false)),
                (
                    "w:wf-tab",
                    TabTag {
                        kind: TabKind::Workflow,
                        parent: Some("w:parent".into()),
                        ..TabTag::default()
                    },
                ),
                ("w:quiet", lane(Vec::new(), false)),
                (
                    "w:done-wf-tab",
                    TabTag {
                        kind: TabKind::Workflow,
                        parent: Some("w:quiet".into()),
                        done: true,
                        ..TabTag::default()
                    },
                ),
            ]
            .into_iter()
            .map(|(id, tag)| (id.to_owned(), tag))
            .collect(),
            ..FactoryOverlay::default()
        };
        let mut live = live_factory_tabs(Some(&doc), Some(now), now)
            .into_iter()
            .collect::<Vec<_>>();
        live.sort();
        assert_eq!(live, ["w:fold", "w:parent", "w:wf-tab", "w:workflow"]);
        let stale = now - FACTORY_OVERLAY_FRESH - Duration::from_secs(1);
        assert!(live_factory_tabs(Some(&doc), Some(stale), now).is_empty());
        assert!(live_factory_tabs(None, Some(now), now).is_empty());
    }
}
