use herdr_shell_core::{
    api::ApiClient,
    endpoint::{poll_read, prepare_polled, write_all_polled, Endpoint, ReadPoll},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeSet, HashMap},
    fs,
    net::{SocketAddr, TcpListener, TcpStream},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter};

const LIFECYCLE: &[&str] = &[
    "workspace.created",
    "workspace.updated",
    "workspace.metadata_updated",
    "workspace.renamed",
    "workspace.moved",
    "workspace.reordered",
    "workspace.closed",
    "workspace.focused",
    "tab.created",
    "tab.closed",
    "tab.focused",
    "tab.renamed",
    "tab.moved",
    "pane.created",
    "pane.updated",
    "pane.closed",
    "pane.focused",
    "pane.moved",
    "pane.exited",
    "pane.agent_detected",
    "layout.updated",
    "desk.changed",
];

#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum MachineKind {
    #[default]
    Ssh,
    Local,
}
#[derive(Clone, Deserialize, Serialize)]
struct MachineConfig {
    name: String,
    #[serde(default)]
    kind: MachineKind,
    #[serde(default)]
    ssh_host: String,
    herdr_dir: String,
}
#[derive(Clone, Serialize)]
pub struct MachineStatus {
    pub name: String,
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}
struct Machine {
    status: MachineStatus,
    endpoints: Option<(Endpoint, Endpoint)>,
}
#[derive(Clone)]
pub struct Machines {
    inner: Arc<Mutex<HashMap<String, Machine>>>,
    configs: Arc<HashMap<String, MachineConfig>>,
    logs: PathBuf,
    job: Option<Arc<Job>>,
}

