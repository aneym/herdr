//! Direct-terminal attach client: the Shell's own `herdr terminal attach --no-escape`.
//!
//! One I/O thread owns the socket. It drains commands, polls for frames, and turns
//! server messages into [`AttachEvent`]s ready to feed a terminal emulator (xterm.js):
//! terminal bytes as-is, and mouse / keyboard mode changes rendered as the same
//! DECSET and kitty keyboard sequences herdr's attach client writes to Ghostty.

use std::collections::VecDeque;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::endpoint::Conn;

use crate::endpoint::{poll_read, prepare_polled, write_all_polled, Endpoint, ReadPoll};
use crate::wire::{
    encode_client_frame, AttachScrollDirection, AttachScrollSource, ClientMessage, FrameReader,
    RenderEncoding, ServerMessage, MAX_FRAME_SIZE, PROTOCOL_VERSION,
};

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const IDLE_POLL: Duration = Duration::from_millis(2);
const READ_CHUNK: usize = 64 * 1024;

/// herdr `DISABLE_HOST_MOUSE_REPORTING_SEQUENCE` (`src/terminal_modes.rs`).
pub const CLEAR_MOUSE_REPORTING: &[u8] =
    b"\x1b[?1006l\x1b[?1016l\x1b[?1015l\x1b[?1005l\x1b[?1003l\x1b[?1002l\x1b[?1000l\x1b[?9l";
