use std::collections::VecDeque;
use std::time::{Duration, Instant};

use bytes::Bytes;
use crossterm::event::{KeyCode, KeyEventKind, KeyModifiers};

use super::TerminalRuntime;

#[derive(Clone, Copy, Default)]
pub(crate) struct SendOptions {
    pub if_idle: bool,
    pub human: bool,
    pub claude: bool,
    pub settle: Duration,
}

pub(crate) struct SendOutcome {
    pub id: String,
    pub state: crate::api::schema::PaneSendState,
    pub position: Option<usize>,
}

use crate::api::schema::{PaneQueuedSend, PaneSendState};
use std::sync::{Mutex, OnceLock};

struct Receipt {
    owner: u64,
    item: PaneQueuedSend,
    enqueued: Instant,
    delivered: Option<Instant>,
}

#[derive(Default)]
struct History {
    next: u64,
    recent: VecDeque<Receipt>,
}

fn history() -> &'static Mutex<History> {
    static HISTORY: OnceLock<Mutex<History>> = OnceLock::new();
    HISTORY.get_or_init(Mutex::default)
}

fn timestamp() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

fn receipt(
    owner: u64,
    pane: crate::layout::PaneId,
    pgid: Option<u32>,
    method: &str,
    bytes: usize,
) -> String {
    let mut history = history().lock().unwrap();
    history.next += 1;
    let id = format!("q{}", history.next);
    history.recent.push_back(Receipt {
        owner,
        item: PaneQueuedSend {
            id: id.clone(),
            pane: format!("p{}", pane.raw()),
            pgid,
            method: method.into(),
            byte_length: bytes,
            age_secs: 0.0,
            state: PaneSendState::Queued,
            reason: None,
            queued_at: timestamp(),
            delivered_at: None,
            acked_at: None,
            ack_timeout: false,
        },
        enqueued: Instant::now(),
        delivered: None,
    });
    if history.recent.len() > 200 {
        history.recent.pop_front();
    }
    id
}

fn transition(id: &str, state: PaneSendState, reason: Option<&str>) {
    let mut history = history().lock().unwrap();
    if let Some(receipt) = history.recent.iter_mut().find(|r| r.item.id == id) {
        if state == PaneSendState::Delivered {
            receipt.delivered = Some(Instant::now());
            receipt.item.delivered_at = Some(timestamp());
        }
        receipt.item.state = state;
        receipt.item.reason = reason.map(str::to_owned);
    }
}

pub(crate) fn observe_output(pane: u32, pgid: Option<u32>, now: Instant) {
    let mut history = history().lock().unwrap();
    for receipt in &mut history.recent {
        if receipt.item.pane == format!("p{pane}")
            && receipt.item.pgid == pgid
            && receipt.item.state == PaneSendState::Delivered
            && receipt
                .delivered
                .is_some_and(|at| now.saturating_duration_since(at) <= Duration::from_secs(5))
        {
            receipt.item.state = PaneSendState::Acked;
            receipt.item.acked_at = Some(timestamp());
        }
    }
}

pub(crate) fn recent_sends(owner: Option<u64>, id: Option<&str>) -> Vec<PaneQueuedSend> {
    let mut history = history().lock().unwrap();
    let now = Instant::now();
    history
        .recent
        .iter_mut()
        .filter_map(|receipt| {
            if owner.is_some_and(|owner| receipt.owner != owner)
                || id.is_some_and(|id| receipt.item.id != id)
            {
                return None;
            }
            receipt.item.age_secs = now
                .saturating_duration_since(receipt.enqueued)
                .as_secs_f64();
            receipt.item.ack_timeout = receipt.item.state == PaneSendState::Delivered
                && receipt
                    .delivered
                    .is_some_and(|at| now.saturating_duration_since(at) >= Duration::from_secs(5));
            Some(receipt.item.clone())
        })
        .collect()
}

type Completion = std::sync::mpsc::Sender<std::io::Result<()>>;

pub(crate) enum Payload {
    Bytes(Bytes),
    Keys(Vec<Bytes>),
    Submission {
        focus: Bytes,
        text: Bytes,
        enter: Bytes,
        delay: Duration,
        deadline: Option<Instant>,
        completion: Completion,
    },
}

impl Payload {
    fn len(&self) -> usize {
        match self {
            Self::Bytes(bytes) => bytes.len(),
            Self::Keys(keys) => keys.iter().map(Bytes::len).sum(),
            Self::Submission {
                focus, text, enter, ..
            } => focus.len() + text.len() + enter.len(),
        }
    }
}

