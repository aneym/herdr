#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod bridge;
mod control;
mod machines;
use tauri::Manager;

use serde::Serialize;

#[derive(Serialize)]
struct AppInfo {
    version: String,
    commit: String,
    built_at: String,
}

#[tauri::command]
fn app_info() -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
        commit: env!("HERDR_SHELL_COMMIT").to_string(),
        built_at: env!("HERDR_SHELL_BUILT_AT").to_string(),
    }
}

#[tauri::command]
fn ctl_read_result(text: String) -> Result<(), String> {
    control::deliver_read(text)
}

#[tauri::command]
fn ctl_ui_result(result: serde_json::Value) -> Result<(), String> {
    control::deliver_result("ui", result)
}

#[tauri::command]
fn ctl_open_result(result: serde_json::Value) -> Result<(), String> {
    control::deliver_result("open", result)
}

#[tauri::command]
fn ctl_key_result(result: serde_json::Value) -> Result<(), String> {
    control::deliver_result("key", result)
}

#[tauri::command]
fn ctl_chat_result(result: serde_json::Value) -> Result<(), String> {
    control::deliver_result("chat", result)
}

#[tauri::command]
fn ctl_wheel_result(result: serde_json::Value) -> Result<(), String> {
    control::deliver_result("wheel", result)
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            app_info,
            ctl_read_result,
            ctl_ui_result,
            ctl_open_result,
            ctl_key_result,
            ctl_chat_result,
            ctl_wheel_result,
            bridge::machines_list,
            bridge::snapshot,
            bridge::api_request,
            bridge::attach_open,
            bridge::attach_input,
            bridge::attach_resize,
            bridge::attach_scroll,
            bridge::attach_take_control,
            bridge::attach_close,
            bridge::open_url,
            bridge::clipboard_read,
            bridge::clipboard_write
        ])
        .setup(|app| {
            let test_window = std::env::args().any(|a| a == "--test-window")
                || std::env::var("HERDR_SHELL_TEST_WINDOW").as_deref() == Ok("1");
            let config = app
                .config()
                .app
                .windows
                .iter()
                .find(|w| w.label == "main")
                .ok_or("missing main window config")?;
            let mut window = tauri::WebviewWindowBuilder::from_config(app, config)?;
            if test_window {
                window = window
                    .position(-20000.0, 0.0)
                    .skip_taskbar(true)
                    .focused(false);
            }
            window.build()?;
            app.manage(bridge::Attaches::default());
            app.manage(machines::Machines::start(app.handle().clone())?);
            control::start(app.handle().clone());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Herdr Shell");
}
