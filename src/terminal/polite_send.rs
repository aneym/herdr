mod queue;

use std::collections::VecDeque;

pub(crate) use queue::has_queued_sends;
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
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};

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

/// Receipts are plain data, so a panic elsewhere never takes them down.
fn history_lock() -> MutexGuard<'static, History> {
    history().lock().unwrap_or_else(PoisonError::into_inner)
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
    let mut history = history_lock();
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
            healed: None,
        },
        enqueued: Instant::now(),
        delivered: None,
    });
    if history.recent.len() > 200 {
        // A receipt still held in a queue stays visible to `pane queue`
        // however old it is: evict the oldest settled receipt instead.
        let settled = history
            .recent
            .iter()
            .position(|r| r.item.state != PaneSendState::Queued)
            .unwrap_or(0);
        history.recent.remove(settled);
    }
    id
}

/// A held send at the head of an idle pane's queue for this long is
/// delivered past its hold reason (Alex, 2026-10-10: a lane sat idle for
/// hours behind a send that never flushed; "this should never happen").
pub(crate) const SELF_HEAL_AFTER: Duration = Duration::from_secs(60);

/// A draft the human typed is theirs (Alex, 2026-09-30: a held wake must not
/// land in his open draft); the self-heal waits this long after their last
/// keystroke before it treats the draft as abandoned.
pub(crate) const HUMAN_DRAFT_GRACE: Duration = Duration::from_secs(30 * 60);

fn mark_healed(id: &str, note: String) {
    let mut history = history_lock();
    if let Some(receipt) = history.recent.iter_mut().find(|r| r.item.id == id) {
        receipt.item.healed = Some(note);
    }
}

fn transition(id: &str, state: PaneSendState, reason: Option<&str>) {
    let mut history = history_lock();
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
    let mut history = history_lock();
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
    let mut history = history_lock();
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
    fn is_submit(&self) -> bool {
        let Self::Keys(keys) = self else {
            return false;
        };
        keys.len() == 1
            && std::str::from_utf8(&keys[0])
                .ok()
                .and_then(crate::input::parse_terminal_key_sequence)
                .is_some_and(|key| {
                    key.code == KeyCode::Enter
                        && key.modifiers.is_empty()
                        && key.kind != KeyEventKind::Release
                })
    }

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

/// Binds a held send to one agent session: the caller re-checks it at every
/// flush, so the send reaches only that session, only once the agent is idle
/// and the pane has had no human input for `input_quiet`, and never after
/// `expires_at`.
#[derive(Debug, Clone)]
pub(crate) struct DeliveryGuard {
    pub session_id: String,
    /// The runtime instance (`TerminalRuntime::runtime_id`) the caller saw
    /// the agent ready on; a replacement runtime for the same session, such
    /// as a second `agent.resume`, never matches it.
    pub runtime_id: Option<String>,
    pub agent: crate::detect::Agent,
    pub input_quiet: Duration,
    pub expires_at: Instant,
}

/// What the owner of the agent state says about a guarded send right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeliveryVerdict {
    /// Same agent and session, idle or done.
    Ready,
    /// Same agent and session, not idle yet.
    Hold,
    /// The session or the agent process changed; never deliver.
    Stale,
}

struct HeldSend {
    id: String,
    enqueued: Instant,
    pgid: Option<u32>,
    pane_id: crate::layout::PaneId,
    method: &'static str,
    payload: Payload,
    guard: Option<DeliveryGuard>,
}

pub(super) struct PoliteSend {
    owner: u64,
    last_human_input_at: Option<Instant>,
    /// Last time any raw byte batch arrived from a human client, whether or
    /// not it parses as a key (a standalone Escape, a split CSI, F13). Only
    /// the `agent.resume` quiet gate reads it; polite sends keep their own
    /// keystroke classification.
    last_raw_input_at: Option<Instant>,
    last_submit_at: Option<Instant>,
    draft: bool,
    /// Whether this runtime has watched the composer since it began. A
    /// runtime imported by a live handoff has not: text already in the
    /// composer might be a human draft typed before the handoff, so it holds
    /// until a human key, a submit or a write of its own settles who owns it.
    composer_observed: bool,
    /// When a live handoff imported this runtime. Unattributed composer text
    /// gets the human-draft grace from then, as if typed at the handoff.
    inherited_at: Option<Instant>,
    /// An agent.prompt handoff owns the composer until human input or another send.
    agent_prompt_composer: bool,
    queue: queue::PoliteQueue,
    raw_pending: Vec<u8>,
    raw_paste: bool,
}

/// Reason a guarded receipt carries while it is still `queued` but has left
/// the polite queue for the PTY writer: it can no longer be cancelled and may
/// still land, so a caller must not treat it as undelivered.
pub(crate) const HANDED_TO_WRITER: &str = "handed_to_writer";

