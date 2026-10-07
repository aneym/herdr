use crate::machines::Machines;
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    fs::OpenOptions,
    io::{BufRead, BufReader, Read, Seek, SeekFrom, Write},
    path::{Component, Path, PathBuf},
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

// Pure containment policy: canonical paths only, strict descendants, regular files,
// and no hard links. Filesystem resolution and metadata stay at the request boundary.
fn local_allowed(path: &Path, roots: &[PathBuf], regular: bool, links: Option<u64>) -> bool {
    regular
        && links.map_or(true, |n| n <= 1)
        && roots
            .iter()
            .any(|root| path != root && path.starts_with(root))
}
// Resolve existing ancestors too, so missing allowed files can return exists:false
// without permitting traversal or a symlinked parent outside the allowlist.
fn resolve_path(path: &Path) -> Result<PathBuf, String> {
    if !path.is_absolute() {
        return Err("path not allowed".into());
    }
    let mut resolved = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                resolved.pop();
            }
            Component::CurDir => {}
            _ => {
                resolved.push(component.as_os_str());
                match std::fs::canonicalize(&resolved) {
                    Ok(real) => resolved = real,
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e.to_string()),
                }
            }
        }
    }
    Ok(resolved)
}
fn file_identity(file: &std::fs::File) -> Result<(u64, u64, Option<u64>), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let info = file.metadata().map_err(|e| e.to_string())?;
        Ok((info.dev(), info.ino(), Some(info.nlink())))
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
        };
        let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
        // SAFETY: file owns the handle, and info is a valid out pointer.
        if unsafe { GetFileInformationByHandle(file.as_raw_handle() as _, &mut info) } == 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        Ok((
            u64::from(info.dwVolumeSerialNumber),
            (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
            Some(u64::from(info.nNumberOfLinks)),
        ))
    }
}
fn local_request(home: &Path, request: &Value) -> Result<Value, String> {
    let op = request["op"].as_str().ok_or("unknown operation")?;
    if op == "home" {
        return Ok(json!({"home": home}));
    }
    if !matches!(op, "stat" | "read" | "list") {
        return Err("unknown operation".into());
    }
    let supplied = request["path"].as_str().ok_or("path not allowed")?;
    let path = if let Some(relative) = supplied
        .strip_prefix("~/")
        .or_else(|| supplied.strip_prefix("~\\"))
    {
        home.join(relative)
    } else {
        PathBuf::from(supplied)
    };
    let path = resolve_path(&path)?;
    let roots = [".claude/projects", ".codex/sessions", ".agent-rails"]
        .iter()
        .map(|suffix| resolve_path(&home.join(suffix)))
        .collect::<Result<Vec<_>, _>>()?;
    if !local_allowed(&path, &roots, true, None) {
        return Err("path not allowed".into());
    }
    if op == "list" {
        // Entry names only, for card folders such as ~/.agent-rails/agents, as the remote helper.
        let entries = match std::fs::read_dir(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(json!({"names": []})),
            other => other.map_err(|e| e.to_string())?,
        };
        if resolve_path(&path)? != path {
            return Err("path not allowed".into());
        }
        let mut names = entries
            .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
            .collect::<Vec<_>>();
        names.sort();
        names.truncate(256);
        return Ok(json!({"names": names}));
    }
    match std::fs::metadata(&path) {
        Ok(info) if !info.is_file() => return Err("path not allowed".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && op == "stat" => {
            return Ok(json!({"exists":false,"size":0,"mtime_ms":0,"inode":0}))
        }
        Err(e) => return Err(e.to_string()),
        _ => {}
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // Agent logs remain writable while we read; handle identity is revalidated below.
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
        };
        options.share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE);
    }
    let mut file = options.open(&path).map_err(|e| e.to_string())?;
    let info = file.metadata().map_err(|e| e.to_string())?;
    let identity = file_identity(&file)?;
    let real = resolve_path(&path)?;
    if real != path || !local_allowed(&real, &roots, info.is_file(), identity.2) {
        return Err("path not allowed".into());
    }
    let current = options.open(&real).map_err(|e| e.to_string())?;
    if file_identity(&current)? != identity {
        return Err("path not allowed".into());
    }
    let mtime = info
        .modified()
        .map_err(|e| e.to_string())?
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64;
    if op == "stat" {
        return Ok(json!({"exists":true,"size":info.len(),"mtime_ms":mtime,"inode":identity.1}));
    }
    let offset = request["offset"].as_u64().ok_or("invalid read range")?;
    let max = request["max"]
        .as_u64()
        .filter(|n| *n <= u64::from(u32::MAX))
        .ok_or("invalid read range")?;
    let mut data = Vec::new();
    if offset < info.len() {
        file.seek(SeekFrom::Start(offset))
            .map_err(|e| e.to_string())?;
        file.take(max.min(2_097_152).min(info.len() - offset))
            .read_to_end(&mut data)
            .map_err(|e| e.to_string())?;
    }
    Ok(
        json!({"size":info.len(),"mtime_ms":mtime,"inode":identity.1,"offset":offset,"data_b64":STANDARD.encode(data)}),
    )
}

