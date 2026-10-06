use herdr_shell_core::{api::ApiClient, endpoint::Endpoint};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeSet, HashMap},
    fs,
    net::{Shutdown, SocketAddr, TcpListener, TcpStream},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{mpsc, Arc, Mutex},
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
];

#[derive(Clone, Deserialize, Serialize)]
struct MachineConfig {
    name: String,
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
}

impl Machines {
    pub fn start(app: AppHandle) -> Result<Self, String> {
        let config_dir = config_dir()?;
        fs::create_dir_all(&config_dir).map_err(|e| e.to_string())?;
        let path = config_dir.join("machines.json");
        let defaults = vec![MachineConfig {
            name: "studio".into(),
            ssh_host: "studio".into(),
            herdr_dir: "/Users/aneyman/.config/herdr".into(),
        }];
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
        let configs: Vec<MachineConfig> =
            serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        let mut machines = HashMap::new();
        for c in &configs {
            if c.name.is_empty()
                || !c
                    .name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
                || c.ssh_host.is_empty()
                || c.ssh_host.starts_with('-')
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
        let manager = Self {
            inner: Arc::new(Mutex::new(machines)),
        };
        let logs = log_dir()?.join("logs");
        fs::create_dir_all(&logs).map_err(|e| e.to_string())?;
        let job = Arc::new(Job::new()?);
        for config in configs {
            let (manager, app, logs, job) =
                (manager.clone(), app.clone(), logs.clone(), job.clone());
            std::thread::spawn(move || loop {
                let _ = manager.change(&app, &config.name, "connecting", None, None);
                let result = supervise(&manager, &app, &config, &logs, &job);
                let _ = manager.change(
                    &app,
                    &config.name,
                    "down",
                    Some(result.err().unwrap_or_else(|| "ssh exited".into())),
                    None,
                );
                std::thread::sleep(Duration::from_secs(3));
            });
        }
        Ok(manager)
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
    socket: TcpStream,
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
        let stream = ApiClient::new(endpoint.clone())
            .subscribe(json!(subscriptions))
            .map_err(|e| e.to_string())?;
        let socket = stream
            .tcp_shutdown_handle()
            .map_err(|e| e.to_string())?
            .ok_or("expected TCP subscription")?;
        let (tx, receiver) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            for event in stream {
                let event = event.map_err(|e| e.to_string());
                let failed = event.is_err();
                if tx.send(event).is_err() || failed {
                    return;
                }
            }
            let _ = tx.send(Err("event stream closed".into()));
        });
        Ok(Self {
            socket,
            receiver,
            thread: Some(thread),
        })
    }
}
impl Drop for Subscription {
    fn drop(&mut self) {
        let _ = self.socket.shutdown(Shutdown::Both);
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
    let mut panes = BTreeSet::new();
    let mut subscription = Subscription::open(&endpoints.0, &panes)?;
    manager.change(app, &config.name, "up", None, Some(endpoints.clone()))?;
    let mut refresh = Instant::now();
    let mut pending = None;
    loop {
        if let Some(status) = child.0.try_wait().map_err(|e| e.to_string())? {
            return Err(format!("ssh exited: {status}"));
        }
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
struct Job;
#[cfg(not(windows))]
impl Job {
    fn new() -> Result<Self, String> {
        Ok(Self)
    }
    fn assign(&self, _: &Child) -> Result<(), String> {
        Ok(())
    }
}
#[cfg(windows)]
struct Job(windows_sys::Win32::Foundation::HANDLE);
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
    fn assign(&self, child: &Child) -> Result<(), String> {
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
