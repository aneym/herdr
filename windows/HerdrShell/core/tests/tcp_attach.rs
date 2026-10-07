//! Attach lifecycle contracts over real framed TCP, without replacing client internals.
use herdr_shell_core::{
    attach::{AttachClient, AttachEvent, AttachMode, KeyboardMode, MouseMode},
    endpoint::Endpoint,
    wire::{frame, RenderEncoding, ServerMessage, TerminalFrame, PROTOCOL_VERSION},
};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(5);

fn send(stream: &mut TcpStream, message: ServerMessage) {
    stream
        .write_all(&frame(&message.encode().unwrap()).unwrap())
        .unwrap();
}

fn read_client_frame(stream: &mut TcpStream) {
    let mut prefix = [0; 4];
    stream.read_exact(&mut prefix).unwrap();
    let mut payload = vec![0; u32::from_le_bytes(prefix) as usize];
    stream.read_exact(&mut payload).unwrap();
}

fn attach_server(
    after_attach: impl FnOnce(&mut TcpStream) + Send + 'static,
) -> (AttachClient, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream.set_read_timeout(Some(TIMEOUT)).unwrap();
        stream.set_write_timeout(Some(TIMEOUT)).unwrap();
        read_client_frame(&mut stream);
        send(
            &mut stream,
            ServerMessage::Welcome {
                version: PROTOCOL_VERSION,
                encoding: RenderEncoding::TerminalAnsi,
                error: None,
            },
        );
        read_client_frame(&mut stream);
        send(
            &mut stream,
            ServerMessage::Terminal(TerminalFrame {
                seq: 1,
                width: 120,
                height: 40,
                full: true,
                bytes: b"ready".to_vec(),
            }),
        );
        after_attach(&mut stream);
    });
    let client = AttachClient::connect(
        &Endpoint::Tcp(addr),
        "term-test",
        120,
        40,
        AttachMode::Attach,
    )
    .unwrap();
    assert_eq!(
        client.recv_timeout(TIMEOUT),
        Some(AttachEvent::Bytes(b"ready".to_vec()))
    );
    (client, server)
}

/// A pushed keyboard stack and active mouse capture must be undone before Closed.
/// Existing mode-transition coverage never closes a worker or observes event ordering.
#[test]
fn shutdown_restores_pushed_modes_before_closed() {
    let (client, server) = attach_server(|stream| {
        send(
            stream,
            ServerMessage::DirectTerminalKeyboardProtocol {
                flags: 3,
                modify_other_keys_level: 2,
            },
        );
        send(
            stream,
            ServerMessage::MouseCapture {
                enabled: true,
                sgr_pixels: true,
            },
        );
        send(stream, ServerMessage::ServerShutdown { reason: None });
    });
    for _ in 0..2 {
        assert!(matches!(
            client.recv_timeout(TIMEOUT),
            Some(AttachEvent::ModeChange { .. })
        ));
    }
    assert_eq!(client.recv_timeout(TIMEOUT), Some(AttachEvent::ModeChange {
        mouse: MouseMode::default(),
        keyboard: KeyboardMode::default(),
        sequence: b"\x1b[<1u\x1b[>4;0m\x1b[?1006l\x1b[?1016l\x1b[?1015l\x1b[?1005l\x1b[?1003l\x1b[?1002l\x1b[?1000l\x1b[?9l\x1b[?1006l\x1b[?1015l\x1b[?1003l\x1b[?1002l\x1b[?1000l".to_vec(),
    }));
    assert_eq!(
        client.recv_timeout(TIMEOUT),
        Some(AttachEvent::Closed {
            reason: "server shut down".into()
        })
    );
    assert!(!client.is_writable());
    assert_eq!(client.recv_timeout(TIMEOUT), None);
    server.join().unwrap();
}

/// The non-graphics attach rejects a real terminal frame's oversized prefix immediately,
/// without waiting for its body. The generic splitter test does not select the attach cap.
#[test]
fn oversized_terminal_frame_closes_attach_from_prefix() {
    let (client, server) = attach_server(|stream| {
        let payload = ServerMessage::Terminal(TerminalFrame {
            seq: 2,
            width: 120,
            height: 40,
            full: false,
            bytes: vec![b'a'; 2 * 1024 * 1024],
        })
        .encode()
        .unwrap();
        let framed = frame(&payload).unwrap();
        stream.write_all(&framed[..4]).unwrap();
        // Keep the peer open: EOF must not be the reason the test closes.
        let mut byte = [0];
        assert_eq!(stream.read(&mut byte).unwrap(), 0);
    });
    let Some(AttachEvent::Closed { reason }) = client.recv_timeout(TIMEOUT) else {
        panic!("oversized prefix did not close attach");
    };
    assert!(reason.starts_with("read failed:"), "{reason}");
    assert!(reason.contains("frame payload exceeds"), "{reason}");
    assert!(!client.is_writable());
    server.join().unwrap();
}

#[test]
fn host_theme_is_sent_on_the_existing_attach_connection() {
    use herdr_shell_core::wire::{ClientHostAppearance, ClientHostThemeUpdate};
    let (client, server) = attach_server(|stream| {
        let mut prefix = [0; 4];
        stream.read_exact(&mut prefix).unwrap();
        let mut payload = vec![0; u32::from_le_bytes(prefix) as usize];
        stream.read_exact(&mut payload).unwrap();
        assert_eq!(payload, vec![17, 2, 1]);
    });
    let (handle, events) = client.into_parts();
    handle
        .host_theme(vec![ClientHostThemeUpdate::Appearance(
            ClientHostAppearance::Light,
        )])
        .unwrap();
    server.join().unwrap();
    drop(events);
}