/// crossterm `EnableMouseCapture`, which herdr's Unix attach writes after the clear.
pub const ENABLE_MOUSE_CAPTURE: &[u8] = b"\x1b[?1000h\x1b[?1002h\x1b[?1003h\x1b[?1015h\x1b[?1006h";
/// crossterm `DisableMouseCapture`.
pub const DISABLE_MOUSE_CAPTURE: &[u8] = b"\x1b[?1006l\x1b[?1015l\x1b[?1003l\x1b[?1002l\x1b[?1000l";
/// DEC SGR-pixels mouse mode 1016.
pub const ENABLE_SGR_PIXELS: &[u8] = b"\x1b[?1016h";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachMode {
    /// Writable attach (`ClientMessage::AttachTerminal`, no takeover).
    Attach,
    /// Writable attach that replaces whichever client holds the terminal
    /// (`ClientMessage::AttachTerminal { takeover: true }`).
    Takeover,
    /// Read-only observe (`ClientMessage::ObserveTerminal`). Input is refused locally.
    Observe,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MouseMode {
    pub enabled: bool,
    pub sgr_pixels: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KeyboardMode {
    /// Kitty keyboard flags; 0 means no herdr-pushed kitty entry.
    pub kitty_flags: u16,
    pub modify_other_keys_level: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttachEvent {
    /// Terminal bytes to write to the emulator.
    Bytes(Vec<u8>),
    /// New mouse / keyboard modes and the escape bytes that move the emulator there.
    ModeChange {
        mouse: MouseMode,
        keyboard: KeyboardMode,
        sequence: Vec<u8>,
    },
    Bell {
        count: u16,
    },
    /// OSC 52 clipboard data (base64) a program in the pane wrote, for the host clipboard.
    Clipboard {
        data: String,
    },
    /// A non-fatal server error notice.
    Notice {
        message: String,
    },
    /// The attach ended; no more events follow.
    Closed {
        reason: String,
    },
}

/// Tracks emulator modes and renders transitions like herdr's direct attach on Unix.
#[derive(Debug, Default, Clone)]
pub struct ModeTracker {
    mouse: MouseMode,
    keyboard: KeyboardMode,
}

impl ModeTracker {
    /// `ServerMessage::MouseCapture`. Returns the event when the mode changed.
    pub fn mouse_capture(&mut self, enabled: bool, sgr_pixels: bool) -> Option<AttachEvent> {
        let next = MouseMode {
            enabled,
            sgr_pixels: enabled && sgr_pixels,
        };
        if next == self.mouse {
            return None;
        }
        let mut sequence = CLEAR_MOUSE_REPORTING.to_vec();
        if next.enabled {
            sequence.extend_from_slice(ENABLE_MOUSE_CAPTURE);
            if next.sgr_pixels {
                sequence.extend_from_slice(ENABLE_SGR_PIXELS);
            }
        } else {
            sequence.extend_from_slice(DISABLE_MOUSE_CAPTURE);
        }
        self.mouse = next;
        Some(self.event(sequence))
    }

    /// `ServerMessage::DirectTerminalKeyboardProtocol`, mirroring herdr
    /// `set_direct_host_keyboard_protocol`: one kitty stack entry, plus modifyOtherKeys.
    pub fn keyboard_protocol(
        &mut self,
        flags: u16,
        modify_other_keys_level: u8,
    ) -> Option<AttachEvent> {
        let next = KeyboardMode {
            kitty_flags: flags,
            modify_other_keys_level,
        };
        if next == self.keyboard {
            return None;
        }
        let mut sequence = Vec::new();
        if self.keyboard.kitty_flags != next.kitty_flags {
            if self.keyboard.kitty_flags != 0 {
                sequence.extend_from_slice(b"\x1b[<1u");
            }
            if next.kitty_flags != 0 {
                sequence.extend_from_slice(format!("\x1b[>{}u", next.kitty_flags).as_bytes());
            }
        }
        if self.keyboard.modify_other_keys_level != next.modify_other_keys_level {
            sequence
                .extend_from_slice(format!("\x1b[>4;{}m", next.modify_other_keys_level).as_bytes());
        }
        self.keyboard = next;
        Some(self.event(sequence))
    }

    fn event(&self, sequence: Vec<u8>) -> AttachEvent {
        AttachEvent::ModeChange {
            mouse: self.mouse,
            keyboard: self.keyboard,
            sequence,
        }
    }

    /// Undo only modes this attach pushed, before the emulator receives Closed.
    fn restore(&mut self) -> Option<AttachEvent> {
        let mut sequence = Vec::new();
        if let Some(AttachEvent::ModeChange {
            sequence: bytes, ..
        }) = self.keyboard_protocol(0, 0)
        {
            sequence.extend_from_slice(&bytes);
        }
        if let Some(AttachEvent::ModeChange {
            sequence: bytes, ..
        }) = self.mouse_capture(false, false)
        {
            sequence.extend_from_slice(&bytes);
        }
        (!sequence.is_empty()).then(|| self.event(sequence))
    }
}

enum Command {
    Input(Vec<u8>),
    Resize {
        cols: u16,
        rows: u16,
    },
    Scroll {
        direction: AttachScrollDirection,
        lines: u16,
    },
    HostTheme(Vec<crate::wire::ClientHostThemeUpdate>),
    TakeControl,
    Detach,
}

/// A live direct-terminal attach. Dropping it detaches.
pub struct AttachClient {
    handle: AttachHandle,
    events: Option<Receiver<AttachEvent>>,
}

impl AttachClient {
    /// Connects to herdr's client socket, negotiates `TerminalHello`, and attaches to or
    /// observes `terminal_id`. Fails if the handshake is rejected.
    pub fn connect(
        endpoint: &Endpoint,
        terminal_id: &str,
        cols: u16,
        rows: u16,
        mode: AttachMode,
    ) -> io::Result<Self> {
        let request = match mode {
            AttachMode::Attach | AttachMode::Takeover => ClientMessage::AttachTerminal {
                terminal_id: terminal_id.to_owned(),
                takeover: mode == AttachMode::Takeover,
            },
            AttachMode::Observe => ClientMessage::ObserveTerminal {
                target: terminal_id.to_owned(),
            },
        };
        let session = Session::open(endpoint, cols, rows, &request)?;
        let (command_tx, command_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();
        let writable = Arc::new(AtomicBool::new(mode != AttachMode::Observe));
        let worker = Worker {
            endpoint: endpoint.clone(),
            terminal_id: terminal_id.to_owned(),
            cols,
            rows,
            session,
            commands: command_rx,
            events: event_tx,
            writable: Arc::clone(&writable),
            modes: ModeTracker::default(),
            host_theme: Vec::new(),
        };
        std::thread::Builder::new()
            .name("herdr-shell-attach".into())
            .spawn(move || worker.run())?;
        Ok(Self {
            handle: AttachHandle {
                commands: command_tx,
                writable,
            },
            events: Some(event_rx),
        })
    }

    pub fn into_parts(mut self) -> (AttachHandle, Receiver<AttachEvent>) {
        // The receiver exists until the consuming split; dropping the client now must not detach.
        let events = self.events.take().expect("unsplit attach receiver");
        (self.handle.clone(), events)
    }

    pub fn send_input(&self, data: &[u8]) -> io::Result<()> {
        self.handle.send_input(data)
    }
    pub fn resize(&self, cols: u16, rows: u16) -> io::Result<()> {
        self.handle.resize(cols, rows)
    }
    pub fn scroll(&self, direction: AttachScrollDirection, lines: u16) -> io::Result<()> {
        self.handle.scroll(direction, lines)
    }
    pub fn take_control(&self) -> io::Result<()> {
        self.handle.take_control()
    }
    pub fn detach(&self) -> io::Result<()> {
        self.handle.detach()
    }
    pub fn is_writable(&self) -> bool {
        self.handle.is_writable()
    }
    pub fn try_recv(&self) -> Option<AttachEvent> {
        match self.events.as_ref()?.try_recv() {
            Ok(event) => Some(event),
            Err(TryRecvError::Empty | TryRecvError::Disconnected) => None,
        }
    }

    /// Waits up to `timeout` for the next event. `None` on timeout or after `Closed`.
    pub fn recv_timeout(&self, timeout: Duration) -> Option<AttachEvent> {
        match self.events.as_ref()?.recv_timeout(timeout) {
            Ok(event) => Some(event),
            Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => None,
        }
    }
}

impl Drop for AttachClient {
    fn drop(&mut self) {
        if self.events.is_some() {
            let _ = self.handle.detach();
        }
    }
}

/// Command half of an attach; never holds a receive lock. Last sender drop ends the worker.
#[derive(Clone)]
pub struct AttachHandle {
    commands: Sender<Command>,
    writable: Arc<AtomicBool>,
}
impl AttachHandle {
    /// Sends raw input bytes. Refused while observing.
    pub fn send_input(&self, data: &[u8]) -> io::Result<()> {
        if !self.writable.load(Ordering::Acquire) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "attach is read-only; take control first",
            ));
        }
        self.command(Command::Input(data.to_vec()))
    }

    /// Reports host appearance and colors even while observing. Replayed on takeover.
    pub fn host_theme(&self, updates: Vec<crate::wire::ClientHostThemeUpdate>) -> io::Result<()> {
        self.command(Command::HostTheme(updates))
    }

    pub fn resize(&self, cols: u16, rows: u16) -> io::Result<()> {
        self.command(Command::Resize { cols, rows })
    }

    /// Wheel scroll through herdr's host scrollback (`ClientMessage::AttachScroll`).
    pub fn scroll(&self, direction: AttachScrollDirection, lines: u16) -> io::Result<()> {
        if !self.writable.load(Ordering::Acquire) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "attach is read-only; take control first",
            ));
        }
        self.command(Command::Scroll { direction, lines })
    }

    /// Reconnects with `ControlTerminal { takeover: true }`, replacing this observer (or any
    /// other writer). herdr only accepts a terminal-mode request on a fresh connection.
    pub fn take_control(&self) -> io::Result<()> {
        self.command(Command::TakeControl)
    }

    /// Sends `Detach` and ends the attach; a final `Closed` event follows.
    pub fn detach(&self) -> io::Result<()> {
        self.command(Command::Detach)
    }

    pub fn is_writable(&self) -> bool {
        self.writable.load(Ordering::Acquire)
    }

    fn command(&self, command: Command) -> io::Result<()> {
        self.commands
            .send(command)
            .map_err(|_| io::Error::new(io::ErrorKind::NotConnected, "attach has closed"))
    }
}

