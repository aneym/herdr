use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::api::schema::AgentUsageInfo;
use crate::platform::ProcessSubtreeUsage;

const USAGE_CACHE_TTL: Duration = Duration::from_secs(2);

#[derive(Default)]
pub(crate) struct UsageSampler {
    sampled_at: Option<Instant>,
    usage_by_root: HashMap<u32, ProcessSubtreeUsage>,
}

impl UsageSampler {
    fn sample(&mut self, roots: &[u32], now: Instant) -> std::io::Result<()> {
        let is_fresh = self
            .sampled_at
            .is_some_and(|sampled_at| now.duration_since(sampled_at) < USAGE_CACHE_TTL);
        if is_fresh
            && roots
                .iter()
                .all(|root| self.usage_by_root.contains_key(root))
        {
            return Ok(());
        }

        let snapshot = crate::platform::process_snapshot()?;
        self.usage_by_root = crate::platform::aggregate_process_subtrees(&snapshot, roots);
        self.sampled_at = Some(now);
        Ok(())
    }
}

impl super::App {
    pub(crate) fn toggle_usage_overlay(&mut self) {
        if self.state.mode() == super::state::Mode::Usage {
            self.state.replace_mode(super::state::Mode::Terminal);
            self.next_usage_refresh = None;
            return;
        }
        self.state.replace_mode(super::state::Mode::Usage);
        self.refresh_usage_overlay();
    }

    pub(crate) fn refresh_usage_overlay(&mut self) {
        match self.collect_agent_usage() {
            Ok(rows) => {
                self.state.usage.rows = rows;
                self.state.usage.error = None;
            }
            Err(err) => self.state.usage.error = Some(format!("failed to sample usage: {err}")),
        }
        self.next_usage_refresh = Some(Instant::now() + USAGE_CACHE_TTL);
    }

    pub(crate) fn collect_agent_usage(&mut self) -> std::io::Result<Vec<AgentUsageInfo>> {
        let mut panes = Vec::new();
        for (ws_idx, workspace) in self.state.workspaces.iter().enumerate() {
            for tab in &workspace.tabs {
                for pane_id in tab.layout.pane_ids() {
                    let Some(pane) = self.pane_info(ws_idx, pane_id) else {
                        continue;
                    };
                    let root_pid = workspace
                        .terminal_id(pane_id)
                        .and_then(|terminal_id| self.terminal_runtimes.get(terminal_id))
                        .and_then(|runtime| runtime.child_pid());
                    panes.push((pane, root_pid));
                }
            }
        }
        let roots = panes
            .iter()
            .filter_map(|(_, root_pid)| *root_pid)
            .collect::<Vec<_>>();
        self.usage_sampler.sample(&roots, Instant::now())?;

        let mut usage = panes
            .into_iter()
            .map(|(pane, root_pid)| {
                let process = root_pid
                    .and_then(|pid| self.usage_sampler.usage_by_root.get(&pid))
                    .copied()
                    .unwrap_or_default();
                AgentUsageInfo {
                    pane_id: pane.pane_id,
                    workspace_id: pane.workspace_id,
                    tab_id: pane.tab_id,
                    agent: pane.agent,
                    title: pane.terminal_title_stripped,
                    cpu_percent: process.cpu_percent,
                    mem_bytes: process.mem_bytes,
                    process_count: process.process_count,
                }
            })
            .collect::<Vec<_>>();
        usage.sort_by(|left, right| {
            right
                .cpu_percent
                .total_cmp(&left.cpu_percent)
                .then_with(|| left.pane_id.cmp(&right.pane_id))
        });
        Ok(usage)
    }
}
