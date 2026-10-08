use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

struct Server(Child);

struct Replacement(u32);

impl Drop for Replacement {
    fn drop(&mut self) {
        // This PID is the import child of this test's isolated server.
        unsafe { libc::kill(self.0 as libc::pid_t, libc::SIGTERM); }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn request(socket: &Path, params: serde_json::Value) {
    let mut stream = UnixStream::connect(socket).expect("connect to test server");
    let request = serde_json::json!({
        "id": "test:handoff-path", "method": "server.live_handoff", "params": params
    });
    writeln!(stream, "{request}").expect("send handoff request");
    let mut response = String::new();
    BufReader::new(stream).read_line(&mut response).expect("read handoff response");
    let response: serde_json::Value = serde_json::from_str(&response).expect("parse response");
    assert!(response.get("result").is_some(), "handoff failed: {response}");
}

// Real server/CLI integration: inspect the replacement process, not a launch mock.
fn check_handoff_path(cli: bool, expected_path: &str) {
    let base = std::env::temp_dir().join(format!("hhp-{}-{cli}", std::process::id()));
    let config = base.as_path().join("config");
    let runtime = base.as_path().join("runtime");
    let socket = base.as_path().join("api.sock");
    std::fs::create_dir_all(config.join("herdr-dev")).expect("config directory");
    std::fs::create_dir_all(&runtime).expect("runtime directory");
    std::fs::write(config.join("herdr-dev/config.toml"), "onboarding = false\n")
        .expect("test config");
    let exe = std::env::current_exe().expect("test executable")
        .parent().expect("deps directory").parent().expect("target directory").join("herdr");
    let configure = |command: &mut Command| {
        command.env("XDG_CONFIG_HOME", &config)
            .env("XDG_RUNTIME_DIR", &runtime)
            .env("HERDR_SOCKET_PATH", &socket)
            .env("HERDR_CLIENT_SOCKET_PATH", base.as_path().join("client.sock"))
            .env_remove("HERDR_SESSION")
            .env_remove("HERDR_ENV")
            .env("SHELL", "/bin/sh");
    };
    let mut command = Command::new(&exe);
    configure(&mut command);
    let old = Server(command.arg("server").env("PATH", "/usr/bin:/bin")
        .stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null()).spawn().expect("spawn old server"));
    let deadline = Instant::now() + Duration::from_secs(10);
    while !socket.exists() {
        assert!(Instant::now() < deadline, "old server socket not found");
        std::thread::sleep(Duration::from_millis(25));
    }
    assert_eq!(crate::platform::process_env_var(old.0.id(), "PATH").as_deref(), Some("/usr/bin:/bin"));
    if cli {
        let mut command = Command::new(&exe);
        configure(&mut command);
        let output = command.args(["server", "live-handoff"]).env("PATH", "/x:/usr/bin")
            .output().expect("request CLI handoff");
        assert!(output.status.success(), "CLI handoff failed: {}", String::from_utf8_lossy(&output.stderr));
    } else {
        request(&socket, serde_json::json!({}));
    }
    let pattern = format!("herdr-handoff-{}.sock", old.0.id());
    let deadline = Instant::now() + Duration::from_secs(10);
    let replacement = loop {
        let output = Command::new("pgrep").args(["-f", &pattern]).output().expect("find import server");
        if let Some(pid) = String::from_utf8_lossy(&output.stdout).lines()
            .filter_map(|line| line.parse::<u32>().ok()).find(|pid| *pid != old.0.id()) {
            break pid;
        }
        assert!(Instant::now() < deadline, "replacement server not found");
        std::thread::sleep(Duration::from_millis(25));
    };
    let _replacement = Replacement(replacement);
    let actual = crate::platform::process_env_var(replacement, "PATH");
    let mut stop = Command::new(&exe);
    configure(&mut stop);
    let _ = stop.args(["server", "stop"]).output();
    let _ = std::fs::remove_dir_all(&base);
    assert_eq!(actual.as_deref(), Some(expected_path));
}

#[test]
fn live_handoff_path_uses_requester_environment() {
    check_handoff_path(true, "/x:/usr/bin");
}

#[test]
fn live_handoff_path_without_field_preserves_server_environment() {
    check_handoff_path(false, "/usr/bin:/bin");
}
