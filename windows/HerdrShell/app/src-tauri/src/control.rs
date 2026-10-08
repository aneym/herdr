fn control_pipe_name(user: &str, test_window: bool) -> String {
    let suffix = if test_window { "-test" } else { "" };
    format!(r"\\.\pipe\herdr-shell-control-{user}{suffix}")
}

// A retry always yields; repeated failures replace the listener while retaining
// its predecessor until the replacement exists.
#[cfg(any(windows, test))]
fn pipe_retry(failures: u32) -> (std::time::Duration, bool) {
    (std::time::Duration::from_millis(100), failures >= 5)
}

#[cfg(test)]
mod tests {
    use super::{control_pipe_name, pipe_retry};

    // Pure retry policy: the table covers both recovery and persistent errors.
    #[test]
    fn pipe_retry_yields_and_eventually_replaces() {
        for failures in [1, 2, 3, 4, 5, 6, u32::MAX] {
            let (delay, replace) = pipe_retry(failures);
            assert!(delay >= std::time::Duration::from_millis(100));
            assert_eq!(replace, failures >= 5);
        }
    }

    #[test]
    fn control_pipe_names_keep_production_and_test_separate() {
        for user in ["aneym", "user with spaces", "用户"] {
            assert_eq!(
                control_pipe_name(user, false),
                format!(r"\\.\pipe\herdr-shell-control-{user}")
            );
            assert_eq!(
                control_pipe_name(user, true),
                format!(r"\\.\pipe\herdr-shell-control-{user}-test")
            );
        }
    }
}