impl Machines {
    pub fn start(app: AppHandle) -> Result<Self, String> {
        let config_dir = config_dir()?;
        fs::create_dir_all(&config_dir).map_err(|e| e.to_string())?;
        let path = config_dir.join("machines.json");
        let local_dir = local_herdr_dir()?;
        let defaults = default_configs(&local_dir);
        // create_new preserves a config created concurrently rather than overwriting it.
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => serde_json::to_writer_pretty(file, &defaults).map_err(|e| e.to_string())?,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e.to_string()),
        }
        let mut configs: Vec<MachineConfig> =
            serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        append_local(&mut configs, &local_dir, local_dir.is_dir());
        let mut machines = HashMap::new();
        for c in &configs {
            if c.name.is_empty()
                || !c
                    .name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
                || (c.kind == MachineKind::Ssh
                    && (c.ssh_host.is_empty() || c.ssh_host.starts_with('-')))
                || c.herdr_dir.is_empty()
            {
                return Err("invalid machine config".into());
            }
            if machines
                .insert(
                    c.name.clone(),
                    Machine {
                        status: MachineStatus {
                            name: c.name.clone(),
                            state: "connecting".into(),
                            error: None,
                        },
                        endpoints: None,
                    },
                )
                .is_some()
            {
                return Err("duplicate machine name".into());
            }
        }
        let logs = log_dir()?.join("logs");
        fs::create_dir_all(&logs).map_err(|e| e.to_string())?;
        let job = if configs.iter().any(|c| c.kind == MachineKind::Ssh) {
            Some(Arc::new(Job::new()?))
        } else {
            None
        };
        let manager = Self {
            inner: Arc::new(Mutex::new(machines)),
            configs: Arc::new(
                configs
                    .iter()
                    .map(|c| (c.name.clone(), c.clone()))
                    .collect(),
            ),
            logs: logs.clone(),
            job: job.clone(),
        };
        for config in configs {
            let (manager, app, logs, job) =
                (manager.clone(), app.clone(), logs.clone(), job.clone());
            std::thread::spawn(move || loop {
                // Local machines start connecting, then stay down during silent retries.
                if config.kind != MachineKind::Local {
                    let _ = manager.change(&app, &config.name, "connecting", None, None);
                }
                let result = if config.kind == MachineKind::Local {
                    supervise_local(&manager, &app, &config)
                } else {
                    match job.as_deref() {
                        Some(job) => supervise(&manager, &app, &config, &logs, job),
                        None => Err("SSH job unavailable".into()),
                    }
                };
                let _ = manager.change(
                    &app,
                    &config.name,
                    "down",
                    Some(result.err().unwrap_or_else(|| "connection closed".into())),
                    None,
                );
                std::thread::sleep(Duration::from_secs(if config.kind == MachineKind::Local {
                    5
                } else {
                    3
                }));
            });
        }
        Ok(manager)
    }
    pub(crate) fn is_local(&self, name: &str) -> Result<bool, String> {
        Ok(self.configs.get(name).ok_or("unknown machine")?.kind == MachineKind::Local)
    }
    pub(crate) fn file_helper_config(
        &self,
        name: &str,
    ) -> Result<(String, PathBuf, Arc<Job>), String> {
        let config = self.configs.get(name).ok_or("unknown machine")?;
        Ok((
            config.ssh_host.clone(),
            self.logs.join(format!("helper-{name}.log")),
            self.job.clone().ok_or("SSH job unavailable")?,
        ))
    }
    pub fn list(&self) -> Result<Vec<MachineStatus>, String> {
        let mut statuses: Vec<_> = self
            .inner
            .lock()
            .map_err(|_| "machine lock poisoned")?
            .values()
            .map(|m| m.status.clone())
            .collect();
        statuses.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(statuses)
    }
    pub fn endpoints(&self, name: &str) -> Result<(Endpoint, Endpoint), String> {
        self.inner
            .lock()
            .map_err(|_| "machine lock poisoned")?
            .get(name)
            .and_then(|m| m.endpoints.clone())
            .ok_or_else(|| format!("machine {name} is not up"))
    }
    fn change(
        &self,
        app: &AppHandle,
        name: &str,
        state: &str,
        error: Option<String>,
        endpoints: Option<(Endpoint, Endpoint)>,
    ) -> Result<(), String> {
        let status = MachineStatus {
            name: name.into(),
            state: state.into(),
            error,
        };
        {
            let mut guard = self.inner.lock().map_err(|_| "machine lock poisoned")?;
            let machine = guard.get_mut(name).ok_or("unknown machine")?;
            machine.status = status.clone();
            machine.endpoints = endpoints;
        }
        app.emit("herdr://machine", status)
            .map_err(|e| e.to_string())
    }
}
fn local_config(dir: &std::path::Path) -> MachineConfig {
    MachineConfig {
        name: "pc".into(),
        kind: MachineKind::Local,
        ssh_host: String::new(),
        herdr_dir: dir.to_string_lossy().into_owned(),
    }
}
fn default_configs(dir: &std::path::Path) -> Vec<MachineConfig> {
    vec![
        MachineConfig {
            name: "studio".into(),
            kind: MachineKind::Ssh,
            ssh_host: "studio".into(),
            herdr_dir: "/Users/aneyman/.config/herdr".into(),
        },
        local_config(dir),
    ]
}
fn append_local(configs: &mut Vec<MachineConfig>, dir: &std::path::Path, exists: bool) {
    if exists && !configs.iter().any(|c| c.kind == MachineKind::Local) {
        let mut local = local_config(dir);
        let mut suffix = 1;
        while configs.iter().any(|c| c.name == local.name) {
            local.name = format!("pc-local-{suffix}");
            suffix += 1;
        }
        configs.push(local);
    }
}
fn local_herdr_dir() -> Result<PathBuf, String> {
    #[cfg(windows)]
    {
        std::env::var_os("APPDATA")
            .map(|p| PathBuf::from(p).join("herdr"))
            .ok_or("APPDATA not set".into())
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("HOME")
            .map(|p| PathBuf::from(p).join(".config/herdr"))
            .ok_or("HOME not set".into())
    }
}
fn config_dir() -> Result<PathBuf, String> {
    #[cfg(windows)]
    {
        std::env::var_os("APPDATA")
            .map(|p| PathBuf::from(p).join("HerdrShell"))
            .ok_or("APPDATA not set".into())
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("HOME")
            .map(|p| PathBuf::from(p).join(".config/HerdrShell"))
            .ok_or("HOME not set".into())
    }
}
fn log_dir() -> Result<PathBuf, String> {
    #[cfg(windows)]
    {
        std::env::var_os("LOCALAPPDATA")
            .map(|p| PathBuf::from(p).join("HerdrShell"))
            .ok_or("LOCALAPPDATA not set".into())
    }
    #[cfg(not(windows))]
    {
        config_dir()
    }
}
fn free_addr() -> Result<SocketAddr, String> {
    TcpListener::bind("127.0.0.1:0")
        .and_then(|s| s.local_addr())
        .map_err(|e| e.to_string())
}
struct Tunnel(Child);
impl Drop for Tunnel {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
struct Subscription {
    stop: Arc<AtomicBool>,
    receiver: mpsc::Receiver<Result<Value, String>>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Subscription {
    fn open(endpoint: &Endpoint, panes: &BTreeSet<String>) -> Result<Self, String> {
        let mut subscriptions: Vec<Value> = LIFECYCLE.iter().map(|t| json!({"type": t})).collect();
        subscriptions.extend(
            panes
                .iter()
                .map(|id| json!({"type":"pane.agent_status_changed", "pane_id":id})),
        );
        let mut stream = endpoint.connect().map_err(|e| e.to_string())?;
        prepare_polled(&mut stream).map_err(|e| e.to_string())?;
        let mut request = serde_json::to_vec(&json!({"id":"shell-subscription", "method":"events.subscribe", "params":{"subscriptions":subscriptions}})).map_err(|e| e.to_string())?;
        request.push(b'\n');
        write_all_polled(&mut stream, &request).map_err(|e| e.to_string())?;
        let stop = Arc::new(AtomicBool::new(false));
        let cancelled = stop.clone();
        let (tx, receiver) = mpsc::channel();
        let (ready_tx, ready) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            let mut run = || -> Result<(), String> {
                let mut buffer = Vec::new();
                let mut bytes = [0u8; 8192];
                let mut acknowledged = false;
                while !cancelled.load(Ordering::Acquire) {
                    match poll_read(&mut stream, &mut bytes).map_err(|e| e.to_string())? {
                        ReadPoll::Closed => return Err("event stream closed".into()),
                        ReadPoll::Pending => {
                            std::thread::sleep(Duration::from_millis(10));
                            continue;
                        }
                        ReadPoll::Data(n) => buffer.extend_from_slice(&bytes[..n]),
                    }
                    while let Some(end) = buffer.iter().position(|b| *b == b'\n') {
                        let line: Vec<_> = buffer.drain(..=end).collect();
                        if line.iter().all(u8::is_ascii_whitespace) {
                            continue;
                        }
                        let value: Value =
                            serde_json::from_slice(&line).map_err(|e| e.to_string())?;
                        if !acknowledged {
                            if value.get("error").is_some() {
                                return Err(format!("subscription rejected: {}", value["error"]));
                            }
                            if value["result"]["type"] != "subscription_started" {
                                return Err("expected subscription_started".into());
                            }
                            acknowledged = true;
                            let _ = ready_tx.send(Ok(()));
                        } else if tx.send(Ok(value)).is_err() {
                            return Ok(());
                        }
                    }
                    if buffer.len() > 8 * 1024 * 1024 {
                        return Err("event line too large".into());
                    }
                }
                Ok(())
            };
            if let Err(e) = run() {
                let _ = ready_tx.send(Err(e.clone()));
                let _ = tx.send(Err(e));
            }
        });
        let subscription = Self {
            stop,
            receiver,
            thread: Some(thread),
        };
        ready
            .recv_timeout(Duration::from_secs(5))
            .map_err(|e| format!("subscription acknowledgement: {e}"))??;
        Ok(subscription)
    }
}
impl Drop for Subscription {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
fn fetch(endpoint: &Endpoint) -> Result<Value, String> {
    ApiClient::new(endpoint.clone())
        .request("session.snapshot", json!({}))
        .map_err(|e| e.to_string())?
        .get("snapshot")
        .cloned()
        .ok_or("session.snapshot missing snapshot".into())
}
fn pane_set(snapshot: &Value) -> BTreeSet<String> {
    snapshot
        .get("panes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|p| p.get("pane_id").and_then(Value::as_str).map(str::to_owned))
        .collect()
}
fn supervise(
    manager: &Machines,
    app: &AppHandle,
    config: &MachineConfig,
    logs: &std::path::Path,
    job: &Job,
) -> Result<(), String> {
    let p = free_addr()?;
    let q = loop {
        let q = free_addr()?;
        if q != p {
            break q;
        }
    };
    let endpoints = (Endpoint::Tcp(p), Endpoint::Tcp(q));
    let log = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(logs.join(format!("ssh-{}.log", config.name)))
        .map_err(|e| e.to_string())?;
    #[cfg(windows)]
    let mut command = Command::new("ssh.exe");
    #[cfg(not(windows))]
    let mut command = Command::new("ssh");
    command
        .args([
            "-N",
            "-T",
            "-o",
            "ExitOnForwardFailure=yes",
            "-o",
            "ServerAliveInterval=15",
            "-o",
            "ServerAliveCountMax=3",
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=8",
            "-L",
            &format!(
                "127.0.0.1:{}:{}/herdr.sock",
                p.port(),
                config.herdr_dir.trim_end_matches('/')
            ),
            "-L",
            &format!(
                "127.0.0.1:{}:{}/herdr-client.sock",
                q.port(),
                config.herdr_dir.trim_end_matches('/')
            ),
            &config.ssh_host,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(log);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = Tunnel(command.spawn().map_err(|e| format!("ssh spawn: {e}"))?);
    job.assign(&child.0)?;
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Some(status) = child.0.try_wait().map_err(|e| e.to_string())? {
            return Err(format!("ssh exited: {status}"));
        }
        if TcpStream::connect_timeout(&p, Duration::from_millis(100)).is_ok() {
            break;
        }
        if Instant::now() >= deadline {
            return Err("ssh forwarding timed out after 20 s".into());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    stream_snapshots(manager, app, config, endpoints, || {
        if let Some(status) = child.0.try_wait().map_err(|e| e.to_string())? {
            return Err(format!("ssh exited: {status}"));
        }
        Ok(())
    })
}
#[cfg(windows)]
const LOCAL_DOWN: &str = "herdr server is not running on this PC";
fn supervise_local(
    manager: &Machines,
    app: &AppHandle,
    config: &MachineConfig,
) -> Result<(), String> {
    #[cfg(windows)]
    {
        let path = PathBuf::from(&config.herdr_dir).join("herdr.sock");
        match fs::metadata(&path) {
            Ok(info) if !info.is_file() => return Err("local marker is not a file".into()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(LOCAL_DOWN.into()),
            Err(e) => return Err(e.to_string()),
            Ok(_) => {}
        }
        let api = Endpoint::NamedPipe(path);
        let _ = api.connect().map_err(|_| LOCAL_DOWN)?;
        let client = api
            .client_for_api()
            .ok_or("local client endpoint unavailable")?;
        stream_snapshots(manager, app, config, (api, client), || Ok(()))
    }
    #[cfg(not(windows))]
    {
        let _ = (manager, app, config);
        Err("local machines require Windows".into())
    }
}
fn stream_snapshots(
    manager: &Machines,
    app: &AppHandle,
    config: &MachineConfig,
    endpoints: (Endpoint, Endpoint),
    mut health: impl FnMut() -> Result<(), String>,
) -> Result<(), String> {
    let mut panes = BTreeSet::new();
    let mut subscription = Subscription::open(&endpoints.0, &panes)?;
    manager.change(app, &config.name, "up", None, Some(endpoints.clone()))?;
    let mut refresh = Instant::now();
    let mut pending = None;
    loop {
        health()?;
        if Instant::now() >= refresh || pending.is_some_and(|t| Instant::now() >= t) {
            let snapshot = fetch(&endpoints.0)?;
            let next_panes = pane_set(&snapshot);
            if panes != next_panes {
                // Open the replacement before closing the old stream, so invalidations have no gap.
                let next = Subscription::open(&endpoints.0, &next_panes)?;
                subscription = next;
                panes = next_panes;
                pending = Some(Instant::now() + Duration::from_millis(60));
            } else {
                pending = None;
            }
            app.emit(
                "herdr://snapshot",
                json!({"machine":config.name,"snapshot":snapshot}),
            )
            .map_err(|e| e.to_string())?;
            refresh = Instant::now() + Duration::from_secs(10);
        }
        match subscription
            .receiver
            .recv_timeout(Duration::from_millis(10))
        {
            Ok(Ok(_)) => {
                pending.get_or_insert_with(|| Instant::now() + Duration::from_millis(60));
            }
            Ok(Err(e)) => return Err(e),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err("event stream disconnected".into())
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
    }
}

#[cfg(not(windows))]
pub(crate) struct Job;
#[cfg(not(windows))]
impl Job {
    fn new() -> Result<Self, String> {
        Ok(Self)
    }
    pub(crate) fn assign(&self, _: &Child) -> Result<(), String> {
        Ok(())
    }
}
#[cfg(windows)]
pub(crate) struct Job(windows_sys::Win32::Foundation::HANDLE);
// Windows Job handles may be assigned from multiple supervisor threads; ownership stays in Arc.
#[cfg(windows)]
unsafe impl Send for Job {}
#[cfg(windows)]
unsafe impl Sync for Job {}
#[cfg(windows)]
impl Job {
    fn new() -> Result<Self, String> {
        use windows_sys::Win32::System::JobObjects::*;
        unsafe {
            let job = Self(CreateJobObjectW(std::ptr::null(), std::ptr::null()));
            if job.0.is_null() {
                return Err(std::io::Error::last_os_error().to_string());
            }
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(
                job.0,
                JobObjectExtendedLimitInformation,
                &info as *const _ as _,
                std::mem::size_of_val(&info) as u32,
            ) == 0
            {
                return Err(std::io::Error::last_os_error().to_string());
            }
            Ok(job)
        }
    }
    pub(crate) fn assign(&self, child: &Child) -> Result<(), String> {
        use std::os::windows::io::AsRawHandle;
        if unsafe {
            windows_sys::Win32::System::JobObjects::AssignProcessToJobObject(
                self.0,
                child.as_raw_handle() as _,
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error().to_string());
        }
        Ok(())
    }
}
#[cfg(windows)]
impl Drop for Job {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // Real local-socket boundary covers subscription ack/event framing and dropping
    // an idle stream, independently of the TCP-only predecessor and pure config tests.
    #[cfg(unix)]
    #[test]
    fn local_subscription_stream_and_cancellation() -> Result<(), Box<dyn std::error::Error>> {
        use std::io::{BufRead, BufReader, Read, Write};
        use std::os::unix::net::UnixListener;
        let path =
            std::env::temp_dir().join(format!("shell-subscription-{}.sock", std::process::id()));
        let listener = UnixListener::bind(&path)?;
        let server = std::thread::spawn(move || -> Result<(), String> {
            let (mut stream, _) = listener.accept().map_err(|e| e.to_string())?;
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .map_err(|e| e.to_string())?;
            let mut line = String::new();
            BufReader::new(&mut stream)
                .read_line(&mut line)
                .map_err(|e| e.to_string())?;
            let request: Value = serde_json::from_str(&line).map_err(|e| e.to_string())?;
            if request["method"] != "events.subscribe" {
                return Err("wrong subscription method".into());
            }
            stream
                .write_all(b"{\"result\":{\"type\":\"subscription_started\"}}\n{\"event\":\"pane.")
                .map_err(|e| e.to_string())?;
            stream
                .write_all(b"created\"}\n")
                .map_err(|e| e.to_string())?;
            let mut byte = [0u8; 1];
            if stream.read(&mut byte).map_err(|e| e.to_string())? != 0 {
                return Err("subscription not closed".into());
            }
            Ok(())
        });
        let result = (|| -> Result<(), Box<dyn std::error::Error>> {
            let subscription = Subscription::open(&Endpoint::from_path(&path), &BTreeSet::new())?;
            let event = subscription
                .receiver
                .recv_timeout(Duration::from_secs(5))??;
            assert_eq!(event["event"], "pane.created");
            drop(subscription);
            server
                .join()
                .map_err(|_| "subscription server panicked")??;
            Ok(())
        })();
        fs::remove_file(path)?;
        result
    }
    // Pure migration algorithm covers legacy defaults, absent directories, and
    // idempotence; this config contract had no existing owner-boundary coverage.
    #[test]
    fn defaults_and_legacy_migration() -> Result<(), Box<dyn std::error::Error>> {
        let dir = PathBuf::from("profile/herdr");
        let defaults = default_configs(&dir);
        assert_eq!(defaults.len(), 2);
        assert_eq!(defaults[0].name, "studio");
        assert_eq!(defaults[0].kind, MachineKind::Ssh);
        assert_eq!(defaults[1].name, "pc");
        assert_eq!(defaults[1].kind, MachineKind::Local);
        assert_eq!(defaults[1].herdr_dir, dir.to_string_lossy());
        let legacy = r#"[{"name":"studio","ssh_host":"studio","herdr_dir":"/remote"}]"#;
        let mut configs: Vec<MachineConfig> = serde_json::from_str(legacy)?;
        assert_eq!(configs[0].kind, MachineKind::Ssh);
        append_local(&mut configs, &dir, false);
        assert_eq!(configs.len(), 1);
        append_local(&mut configs, &dir, true);
        append_local(&mut configs, &dir, true);
        assert_eq!(configs.len(), 2);
        assert_eq!(configs[0].herdr_dir, "/remote");
        let mut custom: Vec<MachineConfig> =
            serde_json::from_str(r#"[{"name":"home","kind":"local","herdr_dir":"elsewhere"}]"#)?;
        append_local(&mut custom, &dir, true);
        assert_eq!(custom.len(), 1);
        let mut collision: Vec<MachineConfig> =
            serde_json::from_str(r#"[{"name":"pc","ssh_host":"other","herdr_dir":"/remote"}]"#)?;
        append_local(&mut collision, &dir, true);
        assert_eq!(collision.len(), 2);
        assert_eq!(collision[0].kind, MachineKind::Ssh);
        assert_eq!(collision[1].kind, MachineKind::Local);
        assert_ne!(collision[0].name, collision[1].name);
        assert!(serde_json::from_str::<MachineConfig>(
            r#"{"name":"bad","kind":"other","herdr_dir":"x"}"#
        )
        .is_err());
        Ok(())
    }
}

impl Machines {
    pub(crate) fn remote_action(
        &self,
        machine: &str,
        verb: &str,
        args: &[String],
    ) -> Result<Value, String> {
        use std::io::{Read, Write};
        let config = self.configs.get(machine).ok_or("unknown machine")?;
        let script = include_str!("remote_helper.py");
        let remote = config.kind != MachineKind::Local;
        let mut command = if !remote {
            let mut c = Command::new("python3");
            c.args(["-I", "-u", "-c", script]);
            c
        } else {
            #[cfg(windows)]
            let mut c = Command::new("ssh.exe");
            #[cfg(not(windows))]
            let mut c = Command::new("ssh");
            // File-helper bootstrap: user argv is JSON on stdin, never shell text.
            c.args(["-T", "-o", "BatchMode=yes", "-o", "ConnectTimeout=8", &config.ssh_host,
                "python3 -I -u -c 'import sys;n=int(sys.stdin.buffer.readline());exec(compile(sys.stdin.buffer.read(n),\"herdr-shell-helper\",\"exec\"))'"]);
            c
        };
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let hostname = std::env::var("COMPUTERNAME")
            .or_else(|_| std::env::var("HOSTNAME"))
            .or_else(|_| {
                Command::new("hostname")
                    .output()
                    .map_err(|e| e.to_string())
                    .and_then(|o| String::from_utf8(o.stdout).map_err(|e| e.to_string()))
            })
            .map_err(|e| e.to_string())?;
        let by = format!(
            "herdr-shell@{}",
            hostname
                .trim()
                .split('.')
                .next()
                .ok_or("hostname missing")?
                .to_lowercase()
        );
        let mut child = command.spawn().map_err(|e| e.to_string())?;
        if let Some(job) = &self.job {
            if let Err(error) = job.assign(&child) {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        }
        let mut input = child.stdin.take().ok_or("action stdin missing")?;
        let mut output = child.stdout.take().ok_or("action stdout missing")?;
        let request = json!({"op":"action", "verb":verb, "args":args, "by":by});
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let run = || -> Result<Value, String> {
                if remote {
                    writeln!(input, "{}", script.len()).map_err(|e| e.to_string())?;
                    input
                        .write_all(script.as_bytes())
                        .map_err(|e| e.to_string())?;
                }
                serde_json::to_writer(&mut input, &request).map_err(|e| e.to_string())?;
                input.write_all(b"\n").map_err(|e| e.to_string())?;
                drop(input);
                let mut bytes = Vec::new();
                output
                    .by_ref()
                    .take(1024 * 1024)
                    .read_to_end(&mut bytes)
                    .map_err(|e| e.to_string())?;
                serde_json::from_slice(&bytes).map_err(|e| e.to_string())
            };
            let _ = tx.send(run());
        });
        let result = rx
            .recv_timeout(Duration::from_secs(28))
            .map_err(|e| e.to_string());
        let _ = child.kill();
        let _ = child.wait();
        let value = result??;
        if value["ok"] != true {
            return Err(value["error"].as_str().unwrap_or("action failed").into());
        }
        Ok(value)
    }
}