async fn request(
    files: Files,
    machines: Machines,
    machine: String,
    args: Value,
) -> Result<Value, String> {
    if machines.is_local(&machine)? {
        return tauri::async_runtime::spawn_blocking(move || {
            let home = std::env::var_os("USERPROFILE")
                .map(PathBuf::from)
                .ok_or("USERPROFILE not set")?;
            local_request(&home, &args)
        })
        .await
        .map_err(|e| e.to_string())?;
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    // Pure path policy has prefix, root, file-type and link-count edge cases.
    // This is the allowlist owner; remote-helper tests cannot cover local Rust policy.
    #[test]
    fn allowlist_boundaries() {
        let roots = vec![
            PathBuf::from("profile/.claude/projects"),
            PathBuf::from("profile/.codex/sessions"),
            PathBuf::from("profile/.agent-rails"),
        ];
        for root in &roots {
            assert!(local_allowed(&root.join("a.jsonl"), &roots, true, Some(1)));
            assert!(local_allowed(&root.join("nested/a"), &roots, true, None));
            assert!(!local_allowed(root, &roots, true, Some(1)));
            assert!(!local_allowed(&root.join("a"), &roots, false, Some(1)));
            assert!(!local_allowed(&root.join("a"), &roots, true, Some(2)));
            assert!(!local_allowed(
                &PathBuf::from(format!("{}-other/a", root.display())),
                &roots,
                true,
                Some(1)
            ));
        }
        assert!(!local_allowed(Path::new("outside/a"), &roots, true, None));
    }
    // Real filesystem boundary protects canonicalization, missing files, and range reads.
    #[test]
    fn local_file_boundary() -> Result<(), Box<dyn std::error::Error>> {
        let home = std::env::temp_dir().join(format!("herdr-local-files-{}", std::process::id()));
        std::fs::create_dir_all(home.join(".codex/sessions"))?;
        let run = || -> Result<(), Box<dyn std::error::Error>> {
            let path = home.join(".codex/sessions/log.jsonl");
            std::fs::write(&path, b"hello world")?;
            for name in ["recruiter", "frank"] {
                std::fs::create_dir_all(home.join(".agent-rails/agents").join(name))?;
            }
            assert_eq!(
                local_request(&home, &json!({"op":"list","path":"~/.agent-rails/agents"}))?["names"],
                json!(["frank", "recruiter"])
            );
            assert_eq!(
                local_request(&home, &json!({"op":"list","path":"~/.agent-rails/none"}))?["names"],
                json!([])
            );
            assert!(local_request(&home, &json!({"op":"list","path":"~/.agent-rails/../outside"})).is_err());
            assert!(local_request(&home, &json!({"op":"list","path":path})).is_err());
            assert_eq!(
                local_request(&home, &json!({"op":"stat","path":path}))?["exists"],
                true
            );
            assert_eq!(
                local_request(&home, &json!({"op":"read","path":path,"offset":6,"max":99}))?
                    ["data_b64"],
                "d29ybGQ="
            );
            assert_eq!(
                local_request(
                    &home,
                    &json!({"op":"read","path":path,"offset":99,"max":99})
                )?["data_b64"],
                ""
            );
            assert_eq!(
                local_request(
                    &home,
                    &json!({"op":"stat","path":"~/.codex/sessions/missing"})
                )?["exists"],
                false
            );
            assert!(local_request(
                &home,
                &json!({"op":"stat","path":"~/.codex/sessions/../../outside"})
            )
            .is_err());
            assert!(local_request(&home, &json!({"op":"stat","path":"relative/log"})).is_err());
            assert!(
                local_request(&home, &json!({"op":"stat","path":"~/.codex/sessions"})).is_err()
            );
            assert!(
                local_request(&home, &json!({"op":"read","path":path,"offset":-1,"max":1}))
                    .is_err()
            );
            std::fs::hard_link(&path, home.join(".codex/sessions/alias"))?;
            assert!(
                local_request(&home, &json!({"op":"read","path":path,"offset":0,"max":99}))
                    .is_err()
            );
            #[cfg(unix)]
            {
                std::fs::write(home.join("outside"), b"secret fixture")?;
                std::os::unix::fs::symlink(
                    home.join("outside"),
                    home.join(".codex/sessions/escape"),
                )?;
                assert!(local_request(
                    &home,
                    &json!({"op":"stat","path":"~/.codex/sessions/escape"})
                )
                .is_err());
            }
            Ok(())
        };
        let result = run();
        std::fs::remove_dir_all(&home)?;
        result
    }
}
