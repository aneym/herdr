#![cfg(unix)]

pub mod support;

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use support::{
    cleanup_test_base, register_runtime_dir, register_spawned_herdr_pid,
    unregister_spawned_herdr_pid, wait_for_socket,
};

const PANES: usize = 50;
const EXIT_BUDGET: Duration = Duration::from_secs(5);
const SAVED_LABEL: &str = "saved-at-shutdown";

struct ServerGuard(Child);

impl Drop for ServerGuard {
    fn drop(&mut self) {
        let pid = self.0.id();
        let _ = self.0.kill();
        let _ = self.0.wait();
        unregister_spawned_herdr_pid(Some(pid));
    }
}

struct Shutdown {
    base: PathBuf,
    app_dir: PathBuf,
    elapsed: Duration,
    status: Option<ExitStatus>,
    shells: Vec<libc::pid_t>,
}

fn test_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn unique_base() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    PathBuf::from(format!("/tmp/hsig-{}-{nanos}", std::process::id()))
}

fn request(socket: &Path, method: &str, params: serde_json::Value) -> serde_json::Value {
    let mut stream = UnixStream::connect(socket).expect("connect api socket");
    stream
        .set_read_timeout(Some(Duration::from_secs(20)))
        .unwrap();
    writeln!(
        stream,
        "{}",
        serde_json::json!({"id": method, "method": method, "params": params})
    )
    .unwrap();
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line).unwrap();
    let response: serde_json::Value = serde_json::from_str(&line).expect("json response");
    assert!(response.get("error").is_none(), "{method}: {response}");
    response
}

fn alive(pid: libc::pid_t) -> bool {
    unsafe { libc::kill(pid, 0) == 0 }
}

fn ready_shells(ready_dir: &Path) -> Vec<libc::pid_t> {
    fs::read_dir(ready_dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| entry.file_name().to_str()?.parse().ok())
        .collect()
}

fn reap_shells(shells: &[libc::pid_t], settle: Duration) -> Vec<libc::pid_t> {
    let deadline = Instant::now() + settle;
    while shells.iter().any(|pid| alive(*pid)) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(50));
    }
    let survivors: Vec<_> = shells.iter().copied().filter(|pid| alive(*pid)).collect();
    for pid in shells {
        unsafe {
            libc::kill(-*pid, libc::SIGKILL);
            libc::kill(*pid, libc::SIGKILL);
        }
    }
    survivors
}