struct Session {
    stream: Conn,
    initial: VecDeque<ServerMessage>,
    reader: FrameReader,
    buf: Vec<u8>,
}

impl Session {
    fn open(
        endpoint: &Endpoint,
        cols: u16,
        rows: u16,
        request: &ClientMessage,
    ) -> io::Result<Self> {
        let mut stream = endpoint.connect()?;
        prepare_polled(&mut stream)?;
        let mut session = Self {
            stream,
            initial: VecDeque::new(),
            // TerminalHello does not negotiate graphics, just like Unix direct attach.
            reader: FrameReader::new(MAX_FRAME_SIZE),
            buf: vec![0u8; READ_CHUNK],
        };
        session.send(&ClientMessage::TerminalHello {
            version: PROTOCOL_VERSION,
            cols,
            rows,
            cell_width_px: 0,
            cell_height_px: 0,
            pixel_mouse: false,
        })?;
        let deadline = Instant::now() + HANDSHAKE_TIMEOUT;
        loop {
            match session.next_message()? {
                Some(ServerMessage::Welcome {
                    version,
                    encoding,
                    error,
                }) => {
                    if let Some(error) = error {
                        return Err(io::Error::new(
                            io::ErrorKind::ConnectionRefused,
                            format!(
                                "herdr rejected handshake (server protocol {version}): {error}"
                            ),
                        ));
                    }
                    if encoding != RenderEncoding::TerminalAnsi {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            format!("herdr negotiated {encoding:?}, expected TerminalAnsi"),
                        ));
                    }
                    break;
                }
                Some(_) => continue,
                None if Instant::now() >= deadline => {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "timed out waiting for herdr Welcome",
                    ))
                }
                None => std::thread::sleep(IDLE_POLL),
            }
        }
        session.send(request)?;
        // Welcome only accepts the protocol. The attach itself can still be refused
        // (for example, another client owns the terminal). A first render confirms it.
        let deadline = Instant::now() + HANDSHAKE_TIMEOUT;
        let mut initial = VecDeque::new();
        loop {
            match session.next_message()? {
                Some(ServerMessage::ServerShutdown { reason }) => {
                    return Err(io::Error::new(
                        io::ErrorKind::ConnectionRefused,
                        reason.unwrap_or_else(|| "herdr refused attach".into()),
                    ));
                }
                Some(ServerMessage::ClientShellError { message }) => {
                    return Err(io::Error::new(io::ErrorKind::ConnectionRefused, message));
                }
                Some(message) => {
                    let rendered = matches!(message, ServerMessage::Terminal(_));
                    initial.push_back(message);
                    if rendered {
                        break;
                    }
                }
                None if Instant::now() >= deadline => {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "timed out waiting for initial terminal render",
                    ));
                }
                None => std::thread::sleep(IDLE_POLL),
            }
        }
        session.initial = initial;
        Ok(session)
    }

    fn send(&mut self, message: &ClientMessage) -> io::Result<()> {
        let frame = encode_client_frame(message)?;
        write_all_polled(&mut self.stream, &frame)
    }

    /// Returns a buffered or newly read message; `Ok(None)` when nothing is ready.
    /// EOF surfaces as `UnexpectedEof`.
    fn next_message(&mut self) -> io::Result<Option<ServerMessage>> {
        if let Some(message) = self.initial.pop_front() {
            return Ok(Some(message));
        }
        if let Some(message) = self.buffered_message()? {
            return Ok(Some(message));
        }
        match poll_read(&mut self.stream, &mut self.buf)? {
            ReadPoll::Data(read) => {
                self.reader.push(&self.buf[..read]);
                self.buffered_message()
            }
            ReadPoll::Pending => Ok(None),
            ReadPoll::Closed => Err(io::ErrorKind::UnexpectedEof.into()),
        }
    }

    fn buffered_message(&mut self) -> io::Result<Option<ServerMessage>> {
        loop {
            let payload = self.reader.next_payload();
            // Unix's read_message rejects the length prefix and disconnects immediately.
            // Do not let the generic splitter silently skip an oversized attach frame.
            if self.reader.skipped_frames() != 0 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("frame payload exceeds {MAX_FRAME_SIZE} byte attach limit"),
                ));
            }
            let Some(payload) = payload else {
                return Ok(None);
            };
            match ServerMessage::decode(&payload)? {
                ServerMessage::Unknown { .. } => continue,
                message => return Ok(Some(message)),
            }
        }
    }
}

