//! Hand mirror of the herdr wire subset used by direct-terminal attach.
//!
//! herdr encodes `ClientMessage` and `ServerMessage` with serde through bincode 2
//! (`bincode::config::standard()`): a varint `u32` variant index followed by the
//! variant's fields in declaration order, inside a `[u32 LE length][payload]` frame.
//! This module writes the variant index explicitly (the `*_TAG` constants below are
//! herdr's variant positions in `src/protocol/wire.rs`) and encodes each payload as a
//! field tuple, which bincode lays out exactly like the struct variant.
//!
//! Only the variants direct attach needs are mirrored. A server frame with any other
//! tag decodes to [`ServerMessage::Unknown`] and callers skip it. The golden fixtures
//! in `tests/fixtures` are written by herdr's own types
//! (`src/protocol/shell_core_goldens.rs`), so drift on either side fails a test.

use std::fmt;
use std::io::{self, Write};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

/// herdr `PROTOCOL_VERSION`; direct-terminal hello must match it exactly.
pub const PROTOCOL_VERSION: u32 = 22;
/// herdr `MAX_FRAME_SIZE` for client-to-server frames.
pub const MAX_FRAME_SIZE: usize = 2 * 1024 * 1024;
/// herdr `MAX_GRAPHICS_FRAME_SIZE`; direct attach readers accept frames up to this size.
pub const MAX_GRAPHICS_FRAME_SIZE: usize = 32 * 1024 * 1024;
/// Length of the little-endian frame length prefix.
pub const LENGTH_PREFIX_BYTES: usize = 4;

/// herdr `ClientMessage` variant indices for the mirrored variants.
pub mod client_tag {
    pub const TERMINAL_HELLO: u32 = 0;
    pub const INPUT: u32 = 1;
    pub const RESIZE: u32 = 3;
    pub const DETACH: u32 = 4;
    pub const ATTACH_TERMINAL: u32 = 5;
    pub const ATTACH_SCROLL: u32 = 6;
    pub const OBSERVE_TERMINAL: u32 = 7;
    pub const CONTROL_TERMINAL: u32 = 8;
}

/// herdr `ServerMessage` variant indices for the mirrored variants.
pub mod server_tag {
    pub const WELCOME: u32 = 0;
    pub const TERMINAL: u32 = 1;
    pub const SERVER_SHUTDOWN: u32 = 3;
    pub const MOUSE_CAPTURE: u32 = 8;
    pub const TERMINAL_BELL: u32 = 9;
    pub const CLIENT_SHELL_ERROR: u32 = 15;
    pub const DIRECT_TERMINAL_KEYBOARD_PROTOCOL: u32 = 16;
}

/// Full mirror of herdr `RenderEncoding` (both variants, same order).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RenderEncoding {
    SemanticFrame,
    TerminalAnsi,
}

/// Full mirror of herdr `AttachScrollDirection`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AttachScrollDirection {
    Up,
    Down,
}

/// Full mirror of herdr `AttachScrollSource`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AttachScrollSource {
    Wheel,
    PageKey { input: Vec<u8> },
}

/// Mirror of herdr `TerminalFrame`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalFrame {
    pub seq: u64,
    pub width: u16,
    pub height: u16,
    pub full: bool,
    pub bytes: Vec<u8>,
}

/// Client-to-server messages used by direct-terminal attach.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientMessage {
    TerminalHello {
        version: u32,
        cols: u16,
        rows: u16,
        cell_width_px: u32,
        cell_height_px: u32,
        pixel_mouse: bool,
    },
    Input {
        data: Vec<u8>,
    },
    Resize {
        cols: u16,
        rows: u16,
        cell_width_px: u32,
        cell_height_px: u32,
        pixel_mouse: bool,
    },
    Detach,
    AttachTerminal {
        terminal_id: String,
        takeover: bool,
    },
    AttachScroll {
        source: AttachScrollSource,
        direction: AttachScrollDirection,
        lines: u16,
        column: Option<u16>,
        row: Option<u16>,
        modifiers: u8,
    },
    ObserveTerminal {
        target: String,
    },
    ControlTerminal {
        target: String,
        takeover: bool,
    },
}

