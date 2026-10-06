#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod control;

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
fn ctl_read_result(text: String) {
    control::deliver_read(text);
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![app_info, ctl_read_result])
        .setup(|app| {
            control::start(app.handle().clone());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Herdr Shell");
}