struct Worker {
    endpoint: Endpoint,
    terminal_id: String,
    cols: u16,
    rows: u16,
    session: Session,
    commands: Receiver<Command>,
    events: Sender<AttachEvent>,
    writable: Arc<AtomicBool>,
    modes: ModeTracker,
    host_theme: Vec<crate::wire::ClientHostThemeUpdate>,
}

enum Flow {
    Continue,
    Stop(String),
}

impl Worker {
    fn run(mut self) {
        let reason = loop {
            match self.drain_commands() {
                Ok(Flow::Continue) => {}
                Ok(Flow::Stop(reason)) => break reason,
                Err(err) => break format!("write failed: {err}"),
            }
            match self.pump_messages() {
                Ok((Flow::Continue, busy)) => {
                    if !busy {
                        std::thread::sleep(IDLE_POLL);
                    }
                }
                Ok((Flow::Stop(reason), _)) => break reason,
                Err(err) if err.kind() == io::ErrorKind::UnexpectedEof => {
                    break "connection closed".to_owned()
                }
                Err(err) => break format!("read failed: {err}"),
            }
        };
        self.writable.store(false, Ordering::Release);
        if let Some(event) = self.modes.restore() {
            let _ = self.events.send(event);
        }
        let _ = self.events.send(AttachEvent::Closed { reason });
    }