/// Server-to-client messages a direct-terminal attach client acts on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerMessage {
    Welcome {
        version: u32,
        encoding: RenderEncoding,
        error: Option<String>,
    },
    Terminal(TerminalFrame),
    ServerShutdown {
        reason: Option<String>,
    },
    MouseCapture {
        enabled: bool,
        sgr_pixels: bool,
    },
    TerminalBell {
        count: u16,
    },
    ClientShellError {
        message: String,
    },
    DirectTerminalKeyboardProtocol {
        flags: u16,
        modify_other_keys_level: u8,
    },
    /// Any variant this mirror does not model. The frame was consumed by length.
    Unknown {
        tag: u32,
    },
}

#[derive(Debug)]
pub enum WireError {
    Encode(String),
    Decode(String),
    /// A known variant decoded without consuming the whole payload.
    TrailingBytes {
        tag: u32,
        consumed: usize,
        len: usize,
    },
    /// A client message tag this mirror does not model.
    UnknownClientTag(u32),
    /// `ServerMessage::Unknown` cannot be encoded.
    NotEncodable,
    PayloadTooLarge(usize),
}

impl fmt::Display for WireError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WireError::Encode(e) => write!(f, "encode failed: {e}"),
            WireError::Decode(e) => write!(f, "decode failed: {e}"),
            WireError::TrailingBytes { tag, consumed, len } => write!(
                f,
                "message tag {tag} decoded {consumed} of {len} payload bytes"
            ),
            WireError::UnknownClientTag(tag) => write!(f, "unknown client message tag {tag}"),
            WireError::NotEncodable => write!(f, "unknown server message cannot be encoded"),
            WireError::PayloadTooLarge(len) => write!(f, "payload of {len} bytes exceeds u32"),
        }
    }
}

impl std::error::Error for WireError {}

impl From<WireError> for io::Error {
    fn from(error: WireError) -> Self {
        io::Error::new(io::ErrorKind::InvalidData, error)
    }
}

fn config() -> bincode::config::Configuration {
    bincode::config::standard()
}

fn encode_variant<T: Serialize>(tag: u32, payload: &T) -> Result<Vec<u8>, WireError> {
    let mut out =
        bincode::encode_to_vec(tag, config()).map_err(|e| WireError::Encode(e.to_string()))?;
    let body = bincode::serde::encode_to_vec(payload, config())
        .map_err(|e| WireError::Encode(e.to_string()))?;
    out.extend_from_slice(&body);
    Ok(out)
}

fn decode_tag(payload: &[u8]) -> Result<(u32, usize), WireError> {
    bincode::decode_from_slice::<u32, _>(payload, config())
        .map_err(|e| WireError::Decode(e.to_string()))
}

fn decode_body<T: DeserializeOwned>(bytes: &[u8]) -> Result<(T, usize), WireError> {
    bincode::serde::decode_from_slice::<T, _>(bytes, config())
        .map_err(|e| WireError::Decode(e.to_string()))
}

fn ensure_consumed(tag: u32, consumed: usize, len: usize) -> Result<(), WireError> {
    if consumed == len {
        Ok(())
    } else {
        Err(WireError::TrailingBytes { tag, consumed, len })
    }
}

impl ClientMessage {
    /// herdr variant index of this message.
    pub fn tag(&self) -> u32 {
        match self {
            ClientMessage::TerminalHello { .. } => client_tag::TERMINAL_HELLO,
            ClientMessage::Input { .. } => client_tag::INPUT,
            ClientMessage::Resize { .. } => client_tag::RESIZE,
            ClientMessage::Detach => client_tag::DETACH,
            ClientMessage::AttachTerminal { .. } => client_tag::ATTACH_TERMINAL,
            ClientMessage::AttachScroll { .. } => client_tag::ATTACH_SCROLL,
            ClientMessage::ObserveTerminal { .. } => client_tag::OBSERVE_TERMINAL,
            ClientMessage::ControlTerminal { .. } => client_tag::CONTROL_TERMINAL,
        }
    }

