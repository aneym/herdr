#![cfg(unix)]

mod support;

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

// Integration boundary: execute the actual detached-helper CLI against a peer
// that stalls at either request. Existing real-server close tests cannot catch
// an orphan when the server never answers. No production test seam is needed.
#[test]
fn pane_close_helper_exits_when_probe_or_close_never_answers() {
    for stall_probe in [true, false] {
        let socket_path = std::env::temp_dir().join(format!(
            "hclose-{}-{}.sock",
            std::process::id(),
            stall_probe
        ));
        let listener = UnixListener::bind(&socket_path).unwrap();
        listener.set_nonblocking(true).unwrap();
        let mut command = Command::new(env!("CARGO_BIN_EXE_herdr"));
        // Run the same way from inside a Herdr pane as from CI: inherited
        // HERDR_* variables must not reroute the helper.
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("HERDR_") {
                command.env_remove(key);
            }
        }
        let mut helper = command
            .args(["pane", "close", "w1:p1"])
            .env("HERDR_SOCKET_PATH", &socket_path)
            .env("HERDR_PANE_CLOSE_DELAY_MS", "0")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(7);
        let mut requests = Vec::new();
        let mut streams = Vec::new();
        let mut exited = false;
        while Instant::now() < deadline {
            if helper.try_wait().unwrap().is_some() {
                exited = true;
                break;
            }
            if let Ok((stream, _)) = listener.accept() {
                stream
                    .set_read_timeout(Some(Duration::from_secs(1)))
                    .unwrap();
                let mut reader = BufReader::new(stream);
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let request: serde_json::Value = serde_json::from_str(&line).unwrap();
                requests.push(request["method"].as_str().unwrap().to_string());
                if !stall_probe && request["method"] == "ping" {
                    let response = serde_json::json!({
                        "id": request["id"],
                        "result": {
                            "type": "pong",
                            "version": env!("CARGO_PKG_VERSION"),
                            "protocol": support::CURRENT_PROTOCOL,
                            "capabilities": { "live_handoff": false }
                        }
                    });
                    writeln!(reader.get_mut(), "{response}").unwrap();
                }
                streams.push(reader);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        if !exited {
            helper.kill().unwrap();
            helper.wait().unwrap();
        }
        drop(streams);
        drop(listener);
        std::fs::remove_file(socket_path).unwrap();
        assert!(exited, "helper leaked with stalled requests: {requests:?}");
        assert_eq!(
            requests,
            if stall_probe {
                vec!["ping"]
            } else {
                vec!["ping", "pane.close"]
            }
        );
    }
}
