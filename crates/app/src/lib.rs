//! Library half of the `tidal-player` binary: rendering, key mapping and the
//! panic hook, kept here so integration tests can import them.

pub mod client;
pub mod daemon;
pub mod http_source;
pub mod input;
pub mod ipc;
pub mod login;
pub mod oneshot;
pub mod panic_hook;
pub mod passphrase;
pub mod play;
pub mod playback;
pub mod player_runtime;
pub mod reserve;
pub mod session_store;
pub mod store_setup;
pub mod ui;