    /// Encodes the bincode payload (without the length prefix).
    pub fn encode(&self) -> Result<Vec<u8>, WireError> {
        let tag = self.tag();
        match self {
            ClientMessage::TerminalHello {
                version,
                cols,
                rows,
                cell_width_px,
                cell_height_px,
                pixel_mouse,
            } => encode_variant(
                tag,
                &(
                    version,
                    cols,
                    rows,
                    cell_width_px,
                    cell_height_px,
                    pixel_mouse,
                ),
            ),
            ClientMessage::Input { data } => encode_variant(tag, &(data,)),
            ClientMessage::Resize {
                cols,
                rows,
                cell_width_px,
                cell_height_px,
                pixel_mouse,
            } => encode_variant(
                tag,
                &(cols, rows, cell_width_px, cell_height_px, pixel_mouse),
            ),
            ClientMessage::Detach => encode_variant(tag, &()),
            ClientMessage::AttachTerminal {
                terminal_id,
                takeover,
            } => encode_variant(tag, &(terminal_id, takeover)),
            ClientMessage::AttachScroll {
                source,
                direction,
                lines,
                column,
                row,
                modifiers,
            } => encode_variant(tag, &(source, direction, lines, column, row, modifiers)),
            ClientMessage::ObserveTerminal { target } => encode_variant(tag, &(target,)),
            ClientMessage::ControlTerminal { target, takeover } => {
                encode_variant(tag, &(target, takeover))
            }
        }
    }

    /// Decodes a payload. Used by tests and fake servers; unknown tags are errors here.
    pub fn decode(payload: &[u8]) -> Result<Self, WireError> {
        let (tag, head) = decode_tag(payload)?;
        let body = &payload[head..];
        let (message, used) = match tag {
            client_tag::TERMINAL_HELLO => {
                let ((version, cols, rows, cell_width_px, cell_height_px, pixel_mouse), used) =
                    decode_body::<(u32, u16, u16, u32, u32, bool)>(body)?;
                (
                    ClientMessage::TerminalHello {
                        version,
                        cols,
                        rows,
                        cell_width_px,
                        cell_height_px,
                        pixel_mouse,
                    },
                    used,
                )
            }
            client_tag::INPUT => {
                let ((data,), used) = decode_body::<(Vec<u8>,)>(body)?;
                (ClientMessage::Input { data }, used)
            }
            client_tag::RESIZE => {
                let ((cols, rows, cell_width_px, cell_height_px, pixel_mouse), used) =
                    decode_body::<(u16, u16, u32, u32, bool)>(body)?;
                (
                    ClientMessage::Resize {
                        cols,
                        rows,
                        cell_width_px,
                        cell_height_px,
                        pixel_mouse,
                    },
                    used,
                )
            }
            client_tag::DETACH => (ClientMessage::Detach, 0),
            client_tag::ATTACH_TERMINAL => {
                let ((terminal_id, takeover), used) = decode_body::<(String, bool)>(body)?;
                (
                    ClientMessage::AttachTerminal {
                        terminal_id,
                        takeover,
                    },
                    used,
                )
            }
            client_tag::ATTACH_SCROLL => {
                let ((source, direction, lines, column, row, modifiers), used) =
                    decode_body::<(
                        AttachScrollSource,
                        AttachScrollDirection,
                        u16,
                        Option<u16>,
                        Option<u16>,
                        u8,
                    )>(body)?;
                (
                    ClientMessage::AttachScroll {
                        source,
                        direction,
                        lines,
                        column,
                        row,
                        modifiers,
                    },
                    used,
                )
            }
            client_tag::OBSERVE_TERMINAL => {
                let ((target,), used) = decode_body::<(String,)>(body)?;
                (ClientMessage::ObserveTerminal { target }, used)
            }
            client_tag::CONTROL_TERMINAL => {
                let ((target, takeover), used) = decode_body::<(String, bool)>(body)?;
                (ClientMessage::ControlTerminal { target, takeover }, used)
            }
            other => return Err(WireError::UnknownClientTag(other)),
        };
        ensure_consumed(tag, head + used, payload.len())?;
        Ok(message)
    }
}

