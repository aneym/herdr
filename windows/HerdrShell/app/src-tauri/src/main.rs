#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod bridge;
mod control;
mod files;
mod machines;
mod update;
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
fn ctl_motion_result(result: serde_json::Value) -> Result<(), String> {
    control::deliver_motion(result)
}

#[tauri::command]
fn ctl_ui_result(result: serde_json::Value) -> Result<(), String> {
    control::deliver_result("ui", result)
}

#[tauri::command]
fn ctl_update_result(result: serde_json::Value) -> Result<(), String> {
    control::deliver_result("update", result)
}

#[tauri::command]
fn ctl_machine_result(result: serde_json::Value) -> Result<(), String> {
    control::deliver_result("machine", result)
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
fn ctl_action_result(result: serde_json::Value) -> Result<(), String> {
    control::deliver_result("action", result)
}

#[tauri::command]
fn ctl_chat_result(result: serde_json::Value) -> Result<(), String> {
    control::deliver_result("chat", result)
}

#[tauri::command]
fn ctl_appearance_result(result: serde_json::Value) -> Result<(), String> {
    control::deliver_result("appearance", result)
}

#[tauri::command]
fn ctl_wheel_result(result: serde_json::Value) -> Result<(), String> {
    control::deliver_result("wheel", result)
}

#[tauri::command]
fn ctl_drag_pin_result(result: serde_json::Value) -> Result<(), String> {
    control::deliver_result("drag_pin", result)
}

#[tauri::command]
fn ctl_drag_divider_result(result: serde_json::Value) -> Result<(), String> {
    control::deliver_result("drag_divider", result)
}

#[tauri::command]
fn ctl_link_click_result(result: serde_json::Value) -> Result<(), String> {
    control::deliver_result("link_click", result)
}

#[tauri::command]
fn ctl_copy_selection_result(result: serde_json::Value) -> Result<(), String> {
    control::deliver_result("copy_selection", result)
}

#[tauri::command]
fn ctl_drag_pane_result(result: serde_json::Value) -> Result<(), String> {
    control::deliver_result("drag_pane", result)
}

#[tauri::command]
fn ctl_open_detail_result(result: serde_json::Value) -> Result<(), String> {
    control::deliver_result("open_detail", result)
}

#[tauri::command]
fn ctl_row_menu_result(result: serde_json::Value) -> Result<(), String> {
    control::deliver_result("row_menu", result)
}

#[tauri::command]
fn ctl_paste_image_result(result: serde_json::Value) -> Result<(), String> {
    control::deliver_result("paste_image", result)
}

#[tauri::command]
fn ctl_hover_result(result: serde_json::Value) -> Result<(), String> {
    control::deliver_result("hover", result)
}

#[tauri::command]
fn ctl_click_result(result: serde_json::Value) -> Result<(), String> {
    control::deliver_result("click", result)
}

#[tauri::command]
fn ctl_drop_paths_result(result: serde_json::Value) -> Result<(), String> {
    control::deliver_result("drop_paths", result)
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            app_info,
            update::update_status,
            update::update_apply,
            update::update_rollback,
            ctl_update_result,
            files::file_stat,
            files::file_read,
            files::remote_home,
            files::file_list,
            files::factory_snapshot,
            files::factory_route_pick,
            ctl_read_result,
            ctl_motion_result,
            ctl_ui_result,
            ctl_machine_result,
            ctl_open_result,
            ctl_key_result,
            ctl_action_result,
            ctl_chat_result,
            ctl_wheel_result,
            ctl_appearance_result,
            ctl_drag_pane_result,
            ctl_open_detail_result,
            ctl_row_menu_result,
            ctl_paste_image_result,
            ctl_drag_pin_result,
            ctl_drag_divider_result,
            ctl_link_click_result,
            ctl_copy_selection_result,
            ctl_drop_paths_result,
            ctl_click_result,
            ctl_hover_result,
            bridge::machines_list,
            bridge::snapshot,
            bridge::api_request,
            bridge::remote_action,
            bridge::attach_open,
            bridge::attach_input,
            bridge::attach_resize,
            bridge::attach_theme,
            bridge::attach_scroll,
            bridge::attach_take_control,
            bridge::attach_close,
            bridge::open_url,
            bridge::clipboard_read,
            bridge::clipboard_read_image,
            bridge::drop_read_image,
            bridge::clipboard_write
        ])
        .setup(|app| {
            let test_window = std::env::args().any(|a| a == "--test-window")
                || std::env::var("HERDR_SHELL_TEST_WINDOW").as_deref() == Ok("1");
            // Install relaunches open the window without taking focus from Alex.
            let background = std::env::args().any(|a| a == "--background");
            let control_motion = test_window
                || std::env::var("HERDR_SHELL_CONTROL").as_deref() == Ok("1");
            let mut config = app
                .config()
                .app
                .windows
                .iter()
                .find(|w| w.label == "main")
                .ok_or("missing main window config")?
                .clone();
            if control_motion {
                config.background_throttling = Some(tauri::utils::config::BackgroundThrottlingPolicy::Disabled);
            }
            let mut window = tauri::WebviewWindowBuilder::from_config(app, &config)?;
            // backgroundThrottling is unsupported by WebView2. Disable Chromium's
            // background timer/renderer throttling so queued control hooks still run.
            #[cfg(windows)]
            {
                // WebView2 registers this before parsing every document, including
                // cross-origin iframe navigations. No remote Tauri IPC is enabled.
                window = window.initialization_script_for_all_frames(include_str!("../../src/deskFrameLinks.js"));
                if test_window {
                    window = window.data_directory(
                        app.path().app_local_data_dir()?.join("test-window-webview"),
                    );
                }
                if control_motion {
                    window = window.additional_browser_args("--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --disable-background-timer-throttling --disable-renderer-backgrounding --disable-backgrounding-occluded-windows");
                }
            }
            if test_window {
                window = window
                    .position(-20000.0, 0.0)
                    .skip_taskbar(true)
                    .focused(false);
            } else if background {
                window = window.focused(false);
            }
            window.build()?;
            app.manage(bridge::Attaches::default());
            app.manage(files::Files::default());
            app.manage(machines::Machines::start(app.handle().clone())?);
            control::start(app.handle().clone(), test_window);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Herdr Shell");
}
