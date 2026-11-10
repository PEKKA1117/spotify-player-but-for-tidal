//! What the program logs (spec 0027 "What the program logs"): one function
//! per line of its tables, so each line's level, target, message and fields
//! are in one place and tested there. The call sites are the seams: the
//! player's runtime, the TUI's loop, `main`.
//!
//! Never logged, in any of these: a token, the `Authorization` header, a
//! stream or cover URL (its host is allowed), the account's name, e-mail or
//! ID, a request's payload. Text that was shown to the user and may carry a
//! URL goes through [`safe`].

use std::path::Path;

use tidal_player_audio::{Event as EngineEvent, OutputInfo, SampleFormat};
use tidal_player_core::AudioQuality;
use tidal_player_core::log::LogSetting;
use tidal_player_core::protocol::{PlaybackState, PlayerSnapshot};

use super::Mode;

/// Why a process shuts down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shutdown {
    Quit,
    Command,
    Signal,
    PlayerGone,
}

/// `text` with every URL cut down to its scheme and host, and a bearer
/// token replaced: the shown text of a failure may quote either.
pub fn safe(text: &str) -> String {
    let _ = text;
    String::new()
}

/// The log is installed (`starting`).
pub fn starting(version: &str, mode: Mode, level: LogSetting, config: &Path) {
    let _ = (version, mode, level, config);
}

/// The login is lost.
pub fn login_required() {}

/// The login is restored.
pub fn login_restored() {}

/// A process shuts down in order.
pub fn shutting_down(reason: Shutdown) {
    let _ = reason;
}

/// A client connected to a player.
pub fn client_connected(client: u64) {
    let _ = client;
}

/// A client left a player.
pub fn client_disconnected(client: u64) {
    let _ = client;
}

/// A player answered `request` with an `Err`.
pub fn request_failed(request: &str, reason: &str) {
    let _ = (request, reason);
}

/// The name of a request: its variant's, without the payload.
pub fn request_name(debug: &str) -> String {
    let _ = debug;
    String::new()
}

/// The TUI lost its player (`reason` is the text shown).
pub fn disconnected(reason: &str) {
    let _ = reason;
}

/// The TUI joined its player again.
pub fn reconnected() {}

/// A start-up notice the TUI shows.
pub fn notice(text: &str) {
    let _ = text;
}

/// An export of the logs page failed.
pub fn export_failed(reason: &str) {
    let _ = reason;
}

/// The track an engine start event is about.
#[derive(Debug, Clone, Copy)]
pub struct Playing<'a> {
    pub track: u64,
    pub title: &'a str,
    pub quality: AudioQuality,
}

/// What a player remembers between inputs to tell what changed.
#[derive(Debug)]
pub struct PlayerLog {
    requested: AudioQuality,
    state: PlaybackState,
    message: Option<String>,
    open: Option<OpenOutput>,
    last: Option<OpenOutput>,
}

/// An output as `device opened` says it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct OpenOutput {
    device: String,
    format: SampleFormat,
    rate: u32,
    channels: u16,
    bit_perfect: bool,
}

impl OpenOutput {
    fn of(output: &OutputInfo) -> Self {
        Self {
            device: output.device.clone(),
            format: output.sample_format,
            rate: output.sample_rate,
            channels: output.channels,
            bit_perfect: output.bit_perfect,
        }
    }
}

impl PlayerLog {
    /// A player that asks Tidal for `requested`, in `state` now (a restored
    /// player is stopped: that is not a `stopped`).
    pub fn new(requested: AudioQuality, state: PlaybackState) -> Self {
        Self {
            requested,
            state,
            message: None,
            open: None,
            last: None,
        }
    }

    /// An engine event: the device opening and closing, and `playing`
    /// for the event that starts an entry (`playing` is what the player
    /// made of the event, `None` for a stale one).
    pub fn engine(&mut self, event: &EngineEvent, playing: Option<Playing<'_>>) {
        let _ = (event, playing, &self.requested, &self.open, &self.last);
    }

    /// The snapshot after an input: `stopped` when playback ended, and
    /// `message` when its text changed.
    pub fn snapshot(&mut self, snapshot: &PlayerSnapshot) {
        let _ = (snapshot, &self.state, &self.message);
    }
}

#[cfg(test)]
mod tests;
