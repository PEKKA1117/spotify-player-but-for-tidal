//! Library half of the `tidal-player` binary, kept here so integration
//! tests can import it. The modules follow spec 0001's split:
//!
//! - [`player`]: the player side: the runtime around
//!   `tidal_player_core::player::update` and everything only a player does
//!   (streams, the audio engine's wiring, the remembered state, the cover
//!   cache, MPRIS, the event hook)
//! - [`client`] and [`ipc`]: how a client reaches a player, in the same
//!   process or over the socket
//! - [`tui`]: the terminal client: keys in, frames out
//! - [`cli`]: the subcommands (`play`, `playback`, `playlist`, `daemon`)
//! - [`session`]: where the login session is kept
//! - [`config`]: the files in the config directory

pub mod cli;
pub mod client;
pub mod config;
pub mod ipc;
pub mod logging;
pub mod panic_hook;
pub mod player;
pub mod session;
pub mod settings;
pub mod tui;
