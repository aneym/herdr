//! Golden encodings for the Windows Shell's hand-mirrored wire subset.
//!
//! `windows/HerdrShell/core` mirrors the direct-terminal attach messages by hand. This
//! test encodes each mirrored message with herdr's real types and compares the bytes to
//! `windows/HerdrShell/core/tests/fixtures/*.bin`; the core crate's `tests/goldens.rs`
//! decodes and re-encodes the same files with the mirror. A wire change on either side
//! fails one of the two. After an intended herdr change, rerun this test with
//! `HERDR_BLESS_SHELL_CORE_GOLDENS=1`, then update the mirror until its test passes.

use std::path::PathBuf;

use super::{
    AttachScrollDirection, AttachScrollSource, ClientMessage, NotifyKind, RenderEncoding,
    ServerMessage, TerminalFrame, PROTOCOL_VERSION,
};

const BLESS_ENV: &str = "HERDR_BLESS_SHELL_CORE_GOLDENS";

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("windows/HerdrShell/core/tests/fixtures")
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
            "server_clipboard",
            ServerMessage::Clipboard {
                data: "Y29waWVkIOKAlCB0ZXh0".into(),
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
        // Variants the mirror does not model: the Shell must skip them by length.
        (
            "server_unknown_graphics",
            ServerMessage::Graphics {
                bytes: b"\x1b_Gf=100;\x1b\\".to_vec(),
            },
        ),
        (
            "server_unknown_notify",
            ServerMessage::Notify {
                kind: NotifyKind::Toast,
                message: "done".into(),
                body: Some("ok".into()),
            },
        ),
        (
            "server_unknown_endpoint_control",
            ServerMessage::EndpointControl {
                kind: "endpoint.health.ping.v1".into(),
                data: "{}".into(),
            },
        ),
    ]
}

fn encode<M: serde::Serialize>(message: &M) -> Vec<u8> {
    bincode::serde::encode_to_vec(message, bincode::config::standard()).expect("encode golden")
}

fn decode<M: serde::de::DeserializeOwned>(bytes: &[u8]) -> M {
    let (message, consumed) = bincode::serde::decode_from_slice(bytes, bincode::config::standard())
        .expect("decode golden");
    assert_eq!(consumed, bytes.len(), "golden has trailing bytes");
    message
}

#[test]
fn shell_core_goldens_match_herdr_encoding() {
    let dir = fixtures_dir();
    let bless = std::env::var_os(BLESS_ENV).is_some();
    let mut cases: Vec<(&str, Vec<u8>)> = Vec::new();
    for (name, message) in client_goldens() {
        let bytes = encode(&message);
        assert_eq!(
            decode::<ClientMessage>(&bytes),
            message,
            "{name} round trip"
        );
        cases.push((name, bytes));
    }
    for (name, message) in server_goldens() {
        let bytes = encode(&message);
        assert_eq!(
            decode::<ServerMessage>(&bytes),
            message,
            "{name} round trip"
        );
        cases.push((name, bytes));
    }

    if bless {
        std::fs::create_dir_all(&dir).expect("create fixtures dir");
        for (name, bytes) in &cases {
            std::fs::write(dir.join(format!("{name}.bin")), bytes).expect("write golden");
        }
    }

    let mut failures = Vec::new();
    for (name, bytes) in &cases {
        match std::fs::read(dir.join(format!("{name}.bin"))) {
            Ok(existing) if existing == *bytes => {}
            Ok(existing) => failures.push(format!(
                "{name}: fixture {existing:02x?} != herdr {bytes:02x?}"
            )),
            Err(err) => failures.push(format!("{name}: {err}")),
        }
    }
    let known: Vec<String> = cases
        .iter()
        .map(|(name, _)| format!("{name}.bin"))
        .collect();
    for entry in std::fs::read_dir(&dir).expect("read fixtures dir") {
        let file = entry.expect("fixture entry").file_name();
        let file = file.to_string_lossy();
        if file.ends_with(".bin") && !known.iter().any(|name| *name == file) {
            failures.push(format!("{file}: fixture with no herdr golden"));
        }
    }
    assert!(
        failures.is_empty(),
        "herdr wire drifted from the Shell mirror goldens; rerun with {BLESS_ENV}=1 and update \
         windows/HerdrShell/core/src/wire.rs:\n{}",
        failures.join("\n")
    );
}