/// The polite-send state plus the identity of the runtime that owns it, which
/// is readable without the lock (verdict callbacks run under it).
pub(super) struct PoliteSendCell {
    owner: u64,
    state: Mutex<PoliteSend>,
}

impl Default for PoliteSendCell {
    fn default() -> Self {
        let state = PoliteSend::default();
        Self {
            owner: state.owner,
            state: Mutex::new(state),
        }
    }
}

impl PoliteSendCell {
    /// The state is plain data, so a panic while it was held never takes
    /// polite sends down with it.
    fn lock(&self) -> MutexGuard<'_, PoliteSend> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn runtime_token(owner: u64) -> String {
    format!("rt{}-{owner}", std::process::id())
}

impl Default for PoliteSend {
    fn default() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        Self {
            owner: NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            last_human_input_at: None,
            last_raw_input_at: None,
            last_submit_at: None,
            draft: false,
            composer_observed: true,
            inherited_at: None,
            agent_prompt_composer: false,
            queue: queue::PoliteQueue::default(),
            raw_pending: Vec::new(),
            raw_paste: false,
        }
    }
}

impl PoliteSend {
    fn typing(&mut self, draft: bool) {
        self.agent_prompt_composer = false;
        self.composer_observed = true;
        self.last_human_input_at = Some(Instant::now());
        self.draft |= draft;
    }

    fn submit(&mut self) {
        self.agent_prompt_composer = false;
        self.composer_observed = true;
        self.last_human_input_at = Some(Instant::now());
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
    /// The `agent.resume` gate: no human input at all within `quiet`,
    /// counting every raw byte batch too.
    fn human_input_quiet(&self, now: Instant, quiet: Duration) -> bool {
        self.quiet(now, quiet)
            && self
                .last_raw_input_at
                .is_none_or(|at| now.saturating_duration_since(at) >= quiet)
    }

    fn human_input_age(&self, now: Instant) -> Option<Duration> {
        self.last_human_input_at
            .into_iter()
            .chain(self.last_raw_input_at)
            .max()
            .map(|at| now.saturating_duration_since(at))
    }

    /// Whether the composer holds a draft a human typed. Only human keys in
    /// this pane make one: text an agent or a send left there does not, an
    /// emptied composer clears it, and a draft with no human input for
    /// `HUMAN_DRAFT_GRACE` is stale. `screen` is the composer as drawn
    /// (`None` when it cannot be read, e.g. not a Claude pane).
    fn human_draft(&mut self, screen: Option<bool>, now: Instant) -> bool {
        let draft = self.sees_human_draft(screen, now);
        if self.draft && (screen == Some(false) || self.draft_stale(now)) {
            self.draft = false;
        }
        draft
    }

    fn sees_human_draft(&self, screen: Option<bool>, now: Instant) -> bool {
        let typed = self.draft && !self.draft_stale(now);
        match screen {
            Some(false) => false,
            Some(true) => typed || !self.composer_observed,
            None => typed,
        }
    }

    /// Text an `agent.prompt` handoff typed and has not submitted yet: other
    /// sends wait so they never splice into it, but it is not a human draft.
    fn agent_composer_draft(&self, screen: Option<bool>) -> bool {
        screen == Some(true) && self.agent_prompt_composer
    }

    fn draft_stale(&self, now: Instant) -> bool {
        self.last_human_input_at
            .is_none_or(|at| now.saturating_duration_since(at) >= HUMAN_DRAFT_GRACE)
    }

    fn quiet(&self, now: Instant, quiet: Duration) -> bool {
        self.last_human_input_at
            .is_none_or(|at| now.saturating_duration_since(at) >= quiet)
    }
}

impl TerminalRuntime {
    /// Identity of this runtime instance. A replacement runtime for the same
    /// pane (`agent.resume`, respawn) never shares it.
    pub(crate) fn runtime_id(&self) -> String {
        runtime_token(self.1.owner)
    }

    /// Why a guarded send must not go out now, if it must not.
    fn guard_refusal(
        &self,
        guard: &DeliveryGuard,
        verdict: DeliveryVerdict,
        now: Instant,
    ) -> Option<(PaneSendState, &'static str)> {
        if now >= guard.expires_at {
            Some((PaneSendState::Dropped, "expired"))
        } else if guard
            .runtime_id
            .as_deref()
            .is_some_and(|id| id != self.runtime_id())
        {
            Some((PaneSendState::StaleSession, "runtime_changed"))
        } else if verdict == DeliveryVerdict::Stale {
            Some((PaneSendState::StaleSession, "agent_session_changed"))
        } else {
            None
        }
    }

