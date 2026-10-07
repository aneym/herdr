//! Real TCP framing boundary: local-socket goldens cannot catch a broken TCP transport.
use herdr_shell_core::{
    api::{ApiClient, ApiError},
    endpoint::Endpoint,
};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;

#[test]
fn api_request_over_loopback_tcp() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let mut line = String::new();
        BufReader::new(&mut stream).read_line(&mut line).unwrap();
        let request: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(request["method"], "ping");
        assert_eq!(request["params"], json!({}));
        writeln!(
            stream,
            "{}",
            json!({"id": request["id"], "result": {"ok": true}})
        )
        .unwrap();
    });
    assert_eq!(
        ApiClient::new(Endpoint::Tcp(addr))
            .request("ping", json!({}))
            .unwrap(),
        json!({"ok": true})
    );
    server.join().unwrap();
}

/// Preserve server error metadata across the real transport, including legacy servers.
#[test]
fn restart_busy_reason_over_loopback_tcp() {
    for reason in [
        Some("working"),
        Some("restart_pending"),
        Some("blocked"),
        None,
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let mut line = String::new();
            BufReader::new(&mut stream).read_line(&mut line).unwrap();
            let request: Value = serde_json::from_str(&line).unwrap();
            assert_eq!(request["method"], "agent.restart");
            let mut error = json!({"code": "busy", "message": "server wording changed"});
            if let Some(reason) = reason {
                error["reason"] = reason.into();
            }
            writeln!(stream, "{}", json!({"id": request["id"], "error": error})).unwrap();
        });
        match ApiClient::new(Endpoint::Tcp(addr))
            .request("agent.restart", json!({"pane_id": "pane_1"}))
        {
            Err(ApiError::Server {
                code,
                message,
                reason: actual,
            }) => {
                assert_eq!(code, "busy");
                assert_eq!(message, "server wording changed");
                assert_eq!(actual.as_deref(), reason);
            }
            other => panic!("expected server error: {other:?}"),
        }
        server.join().unwrap();
    }
}
