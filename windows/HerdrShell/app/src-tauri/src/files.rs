use crate::machines::Machines;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    fs::OpenOptions,
    io::{BufRead, BufReader, Read, Write},
    process::{Child, Command, Stdio},
    sync::{mpsc, Arc, Mutex},
    time::{Duration, Instant},
};
use tauri::State;

const BOOTSTRAP: &str = "python3 -I -u -c 'import sys;n=int(sys.stdin.buffer.readline());exec(compile(sys.stdin.buffer.read(n),\"herdr-shell-helper\",\"exec\"))'";
const SCRIPT: &str = include_str!("remote_helper.py");

#[derive(Deserialize, Serialize)]
pub struct FileStat {
    exists: bool,
    size: u64,
    mtime_ms: u64,
    inode: u64,
}
#[derive(Deserialize, Serialize)]
pub struct FileChunk {
    size: u64,
    mtime_ms: u64,
    inode: u64,
    offset: u64,
    data_b64: String,
}

struct Helper {
    child: Child,
    requests: mpsc::Sender<Value>,
    responses: mpsc::Receiver<Result<Value, String>>,
}
impl Drop for Helper {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
impl Helper {
    fn spawn(machines: &Machines, machine: &str) -> Result<Self, String> {
        let (host, log, job) = machines.file_helper_config(machine)?;
        let log = OpenOptions::new()
            .create(true)
            .append(true)
            .open(log)
            .map_err(|e| e.to_string())?;
        #[cfg(windows)]
        let mut command = Command::new("ssh.exe");
        #[cfg(not(windows))]
        let mut command = Command::new("ssh");
        command
            .args([
                "-T",
                "-o",
                "BatchMode=yes",
                "-o",
                "ConnectTimeout=8",
                "-o",
                "ServerAliveInterval=15",
                "-o",
                "ServerAliveCountMax=3",
                &host,
                BOOTSTRAP,
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(log);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let (tx, requests) = mpsc::channel::<Value>();
        let (responses, rx) = mpsc::channel();
        let mut helper = Self {
            child: command
                .spawn()
                .map_err(|e| format!("helper ssh spawn: {e}"))?,
            requests: tx,
            responses: rx,
        };
        job.assign(&helper.child)?;
        let mut stdin = helper.child.stdin.take().ok_or("helper stdin missing")?;
        let stdout = helper.child.stdout.take().ok_or("helper stdout missing")?;
        // All pipe I/O, including bootstrap and writes, is covered by recv_timeout.
        std::thread::spawn(move || {
            let run = || -> Result<(), String> {
                writeln!(stdin, "{}", SCRIPT.len()).map_err(|e| e.to_string())?;
                stdin
                    .write_all(SCRIPT.as_bytes())
                    .map_err(|e| e.to_string())?;
                stdin.flush().map_err(|e| e.to_string())?;
                let mut stdout = BufReader::new(stdout);
                for request in requests {
                    serde_json::to_writer(&mut stdin, &request).map_err(|e| e.to_string())?;
                    stdin.write_all(b"\n").map_err(|e| e.to_string())?;
                    stdin.flush().map_err(|e| e.to_string())?;
                    // A read response is at most 2 MiB encoded plus small metadata.
                    let mut line = Vec::new();
                    let count = std::io::Read::by_ref(&mut stdout)
                        .take(3 * 1024 * 1024)
                        .read_until(b'\n', &mut line)
                        .map_err(|e| e.to_string())?;
                    if count == 0 || line.last() != Some(&b'\n') {
                        return Err("helper EOF or oversized response".into());
                    }
                    let response: Value =
                        serde_json::from_slice(&line).map_err(|e| e.to_string())?;
                    if response.get("id") != request.get("id") || !response["ok"].is_boolean() {
                        return Err("invalid helper response".into());
                    }
                    if responses.send(Ok(response)).is_err() {
                        return Ok(());
                    }
                }
                Ok(())
            };
            if let Err(error) = run() {
                let _ = responses.send(Err(error));
            }
        });
        Ok(helper)
    }
}

#[derive(Default)]
struct MachineFiles {
    helper: Option<Helper>,
    last_spawn: Option<Instant>,
    next_id: u64,
}
#[derive(Default, Clone)]
pub struct Files {
    machines: Arc<Mutex<HashMap<String, Arc<Mutex<MachineFiles>>>>>,
}
impl Files {
    fn request(
        &self,
        machines: &Machines,
        machine: &str,
        mut request: Value,
    ) -> Result<Value, String> {
        // Validate before allocating state for an arbitrary caller-supplied name.
        machines.file_helper_config(machine)?;
        let state = self
            .machines
            .lock()
            .map_err(|_| "file map lock poisoned")?
            .entry(machine.to_owned())
            .or_default()
            .clone();
        let mut state = state.lock().map_err(|_| "file helper lock poisoned")?;
        if state.helper.is_none() {
            if state
                .last_spawn
                .is_some_and(|t| t.elapsed() < Duration::from_secs(3))
            {
                return Err("helper restart throttled; retry after 3 s".into());
            }
            state.last_spawn = Some(Instant::now());
            state.helper = Some(Helper::spawn(machines, machine)?);
        }
        state.next_id = state
            .next_id
            .checked_add(1)
            .ok_or("file request ids exhausted")?;
        request["id"] = json!(state.next_id);
        let result = match state.helper.as_ref() {
            Some(helper) => helper
                .requests
                .send(request)
                .map_err(|_| "helper input closed".to_string())
                .and_then(|_| {
                    helper
                        .responses
                        .recv_timeout(Duration::from_secs(10))
                        .map_err(|e| format!("helper response: {e}"))
                })
                .and_then(|r| r),
            None => Err("helper missing".into()),
        };
        let response = match result {
            Ok(value) => value,
            Err(error) => {
                state.helper = None;
                return Err(error);
            }
        };
        if response["ok"] != true {
            return Err(response["error"]
                .as_str()
                .unwrap_or("helper request failed")
                .to_owned());
        }
        Ok(response)
    }
}

async fn request(
    files: Files,
    machines: Machines,
    machine: String,
    args: Value,
) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || files.request(&machines, &machine, args))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn file_stat(
    files: State<'_, Files>,
    machines: State<'_, Machines>,
    machine: String,
    path: String,
) -> Result<FileStat, String> {
    let value = request(
        files.inner().clone(),
        machines.inner().clone(),
        machine,
        json!({"op":"stat", "path":path}),
    )
    .await?;
    serde_json::from_value(value).map_err(|e| e.to_string())
}
#[tauri::command]
pub async fn file_read(
    files: State<'_, Files>,
    machines: State<'_, Machines>,
    machine: String,
    path: String,
    offset: u64,
    max: u32,
) -> Result<FileChunk, String> {
    let value = request(
        files.inner().clone(),
        machines.inner().clone(),
        machine,
        json!({"op":"read", "path":path, "offset":offset, "max":max}),
    )
    .await?;
    serde_json::from_value(value).map_err(|e| e.to_string())
}
#[tauri::command]
pub async fn remote_home(
    files: State<'_, Files>,
    machines: State<'_, Machines>,
    machine: String,
) -> Result<String, String> {
    let value = request(
        files.inner().clone(),
        machines.inner().clone(),
        machine,
        json!({"op":"home"}),
    )
    .await?;
    value["home"]
        .as_str()
        .map(str::to_owned)
        .ok_or("helper home missing".into())
}
#[tauri::command]
pub async fn file_list(
    files: State<'_, Files>,
    machines: State<'_, Machines>,
    machine: String,
    path: String,
) -> Result<Vec<String>, String> {
    let value = request(
        files.inner().clone(),
        machines.inner().clone(),
        machine,
        json!({"op":"list", "path":path}),
    )
    .await?;
    serde_json::from_value(value["names"].clone()).map_err(|e| e.to_string())
}
