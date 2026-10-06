//! herdr-shell-core: the platform-neutral client core of the Windows Herdr Shell.
//!
//! - [`wire`]: hand mirror of herdr's direct-terminal wire messages, pinned by golden fixtures.
//! - [`endpoint`]: socket and named-pipe endpoints named the way herdr names them.
//! - [`attach`]: direct-terminal attach / observe client producing emulator-ready events.
//! - [`api`]: JSON API client (requests and `events.subscribe`).

pub mod api;
pub mod attach;
pub mod endpoint;
pub mod wire;
