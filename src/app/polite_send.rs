use std::time::{Duration, Instant};

use crate::api::schema::{PaneQueueParams, ResponseResult};
use crate::config::PoliteSendConfig;
use crate::layout::PaneId;
use crate::terminal::polite_send::{
    DeliveryGuard, DeliveryVerdict, Payload, SendOptions, SendOutcome,
};

use super::api::responses::{encode_error, encode_success};
use super::App;

impl App {
    pub(crate) fn polite_guarded(&self, ws_idx: usize, pane_id: PaneId) -> bool {
        match self.polite_send_mode {
            PoliteSendConfig::Off => false,
            PoliteSendConfig::All => true,
            PoliteSendConfig::Agents => self
                .state
                .workspaces
                .get(ws_idx)
                .and_then(|ws| ws.terminal_id(pane_id))
                .and_then(|id| self.state.terminals.get(id))
                .is_some_and(|terminal| terminal.detected_agent.is_some()),
        }
    }

    /// Whether a send bound to `guard` may reach the pane's agent now: the
    /// same agent, still the foreground process, on the same session, and
    /// idle or done.
    pub(crate) fn agent_delivery_verdict(
        &self,
        ws_idx: usize,
        pane_id: PaneId,
        guard: &DeliveryGuard,
    ) -> DeliveryVerdict {
        let Some(info) = self.agent_info(ws_idx, pane_id) else {
            return DeliveryVerdict::Stale;
        };
        let same_session = info
            .agent_session
            .as_ref()
            .is_some_and(|session| session.value == guard.session_id);
        let same_agent = info.agent.as_deref() == Some(crate::detect::agent_label(guard.agent));
        let hosted = self
            .lookup_runtime_sender(ws_idx, pane_id)
            .is_some_and(|runtime| super::agents::runtime_hosts_agent(runtime, guard.agent));
        if !(same_session && same_agent && hosted) {
            return DeliveryVerdict::Stale;
        }
        if matches!(
            info.agent_status,
            crate::api::schema::AgentStatus::Idle | crate::api::schema::AgentStatus::Done
        ) {
            DeliveryVerdict::Ready
        } else {
            DeliveryVerdict::Hold
        }
    }

    pub(crate) fn polite_options(
        &self,
        ws_idx: usize,
        pane_id: PaneId,
        if_idle: bool,
        human: bool,
    ) -> SendOptions {
        let claude = self
            .state
            .workspaces
            .get(ws_idx)
            .and_then(|ws| ws.terminal_id(pane_id))
            .and_then(|id| self.state.terminals.get(id))
            .is_some_and(|terminal| terminal.detected_agent == Some(crate::detect::Agent::Claude));
        SendOptions {
            if_idle,
            human,
            claude,
            settle: self.polite_send_settle,
        }
    }

    pub(crate) fn send_polite_bytes(
        &self,
        ws_idx: usize,
        pane_id: PaneId,
        method: &'static str,
        bytes: bytes::Bytes,
        if_idle: bool,
        human: bool,
    ) -> std::io::Result<SendOutcome> {
        let runtime = self
            .lookup_runtime_sender(ws_idx, pane_id)
            .ok_or_else(|| std::io::Error::other("pane runtime closed"))?;
        runtime.polite_send(
            self.polite_guarded(ws_idx, pane_id),
            self.polite_send_quiet,
            method,
            Payload::Bytes(bytes),
            self.polite_options(ws_idx, pane_id, if_idle, human),
        )
    }

    pub(crate) fn polite_send_deadline(&self, now: Instant) -> Option<Instant> {
        self.terminal_runtimes
            .values()
            .any(|runtime| runtime.has_polite_queue())
            .then_some(now + Duration::from_millis(250))
    }

