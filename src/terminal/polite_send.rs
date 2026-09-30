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

pub(crate) enum SendOutcome {
    Sent,
    Queued(usize),
    Dropped,
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
    pane_id: crate::layout::PaneId,
    method: &'static str,
    payload: Payload,
    at: Instant,
}

#[derive(Default)]
pub(super) struct PoliteSend {
    last_human_input_at: Option<Instant>,
    last_submit_at: Option<Instant>,
    draft: bool,
    queue: VecDeque<HeldSend>,
    raw_pending: Vec<u8>,
    raw_paste: bool,
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
            self.write_polite_payload(&payload, false)?;
            return Ok(SendOutcome::Sent);
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
                return Ok(SendOutcome::Dropped);
            }
            let len = payload.len();
            state.queue.push_back(HeldSend {
                pane_id: self.0.pane_id,
                method,
                payload,
                at: Instant::now(),
            });
            tracing::info!(pane_id = ?self.0.pane_id, method, bytes = len, "polite send held");
            return Ok(SendOutcome::Queued(state.queue.len()));
        }
        self.write_polite_payload(&payload, false)?;
        Ok(SendOutcome::Sent)
    }

    fn write_polite_payload(&self, payload: &Payload, flushing: bool) -> std::io::Result<()> {
        match payload {
            Payload::Bytes(bytes) => self
                .try_send_bytes(bytes.clone())
                .map_err(|err| std::io::Error::other(err.to_string())),
            Payload::Keys(keys) => {
                if flushing {
                    let bytes: Vec<u8> = keys.iter().flat_map(|key| key.iter().copied()).collect();
                    return self
                        .try_send_bytes(Bytes::from(bytes))
                        .map_err(|err| std::io::Error::other(err.to_string()));
                }
                for bytes in keys {
                    self.try_send_bytes(bytes.clone())
                        .map_err(|err| std::io::Error::other(err.to_string()))?;
                }
                Ok(())
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
                match self.queue_user_input_submission(
                    text.clone(),
                    enter.clone(),
                    *delay,
                    *deadline,
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
                        let _ = completion.send(Err(err));
                    }
                }
                Ok(())
            }
        }
    }

    pub(crate) fn polite_queue(&self) -> Vec<crate::api::schema::PaneQueuedSend> {
        self.1
            .lock()
            .unwrap()
            .queue
            .iter()
            .map(|item| crate::api::schema::PaneQueuedSend {
                method: item.method.into(),
                byte_length: item.payload.len(),
                age_secs: item.at.elapsed().as_secs_f64(),
            })
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
            self.write_polite_payload(&item.payload, true)?;
            tracing::info!(pane_id = ?self.0.pane_id, method = item.method, bytes = len, "polite send flushed");
            state.queue.pop_front();
        }
        Ok(())
    }
}

impl Drop for PoliteSend {
    fn drop(&mut self) {
        for item in &self.queue {
            tracing::info!(pane_id = ?item.pane_id, method = item.method, bytes = item.payload.len(), "polite send dropped");
        }
    }
}