    fn drain_commands(&mut self) -> io::Result<Flow> {
        loop {
            let command = match self.commands.try_recv() {
                Ok(command) => command,
                Err(TryRecvError::Empty) => return Ok(Flow::Continue),
                Err(TryRecvError::Disconnected) => {
                    let _ = self.session.send(&ClientMessage::Detach);
                    return Ok(Flow::Stop("client dropped".into()));
                }
            };
            match command {
                Command::Input(data) => {
                    if self.writable.load(Ordering::Acquire) {
                        self.session.send(&ClientMessage::Input { data })?;
                    }
                }
                Command::Resize { cols, rows } => {
                    self.cols = cols;
                    self.rows = rows;
                    self.session.send(&ClientMessage::Resize {
                        cols,
                        rows,
                        cell_width_px: 0,
                        cell_height_px: 0,
                        pixel_mouse: false,
                    })?;
                }
                Command::Scroll { direction, lines } => {
                    if self.writable.load(Ordering::Acquire) {
                        self.session.send(&ClientMessage::AttachScroll {
                            source: AttachScrollSource::Wheel,
                            direction,
                            lines: lines.max(1),
                            column: None,
                            row: None,
                            modifiers: 0,
                        })?;
                    }
                }
                Command::HostTheme(updates) => {
                    for update in &updates {
                        self.session.send(&ClientMessage::ClientShellHostTheme {
                            update: update.clone(),
                        })?;
                    }
                    self.host_theme = updates;
                }
                Command::TakeControl => {
                    let request = ClientMessage::ControlTerminal {
                        target: self.terminal_id.clone(),
                        takeover: true,
                    };
                    match Session::open(&self.endpoint, self.cols, self.rows, &request) {
                        Ok(session) => {
                            let mut old = std::mem::replace(&mut self.session, session);
                            let _ = old.send(&ClientMessage::Detach);
                            for update in &self.host_theme {
                                self.session.send(&ClientMessage::ClientShellHostTheme {
                                    update: update.clone(),
                                })?;
                            }
                            self.writable.store(true, Ordering::Release);
                        }
                        Err(err) => {
                            let _ = self.events.send(AttachEvent::Notice {
                                message: format!("take control failed: {err}"),
                            });
                        }
                    }
                }
                Command::Detach => {
                    let _ = self.session.send(&ClientMessage::Detach);
                    return Ok(Flow::Stop("detached".into()));
                }
            }
        }
    }