    pub(crate) fn flush_polite_sends(&self, now: Instant) {
        for (ws_idx, ws) in self.state.workspaces.iter().enumerate() {
            for (pane_id, terminal_id) in ws
                .tabs
                .iter()
                .flat_map(|tab| tab.panes.keys().copied())
                .filter_map(|pane| ws.terminal_id(pane).map(|id| (pane, id)))
            {
                let Some(runtime) = self.terminal_runtimes.get(terminal_id) else {
                    continue;
                };
                if runtime.has_polite_queue() {
                    let agent_idle = self.pane_info(ws_idx, pane_id).is_some_and(|pane| {
                        matches!(
                            pane.agent_status,
                            crate::api::schema::AgentStatus::Idle
                                | crate::api::schema::AgentStatus::Done
                        )
                    });
                    if let Err(err) = runtime.flush_polite_queue_guarded(
                        now,
                        self.polite_send_quiet,
                        self.polite_send_mode == PoliteSendConfig::Off,
                        agent_idle,
                        self.polite_options(ws_idx, pane_id, false, false),
                        &|guard| self.agent_delivery_verdict(ws_idx, pane_id, guard),
                    ) {
                        tracing::warn!(%err, "polite send flush failed");
                    }
                }
            }
        }
    }

    fn public_queue_receipts(
        &self,
        mut items: Vec<crate::api::schema::PaneQueuedSend>,
    ) -> Vec<crate::api::schema::PaneQueuedSend> {
        for item in &mut items {
            if let Some(raw) = item
                .pane
                .strip_prefix('p')
                .and_then(|raw| raw.parse::<u32>().ok())
            {
                let pane = PaneId::from_raw(raw);
                if let Some((ws_idx, _)) = self.find_pane(pane) {
                    if let Some(public) = self.public_pane_id(ws_idx, pane) {
                        item.pane = public;
                    }
                }
            }
        }
        items
    }

    pub(super) fn handle_pane_queue(&self, id: String, params: PaneQueueParams) -> String {
        if let Some(qid) = &params.id {
            if params.flush {
                for (ws_idx, ws) in self.state.workspaces.iter().enumerate() {
                    for pane_id in ws.tabs.iter().flat_map(|tab| tab.panes.keys().copied()) {
                        let Some(runtime) = self.lookup_runtime_sender(ws_idx, pane_id) else {
                            continue;
                        };
                        if runtime
                            .held_polite_sends()
                            .iter()
                            .any(|item| item.id == *qid)
                        {
                            if let Err(err) = runtime.flush_polite_send_guarded(
                                qid,
                                Instant::now(),
                                self.polite_send_quiet,
                                self.polite_options(ws_idx, pane_id, false, false),
                                &|guard| self.agent_delivery_verdict(ws_idx, pane_id, guard),
                            ) {
                                return encode_error(id, "pane_send_failed", err.to_string());
                            }
                        }
                    }
                }
            }
            if params.cancel {
                // Only the runtime holding the send can remove it: the named
                // pane's first, else any.
                let named = self
                    .parse_pane_id(&params.pane_id)
                    .and_then(|(ws_idx, pane_id)| self.lookup_runtime_sender(ws_idx, pane_id));
                if !named.is_some_and(|runtime| runtime.cancel_polite_send(qid)) {
                    let _ = self
                        .terminal_runtimes
                        .values()
                        .any(|runtime| runtime.cancel_polite_send(qid));
                }
            }
            let recent = self
                .public_queue_receipts(crate::terminal::polite_send::recent_sends(None, Some(qid)));
            if recent.is_empty() {
                return encode_error(id, "queue_item_not_found", "queue item not found");
            }
            return encode_success(
                id,
                ResponseResult::PaneQueue {
                    sends: vec![],
                    recent,
                },
            );
        }
        let Some((ws_idx, pane_id)) = self.parse_pane_id(&params.pane_id) else {
            return encode_error(id, "pane_not_found", "pane not found");
        };
        let Some(runtime) = self.lookup_runtime_sender(ws_idx, pane_id) else {
            return encode_error(id, "pane_not_found", "pane not found");
        };
        if params.flush {
            if let Err(err) = runtime.flush_polite_queue_guarded(
                Instant::now(),
                self.polite_send_quiet,
                true,
                false,
                self.polite_options(ws_idx, pane_id, false, false),
                &|guard| self.agent_delivery_verdict(ws_idx, pane_id, guard),
            ) {
                return encode_error(id, "pane_send_failed", err.to_string());
            }
        }
        encode_success(
            id,
            ResponseResult::PaneQueue {
                sends: self.public_queue_receipts(runtime.held_polite_sends()),
                recent: self.public_queue_receipts(runtime.polite_queue()),
            },
        )
    }
}
