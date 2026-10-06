use crate::machines::{MachineStatus, Machines};
use base64::{engine::general_purpose::STANDARD, Engine};
use herdr_shell_core::{
    api::ApiClient,
    attach::{AttachClient, AttachEvent, AttachHandle, AttachMode},
    wire::AttachScrollDirection,
};
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU32, Ordering},
        mpsc::{Receiver, RecvTimeoutError},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tauri::{ipc::Channel, State};

#[derive(Default, Clone)]
pub struct Attaches {
    handles: Arc<Mutex<HashMap<u32, AttachHandle>>>,
    next: Arc<AtomicU32>,
}
impl Attaches {
    fn allocate(&self) -> Result<u32, String> {
        let mut current = self.next.load(Ordering::Relaxed);
        loop {
            let next = current.checked_add(1).ok_or("attach handles exhausted")?;
            match self.next.compare_exchange_weak(
                current,
                next,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => return Ok(current),
                Err(value) => current = value,
            }
        }
    }
    fn get(&self, id: u32) -> Result<AttachHandle, String> {
        self.handles
            .lock()
            .map_err(|_| "attach lock poisoned")?
            .get(&id)
            .cloned()
            .ok_or("unknown attach handle".into())
    }
}
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum AttachEventJs {
    Bytes {
        b64: String,
    },
    Mode {
        b64: String,
        mouse: bool,
        #[serde(rename = "sgrPixels")]
        sgr_pixels: bool,
        #[serde(rename = "kittyFlags")]
        kitty_flags: u16,
        #[serde(rename = "modifyOtherKeys")]
        modify_other_keys: u8,
    },
    Bell {
        count: u16,
    },
    Notice {
        message: String,
    },
    Closed {
        reason: String,
    },
}
#[tauri::command]
pub fn machines_list(machines: State<'_, Machines>) -> Result<Vec<MachineStatus>, String> {
    machines.list()
}
#[tauri::command]
pub async fn api_request(
    machines: State<'_, Machines>,
    machine: String,
    method: String,
    params: Value,
) -> Result<Value, String> {
    let (endpoint, _) = machines.endpoints(&machine)?;
    tauri::async_runtime::spawn_blocking(move || {
        ApiClient::new(endpoint)
            .request(&method, params)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn snapshot(machines: State<'_, Machines>, machine: String) -> Result<Value, String> {
    let result = api_request(
        machines,
        machine,
        "session.snapshot".into(),
        serde_json::json!({}),
    )
    .await?;
    result
        .get("snapshot")
        .cloned()
        .ok_or("session.snapshot missing snapshot".into())
}
#[tauri::command]
pub async fn attach_open(
    machines: State<'_, Machines>,
    attaches: State<'_, Attaches>,
    machine: String,
    terminal_id: String,
    cols: u16,
    rows: u16,
    mode: String,
    on_event: Channel<AttachEventJs>,
) -> Result<u32, String> {
    let (_, endpoint) = machines.endpoints(&machine)?;
    let mode = match mode.as_str() {
        "attach" => AttachMode::Attach,
        "observe" => AttachMode::Observe,
        _ => return Err("mode must be attach or observe".into()),
    };
    let attaches = attaches.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let client = AttachClient::connect(&endpoint, &terminal_id, cols, rows, mode)
            .map_err(|e| e.to_string())?;
        let id = attaches.allocate()?;
        let (handle, events) = client.into_parts();
        attaches
            .handles
            .lock()
            .map_err(|_| "attach lock poisoned")?
            .insert(id, handle.clone());
        std::thread::spawn(move || {
            forward(events, &on_event);
            let _ = handle.detach();
            if let Ok(mut guard) = attaches.handles.lock() {
                guard.remove(&id);
            }
        });
        Ok(id)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub fn attach_input(
    attaches: State<'_, Attaches>,
    handle: u32,
    data: String,
) -> Result<(), String> {
    let bytes = STANDARD.decode(data).map_err(|e| e.to_string())?;
    attaches
        .get(handle)?
        .send_input(&bytes)
        .map_err(|e| e.to_string())
}
#[tauri::command]
pub fn attach_resize(
    attaches: State<'_, Attaches>,
    handle: u32,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    attaches
        .get(handle)?
        .resize(cols, rows)
        .map_err(|e| e.to_string())
}
#[tauri::command]
pub fn attach_scroll(
    attaches: State<'_, Attaches>,
    handle: u32,
    up: bool,
    lines: u16,
) -> Result<(), String> {
    attaches
        .get(handle)?
        .scroll(
            if up {
                AttachScrollDirection::Up
            } else {
                AttachScrollDirection::Down
            },
            lines,
        )
        .map_err(|e| e.to_string())
}
#[tauri::command]
pub fn attach_take_control(attaches: State<'_, Attaches>, handle: u32) -> Result<(), String> {
    attaches
        .get(handle)?
        .take_control()
        .map_err(|e| e.to_string())
}
#[tauri::command]
pub fn attach_close(attaches: State<'_, Attaches>, handle: u32) -> Result<(), String> {
    let entry = attaches
        .handles
        .lock()
        .map_err(|_| "attach lock poisoned")?
        .remove(&handle);
    if let Some(entry) = entry {
        entry.detach().map_err(|e| e.to_string())?;
    }
    Ok(())
}
fn flush(bytes: &mut Vec<u8>, channel: &Channel<AttachEventJs>) -> bool {
    if bytes.is_empty() {
        return true;
    }
    let sent = channel
        .send(AttachEventJs::Bytes {
            b64: STANDARD.encode(&*bytes),
        })
        .is_ok();
    bytes.clear();
    sent
}
fn forward(events: Receiver<AttachEvent>, channel: &Channel<AttachEventJs>) {
    const LIMIT: usize = 256 * 1024;
    let mut bytes = Vec::new();
    let mut deadline = Instant::now() + Duration::from_millis(8);
    loop {
        if !bytes.is_empty() && Instant::now() >= deadline {
            if !flush(&mut bytes, channel) {
                return;
            }
        }
        let event = if bytes.is_empty() {
            events.recv().map_err(|_| RecvTimeoutError::Disconnected)
        } else {
            events.recv_timeout(deadline.saturating_duration_since(Instant::now()))
        };
        let event = match event {
            Ok(event) => event,
            Err(RecvTimeoutError::Timeout) => {
                if !flush(&mut bytes, channel) {
                    return;
                }
                continue;
            }
            Err(RecvTimeoutError::Disconnected) => {
                if flush(&mut bytes, channel) {
                    let _ = channel.send(AttachEventJs::Closed {
                        reason: "attach worker ended".into(),
                    });
                }
                return;
            }
        };
        if let AttachEvent::Bytes(chunk) = event {
            let mut remaining = chunk.as_slice();
            while !remaining.is_empty() {
                if bytes.is_empty() {
                    deadline = Instant::now() + Duration::from_millis(8);
                }
                let n = remaining.len().min(LIMIT - bytes.len());
                bytes.extend_from_slice(&remaining[..n]);
                remaining = &remaining[n..];
                if bytes.len() == LIMIT && !flush(&mut bytes, channel) {
                    return;
                }
            }
            continue;
        }
        if !flush(&mut bytes, channel) {
            return;
        }
        let closed = matches!(event, AttachEvent::Closed { .. });
        let event = match event {
            AttachEvent::ModeChange {
                mouse,
                keyboard,
                sequence,
            } => AttachEventJs::Mode {
                b64: STANDARD.encode(sequence),
                mouse: mouse.enabled,
                sgr_pixels: mouse.sgr_pixels,
                kitty_flags: keyboard.kitty_flags,
                modify_other_keys: keyboard.modify_other_keys_level,
            },
            AttachEvent::Bell { count } => AttachEventJs::Bell { count },
            AttachEvent::Notice { message } => AttachEventJs::Notice { message },
            AttachEvent::Closed { reason } => AttachEventJs::Closed { reason },
            AttachEvent::Bytes(_) => unreachable!(),
        };
        if channel.send(event).is_err() || closed {
            return;
        }
    }
}
#[tauri::command]
pub fn open_url(url: String) -> Result<(), String> {
    let parsed = tauri::Url::parse(&url).map_err(|e| e.to_string())?;
    if !matches!(parsed.scheme(), "http" | "https" | "mailto") {
        return Err("only http, https and mailto URLs are allowed".into());
    }
    #[cfg(windows)]
    {
        let url: Vec<u16> = parsed
            .as_str()
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let open: Vec<u16> = "open".encode_utf16().chain(std::iter::once(0)).collect();
        let result = unsafe {
            windows_sys::Win32::UI::Shell::ShellExecuteW(
                std::ptr::null_mut(),
                open.as_ptr(),
                url.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                1,
            )
        };
        if result as isize <= 32 {
            return Err(format!("ShellExecuteW failed: {}", result as isize));
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        Err("open_url is available on Windows only".into())
    }
}
#[tauri::command]
pub fn clipboard_read() -> Result<String, String> {
    arboard::Clipboard::new()
        .and_then(|mut c| c.get_text())
        .map_err(|e| e.to_string())
}
#[tauri::command]
pub fn clipboard_write(text: String) -> Result<(), String> {
    arboard::Clipboard::new()
        .and_then(|mut c| c.set_text(text))
        .map_err(|e| e.to_string())
}
