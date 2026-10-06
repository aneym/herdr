#[cfg(windows)]
mod imp {
    use serde_json::{json, Value};
    use std::sync::mpsc::{channel, Sender};
    use std::sync::Mutex;
    use std::time::Duration;
    use tauri::{AppHandle, Emitter, Manager};
    use windows_sys::Win32::Foundation::{
        CloseHandle, GetLastError, ERROR_PIPE_CONNECTED, HANDLE, HWND, INVALID_HANDLE_VALUE, RECT,
    };
    use windows_sys::Win32::Graphics::Gdi::{
        BitBlt, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetWindowDC,
        ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, SRCCOPY,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        FlushFileBuffers, ReadFile, WriteFile, PIPE_ACCESS_DUPLEX,
    };
    use windows_sys::Win32::Storage::Xps::PrintWindow;
    use windows_sys::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE,
        PIPE_TYPE_BYTE, PIPE_WAIT,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetClientRect, GetForegroundWindow, IsWindowVisible, PW_RENDERFULLCONTENT,
    };

    static RESULT_TX: Mutex<Option<(String, Sender<Value>)>> = Mutex::new(None);

    static ACTION_TX: Mutex<Option<(String, Sender<Value>)>> = Mutex::new(None);

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

    pub fn start(app: AppHandle) {
        std::thread::spawn(move || serve(app));
    }

    fn pipe_name() -> Vec<u16> {
        let user = std::env::var("USERNAME").unwrap_or_else(|_| "user".to_string());
        format!(r"\\.\pipe\herdr-shell-control-{user}")
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect()
    }

    fn serve(app: AppHandle) {
        let name = pipe_name();
        loop {
            unsafe {
                let h = CreateNamedPipeW(
                    name.as_ptr(),
                    PIPE_ACCESS_DUPLEX,
                    PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
                    255,
                    65536,
                    65536,
                    0,
                    std::ptr::null(),
                );
                if h == INVALID_HANDLE_VALUE {
                    std::thread::sleep(Duration::from_secs(1));
                    continue;
                }
                if ConnectNamedPipe(h, std::ptr::null_mut()) == 0
                    && GetLastError() != ERROR_PIPE_CONNECTED
                {
                    CloseHandle(h);
                    continue;
                }
                handle_conn(h, &app);
                DisconnectNamedPipe(h);
                CloseHandle(h);
            }
        }
    }

    fn handle_conn(h: HANDLE, app: &AppHandle) {
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
        let resp = dispatch(app, &req);
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

    fn dispatch(app: &AppHandle, req: &Value) -> Value {
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
            cmd @ ("ui" | "open" | "key" | "wheel" | "action" | "chat" | "update") => {
                forward_cmd(app, cmd, req)
            }
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
            if PrintWindow(hwnd, mem, PW_RENDERFULLCONTENT) == 0 || pixels_black(bits, w, hgt) {
                let src = GetWindowDC(hwnd);
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
pub fn start(_app: tauri::AppHandle) {}

#[cfg(not(windows))]
pub fn deliver_read(_text: String) -> Result<(), String> {
    Ok(())
}

#[cfg(not(windows))]
pub fn deliver_result(_cmd: &str, _result: serde_json::Value) -> Result<(), String> {
    Ok(())
}
