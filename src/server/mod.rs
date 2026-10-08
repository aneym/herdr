mod alt_screen_read;
pub mod autodetect;
#[cfg(unix)]
pub(crate) mod client_accept;
pub(crate) mod client_commands;
mod client_endpoint_control;
pub(crate) mod client_shell;
pub(crate) mod client_shell_graphics;
pub(crate) mod client_transport;
pub(crate) mod clients;
pub(crate) mod clipboard_image;
#[cfg(unix)]
pub(crate) mod handoff;
#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
mod handoff_path_tests;
pub mod headless;
pub(crate) mod keybindings;
pub(crate) mod notifications;
pub(crate) mod pane_input;
#[cfg(test)]
mod render_scale_benchmark;
pub(crate) mod render_stream;
mod signals;
pub mod socket_paths;
pub(crate) mod terminal_attach;
