use std::time::{Duration, Instant};

use crate::api::schema::{PaneQueueParams, ResponseResult};
use crate::config::PoliteSendConfig;
use crate::layout::PaneId;
use crate::terminal::polite_send::Payload;

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

    pub(crate) fn send_polite_bytes(
        &self,
        ws_idx: usize,
        pane_id: PaneId,
        method: &'static str,
        bytes: bytes::Bytes,
    ) -> std::io::Result<Option<usize>> {
        let runtime = self
            .lookup_runtime_sender(ws_idx, pane_id)
            .ok_or_else(|| std::io::Error::other("pane runtime closed"))?;
        runtime.polite_send(
            self.polite_guarded(ws_idx, pane_id),
            self.polite_send_quiet,
            method,
            Payload::Bytes(bytes),
        )
    }

    pub(crate) fn polite_send_deadline(&self, now: Instant) -> Option<Instant> {
        self.terminal_runtimes
            .values()
            .any(|runtime| runtime.has_polite_queue())
            .then_some(now + Duration::from_millis(250))
    }

    pub(crate) fn flush_polite_sends(&self, now: Instant) {
        for runtime in self.terminal_runtimes.values() {
            if runtime.has_polite_queue() {
                if let Err(err) = runtime.flush_polite_queue(
                    now,
                    self.polite_send_quiet,
                    self.polite_send_mode == PoliteSendConfig::Off,
                ) {
                    tracing::warn!(%err, "polite send flush failed");
                }
            }
        }
    }

    pub(super) fn handle_pane_queue(&self, id: String, params: PaneQueueParams) -> String {
        let Some((ws_idx, pane_id)) = self.parse_pane_id(&params.pane_id) else {
            return encode_error(id, "pane_not_found", "pane not found");
        };
        let Some(runtime) = self.lookup_runtime_sender(ws_idx, pane_id) else {
            return encode_error(id, "pane_not_found", "pane not found");
        };
        if params.flush {
            if let Err(err) =
                runtime.flush_polite_queue(Instant::now(), self.polite_send_quiet, true)
            {
                return encode_error(id, "pane_send_failed", err.to_string());
            }
        }
        encode_success(
            id,
            ResponseResult::PaneQueue {
                sends: runtime.polite_queue(),
            },
        )
    }
}