impl ServerMessage {
    /// herdr variant index, or the unknown tag that was skipped.
    pub fn tag(&self) -> u32 {
        match self {
            ServerMessage::Welcome { .. } => server_tag::WELCOME,
            ServerMessage::Terminal(_) => server_tag::TERMINAL,
            ServerMessage::ServerShutdown { .. } => server_tag::SERVER_SHUTDOWN,
            ServerMessage::MouseCapture { .. } => server_tag::MOUSE_CAPTURE,
            ServerMessage::TerminalBell { .. } => server_tag::TERMINAL_BELL,
            ServerMessage::ClientShellError { .. } => server_tag::CLIENT_SHELL_ERROR,
            ServerMessage::DirectTerminalKeyboardProtocol { .. } => {
                server_tag::DIRECT_TERMINAL_KEYBOARD_PROTOCOL
            }
            ServerMessage::Unknown { tag } => *tag,
        }
    }

    /// Encodes the bincode payload (without the length prefix). Used by tests and fake servers.
    pub fn encode(&self) -> Result<Vec<u8>, WireError> {
        let tag = self.tag();
        match self {
            ServerMessage::Welcome {
                version,
                encoding,
                error,
            } => encode_variant(tag, &(version, encoding, error)),
            ServerMessage::Terminal(frame) => encode_variant(tag, frame),
            ServerMessage::ServerShutdown { reason } => encode_variant(tag, &(reason,)),
            ServerMessage::MouseCapture {
                enabled,
                sgr_pixels,
            } => encode_variant(tag, &(enabled, sgr_pixels)),
            ServerMessage::TerminalBell { count } => encode_variant(tag, &(count,)),
            ServerMessage::ClientShellError { message } => encode_variant(tag, &(message,)),
            ServerMessage::DirectTerminalKeyboardProtocol {
                flags,
                modify_other_keys_level,
            } => encode_variant(tag, &(flags, modify_other_keys_level)),
            ServerMessage::Unknown { .. } => Err(WireError::NotEncodable),
        }
    }

    /// Decodes a payload. Unmodelled tags return `Unknown { tag }` and are never fatal.
    pub fn decode(payload: &[u8]) -> Result<Self, WireError> {
        let (tag, head) = decode_tag(payload)?;
        let body = &payload[head..];
        let (message, used) = match tag {
            server_tag::WELCOME => {
                let ((version, encoding, error), used) =
                    decode_body::<(u32, RenderEncoding, Option<String>)>(body)?;
                (
                    ServerMessage::Welcome {
                        version,
                        encoding,
                        error,
                    },
                    used,
                )
            }
            server_tag::TERMINAL => {
                let (frame, used) = decode_body::<TerminalFrame>(body)?;
                (ServerMessage::Terminal(frame), used)
            }
            server_tag::SERVER_SHUTDOWN => {
                let ((reason,), used) = decode_body::<(Option<String>,)>(body)?;
                (ServerMessage::ServerShutdown { reason }, used)
            }
            server_tag::MOUSE_CAPTURE => {
                let ((enabled, sgr_pixels), used) = decode_body::<(bool, bool)>(body)?;
                (
                    ServerMessage::MouseCapture {
                        enabled,
                        sgr_pixels,
                    },
                    used,
                )
            }
            server_tag::TERMINAL_BELL => {
                let ((count,), used) = decode_body::<(u16,)>(body)?;
                (ServerMessage::TerminalBell { count }, used)
            }
            server_tag::CLIENT_SHELL_ERROR => {
                let ((message,), used) = decode_body::<(String,)>(body)?;
                (ServerMessage::ClientShellError { message }, used)
            }
            server_tag::DIRECT_TERMINAL_KEYBOARD_PROTOCOL => {
                let ((flags, modify_other_keys_level), used) = decode_body::<(u16, u8)>(body)?;
                (
                    ServerMessage::DirectTerminalKeyboardProtocol {
                        flags,
                        modify_other_keys_level,
                    },
                    used,
                )
            }
            other => return Ok(ServerMessage::Unknown { tag: other }),
        };
        ensure_consumed(tag, head + used, payload.len())?;
        Ok(message)
    }
}

/// Prefixes a payload with its `u32` little-endian length.
pub fn frame(payload: &[u8]) -> Result<Vec<u8>, WireError> {
    let len =
        u32::try_from(payload.len()).map_err(|_| WireError::PayloadTooLarge(payload.len()))?;
    let mut out = Vec::with_capacity(LENGTH_PREFIX_BYTES + payload.len());
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(payload);
    Ok(out)
}