    /// Handles every message that is ready. Returns whether any arrived.
    fn pump_messages(&mut self) -> io::Result<(Flow, bool)> {
        let mut busy = false;
        while let Some(message) = self.session.next_message()? {
            busy = true;
            let event = match message {
                ServerMessage::Terminal(frame) if frame.bytes.is_empty() => None,
                ServerMessage::Terminal(frame) => Some(AttachEvent::Bytes(frame.bytes)),
                ServerMessage::MouseCapture {
                    enabled,
                    sgr_pixels,
                } => self.modes.mouse_capture(enabled, sgr_pixels),
                ServerMessage::DirectTerminalKeyboardProtocol {
                    flags,
                    modify_other_keys_level,
                } => self.modes.keyboard_protocol(flags, modify_other_keys_level),
                ServerMessage::TerminalBell { count } => Some(AttachEvent::Bell { count }),
                ServerMessage::Clipboard { data } => Some(AttachEvent::Clipboard { data }),
                ServerMessage::ClientShellError { message } => {
                    Some(AttachEvent::Notice { message })
                }
                ServerMessage::ServerShutdown { reason } => {
                    return Ok((
                        Flow::Stop(reason.unwrap_or_else(|| "server shut down".into())),
                        true,
                    ))
                }
                ServerMessage::Welcome { .. } | ServerMessage::Unknown { .. } => None,
            };
            if let Some(event) = event {
                if self.events.send(event).is_err() {
                    let _ = self.session.send(&ClientMessage::Detach);
                    return Ok((Flow::Stop("client dropped".into()), true));
                }
            }
        }
        Ok((Flow::Continue, busy))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mode transitions are a small state machine the emulator depends on; the expected
    /// bytes are herdr's own `terminal_modes.rs` test vectors, not this renderer's output.
    #[test]
    fn mode_tracker_matches_herdr_direct_attach_sequences() {
        let mut modes = ModeTracker::default();
        let sequences: Vec<Vec<u8>> = [(3, 0), (15, 2), (0, 0)]
            .into_iter()
            .filter_map(|(flags, level)| modes.keyboard_protocol(flags, level))
            .map(|event| match event {
                AttachEvent::ModeChange { sequence, .. } => sequence,
                other => panic!("unexpected {other:?}"),
            })
            .collect();
        assert_eq!(
            sequences.concat(),
            b"\x1b[>3u\x1b[<1u\x1b[>15u\x1b[>4;2m\x1b[<1u\x1b[>4;0m".to_vec()
        );
        assert!(modes.keyboard_protocol(0, 0).is_none());

        let enable = modes.mouse_capture(true, true).expect("mouse on");
        let AttachEvent::ModeChange {
            sequence, mouse, ..
        } = enable
        else {
            panic!("expected mode change");
        };
        assert!(mouse.enabled && mouse.sgr_pixels);
        assert_eq!(
            sequence,
            b"\x1b[?1006l\x1b[?1016l\x1b[?1015l\x1b[?1005l\x1b[?1003l\x1b[?1002l\x1b[?1000l\x1b[?9l\x1b[?1000h\x1b[?1002h\x1b[?1003h\x1b[?1015h\x1b[?1006h\x1b[?1016h".to_vec()
        );
        assert!(modes.mouse_capture(true, true).is_none());
        let AttachEvent::ModeChange { sequence, .. } =
            modes.mouse_capture(false, true).expect("mouse off")
        else {
            panic!("expected mode change");
        };
        assert_eq!(
            sequence,
            b"\x1b[?1006l\x1b[?1016l\x1b[?1015l\x1b[?1005l\x1b[?1003l\x1b[?1002l\x1b[?1000l\x1b[?9l\x1b[?1006l\x1b[?1015l\x1b[?1003l\x1b[?1002l\x1b[?1000l".to_vec()
        );
    }
}