struct HeldSend {
    id: String,
    pgid: Option<u32>,
    pane_id: crate::layout::PaneId,
    method: &'static str,
    payload: Payload,
}

pub(super) struct PoliteSend {
    owner: u64,
    last_human_input_at: Option<Instant>,
    last_submit_at: Option<Instant>,
    draft: bool,
    queue: VecDeque<HeldSend>,
    raw_pending: Vec<u8>,
    raw_paste: bool,
}

impl Default for PoliteSend {
    fn default() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        Self {
            owner: NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            last_human_input_at: None,
            last_submit_at: None,
            draft: false,
            queue: VecDeque::new(),
            raw_pending: Vec::new(),
            raw_paste: false,
        }
    }
}

impl PoliteSend {
    fn typing(&mut self, draft: bool) {
        self.last_human_input_at = Some(Instant::now());
        self.draft |= draft;
    }

    fn submit(&mut self) {
        self.draft = false;
        self.last_submit_at = Some(Instant::now());
    }

    fn key(&mut self, key: &crate::input::TerminalKey) {
        if key.kind == KeyEventKind::Release {
            return;
        }
        if key.code == KeyCode::Enter && key.modifiers.is_empty() {
            self.submit();
        } else if matches!(key.code, KeyCode::Char('c' | 'u'))
            && key.modifiers == KeyModifiers::CONTROL
        {
            self.draft = false;
            self.typing(false);
        } else {
            self.typing(matches!(
                key.code,
                KeyCode::Char(_) | KeyCode::Backspace | KeyCode::Delete | KeyCode::Enter
            ));
        }
    }
    fn quiet(&self, now: Instant, quiet: Duration) -> bool {
        self.last_human_input_at
            .is_none_or(|at| now.saturating_duration_since(at) >= quiet)
    }
}

impl TerminalRuntime {
    /// Whether no human keystroke reached this runtime within `quiet`.
    pub(crate) fn human_input_quiet_for(&self, quiet: Duration) -> bool {
        self.1
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .quiet(Instant::now(), quiet)
    }

    pub(crate) fn record_human_text(&self) {
        self.1.lock().unwrap().typing(true);
    }

    pub(crate) fn record_human_key(&self, key: &crate::input::TerminalKey) {
        self.1.lock().unwrap().key(key);
    }

    pub(crate) fn record_human_bytes(&self, bytes: &[u8]) {
        let mut state = self.1.lock().unwrap();
        let mut input = std::mem::take(&mut state.raw_pending);
        input.extend_from_slice(bytes);
        let mut offset = 0;
        while offset < input.len() {
            let rest = &input[offset..];
            if rest[0] == 0x1b {
                if rest.len() == 1 {
                    break;
                }
                let end = match rest[1] {
                    b'[' => {
                        let Some(final_byte) = rest[2..]
                            .iter()
                            .position(|byte| (0x40..=0x7e).contains(byte))
                        else {
                            break;
                        };
                        let end = final_byte + 3;
                        // X10's three coordinates follow the CSI final byte.
                        if &rest[..end] == b"\x1b[M" {
                            if rest.len() < end + 3 {
                                break;
                            }
                            end + 3
                        } else {
                            end
                        }
                    }
                    b'O' => {
                        if rest.len() < 3 {
                            break;
                        }
                        3
                    }
                    b']' | b'P' | b'_' | b'^' => {
                        let mut end = None;
                        for index in 2..rest.len() {
                            if rest[index] == 7 {
                                end = Some(index + 1);
                                break;
                            }
                            if rest[index] == b'\\' && rest[index - 1] == 0x1b {
                                end = Some(index + 1);
                                break;
                            }
                        }
                        let Some(end) = end else {
                            break;
                        };
                        end
                    }
                    _ => 2,
                };
                let sequence = &rest[..end];
                if sequence == b"\x1b[200~" {
                    state.raw_paste = true;
                } else if sequence == b"\x1b[201~" {
                    state.raw_paste = false;
                } else if !state.raw_paste {
                    // Focus/mouse reports are not human key activity. Other encoded
                    // keys retain their modifiers (including kitty Enter).
                    let report = sequence == b"\x1b[I"
                        || sequence == b"\x1b[O"
                        || sequence.starts_with(b"\x1b[<")
                        || sequence.starts_with(b"\x1b[M");
                    if !report {
                        if sequence == b"\x1b\r" {
                            state.typing(true);
                        } else if let Ok(text) = std::str::from_utf8(sequence) {
                            if let Some(key) = crate::input::parse_terminal_key_sequence(text) {
                                state.key(&key);
                            }
                        }
                    }
                }
                offset += end;
                continue;
            }
            if state.raw_paste {
                state.typing(true);
            } else {
                match rest[0] {
                    b'\r' => state.submit(),
                    3 | 21 => {
                        state.draft = false;
                        state.typing(false);
                    }
                    b'\n' | 8 | 127 => state.typing(true),
                    byte if byte >= 32 => state.typing(true),
                    _ => state.typing(false),
                }
            }
            offset += 1;
        }
        state.raw_pending.extend_from_slice(&input[offset..]);
    }