    pub(crate) fn cancel_polite_send(&self, id: &str) -> bool {
        let mut state = self.1.lock();
        let Some(index) = state.queue.iter().position(|item| item.id == id) else {
            return false;
        };
        if let Some(item) = state.queue.remove(index) {
            transition(&item.id, PaneSendState::Dropped, Some("cancelled"));
            tracing::info!(pane_id = ?item.pane_id, method = item.method, "polite send cancelled");
        }
        true
    }

    /// Whether no human keystroke reached this runtime within `quiet`.
    pub(crate) fn human_input_quiet_for(&self, quiet: Duration) -> bool {
        self.1.lock().human_input_quiet(Instant::now(), quiet)
    }

    /// Time since any human input reached this runtime, counting every raw
    /// byte batch as `agent.resume` does; `None` when none has.
    pub(crate) fn human_input_age(&self) -> Option<Duration> {
        self.1.lock().human_input_age(Instant::now())
    }

    /// This runtime took over a live pane (handoff import): it did not see
    /// who typed what is already in the composer.
    pub(crate) fn inherit_composer(&self) {
        let mut state = self.1.lock();
        state.composer_observed = false;
        state.inherited_at = Some(Instant::now());
    }

    /// Read-only view of what the draft check sees for this pane.
    pub(crate) fn composer_state(&self, claude: bool) -> crate::api::schema::PaneComposerState {
        let screen = claude.then(|| self.0.claude_prompt_draft()).flatten();
        let now = Instant::now();
        let state = self.1.lock();
        let ms =
            |at: Option<Instant>| at.map(|at| now.saturating_duration_since(at).as_millis() as u64);
        crate::api::schema::PaneComposerState {
            human_draft: state.sees_human_draft(screen, now),
            screen_draft: screen,
            typed_draft: state.draft,
            composer_observed: state.composer_observed,
            agent_prompt_composer: state.agent_prompt_composer,
            last_human_key_age_ms: ms(state.last_human_input_at),
            last_raw_input_age_ms: ms(state.last_raw_input_at),
            last_submit_age_ms: ms(state.last_submit_at),
        }
    }

    pub(crate) fn record_human_text(&self) {
        self.1.lock().typing(true);
    }

    pub(crate) fn record_human_key(&self, key: &crate::input::TerminalKey) {
        self.1.lock().key(key);
    }

    pub(crate) fn record_human_bytes(&self, bytes: &[u8]) {
        self.1.lock().raw_bytes(bytes);
    }
}

impl PoliteSend {
    /// Classify raw human input bytes. For the `agent.resume` quiet gate
    /// every batch counts, whatever it parses as (or fails to).
    fn raw_bytes(&mut self, bytes: &[u8]) {
        let state = self;
        if !bytes.is_empty() {
            state.last_raw_input_at = Some(Instant::now());
        }
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
                    state.agent_prompt_composer = false;
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
}

impl TerminalRuntime {
    pub(crate) fn polite_send(
        &self,
        guarded: bool,
        quiet: Duration,
        method: &'static str,
        payload: Payload,
        options: SendOptions,
    ) -> std::io::Result<SendOutcome> {
        self.polite_send_with_guard(guarded, quiet, method, payload, options, None)
    }

    /// `polite_send` bound to a `DeliveryGuard`; `guard` carries the guard
    /// and its verdict at enqueue time.
    pub(crate) fn polite_send_with_guard(
        &self,
        guarded: bool,
        quiet: Duration,
        method: &'static str,
        payload: Payload,
        options: SendOptions,
        guard: Option<(DeliveryGuard, DeliveryVerdict)>,
    ) -> std::io::Result<SendOutcome> {
        let pgid = self.0.foreground_process_group_id();
        let id = receipt(
            self.1.lock().owner,
            self.0.pane_id,
            pgid,
            method,
            payload.len(),
        );
        let screen_draft = options
            .claude
            .then(|| self.0.claude_prompt_draft())
            .flatten();
        let mut state = self.1.lock();
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
        let now = Instant::now();
        let human_draft = state.human_draft(screen_draft, now);
        let agent_draft = state.agent_composer_draft(screen_draft);
        if let Some((refused, reason)) = guard
            .as_ref()
            .and_then(|(guard, verdict)| self.guard_refusal(guard, *verdict, now))
        {
            transition(&id, refused.clone(), Some(reason));
            tracing::info!(pane_id = ?self.0.pane_id, method, reason, "guarded polite send refused");
            return Ok(SendOutcome {
                id,
                state: refused,
                position: None,
            });
        }
        let guard_holds = guard.as_ref().is_some_and(|(guard, verdict)| {
            *verdict == DeliveryVerdict::Hold || !state.human_input_quiet(now, guard.input_quiet)
        });
        let guard = guard.map(|(guard, _)| guard);
        let settling = state
            .last_submit_at
            .is_some_and(|at| now.saturating_duration_since(at) < options.settle);
        let owned_submit = state.agent_prompt_composer && payload.is_submit();
        if (!state.queue.is_empty() && !owned_submit)
            || guard_holds
            || (guarded
                && (((human_draft || agent_draft) && !owned_submit)
                    || !state.quiet(now, quiet)
                    || settling))
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
            let reason = if !state.queue.is_empty() {
                "predecessor_queued"
            } else if guard_holds {
                "delivery_guard"
            } else if human_draft && !owned_submit {
                "human_composer_draft"
            } else if agent_draft && !owned_submit {
                "agent_composer_draft"
            } else if settling {
                "submit_settling"
            } else {
                "human_input_quiet"
            };
            transition(&id, PaneSendState::Queued, Some(reason));
            let len = payload.len();
            state.queue.push_back(HeldSend {
                id: id.clone(),
                enqueued: now,
                pgid,
                pane_id: self.0.pane_id,
                method,
                payload,
                guard,
            });
            tracing::info!(pane_id = ?self.0.pane_id, method, bytes = len, "polite send held");
            return Ok(SendOutcome {
                id,
                state: PaneSendState::Queued,
                position: Some(state.queue.len()),
            });
        }
        if guard.is_some() {
            transition(&id, PaneSendState::Queued, Some(HANDED_TO_WRITER));
        }
        self.write_polite_payload(&payload, false, &id)?;
        state.agent_prompt_composer = method == "agent.prompt";
        state.composer_observed = true;
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
        recent_sends(Some(self.1.lock().owner), None)
    }

