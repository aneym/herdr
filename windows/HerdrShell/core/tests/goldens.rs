//! The mirror against herdr's own encodings.
//!
//! `tests/fixtures/*.bin` are written by herdr's `src/protocol/shell_core_goldens.rs` from
//! herdr's real `ClientMessage` / `ServerMessage` types with the same fixed values used
//! here. Client messages must encode to the fixture bytes; server messages must decode
//! from them (and re-encode identically); unmodelled server variants must decode to
//! `Unknown` with herdr's tag.

use std::path::PathBuf;

use herdr_shell_core::wire::{
    AttachScrollDirection, AttachScrollSource, ClientMessage, FrameReader, RenderEncoding,
    ServerMessage, TerminalFrame, MAX_FRAME_SIZE, PROTOCOL_VERSION,
};

fn fixture(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(format!("{name}.bin"));
    std::fs::read(&path).unwrap_or_else(|err| panic!("{}: {err}", path.display()))
}

fn client_goldens() -> Vec<(&'static str, ClientMessage)> {
    vec![
        (
            "client_terminal_hello",
            ClientMessage::TerminalHello {
                version: PROTOCOL_VERSION,
                cols: 300,
                rows: 80,
                cell_width_px: 1000,
                cell_height_px: 20,
                pixel_mouse: true,
            },
        ),
        (
            "client_input",
            ClientMessage::Input {
                data: b"ls -la\r\x1b[A".to_vec(),
            },
        ),
        (
            "client_resize",
            ClientMessage::Resize {
                cols: 132,
                rows: 300,
                cell_width_px: 0,
                cell_height_px: 0,
                pixel_mouse: false,
            },
        ),
        ("client_detach", ClientMessage::Detach),
        (
            "client_attach_terminal",
            ClientMessage::AttachTerminal {
                terminal_id: "term-7f3a".into(),
                takeover: false,
            },
        ),
        (
            "client_attach_scroll_wheel",
            ClientMessage::AttachScroll {
                source: AttachScrollSource::Wheel,
                direction: AttachScrollDirection::Up,
                lines: 3,
                column: Some(10),
                row: Some(300),
                modifiers: 4,
            },
        ),
        (
            "client_attach_scroll_page_key",
            ClientMessage::AttachScroll {
                source: AttachScrollSource::PageKey {
                    input: b"\x1b[6~".to_vec(),
                },
                direction: AttachScrollDirection::Down,
                lines: 39,
                column: None,
                row: None,
                modifiers: 0,
            },
        ),
        (
            "client_observe_terminal",
            ClientMessage::ObserveTerminal {
                target: "w1:p2".into(),
            },
        ),
        (
            "client_control_terminal",
            ClientMessage::ControlTerminal {
                target: "term-7f3a".into(),
                takeover: true,
            },
        ),
    ]
}

fn server_goldens() -> Vec<(&'static str, ServerMessage)> {
    vec![
        (
            "server_welcome",
            ServerMessage::Welcome {
                version: PROTOCOL_VERSION,
                encoding: RenderEncoding::TerminalAnsi,
                error: None,
            },
        ),
        (
            "server_welcome_rejected",
            ServerMessage::Welcome {
                version: PROTOCOL_VERSION,
                encoding: RenderEncoding::TerminalAnsi,
                error: Some("version mismatch".into()),
            },
        ),
        (
            "server_terminal",
            ServerMessage::Terminal(TerminalFrame {
                seq: 70_000,
                width: 120,
                height: 40,
                full: true,
                bytes: b"\x1b[H\x1b[2Jhello\r\n".to_vec(),
            }),
        ),
        (
            "server_shutdown",
            ServerMessage::ServerShutdown {
                reason: Some("terminal attach taken over".into()),
            },
        ),
        (
            "server_shutdown_bare",
            ServerMessage::ServerShutdown { reason: None },
        ),
        (
            "server_mouse_capture",
            ServerMessage::MouseCapture {
                enabled: true,
                sgr_pixels: true,
            },
        ),
        (
            "server_terminal_bell",
            ServerMessage::TerminalBell { count: 2 },
        ),
        (
            "server_client_shell_error",
            ServerMessage::ClientShellError {
                message: "endpoint failed".into(),
            },
        ),
        (
            "server_direct_keyboard_protocol",
            ServerMessage::DirectTerminalKeyboardProtocol {
                flags: 31,
                modify_other_keys_level: 2,
            },
        ),
    ]
}

/// herdr variants the mirror skips, with herdr's variant index.
const UNKNOWN_SERVER_GOLDENS: &[(&str, u32)] = &[
    ("server_unknown_graphics", 2),
    ("server_unknown_notify", 4),
    ("server_unknown_endpoint_control", 20),
];

#[test]
fn client_messages_encode_to_herdr_bytes() {
    for (name, message) in client_goldens() {
        let golden = fixture(name);
        assert_eq!(message.encode().unwrap(), golden, "{name} encode");
        assert_eq!(
            ClientMessage::decode(&golden).unwrap(),
            message,
            "{name} decode"
        );
    }
}

#[test]
fn server_messages_decode_from_herdr_bytes() {
    for (name, message) in server_goldens() {
        let golden = fixture(name);
        assert_eq!(
            ServerMessage::decode(&golden).unwrap(),
            message,
            "{name} decode"
        );
        assert_eq!(message.encode().unwrap(), golden, "{name} encode");
    }
    for (name, tag) in UNKNOWN_SERVER_GOLDENS {
        assert_eq!(
            ServerMessage::decode(&fixture(name)).unwrap(),
            ServerMessage::Unknown { tag: *tag },
            "{name}"
        );
    }
}

/// A framed stream of real herdr frames, unknown ones included, yields every modelled
/// message in order: unknown variants never stop the reader.
#[test]
fn framed_stream_with_unknown_variants_is_not_fatal() {
    let mut stream = Vec::new();
    let mut expected = Vec::new();
    for (name, _) in UNKNOWN_SERVER_GOLDENS {
        stream.extend(herdr_shell_core::wire::frame(&fixture(name)).unwrap());
    }
    for (name, message) in server_goldens() {
        stream.extend(herdr_shell_core::wire::frame(&fixture(name)).unwrap());
        expected.push(message);
    }
    let mut reader = FrameReader::new(MAX_FRAME_SIZE);
    let mut decoded = Vec::new();
    for chunk in stream.chunks(5) {
        reader.push(chunk);
        while let Some(payload) = reader.next_payload() {
            match ServerMessage::decode(&payload).unwrap() {
                ServerMessage::Unknown { .. } => {}
                message => decoded.push(message),
            }
        }
    }
    assert_eq!(decoded, expected);
}

#[test]
fn every_fixture_has_a_mirror_golden() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut known: Vec<String> = client_goldens()
        .iter()
        .map(|(name, _)| name.to_string())
        .chain(server_goldens().iter().map(|(name, _)| name.to_string()))
        .chain(
            UNKNOWN_SERVER_GOLDENS
                .iter()
                .map(|(name, _)| name.to_string()),
        )
        .map(|name| format!("{name}.bin"))
        .collect();
    known.sort();
    let mut present: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".bin"))
        .collect();
    present.sort();
    assert_eq!(present, known);
}