    pub(crate) fn polite_send(
        &self,
        guarded: bool,
        quiet: Duration,
        method: &'static str,
        payload: Payload,
        options: SendOptions,
    ) -> std::io::Result<SendOutcome> {
        let pgid = self.0.foreground_process_group_id();
        let id = receipt(
            self.1.lock().unwrap().owner,
            self.0.pane_id,
            pgid,
            method,
            payload.len(),
        );
        let screen_draft = options
            .claude
            .then(|| self.0.claude_prompt_draft())
            .flatten();
        let mut state = self.1.lock().unwrap();
        if options.human {
            drop(state);
            match &payload {
                Payload::Bytes(bytes) => self.record_human_bytes(bytes),
                Payload::Keys(keys) => {
                    for bytes in keys {
                        self.record_human_bytes(bytes);
                    }
                }
                Payload::Submission { .. } => unreachable!("human submissions use pane sends"),
            }
            self.write_polite_payload(&payload, false, &id)?;
            return Ok(SendOutcome {
                state: self.polite_receipt_state(&id),
                id,
                position: None,
            });
        }
        if screen_draft == Some(false) {
            state.draft = false;
        }
        let now = Instant::now();
        let settling = state
            .last_submit_at
            .is_some_and(|at| now.saturating_duration_since(at) < options.settle);
        if !state.queue.is_empty()
            || (guarded
                && (screen_draft.unwrap_or(state.draft) || !state.quiet(now, quiet) || settling))
        {
            if options.if_idle {
                tracing::info!(pane_id = ?self.0.pane_id, method, bytes = payload.len(), "polite send dropped");
                transition(&id, PaneSendState::Dropped, Some("if_idle_busy"));
                return Ok(SendOutcome {
                    id,
                    state: PaneSendState::Dropped,
                    position: None,
                });
            }
            let len = payload.len();
            state.queue.push_back(HeldSend {
                id: id.clone(),
                pgid,
                pane_id: self.0.pane_id,
                method,
                payload,
            });
            tracing::info!(pane_id = ?self.0.pane_id, method, bytes = len, "polite send held");
            return Ok(SendOutcome {
                id,
                state: PaneSendState::Queued,
                position: Some(state.queue.len()),
            });
        }
        self.write_polite_payload(&payload, false, &id)?;
        Ok(SendOutcome {
            state: self.polite_receipt_state(&id),
            id,
            position: None,
        })
    }