    pub(crate) fn held_polite_sends(&self) -> Vec<PaneQueuedSend> {
        let state = self.1.lock();
        recent_sends(Some(state.owner), None)
            .into_iter()
            .filter(|receipt| state.queue.iter().any(|item| item.id == receipt.id))
            .collect()
    }

    pub(crate) fn has_polite_queue(&self) -> bool {
        !self.1.lock().queue.is_empty()
    }

    #[cfg(test)]
    pub(crate) fn flush_polite_queue(
        &self,
        now: Instant,
        quiet: Duration,
        force: bool,
        options: SendOptions,
    ) -> std::io::Result<()> {
        self.flush_polite_queue_guarded(now, quiet, force, false, options, &|_| {
            DeliveryVerdict::Ready
        })
    }

    /// Flush held sends. A guarded send is dropped once it expires or
    /// `verdict` calls it stale, and held (even when forced) until `verdict`
    /// is ready and the pane has had no human input for its quiet interval.
    /// With `agent_idle`, an unguarded head held past `SELF_HEAL_AFTER` with
    /// no human input for as long (`HUMAN_DRAFT_GRACE` when the human typed
    /// an unsubmitted draft) is delivered anyway and its receipt says so.
    pub(crate) fn flush_polite_queue_guarded(
        &self,
        now: Instant,
        quiet: Duration,
        force: bool,
        agent_idle: bool,
        options: SendOptions,
        verdict: &dyn Fn(&DeliveryGuard) -> DeliveryVerdict,
    ) -> std::io::Result<()> {
        self.flush_polite_queue_through(now, quiet, force, agent_idle, options, verdict, None)
    }

    /// Flush only the named receipt and its FIFO predecessors, never successors.
    pub(crate) fn flush_polite_send_guarded(
        &self,
        id: &str,
        now: Instant,
        quiet: Duration,
        options: SendOptions,
        verdict: &dyn Fn(&DeliveryGuard) -> DeliveryVerdict,
    ) -> std::io::Result<()> {
        self.flush_polite_queue_through(now, quiet, true, false, options, verdict, Some(id))
    }