/// Encodes and frames one client message.
pub fn encode_client_frame(message: &ClientMessage) -> Result<Vec<u8>, WireError> {
    frame(&message.encode()?)
}

/// Writes one framed client message and flushes.
pub fn write_client_message<W: Write>(writer: &mut W, message: &ClientMessage) -> io::Result<()> {
    writer.write_all(&encode_client_frame(message)?)?;
    writer.flush()
}

/// Incremental frame splitter. Frames above `max_frame_size` are discarded by
/// length as their bytes arrive, so an oversized or unknown frame is never fatal.
#[derive(Debug)]
pub struct FrameReader {
    buffer: Vec<u8>,
    skip_remaining: usize,
    max_frame_size: usize,
    skipped_frames: u64,
}

impl FrameReader {
    pub fn new(max_frame_size: usize) -> Self {
        Self {
            buffer: Vec::new(),
            skip_remaining: 0,
            max_frame_size,
            skipped_frames: 0,
        }
    }

    /// Appends bytes read from the stream.
    pub fn push(&mut self, mut bytes: &[u8]) {
        if self.skip_remaining > 0 {
            let skip = self.skip_remaining.min(bytes.len());
            self.skip_remaining -= skip;
            bytes = &bytes[skip..];
        }
        self.buffer.extend_from_slice(bytes);
    }

    /// Returns the next complete payload, if one is buffered.
    pub fn next_payload(&mut self) -> Option<Vec<u8>> {
        loop {
            if self.skip_remaining > 0 || self.buffer.len() < LENGTH_PREFIX_BYTES {
                return None;
            }
            let mut prefix = [0u8; LENGTH_PREFIX_BYTES];
            prefix.copy_from_slice(&self.buffer[..LENGTH_PREFIX_BYTES]);
            let len = u32::from_le_bytes(prefix) as usize;
            if len > self.max_frame_size {
                self.skipped_frames += 1;
                let available = self.buffer.len() - LENGTH_PREFIX_BYTES;
                if available >= len {
                    self.buffer.drain(..LENGTH_PREFIX_BYTES + len);
                } else {
                    self.skip_remaining = len - available;
                    self.buffer.clear();
                }
                continue;
            }
            if self.buffer.len() < LENGTH_PREFIX_BYTES + len {
                return None;
            }
            let payload = self.buffer[LENGTH_PREFIX_BYTES..LENGTH_PREFIX_BYTES + len].to_vec();
            self.buffer.drain(..LENGTH_PREFIX_BYTES + len);
            return Some(payload);
        }
    }

    /// Number of frames dropped because they exceeded the size cap.
    pub fn skipped_frames(&self) -> u64 {
        self.skipped_frames
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parser edge cases (split prefixes, oversized frames spanning reads) that the
    /// live attach check cannot reach deterministically.
    #[test]
    fn frame_reader_reassembles_split_frames_and_skips_oversized() {
        let small = frame(&ServerMessage::TerminalBell { count: 2 }.encode().unwrap()).unwrap();
        let mut oversized = 64u32.to_le_bytes().to_vec();
        oversized.extend(std::iter::repeat_n(0xAA, 64));
        let mut stream = oversized.clone();
        stream.extend_from_slice(&small);
        stream.extend_from_slice(&small);

        let mut reader = FrameReader::new(16);
        let mut payloads = Vec::new();
        for chunk in stream.chunks(3) {
            reader.push(chunk);
            while let Some(payload) = reader.next_payload() {
                payloads.push(ServerMessage::decode(&payload).unwrap());
            }
        }
        assert_eq!(reader.skipped_frames(), 1);
        assert_eq!(
            payloads,
            vec![
                ServerMessage::TerminalBell { count: 2 },
                ServerMessage::TerminalBell { count: 2 }
            ]
        );
    }

    /// herdr rejects trailing bytes after a decoded message; the mirror does too.
    #[test]
    fn known_tag_with_trailing_bytes_is_an_error() {
        let mut payload = ServerMessage::TerminalBell { count: 1 }.encode().unwrap();
        payload.push(0);
        assert!(matches!(
            ServerMessage::decode(&payload),
            Err(WireError::TrailingBytes { tag: 9, .. })
        ));
    }
}
