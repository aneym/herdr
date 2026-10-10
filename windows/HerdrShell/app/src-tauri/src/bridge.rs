use crate::machines::{MachineStatus, Machines};
use base64::{engine::general_purpose::STANDARD, Engine};
use herdr_shell_core::{
    api::{ApiClient, ApiError},
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
    /// OSC 52 data from the pane's program, still base64 as the server sent it.
    Clipboard {
        b64: String,
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
) -> Result<Value, Value> {
    let (endpoint, _) = machines.endpoints(&machine).map_err(Value::String)?;
    tauri::async_runtime::spawn_blocking(move || {
        (if method == "clipboard.image.write" {
            clipboard_image_request(endpoint, params)
        } else {
            ApiClient::new(endpoint).request(&method, params)
        })
        .map_err(|e| match e {
            ApiError::Server {
                code,
                message,
                reason,
            } => serde_json::json!({"code": code, "message": message, "reason": reason}),
            other => Value::String(other.to_string()),
        })
    })
    .await
    .map_err(|e| Value::String(e.to_string()))?
}
// Match the Mac clipboard exchange's request ID and 30-second deadline without
// changing timeouts for ordinary API calls. A timed-out reply is never pasted.
fn clipboard_image_request(
    endpoint: herdr_shell_core::endpoint::Endpoint,
    params: Value,
) -> Result<Value, ApiError> {
    use herdr_shell_core::endpoint::Conn;
    use std::io::{BufRead, BufReader, Write};
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let result = (|| {
            let mut stream = endpoint.connect()?;
            if let Conn::Tcp(tcp) = &mut stream {
                tcp.set_read_timeout(Some(Duration::from_secs(30)))?;
                tcp.set_write_timeout(Some(Duration::from_secs(30)))?;
            }
            let mut body = serde_json::to_vec(&serde_json::json!({
                "id": "shell:clipboard-image", "method": "clipboard.image.write", "params": params
            }))
            .map_err(|e| ApiError::InvalidResponse(e.to_string()))?;
            body.push(b'\n');
            stream.write_all(&body)?;
            stream.flush()?;
            let mut line = String::new();
            BufReader::new(stream).read_line(&mut line)?;
            let reply: Value = serde_json::from_str(&line)
                .map_err(|e| ApiError::InvalidResponse(e.to_string()))?;
            if let Some(error) = reply.get("error") {
                return Err(ApiError::InvalidResponse(error.to_string()));
            }
            reply
                .get("result")
                .cloned()
                .ok_or_else(|| ApiError::InvalidResponse("missing clipboard result".into()))
        })();
        let _ = tx.send(result);
    });
    rx.recv_timeout(Duration::from_secs(30))
        .map_err(|e| ApiError::InvalidResponse(format!("clipboard image upload failed: {e}")))?
}
#[tauri::command]
pub async fn snapshot(machines: State<'_, Machines>, machine: String) -> Result<Value, String> {
    let result = api_request(
        machines,
        machine,
        "session.snapshot".into(),
        serde_json::json!({}),
    )
    .await
    .map_err(|error| {
        error
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| error.to_string())
    })?;
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
        "takeover" => AttachMode::Takeover,
        "observe" => AttachMode::Observe,
        _ => return Err("mode must be attach, takeover or observe".into()),
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
pub fn attach_theme(
    attaches: State<'_, Attaches>,
    handle: u32,
    dark: bool,
    foreground: [u8; 3],
    background: [u8; 3],
) -> Result<(), String> {
    use herdr_shell_core::wire::{
        ClientHostAppearance, ClientHostColor, ClientHostDefaultColorKind, ClientHostThemeUpdate,
    };
    let color = |[r, g, b]: [u8; 3]| ClientHostColor { r, g, b };
    attaches
        .get(handle)?
        .host_theme(vec![
            ClientHostThemeUpdate::DefaultColor {
                kind: ClientHostDefaultColorKind::Foreground,
                color: color(foreground),
            },
            ClientHostThemeUpdate::DefaultColor {
                kind: ClientHostDefaultColorKind::Background,
                color: color(background),
            },
            ClientHostThemeUpdate::Appearance(if dark {
                ClientHostAppearance::Dark
            } else {
                ClientHostAppearance::Light
            }),
        ])
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
            AttachEvent::Clipboard { data } => AttachEventJs::Clipboard { b64: data },
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
/// Image-only user pastes encode the native RGBA clipboard as PNG, never a
/// client-local filename. No image is a normal clipboard state.
#[tauri::command]
pub fn clipboard_read_image() -> Result<Option<String>, String> {
    let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    let image = match clipboard.get_image() {
        Ok(image) => image,
        Err(arboard::Error::ContentNotAvailable) => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    let width = u32::try_from(image.width).map_err(|_| "Clipboard image too wide")?;
    let height = u32::try_from(image.height).map_err(|_| "Clipboard image too tall")?;
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
        writer
            .write_image_data(&image.bytes)
            .map_err(|e| e.to_string())?;
    }
    if bytes.is_empty() || bytes.len() > 16 * 1024 * 1024 {
        return Err("Clipboard image upload failed: PNG exceeds 16 MiB".into());
    }
    Ok(Some(STANDARD.encode(bytes)))
}
#[tauri::command]
pub fn clipboard_write(text: String) -> Result<(), String> {
    arboard::Clipboard::new()
        .and_then(|mut c| c.set_text(text))
        .map_err(|e| e.to_string())
}

fn validate_action(verb: &str, args: &[String]) -> Result<(), String> {
    if !matches!(verb, "park" | "unpark" | "approve") {
        return Err("invalid action".into());
    }
    let target = args.first().ok_or("missing target")?;
    if target.is_empty()
        || target.starts_with('-')
        || !target
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._:-".contains(&b))
        || args.iter().any(|a| a.contains('\0'))
    {
        return Err("invalid target or arguments".into());
    }
    let valid = match verb {
        "approve" => {
            args.len() == 3
                && args[1].starts_with("--quote=")
                && !args[1][8..].trim().is_empty()
                && args[2] == "--by=alex"
                && target.len() <= 81
                && target
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
                && target.as_bytes()[0].is_ascii_alphanumeric()
        }
        "park" => args.len() == 1 || (args.len() == 2 && args[1].starts_with("--note=")),
        _ => args.len() == 1,
    };
    if valid {
        Ok(())
    } else {
        Err("invalid action arguments".into())
    }
}

#[tauri::command]
pub async fn remote_action(
    machines: State<'_, Machines>,
    machine: String,
    verb: String,
    args: Vec<String>,
) -> Result<Value, String> {
    validate_action(&verb, &args)?;
    let machines = machines.inner().clone();
    tauri::async_runtime::spawn_blocking(move || machines.remote_action(&machine, &verb, &args))
        .await
        .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod action_tests {
    use super::validate_action;
    // Pure allow-list has adversarial target, verb and option edge cases.
    #[test]
    fn action_allowlist() {
        for (verb, args, valid) in [
            ("park", vec!["w1:t1", "--note=a '; $(bad)"], true),
            ("unpark", vec!["w1:t1"], true),
            (
                "approve",
                vec!["scope-name", "--quote=approved", "--by=alex"],
                true,
            ),
            ("exec", vec!["w1:t1"], false),
            ("park", vec!["../bad"], false),
            ("park", vec!["--help"], false),
            ("unpark", vec!["-tab"], false),
            ("park", vec!["w1:t1", "--by=other"], false),
            ("unpark", vec!["w1:t1", "--note=x"], false),
            (
                "approve",
                vec!["scope-name", "--quote= ", "--by=alex"],
                false,
            ),
        ] {
            assert_eq!(
                validate_action(
                    verb,
                    &args.into_iter().map(str::to_owned).collect::<Vec<_>>()
                )
                .is_ok(),
                valid
            );
        }
    }
}

/// Stage dropped images through the same PNG host upload as clipboard images.
/// Directories (even ones named *.png) and ordinary files remain path drops.
#[tauri::command]
pub fn drop_read_image(path: String) -> Result<Option<String>, String> {
    let path = std::path::Path::new(&path);
    let extension = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if !matches!(
        extension.as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp"
    ) {
        return Ok(None);
    }
    let metadata = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if metadata.is_dir() {
        return Ok(None);
    }
    if metadata.len() > 16 * 1024 * 1024 {
        return Err("Dropped image exceeds 16 MiB".into());
    }
    #[cfg(windows)]
    {
        let mut reader = image::ImageReader::open(path).map_err(|e| e.to_string())?;
        let mut limits = image::Limits::default();
        limits.max_alloc = Some(64 * 1024 * 1024);
        reader.limits(limits);
        let image = reader.decode().map_err(|e| e.to_string())?;
        let mut bytes = std::io::Cursor::new(Vec::new());
        image
            .write_to(&mut bytes, image::ImageFormat::Png)
            .map_err(|e| e.to_string())?;
        let bytes = bytes.into_inner();
        if bytes.is_empty() || bytes.len() > 16 * 1024 * 1024 {
            return Err("Dropped image PNG exceeds 16 MiB".into());
        }
        Ok(Some(STANDARD.encode(bytes)))
    }
    #[cfg(not(windows))]
    {
        Err("Image drops are available on Windows only".into())
    }
}