    fn flush_polite_queue_through(
        &self,
        now: Instant,
        quiet: Duration,
        force: bool,
        agent_idle: bool,
        options: SendOptions,
        verdict: &dyn Fn(&DeliveryGuard) -> DeliveryVerdict,
        through: Option<&str>,
    ) -> std::io::Result<()> {
        let screen_draft = options
            .claude
            .then(|| self.0.claude_prompt_draft())
            .flatten();
        let mut state = self.1.lock();
        let mut remaining = match through {
            Some(id) => match state.queue.iter().position(|item| item.id == id) {
                Some(index) => index + 1,
                None => return Ok(()),
            },
            None => state.queue.len(),
        };
        let limit = remaining;
        let mut index = 0;
        state.queue.retain(|item| {
            index += 1;
            if index > limit {
                return true;
            }
            let Some(guard) = &item.guard else {
                return true;
            };
            let Some((drop_state, reason)) = self.guard_refusal(guard, verdict(guard), now) else {
                return true;
            };
            remaining -= 1;
            transition(&item.id, drop_state, Some(reason));
            tracing::info!(pane_id = ?item.pane_id, method = item.method, reason, "guarded polite send dropped");
            false
        });
        // A typed draft with no human key for `HUMAN_DRAFT_GRACE` is stale:
        // what it held goes out now, and its receipts say so.
        let stale_draft = state.draft && screen_draft != Some(false) && state.draft_stale(now);
        let human_draft = state.human_draft(screen_draft, now);
        let agent_draft = state.agent_composer_draft(screen_draft);
        let settled = state
            .last_submit_at
            .is_none_or(|at| now.saturating_duration_since(at) >= options.settle);
        let submitted = state.last_submit_at.is_some_and(|submit| {
            state
                .last_human_input_at
                .is_none_or(|input| submit >= input)
        });
        while remaining > 0 {
            let Some(item) = state.queue.front() else {
                break;
            };
            let owned_submit = state.agent_prompt_composer && item.payload.is_submit();
            let reason = if human_draft && !owned_submit {
                Some("human_composer_draft")
            } else if agent_draft && !owned_submit {
                Some("agent_composer_draft")
            } else if !settled {
                Some("submit_settling")
            } else if !state.quiet(now, quiet) && !submitted {
                Some("human_input_quiet")
            } else {
                None
            };
            let mut healed = None;
            if !force {
                if let Some(reason) = reason {
                    let held = now.saturating_duration_since(item.enqueued);
                    // Incident 2026-10-11: after a live handoff, a wake
                    // self-healed 60 s into Alex's unattributed draft and
                    // submitted it. Any human draft, typed or inherited, gets
                    // the full grace.
                    let human_quiet = if human_draft || state.draft {
                        HUMAN_DRAFT_GRACE
                    } else {
                        SELF_HEAL_AFTER
                    };
                    let inherited = (!state.composer_observed)
                        .then_some(state.inherited_at)
                        .flatten()
                        .map(|at| now.saturating_duration_since(at));
                    let heal = agent_idle
                        && held >= SELF_HEAL_AFTER
                        && state
                            .human_input_age(now)
                            .into_iter()
                            .chain(inherited)
                            .min()
                            .is_none_or(|age| age >= human_quiet);
                    if !heal {
                        transition(&item.id, PaneSendState::Queued, Some(reason));
                        break;
                    }
                    healed = Some((reason, held));
                }
            }
            let len = item.payload.len();
            if self.0.foreground_process_group_id() != item.pgid {
                transition(
                    &item.id,
                    PaneSendState::StaleSession,
                    Some("foreground_pgid_changed"),
                );
                state.queue.pop_front();
                remaining -= 1;
                continue;
            }
            if let Some(guard) = &item.guard {
                // Recheck at the handoff itself: `now` predates the
                // foreground lookup above and the verdict callback, both of
                // which can block, so sample the clock after them.
                let ready = verdict(guard);
                let handoff = Instant::now();
                if let Some((drop_state, reason)) = self.guard_refusal(guard, ready, handoff) {
                    transition(&item.id, drop_state, Some(reason));
                    tracing::info!(pane_id = ?item.pane_id, method = item.method, reason, "guarded polite send dropped at handoff");
                    state.queue.pop_front();
                    remaining -= 1;
                    continue;
                }
                if ready != DeliveryVerdict::Ready
                    || !state.human_input_quiet(handoff, guard.input_quiet)
                {
                    transition(&item.id, PaneSendState::Queued, Some("delivery_guard"));
                    break;
                }
                transition(&item.id, PaneSendState::Queued, Some(HANDED_TO_WRITER));
            }
            self.write_polite_payload(&item.payload, true, &item.id)?;
            tracing::info!(pane_id = ?self.0.pane_id, method = item.method, bytes = len, "polite send flushed");
            if healed.is_none() && stale_draft {
                let held = now.saturating_duration_since(item.enqueued);
                tracing::warn!(pane_id = ?item.pane_id, method = item.method, id = %item.id, held_secs = held.as_secs(), "polite send released by a stale human draft");
                mark_healed(
                    &item.id,
                    format!("human_composer_draft stale, held {}s", held.as_secs()),
                );
            }
            if let Some((reason, held)) = healed {
                tracing::warn!(pane_id = ?item.pane_id, method = item.method, id = %item.id, reason, held_secs = held.as_secs(), "polite send self-healed");
                mark_healed(
                    &item.id,
                    format!("{reason} held {}s on an idle pane", held.as_secs()),
                );
            }
            state.agent_prompt_composer = item.method == "agent.prompt";
            state.composer_observed = true;
            state.queue.pop_front();
            remaining -= 1;
        }
        Ok(())
    }
}

impl Drop for PoliteSend {
    fn drop(&mut self) {
        for item in self.queue.iter() {
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

    /// A guarded send that left the queue for a writer still busy with it
    /// reports `handed_to_writer`: no longer cancellable, possibly landing.
    #[tokio::test]
    async fn guarded_send_reports_handed_to_writer_until_the_writer_finishes() {
        let runtime = child("stty -echo; while IFS= read -r line; do :; done");
        ready(&runtime).await;
        let (tx, completion) = std::sync::mpsc::channel();
        let guard = DeliveryGuard {
            session_id: "sess-1".into(),
            runtime_id: Some(runtime.runtime_id()),
            agent: crate::detect::Agent::Claude,
            input_quiet: Duration::ZERO,
            expires_at: Instant::now() + Duration::from_secs(10),
        };
        let outcome = runtime
            .polite_send_with_guard(
                false,
                Duration::ZERO,
                "agent.prompt",
                Payload::Submission {
                    focus: Bytes::new(),
                    text: Bytes::from_static(b"handed-line"),
                    enter: Bytes::from_static(b"\r"),
                    delay: Duration::from_millis(500),
                    deadline: None,
                    completion: tx,
                },
                SendOptions::default(),
                Some((guard, DeliveryVerdict::Ready)),
            )
            .unwrap();
        let item = runtime
            .polite_queue()
            .into_iter()
            .find(|item| item.id == outcome.id)
            .unwrap();
        assert_eq!(item.state, PaneSendState::Queued);
        assert_eq!(item.reason.as_deref(), Some(HANDED_TO_WRITER));
        assert!(!runtime.cancel_polite_send(&outcome.id));
        completion
            .recv_timeout(Duration::from_secs(3))
            .unwrap()
            .unwrap();
        let item = wait_for(&runtime, &outcome.id, PaneSendState::Delivered).await;
        assert_eq!(item.reason, None);
    }

    /// A flush whose timestamp predates the guard's expiry still drops the
    /// line once it has expired by the time of the writer handoff.
    #[tokio::test]
    async fn guarded_flush_rechecks_expiry_at_the_writer_handoff() {
        let (runtime, mut rx) = TerminalRuntime::test_with_channel(80, 24);
        let flush_started = Instant::now();
        let guard = DeliveryGuard {
            session_id: "sess-1".into(),
            runtime_id: Some(runtime.runtime_id()),
            agent: crate::detect::Agent::Claude,
            input_quiet: Duration::ZERO,
            expires_at: flush_started + Duration::from_millis(50),
        };
        let outcome = runtime
            .polite_send_with_guard(
                false,
                Duration::ZERO,
                "agent.prompt",
                Payload::Bytes(Bytes::from_static(b"late-line")),
                SendOptions::default(),
                Some((guard, DeliveryVerdict::Hold)),
            )
            .unwrap();
        assert_eq!(outcome.state, PaneSendState::Queued);
        // The flush began before expiry, but reaches the handoff after it
        // (as when the foreground lookup blocks).
        tokio::time::sleep(Duration::from_millis(100)).await;
        runtime
            .flush_polite_queue_guarded(
                flush_started,
                Duration::ZERO,
                true,
                false,
                SendOptions::default(),
                &|_| DeliveryVerdict::Ready,
            )
            .unwrap();
        let item = runtime
            .polite_queue()
            .into_iter()
            .find(|item| item.id == outcome.id)
            .unwrap();
        assert_eq!(item.state, PaneSendState::Dropped);
        assert_eq!(item.reason.as_deref(), Some("expired"));
        assert!(!runtime.has_polite_queue());
        assert!(rx.try_recv().is_err(), "an expired line reached the writer");
    }

    /// The readiness verdict can block (it reads agent state behind the
    /// terminal lock); a line that expires while it runs is never written.
    #[tokio::test]
    async fn guarded_flush_samples_expiry_after_the_readiness_verdict() {
        let (runtime, mut rx) = TerminalRuntime::test_with_channel(80, 24);
        let guard = DeliveryGuard {
            session_id: "sess-1".into(),
            runtime_id: Some(runtime.runtime_id()),
            agent: crate::detect::Agent::Claude,
            input_quiet: Duration::ZERO,
            expires_at: Instant::now() + Duration::from_millis(200),
        };
        let outcome = runtime
            .polite_send_with_guard(
                false,
                Duration::ZERO,
                "agent.prompt",
                Payload::Bytes(Bytes::from_static(b"late-line")),
                SendOptions::default(),
                Some((guard, DeliveryVerdict::Hold)),
            )
            .unwrap();
        assert_eq!(outcome.state, PaneSendState::Queued);
        // Ready at once for the pre-pass; the handoff verdict blocks until
        // after expiry.
        let calls = std::cell::Cell::new(0);
        let verdict = |_: &DeliveryGuard| {
            calls.set(calls.get() + 1);
            if calls.get() == 2 {
                std::thread::sleep(Duration::from_millis(300));
            }
            DeliveryVerdict::Ready
        };
        runtime
            .flush_polite_queue_guarded(
                Instant::now(),
                Duration::ZERO,
                true,
                false,
                SendOptions::default(),
                &verdict,
            )
            .unwrap();
        assert_eq!(calls.get(), 2, "pre-pass and handoff each ask once");
        let item = runtime
            .polite_queue()
            .into_iter()
            .find(|item| item.id == outcome.id)
            .unwrap();
        assert_eq!(item.state, PaneSendState::Dropped);
        assert_eq!(item.reason.as_deref(), Some("expired"));
        assert!(rx.try_recv().is_err(), "an expired line reached the writer");
    }

    /// A panic while the polite-send state was locked must not turn every
    /// later send, flush or input record on that pane into a panic.
    #[tokio::test]
    async fn poisoned_polite_state_keeps_serving_sends() {
        let (runtime, mut rx) = TerminalRuntime::test_with_channel(80, 24);
        let poisoned = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _held = runtime.1.state.lock();
            panic!("poison the polite-send state");
        }));
        assert!(poisoned.is_err());
        assert!(runtime.1.state.is_poisoned());
        runtime.record_human_bytes(b"x");
        assert!(!runtime.human_input_quiet_for(Duration::from_secs(20)));
        let outcome = runtime
            .polite_send(
                false,
                Duration::ZERO,
                "pane.send",
                Payload::Bytes(Bytes::from_static(b"after-poison")),
                SendOptions::default(),
            )
            .unwrap();
        assert_eq!(outcome.state, PaneSendState::Delivered);
        assert_eq!(rx.try_recv().unwrap(), Bytes::from_static(b"after-poison"));
        runtime
            .flush_polite_queue(Instant::now(), Duration::ZERO, true, SendOptions::default())
            .unwrap();
        assert!(!runtime.has_polite_queue());
        assert!(!runtime.cancel_polite_send(&outcome.id));
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

#[cfg(test)]
mod agent_resume_quiet_tests {
    use super::*;
    #[test]
    fn polite_send_submit_counts_as_human_input() {
        let mut state = PoliteSend::default();
        state.submit();
        assert!(!state.quiet(Instant::now(), Duration::from_secs(20)));
    }