    fn write_polite_payload(
        &self,
        payload: &Payload,
        flushing: bool,
        id: &str,
    ) -> std::io::Result<()> {
        let result = match payload {
            Payload::Bytes(bytes) => self
                .0
                .queue_tracked_submission(
                    bytes.clone(),
                    Bytes::new(),
                    Duration::ZERO,
                    None,
                    id.into(),
                )
                .map(|_| ()),
            Payload::Keys(keys) => {
                let bytes: Vec<u8> = keys.iter().flat_map(|key| key.iter().copied()).collect();
                if flushing {
                    self.0
                        .queue_tracked_submission(
                            Bytes::from(bytes),
                            Bytes::new(),
                            Duration::ZERO,
                            None,
                            id.into(),
                        )
                        .map(|_| ())
                } else {
                    for bytes in keys.iter().take(keys.len().saturating_sub(1)) {
                        self.try_send_bytes(bytes.clone())
                            .map_err(|err| std::io::Error::other(err.to_string()))?;
                    }
                    self.0
                        .queue_tracked_submission(
                            keys.last().cloned().unwrap_or_default(),
                            Bytes::new(),
                            Duration::ZERO,
                            None,
                            id.into(),
                        )
                        .map(|_| ())
                }
            }
            Payload::Submission {
                focus,
                text,
                enter,
                delay,
                deadline,
                completion,
            } => {
                if !focus.is_empty() {
                    self.try_send_bytes(focus.clone())
                        .map_err(|err| std::io::Error::other(err.to_string()))?;
                }
                match self.0.queue_tracked_submission(
                    text.clone(),
                    enter.clone(),
                    *delay,
                    *deadline,
                    id.to_owned(),
                ) {
                    Ok(receiver) => {
                        let completion = completion.clone();
                        std::thread::spawn(move || {
                            let result = receiver
                                .recv()
                                .unwrap_or_else(|_| Err(std::io::Error::other("pty actor closed")));
                            let _ = completion.send(result);
                        });
                    }
                    Err(err) => {
                        transition(id, PaneSendState::Dropped, Some("write_failed"));
                        let _ = completion.send(Err(err));
                    }
                }
                return Ok(());
            }
        };
        if result.is_err() {
            transition(id, PaneSendState::Dropped, Some("write_failed"));
        }
        result
    }

    fn polite_receipt_state(&self, id: &str) -> PaneSendState {
        recent_sends(None, Some(id))
            .into_iter()
            .next()
            .map(|item| item.state)
            .unwrap_or(PaneSendState::Queued)
    }

    pub(crate) fn polite_queue(&self) -> Vec<crate::api::schema::PaneQueuedSend> {
        recent_sends(Some(self.1.lock().unwrap().owner), None)
    }

    pub(crate) fn held_polite_sends(&self) -> Vec<PaneQueuedSend> {
        let state = self.1.lock().unwrap();
        recent_sends(Some(state.owner), None)
            .into_iter()
            .filter(|receipt| state.queue.iter().any(|item| item.id == receipt.id))
            .collect()
    }

    pub(crate) fn has_polite_queue(&self) -> bool {
        !self.1.lock().unwrap().queue.is_empty()
    }

    pub(crate) fn flush_polite_queue(
        &self,
        now: Instant,
        quiet: Duration,
        force: bool,
        options: SendOptions,
    ) -> std::io::Result<()> {
        let screen_draft = options
            .claude
            .then(|| self.0.claude_prompt_draft())
            .flatten();
        let mut state = self.1.lock().unwrap();
        if screen_draft == Some(false) {
            state.draft = false;
        }
        let settled = state
            .last_submit_at
            .is_none_or(|at| now.saturating_duration_since(at) >= options.settle);
        let submitted = state.last_submit_at.is_some_and(|submit| {
            state
                .last_human_input_at
                .is_none_or(|input| submit >= input)
        });
        if !force
            && (screen_draft.unwrap_or(state.draft)
                || !settled
                || (!state.quiet(now, quiet) && !submitted))
        {
            return Ok(());
        }
        while let Some(item) = state.queue.front() {
            let len = item.payload.len();
            if self.0.foreground_process_group_id() != item.pgid {
                transition(
                    &item.id,
                    PaneSendState::StaleSession,
                    Some("foreground_pgid_changed"),
                );
                state.queue.pop_front();
                continue;
            }
            self.write_polite_payload(&item.payload, true, &item.id)?;
            tracing::info!(pane_id = ?self.0.pane_id, method = item.method, bytes = len, "polite send flushed");
            state.queue.pop_front();
        }
        Ok(())
    }
}

impl Drop for PoliteSend {
    fn drop(&mut self) {
        for item in &self.queue {
            transition(&item.id, PaneSendState::Dropped, Some("pane_closed"));
            tracing::info!(pane_id = ?item.pane_id, method = item.method, bytes = item.payload.len(), "polite send dropped");
        }
    }
}