fn sigterm_server_with_busy_panes(deadline_ms: Option<u64>) -> Shutdown {
    let base = unique_base();
    let config_home = base.join("config");
    let runtime_dir = base.join("runtime");
    let ready_dir = base.join("ready");
    let socket = runtime_dir.join("herdr.sock");
    let app_dir = config_home.join(if cfg!(debug_assertions) {
        "herdr-dev"
    } else {
        "herdr"
    });
    fs::create_dir_all(&app_dir).unwrap();
    fs::create_dir_all(&runtime_dir).unwrap();
    fs::create_dir_all(&ready_dir).unwrap();
    register_runtime_dir(&runtime_dir);

    let shell = base.join("busy.sh");
    fs::write(
        &shell,
        "#!/bin/sh\ntrap '' HUP TERM INT\nsleep 600 &\n: > \"$HSIG_READY/$$\"\nwhile :; do i=0; while [ $i -lt 400 ]; do i=$((i+1)); done; sleep 0.02; done\n",
    )
    .unwrap();
    fs::set_permissions(&shell, fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(
        app_dir.join("config.toml"),
        format!(
            "onboarding = false\n[experimental]\nallow_nested = true\n[terminal]\ndefault_shell = \"{}\"\n",
            shell.display()
        ),
    )
    .unwrap();

    let mut command = Command::new(env!("CARGO_BIN_EXE_herdr"));
    command
        .arg("server")
        .env("XDG_CONFIG_HOME", &config_home)
        .env("XDG_STATE_HOME", &config_home)
        .env("XDG_RUNTIME_DIR", &runtime_dir)
        .env("HERDR_SOCKET_PATH", &socket)
        .env("HERDR_SERVER_SIGNAL_QUIT", "1")
        .env("HSIG_READY", &ready_dir)
        .env("SHELL", &shell)
        .env_remove("HERDR_SERVER_SHUTDOWN_DEADLINE_MS")
        .env_remove("HERDR_CLIENT_SOCKET_PATH")
        .env_remove("HERDR_SESSION")
        .env_remove("HERDR_ENV")
        .env_remove("HERDR_WORKSPACE_ID")
        .env_remove("HERDR_TAB_ID")
        .env_remove("HERDR_PANE_ID")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(ms) = deadline_ms {
        command.env("HERDR_SERVER_SHUTDOWN_DEADLINE_MS", ms.to_string());
    }
    let mut server = ServerGuard(command.spawn().expect("spawn server"));
    let server_pid = server.0.id() as libc::pid_t;
    register_spawned_herdr_pid(Some(server_pid as u32));
    wait_for_socket(&socket, Duration::from_secs(20));

    let created = request(
        &socket,
        "workspace.create",
        serde_json::json!({"cwd": base, "focus": true, "label": "shutdown"}),
    );
    let workspace_id = created["result"]["workspace"]["workspace_id"]
        .as_str()
        .expect("workspace id")
        .to_owned();
    let mut last_tab = String::new();
    for _ in 1..PANES {
        let tab = request(
            &socket,
            "tab.create",
            serde_json::json!({"workspace_id": workspace_id, "cwd": base}),
        );
        last_tab = tab["result"]["tab"]["tab_id"]
            .as_str()
            .expect("tab id")
            .to_owned();
    }

    let ready_deadline = Instant::now() + Duration::from_secs(30);
    let mut shells = ready_shells(&ready_dir);
    while shells.len() < PANES && Instant::now() < ready_deadline {
        thread::sleep(Duration::from_millis(100));
        shells = ready_shells(&ready_dir);
    }
    if shells.len() < PANES {
        reap_shells(&shells, Duration::ZERO);
        panic!("expected {PANES} ready pane shells, saw {}", shells.len());
    }

    request(
        &socket,
        "tab.rename",
        serde_json::json!({"tab_id": last_tab, "label": SAVED_LABEL}),
    );

    let sent = Instant::now();
    assert_eq!(unsafe { libc::kill(server_pid, libc::SIGTERM) }, 0);
    let status = loop {
        if let Some(status) = server.0.try_wait().unwrap() {
            break Some(status);
        }
        if sent.elapsed() > Duration::from_secs(60) {
            break None;
        }
        thread::sleep(Duration::from_millis(10));
    };
    let elapsed = sent.elapsed();
    eprintln!("server exited {elapsed:?} after SIGTERM with {status:?}");
    drop(server);
    Shutdown {
        base,
        app_dir,
        elapsed,
        status,
        shells,
    }
}

fn saved_session(app_dir: &Path) -> String {
    let bytes = fs::read(app_dir.join("session.json")).expect("session.json");
    let session: serde_json::Value = serde_json::from_slice(&bytes).expect("session json");
    session.to_string()
}

#[test]
fn sigterm_exits_within_budget_with_busy_panes_and_saves_session() {
    let _lock = test_lock();
    let shutdown = sigterm_server_with_busy_panes(None);
    let survivors = reap_shells(&shutdown.shells, Duration::from_secs(3));

    assert!(
        shutdown.status.is_some(),
        "server still running 60 s after SIGTERM"
    );
    assert!(
        shutdown.elapsed < EXIT_BUDGET,
        "server took {:?} to exit after SIGTERM, budget {EXIT_BUDGET:?}",
        shutdown.elapsed
    );
    assert!(
        saved_session(&shutdown.app_dir).contains(SAVED_LABEL),
        "session.json lacks the rename made just before SIGTERM"
    );
    assert!(
        survivors.is_empty(),
        "pane shells that ignore HUP and TERM outlived the server: {survivors:?}"
    );
    cleanup_test_base(&shutdown.base);
}

#[test]
fn shutdown_deadline_exits_even_when_teardown_would_take_longer() {
    let _lock = test_lock();
    let shutdown = sigterm_server_with_busy_panes(Some(300));
    reap_shells(&shutdown.shells, Duration::ZERO);

    assert!(
        shutdown.status.is_some(),
        "server still running 60 s after SIGTERM"
    );
    assert!(
        shutdown.elapsed < Duration::from_millis(1200),
        "a 300 ms deadline let the server run {:?} after SIGTERM",
        shutdown.elapsed
    );
    cleanup_test_base(&shutdown.base);
}