    #[test]
    fn polite_send_standalone_escape_counts_as_human_input() {
        let quiet = Duration::from_secs(20);
        let mut state = PoliteSend::default();
        state.raw_bytes(b"\x1b");
        assert_eq!(state.raw_pending, b"\x1b");
        assert!(!state.human_input_quiet(Instant::now(), quiet));

        let mut split = PoliteSend::default();
        split.raw_bytes(b"\x1b[1;5");
        assert!(!split.human_input_quiet(Instant::now(), quiet));

        let mut key = PoliteSend::default();
        key.raw_bytes(b"x");
        assert!(!key.human_input_quiet(Instant::now(), quiet));

        // A complete key with no parsed meaning (F13) still counts.
        let mut unparsed = PoliteSend::default();
        unparsed.raw_bytes(b"\x1b[25~");
        assert!(unparsed.raw_pending.is_empty());
        assert!(unparsed.quiet(Instant::now(), quiet));
        assert!(!unparsed.human_input_quiet(Instant::now(), quiet));

        assert!(PoliteSend::default().human_input_quiet(Instant::now(), quiet));
    }
}

/// Composer-draft detection (2026-10-11 incident: four panes held behind
/// "human_composer_draft" while Alex typed in one other chat).
#[cfg(test)]
mod composer_draft_tests {
    use super::*;