pub(crate) fn submission_delivered(id: &str, success: bool) {
    transition(
        id,
        if success {
            PaneSendState::Delivered
        } else {
            PaneSendState::Dropped
        },
        if success { None } else { Some("write_failed") },
    );
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn child(command: &str) -> TerminalRuntime {
        let (events, _) = tokio::sync::mpsc::channel(32);
        TerminalRuntime::spawn_shell_command(
            crate::layout::PaneId::alloc(),
            24,
            80,
            std::env::current_dir().unwrap(),
            command,
            &crate::pane::PaneLaunchEnv::default(),
            crate::pane::AgentDetection::Disabled,
            0,
            crate::terminal_theme::TerminalTheme::default(),
            None,
            events,
            Arc::new(tokio::sync::Notify::new()),
            Arc::new(crate::render_signal::RenderSignal::new()),
        )
        .unwrap()
    }

    async fn wait_for(runtime: &TerminalRuntime, id: &str, state: PaneSendState) -> PaneQueuedSend {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let item = runtime
                .polite_queue()
                .into_iter()
                .find(|item| item.id == id)
                .unwrap();
            if item.state == state {
                return item;
            }
            assert!(Instant::now() < deadline, "wanted {state:?}, got {item:?}");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    async fn ready(runtime: &TerminalRuntime) {
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert!(runtime.0.foreground_process_group_id().is_some());
    }

    #[tokio::test]
    async fn polite_submission_receipts_follow_real_echo_and_have_unique_ids() {
        let runtime =
            child("stty -echo; while IFS= read -r line; do printf 'reply:%s\\n' \"$line\"; done");
        ready(&runtime).await;
        runtime.record_human_text();
        let (tx, completion) = std::sync::mpsc::channel();
        let outcome = runtime
            .polite_send(
                true,
                Duration::ZERO,
                "agent.prompt",
                Payload::Submission {
                    focus: Bytes::new(),
                    text: Bytes::from_static(b"private-message"),
                    enter: Bytes::from_static(b"\r"),
                    delay: Duration::from_millis(50),
                    deadline: None,
                    completion: tx,
                },
                SendOptions::default(),
            )
            .unwrap();
        assert_eq!(outcome.state, PaneSendState::Queued);
        assert_eq!(runtime.polite_queue()[0].state, PaneSendState::Queued);
        runtime
            .flush_polite_queue(Instant::now(), Duration::ZERO, true, SendOptions::default())
            .unwrap();
        completion
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .unwrap();
        let item = wait_for(&runtime, &outcome.id, PaneSendState::Acked).await;
        assert!(item.delivered_at.is_some() && item.acked_at >= item.delivered_at);
        assert!(!serde_json::to_string(&item)
            .unwrap()
            .contains("private-message"));
        let second = runtime
            .polite_send(
                false,
                Duration::ZERO,
                "pane.send",
                Payload::Bytes(Bytes::from_static(b"another\r")),
                SendOptions::default(),
            )
            .unwrap();
        assert_ne!(second.id, outcome.id);
        wait_for(&runtime, &second.id, PaneSendState::Acked).await;
    }

    #[tokio::test]
    async fn polite_queue_rejects_changed_foreground_without_writing() {
        let runtime = child(
            "set -m; sleep 0.6; stty -echo; IFS= read -r line; printf 'unexpected:%s\\n' \"$line\"",
        );
        ready(&runtime).await;
        runtime.record_human_text();
        let outcome = runtime
            .polite_send(
                true,
                Duration::ZERO,
                "pane.send",
                Payload::Bytes(Bytes::from_static(b"must-not-write\r")),
                SendOptions::default(),
            )
            .unwrap();
        let bound = runtime.polite_queue()[0].pgid;
        let deadline = Instant::now() + Duration::from_secs(3);
        while runtime.0.foreground_process_group_id() == bound {
            assert!(Instant::now() < deadline);
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        runtime
            .flush_polite_queue(Instant::now(), Duration::ZERO, true, SendOptions::default())
            .unwrap();
        let item = wait_for(&runtime, &outcome.id, PaneSendState::StaleSession).await;
        assert!(item.delivered_at.is_none());
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(!runtime.visible_text().contains("must-not-write"));
    }

    #[tokio::test]
    async fn polite_silent_child_keeps_delivered_with_ack_timeout() {
        let runtime = child("stty -echo; while IFS= read -r line; do :; done");
        ready(&runtime).await;
        let outcome = runtime
            .polite_send(
                false,
                Duration::ZERO,
                "pane.send",
                Payload::Bytes(Bytes::from_static(b"silent\r")),
                SendOptions::default(),
            )
            .unwrap();
        wait_for(&runtime, &outcome.id, PaneSendState::Delivered).await;
        tokio::time::sleep(Duration::from_millis(5100)).await;
        let item = runtime
            .polite_queue()
            .into_iter()
            .find(|item| item.id == outcome.id)
            .unwrap();
        assert_eq!(item.state, PaneSendState::Delivered);
        assert!(item.ack_timeout);
    }
}
