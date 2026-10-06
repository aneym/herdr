//! Real TCP framing boundary: local-socket goldens cannot catch a broken TCP transport.
use herdr_shell_core::{api::ApiClient, endpoint::Endpoint};
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