    fn screen(runtime: &TerminalRuntime, bytes: &str) -> Option<bool> {
        runtime.test_process_pty_bytes(format!("\x1b[2J\x1b[H{bytes}").as_bytes());
        runtime.0.claude_prompt_draft()
    }

    #[tokio::test]
    async fn composer_screen_ignores_the_background_agent_footer() {
        let (runtime, _rx) = TerminalRuntime::test_with_channel(80, 12);
        // p169's layout: no closing rule, a blank row, then the agent footer.
        let footer = "● done\r\n────────\r\n❯ \x1b[2mrails-dev-fix fallback check\x1b[0m\r\n\r\n   4  · ← for agents · ↓ to manage\r\n  ◯ gpt-implementer  Polling marker";
        assert_eq!(screen(&runtime, footer), Some(false));
        let empty = "────────\r\n❯ \r\n\r\n   4  · ← for agents";
        assert_eq!(screen(&runtime, empty), Some(false));
        let typed = "────────\r\n❯ turn shado\r\n\r\n   4  · ← for agents";
        assert_eq!(screen(&runtime, typed), Some(true));
    }

    #[tokio::test]
    async fn composer_screen_reads_only_the_ruled_composer() {
        let (runtime, _rx) = TerminalRuntime::test_with_channel(80, 12);
        // A past prompt echoed in the transcript is not the composer.
        let echoed = "❯ an old message\r\n\r\n● reply\r\n────────\r\n❯ \r\n────────\r\n  footer";
        assert_eq!(screen(&runtime, echoed), Some(false));
        let draft = "❯ an old message\r\n────────\r\n❯ real draft\r\n────────";
        assert_eq!(screen(&runtime, draft), Some(true));
        let faint = "────────\r\n❯ \x1b[2msuggestion\x1b[0m\r\n────────";
        assert_eq!(screen(&runtime, faint), Some(false));
        // No ruled composer on screen: unknown, the typed flag decides.
        assert_eq!(screen(&runtime, "❯ just a shell prompt"), None);
    }