#[cfg(windows)]
mod imp {
    use serde_json::{json, Value};
    use std::sync::mpsc::{channel, Sender};
    use std::sync::Mutex;
    use std::time::Duration;
    use tauri::{AppHandle, Emitter, Manager};
    use windows_sys::Win32::Foundation::{
        CloseHandle, GetLastError, ERROR_NO_DATA, ERROR_PIPE_CONNECTED, HANDLE, HWND,
        INVALID_HANDLE_VALUE, RECT,
    };
    use windows_sys::Win32::Graphics::Gdi::{
        BitBlt, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC,
        ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, SRCCOPY,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        FlushFileBuffers, ReadFile, WriteFile, FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX,
    };
    use windows_sys::Win32::Storage::Xps::PrintWindow;
    use windows_sys::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE,
        PIPE_TYPE_BYTE, PIPE_WAIT,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetClientRect, GetForegroundWindow, IsWindowVisible, PW_RENDERFULLCONTENT,
    };

    // windows-sys does not export PW_CLIENTONLY from WindowsAndMessaging; value from WinUser.h.
    const PW_CLIENTONLY: u32 = 0x1;

    static RESULT_TX: Mutex<Option<(String, Sender<Value>)>> = Mutex::new(None);

    static ACTION_TX: Mutex<Option<(String, Sender<Value>)>> = Mutex::new(None);

    static MOTION_RX: Mutex<Option<std::sync::mpsc::Receiver<Value>>> = Mutex::new(None);
    static MOTION_TX: Mutex<Option<Sender<Value>>> = Mutex::new(None);

    pub fn deliver_motion(result: Value) -> Result<(), String> {
        if let Some(tx) = MOTION_TX.lock().map_err(|_| "motion lock poisoned")?.take() {
            let _ = tx.send(result);
        }
        Ok(())
    }

    static READ_TX: Mutex<Option<Sender<String>>> = Mutex::new(None);

    pub fn deliver_read(text: String) -> Result<(), String> {
        if let Some(tx) = READ_TX
            .lock()
            .map_err(|_| "control read lock poisoned")?
            .take()
        {
            let _ = tx.send(text);
        }
        Ok(())
    }
    pub fn deliver_result(cmd: &str, result: Value) -> Result<(), String> {
        let mut guard = result_slot(cmd)
            .lock()
            .map_err(|_| "control result lock poisoned")?;
        if guard.as_ref().is_some_and(|(pending, _)| pending == cmd) {
            if let Some((_, tx)) = guard.take() {
                let _ = tx.send(result);
            }
        }
        Ok(())
    }

    pub fn start(app: AppHandle, test_window: bool) {
        std::thread::spawn(move || serve(app, test_window));
    }

    static LAST_ERROR: Mutex<Option<u32>> = Mutex::new(None);

    fn log_pipe_error(pipe: &str, error: u32, message: &str) {
        use std::io::Write;
        {
            let mut last = LAST_ERROR.lock().unwrap_or_else(|error| error.into_inner());
            if *last == Some(error) {
                return;
            }
            *last = Some(error);
        }
        tracing::warn!(pipe, error, message);
        let logged = (|| -> std::io::Result<()> {
            let base = std::env::var_os("LOCALAPPDATA").ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::NotFound, "LOCALAPPDATA not set")
            })?;
            let logs = std::path::PathBuf::from(base)
                .join("HerdrShell")
                .join("logs");
            std::fs::create_dir_all(&logs)?;
            let mut log = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(logs.join("control.log"))?;
            writeln!(log, "{pipe}: {message} (Windows error {error})")
        })();
        if let Err(log_error) = logged {
            eprintln!("{pipe}: {message} (Windows error {error}); log: {log_error}");
        }
    }

    fn serve(app: AppHandle, test_window: bool) {
        let user = std::env::var("USERNAME").unwrap_or_else(|_| "user".to_string());
        let pipe = super::control_pipe_name(&user, test_window);
        let name: Vec<u16> = pipe.encode_utf16().chain(std::iter::once(0)).collect();
        let create = |first| unsafe {
            CreateNamedPipeW(
                name.as_ptr(),
                PIPE_ACCESS_DUPLEX
                    | if first {
                        FILE_FLAG_FIRST_PIPE_INSTANCE
                    } else {
                        0
                    },
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
                255,
                65536,
                65536,
                0,
                std::ptr::null(),
            )
        };
        let mut h = create(true);
        if h == INVALID_HANDLE_VALUE {
            log_pipe_error(
                &pipe,
                unsafe { GetLastError() },
                "control server disabled: cannot own pipe",
            );
            return;
        }
        let mut failures = 0u32;
        loop {
            unsafe {
                if ConnectNamedPipe(h, std::ptr::null_mut()) == 0 {
                    let error = GetLastError();
                    if error != ERROR_PIPE_CONNECTED {
                        let disconnected = DisconnectNamedPipe(h) != 0;
                        if error == ERROR_NO_DATA && disconnected {
                            failures = 0;
                        } else {
                            log_pipe_error(&pipe, error, "cannot connect pipe; retrying");
                            failures = failures.saturating_add(1);
                            let (delay, replace) = super::pipe_retry(failures);
                            std::thread::sleep(delay);
                            if replace || (error != ERROR_NO_DATA && !disconnected) {
                                // Reserve before close: never release the pipe name.
                                let mut next = INVALID_HANDLE_VALUE;
                                while next == INVALID_HANDLE_VALUE {
                                    std::thread::sleep(Duration::from_secs(1));
                                    next = create(false);
                                    if next == INVALID_HANDLE_VALUE {
                                        log_pipe_error(
                                            &pipe,
                                            GetLastError(),
                                            "cannot recreate listener; retrying",
                                        );
                                    }
                                }
                                CloseHandle(h);
                                h = next;
                                failures = 0;
                            }
                        }
                        continue;
                    }
                }
                failures = 0;
                *LAST_ERROR.lock().unwrap_or_else(|error| error.into_inner()) = None;
                // Reserve the next listener before the connection can close.
                // If creation fails, retain this handle until a replacement exists.
                let mut next = create(false);
                while next == INVALID_HANDLE_VALUE {
                    log_pipe_error(
                        &pipe,
                        GetLastError(),
                        "cannot create next listener; retrying",
                    );
                    std::thread::sleep(Duration::from_secs(1));
                    next = create(false);
                }
                // One thread per connection: a client that never reads its reply blocks
                // only its own FlushFileBuffers, never the listener.
                let raw = h as usize;
                let app = app.clone();
                std::thread::spawn(move || {
                    let h = raw as HANDLE;
                    handle_conn(h, &app, test_window);
                    DisconnectNamedPipe(h);
                    CloseHandle(h);
                });
                h = next;
            }
        }
    }

    fn handle_conn(h: HANDLE, app: &AppHandle, test_window: bool) {
        let mut buf = Vec::new();
        let mut tmp = [0u8; 4096];
        loop {
            let mut n = 0u32;
            let ok = unsafe {
                ReadFile(
                    h,
                    tmp.as_mut_ptr(),
                    tmp.len() as u32,
                    &mut n,
                    std::ptr::null_mut(),
                )
            };
            if ok == 0 || n == 0 {
                return;
            }
            buf.extend_from_slice(&tmp[..n as usize]);
            if buf.contains(&b'\n') || buf.len() > 1_048_576 {
                break;
            }
        }
        let line = String::from_utf8_lossy(&buf);
        let req: Value = serde_json::from_str(line.trim()).unwrap_or_else(|_| json!({}));
        // Requests share the result slots, so dispatch one at a time; only the reply
        // write and flush run concurrently.
        static DISPATCH: Mutex<()> = Mutex::new(());
        let resp = {
            let _turn = DISPATCH
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            dispatch(app, &req, test_window)
        };
        let out = format!("{resp}\n");
        let mut rest = out.as_bytes();
        while !rest.is_empty() {
            let mut w = 0u32;
            let ok = unsafe {
                WriteFile(
                    h,
                    rest.as_ptr(),
                    rest.len() as u32,
                    &mut w,
                    std::ptr::null_mut(),
                )
            };
            if ok == 0 || w == 0 {
                return;
            }
            rest = &rest[w as usize..];
        }
        // DisconnectNamedPipe discards unread bytes; wait until the client has read the reply.
        unsafe {
            FlushFileBuffers(h);
        }
    }

    fn dispatch(app: &AppHandle, req: &Value, test_window: bool) -> Value {
        if matches!(req["cmd"].as_str(), Some("open_detail" | "row_menu" | "paste_image"))
            && !test_window
        {
            return json!({"ok": false, "error": "command requires --test-window"});
        }
        // Motion acknowledges enqueue, but subsequent commands (including native
        // screenshots) wait for its frontend completion before they may run.
        let pending_motion = MOTION_RX.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(rx) = pending_motion {
            match rx.recv_timeout(Duration::from_secs(5)) {
                Ok(result) if result["ok"] == true => {}
                Ok(result) => {
                    eprintln!("motion hook failed: {result}");
                    return result;
                }
                Err(error) => {
                    eprintln!("motion hook completion: {error}");
                    // Keep the barrier on timeout; no later command may overtake
                    // an accepted freeze that the frontend has not applied yet.
                    *MOTION_RX.lock().unwrap_or_else(|e| e.into_inner()) = Some(rx);
                    return json!({"ok": false, "error": "motion completion timeout"});
                }
            }
        }
        match req.get("cmd").and_then(|c| c.as_str()).unwrap_or("") {
            "ping" => json!({"ok": true, "commit": env!("HERDR_SHELL_COMMIT")}),
            "state" => state(app),
            "shot" => {
                let out = req.get("out").and_then(|o| o.as_str()).unwrap_or("");
                if out.is_empty() {
                    return json!({"ok": false, "error": "missing out"});
                }
                match shot(app, out) {
                    Ok((w, h)) => json!({"ok": true, "out": out, "w": w, "h": h}),
                    Err(e) => json!({"ok": false, "error": e}),
                }
            }
            "type" => {
                let text = req.get("text").and_then(|t| t.as_str()).unwrap_or("");
                match app.emit("ctl-type", text) {
                    Ok(()) => json!({"ok": true}),
                    Err(e) => json!({"ok": false, "error": e.to_string()}),
                }
            }
            "read" => read_cmd(app),
            "appearance" => {
                let theme = match req.get("mode").and_then(|m| m.as_str()).unwrap_or("system") {
                    "system" => None,
                    "light" => Some(tauri::Theme::Light),
                    "dark" => Some(tauri::Theme::Dark),
                    _ => return json!({"ok": false, "error": "mode must be system, light or dark"}),
                };
                // The window theme drives the DWM title bar and the WebView's color scheme;
                // the UI then applies the same override to its own tokens.
                if let Some(w) = app.get_webview_window("main") {
                    if let Err(e) = w.set_theme(theme) {
                        return json!({"ok": false, "error": e.to_string()});
                    }
                }
                forward_cmd(app, "appearance", req)
            }
            cmd @ ("ui" | "machine" | "open" | "key" | "wheel" | "action" | "chat" | "update"
            | "drag_pane" | "motion" | "drag_pin" | "drag_divider" | "link_click"
            | "copy_selection" | "open_detail" | "row_menu" | "paste_image") => forward_cmd(app, cmd, req),
            _ => json!({"ok": false, "error": "unknown cmd"}),
        }
    }

    fn read_cmd(app: &AppHandle) -> Value {
        let (tx, rx) = channel();
        match READ_TX.lock() {
            Ok(mut guard) => *guard = Some(tx),
            Err(_) => return json!({"ok":false,"error":"control read lock poisoned"}),
        }
        let result = if app.emit("ctl-read", ()).is_err() {
            json!({"ok":false,"error":"frontend not reachable"})
        } else {
            match rx.recv_timeout(Duration::from_secs(5)) {
                Ok(text) => json!({"ok":true,"text":text}),
                Err(_) => json!({"ok":false,"error":"read timeout"}),
            }
        };
        match READ_TX.lock() {
            Ok(mut guard) => {
                *guard = None;
                result
            }
            Err(_) => json!({"ok":false,"error":"control read lock poisoned"}),
        }
    }
    fn result_slot(cmd: &str) -> &'static Mutex<Option<(String, Sender<Value>)>> {
        if cmd == "action" {
            &ACTION_TX
        } else {
            &RESULT_TX
        }
    }

    fn forward_cmd(app: &AppHandle, cmd: &str, req: &Value) -> Value {
        // These hooks acknowledge enqueue, not execution: a frozen/throttled view
        // must not hold the named pipe open. Other commands retain result replies.
        if cmd == "motion"
            || (cmd == "drag_pane"
                && req["hold"] == true
                && req["cancel"] != true
                && req["release"] != true)
        {
            let mut payload = req.clone();
            if let Some(object) = payload.as_object_mut() {
                object.remove("cmd");
            }
            if cmd == "motion" {
                let (tx, rx) = channel();
                *MOTION_TX.lock().unwrap_or_else(|e| e.into_inner()) = Some(tx);
                *MOTION_RX.lock().unwrap_or_else(|e| e.into_inner()) = Some(rx);
            }
            return match app.emit(&format!("ctl-{cmd}"), payload) {
                Ok(()) => json!({"ok": true, "queued": true}),
                Err(_) => {
                    if cmd == "motion" {
                        *MOTION_TX.lock().unwrap_or_else(|e| e.into_inner()) = None;
                        *MOTION_RX.lock().unwrap_or_else(|e| e.into_inner()) = None;
                    }
                    json!({"ok": false, "error": "frontend not reachable"})
                }
            };
        }
        let (tx, rx) = channel();
        match result_slot(cmd).lock() {
            Ok(mut guard) => *guard = Some((cmd.into(), tx)),
            Err(_) => return json!({"ok":false,"error":"control result lock poisoned"}),
        }
        let mut payload = req.clone();
        if let Some(object) = payload.as_object_mut() {
            object.remove("cmd");
        }
        let result = if app.emit(&format!("ctl-{cmd}"), payload).is_err() {
            json!({"ok":false,"error":"frontend not reachable"})
        } else {
            match rx.recv_timeout(Duration::from_secs(3)) {
                Ok(result) => result,
                Err(_) => json!({"ok":false,"error":format!("{cmd} timeout")}),
            }
        };
        match result_slot(cmd).lock() {
            Ok(mut guard) => {
                *guard = None;
                result
            }
            Err(_) => json!({"ok":false,"error":"control result lock poisoned"}),
        }
    }

    fn main_hwnd(app: &AppHandle) -> Option<HWND> {
        let w = app.get_webview_window("main")?;
        w.hwnd().ok().map(|h| h.0 as HWND)
    }

    fn state(app: &AppHandle) -> Value {
        let Some(h) = main_hwnd(app) else {
            return json!({"ok": false, "error": "no main window"});
        };
        unsafe {
            let mut rc: RECT = std::mem::zeroed();
            GetClientRect(h, &mut rc);
            json!({
                "ok": true,
                "visible": IsWindowVisible(h) != 0,
                "focused": GetForegroundWindow() == h,
                "size": [rc.right - rc.left, rc.bottom - rc.top],
            })
        }
    }

    fn shot(app: &AppHandle, out: &str) -> Result<(i32, i32), String> {
        let hwnd = main_hwnd(app).ok_or("no main window")?;
        unsafe {
            let mut rc: RECT = std::mem::zeroed();
            if GetClientRect(hwnd, &mut rc) == 0 {
                return Err("GetClientRect failed".into());
            }
            let (w, hgt) = (rc.right - rc.left, rc.bottom - rc.top);
            if w <= 0 || hgt <= 0 {
                return Err("zero-size window".into());
            }
            let mem = CreateCompatibleDC(std::ptr::null_mut());
            if mem.is_null() {
                return Err("CreateCompatibleDC failed".into());
            }
            let mut bmi: BITMAPINFO = std::mem::zeroed();
            bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
            bmi.bmiHeader.biWidth = w;
            bmi.bmiHeader.biHeight = -hgt;
            bmi.bmiHeader.biPlanes = 1;
            bmi.bmiHeader.biBitCount = 32;
            bmi.bmiHeader.biCompression = BI_RGB;
            let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
            let dib = CreateDIBSection(
                mem,
                &bmi,
                DIB_RGB_COLORS,
                &mut bits,
                std::ptr::null_mut(),
                0,
            );
            if dib.is_null() || bits.is_null() {
                DeleteDC(mem);
                return Err("CreateDIBSection failed".into());
            }
            let old = SelectObject(mem, dib);
            if PrintWindow(hwnd, mem, PW_CLIENTONLY | PW_RENDERFULLCONTENT) == 0 || pixels_black(bits, w, hgt) {
                let src = GetDC(hwnd);
                if !src.is_null() {
                    BitBlt(mem, 0, 0, w, hgt, src, 0, 0, SRCCOPY);
                    ReleaseDC(hwnd, src);
                }
            }
            let len = (w * hgt * 4) as usize;
            let bgra = std::slice::from_raw_parts(bits as *const u8, len);
            let mut rgba = Vec::with_capacity(len);
            for px in bgra.chunks_exact(4) {
                rgba.extend_from_slice(&[px[2], px[1], px[0], 255]);
            }
            SelectObject(mem, old);
            DeleteObject(dib);
            DeleteDC(mem);
            image::save_buffer(out, &rgba, w as u32, hgt as u32, image::ColorType::Rgba8)
                .map_err(|e| e.to_string())?;
            Ok((w, hgt))
        }
    }

    fn pixels_black(bits: *mut core::ffi::c_void, w: i32, h: i32) -> bool {
        let len = (w * h * 4) as usize;
        unsafe { std::slice::from_raw_parts(bits as *const u8, len) }
            .iter()
            .all(|&b| b == 0)
    }
}

#[cfg(windows)]
pub use imp::*;

#[cfg(not(windows))]
pub fn start(_app: tauri::AppHandle, _test_window: bool) {}

#[cfg(not(windows))]
pub fn deliver_read(_text: String) -> Result<(), String> {
    Ok(())
}

#[cfg(not(windows))]
pub fn deliver_result(_cmd: &str, _result: serde_json::Value) -> Result<(), String> {
    Ok(())
}

#[cfg(not(windows))]
pub fn deliver_motion(_result: serde_json::Value) -> Result<(), String> {
    Ok(())
}