    #[test]
    fn agent_inserted_composer_text_is_not_a_human_draft() {
        let now = Instant::now();
        let mut state = PoliteSend::default();
        // A spawned runtime has watched its composer since it began.
        assert!(!state.human_draft(Some(true), now));
        assert!(!state.human_draft(None, now));
    }

    #[test]
    fn unobserved_composer_text_holds_until_someone_owns_it() {
        // A runtime imported by a live handoff cannot tell who typed it.
        let now = Instant::now();
        let mut state = PoliteSend::default();
        state.composer_observed = false;
        assert!(state.human_draft(Some(true), now));
        assert!(!state.human_draft(Some(false), now));
        assert!(!state.human_draft(None, now));
    }

    #[test]
    fn human_keystrokes_make_a_draft_that_an_empty_composer_clears() {
        let mut state = PoliteSend::default();
        state.raw_bytes(b"turn shado");
        let now = Instant::now();
        assert!(state.human_draft(Some(true), now));
        assert!(state.human_draft(None, now));
        // Focus and mouse reports from a client are not keystrokes.
        let mut reports = PoliteSend::default();
        reports.raw_bytes(b"\x1b[I\x1b[O\x1b[<0;10;5M");
        assert!(!reports.draft);
        // The composer drawn empty clears the typed flag for good.
        assert!(!state.human_draft(Some(false), now));
        assert!(!state.draft);
        assert!(!state.human_draft(None, now));
        // Ctrl-U clears it too.
        state.raw_bytes(b"x\x15");
        assert!(!state.human_draft(None, Instant::now()));
    }

    #[test]
    fn a_stale_human_draft_times_out() {
        let mut state = PoliteSend::default();
        state.raw_bytes(b"abandoned");
        let typed = state.last_human_input_at.unwrap();
        assert!(state.human_draft(
            Some(true),
            typed + HUMAN_DRAFT_GRACE - Duration::from_secs(1)
        ));
        assert!(!state.human_draft(Some(true), typed + HUMAN_DRAFT_GRACE));
        assert!(!state.draft);
    }

    #[tokio::test]
    async fn composer_state_reports_what_the_check_sees() {
        let (runtime, _rx) = TerminalRuntime::test_with_channel(80, 12);
        runtime.test_process_pty_bytes("────────\r\n❯ planted\r\n────────".as_bytes());
        let agent = runtime.composer_state(true);
        assert_eq!(agent.screen_draft, Some(true));
        assert!(!agent.human_draft && agent.composer_observed && !agent.typed_draft);
        runtime.inherit_composer();
        let imported = runtime.composer_state(true);
        assert!(imported.human_draft && !imported.composer_observed);
        runtime.record_human_bytes(b"x");
        let human = runtime.composer_state(true);
        assert!(human.human_draft && human.typed_draft);
        assert!(human.last_human_key_age_ms.is_some());
    }

    #[tokio::test]
    async fn inherited_composer_text_gets_the_full_draft_grace() {
        let (runtime, mut rx) = TerminalRuntime::test_with_channel(80, 12);
        runtime.inherit_composer();
        runtime.test_process_pty_bytes("────────\r\n❯ turn shado\r\n────────".as_bytes());
        let options = SendOptions {
            claude: true,
            ..SendOptions::default()
        };
        let quiet = Duration::from_secs(1);
        let sent = runtime
            .polite_send(
                true,
                quiet,
                "pane.send_input",
                Payload::Bytes(Bytes::from_static(b"wake")),
                options,
            )
            .unwrap();
        assert_eq!(sent.state, PaneSendState::Queued);
        let ready = |_: &DeliveryGuard| DeliveryVerdict::Ready;
        let now = Instant::now();
        runtime
            .flush_polite_queue_guarded(
                now + SELF_HEAL_AFTER * 2,
                quiet,
                false,
                true,
                options,
                &ready,
            )
            .unwrap();
        assert!(rx.try_recv().is_err(), "healed into an inherited draft");
        runtime
            .flush_polite_queue_guarded(
                now + HUMAN_DRAFT_GRACE + SELF_HEAL_AFTER,
                quiet,
                false,
                true,
                options,
                &ready,
            )
            .unwrap();
        assert_eq!(rx.try_recv().unwrap().as_ref(), b"wake");
    }
}
