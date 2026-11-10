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

/// The target of a run's own lines (start, shutdown).
const RUN: &str = "tidal_player::run";
/// The target of the player's lines.
const PLAYER: &str = "tidal_player::player";
/// The target of the TUI's lines.
const TUI: &str = "tidal_player::tui";

/// Whether `c` ends a URL or a token inside a sentence.
fn ends_word(c: char) -> bool {
    c.is_whitespace() || matches!(c, ')' | '"' | '\'' | '>' | ';' | ',')
}

/// `text` with every URL cut down to its scheme and host, a bearer token or
/// `Authorization` value replaced: the shown text of a failure may quote
/// any of them.
pub fn safe(text: &str) -> String {
    let lower = text.to_ascii_lowercase();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        let rest = &lower[i..];
        let scheme = if rest.starts_with("https://") {
            8
        } else if rest.starts_with("http://") {
            7
        } else {
            0
        };
        if scheme > 0 {
            let end = text[i..].find(ends_word).map_or(text.len(), |n| i + n);
            let url = &text[i..end];
            let host_end = url[scheme..]
                .find(['/', '?', '#'])
                .map_or(url.len(), |n| scheme + n);
            // `user:password@host` keeps the host only.
            let authority = &url[scheme..host_end];
            out.push_str(&url[..scheme]);
            out.push_str(authority.rsplit('@').next().unwrap_or(authority));
            i = end;
        } else if rest.starts_with("authorization:") || rest.starts_with("bearer ") {
            let (label, skip) = if rest.starts_with("bearer ") {
                ("Bearer <redacted>", 7)
            } else {
                ("Authorization: <redacted>", 14)
            };
            out.push_str(label);
            let value = text[i + skip..].trim_start();
            let start = text.len() - value.len();
            // The value is a word, or after `Authorization:` the scheme and
            // its token, up to the end of the sentence.
            let mut end = value.find(ends_word).map_or(text.len(), |n| start + n);
            if skip == 14 && value.to_ascii_lowercase().starts_with("bearer ") {
                let token = &text[end..].trim_start();
                let token_start = text.len() - token.len();
                end = token
                    .find(ends_word)
                    .map_or(text.len(), |n| token_start + n);
            }
            i = end;
        } else if let Some(c) = text[i..].chars().next() {
            out.push(c);
            i += c.len_utf8();
        } else {
            break;
        }
    }
    out
}

/// The `log_level` as `app.toml` spells it.
fn level_name(setting: LogSetting) -> &'static str {
    match setting {
        LogSetting::Off => "off",
        LogSetting::Error => "error",
        LogSetting::Warn => "warn",
        LogSetting::Info => "info",
        LogSetting::Debug => "debug",
        LogSetting::Trace => "trace",
    }
}

/// The log is installed (`starting`).
pub fn starting(version: &str, mode: Mode, level: LogSetting, config: &Path) {
    tracing::info!(
        target: RUN,
        version = version,
        mode = mode.name(),
        log_level = level_name(level),
        config = %config.display(),
        "starting"
    );
}

/// The login is lost.
pub fn login_required() {
    tracing::warn!(target: PLAYER, "login required");
}

/// The login is restored.
pub fn login_restored() {
    tracing::info!(target: PLAYER, "login restored");
}

/// A process shuts down in order.
pub fn shutting_down(reason: Shutdown) {
    let reason = match reason {
        Shutdown::Quit => "quit",
        Shutdown::Command => "shutdown command",
        Shutdown::Signal => "signal",
        Shutdown::PlayerGone => "player gone",
    };
    tracing::info!(target: RUN, reason = reason, "shutting down");
}

/// A client connected to a player.
pub fn client_connected(client: u64) {
    tracing::debug!(target: PLAYER, client = client, "client connected");
}

/// A client left a player.
pub fn client_disconnected(client: u64) {
    tracing::debug!(target: PLAYER, client = client, "client disconnected");
}

/// A player answered `request` with an `Err`.
pub fn request_failed(request: &str, reason: &str) {
    tracing::warn!(
        target: PLAYER,
        request = request,
        reason = %safe(reason),
        "request failed"
    );
}

/// The name of a request: its variant's, from the request's `Debug`
/// text, without the payload.
pub fn request_name(debug: &str) -> String {
    debug
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect()
}

/// The TUI lost its player (`reason` is the text shown).
pub fn disconnected(reason: &str) {
    tracing::warn!(target: TUI, reason = %safe(reason), "disconnected");
}

/// The TUI joined its player again.
pub fn reconnected() {
    tracing::info!(target: TUI, "reconnected");
}

/// A start-up notice the TUI shows.
pub fn notice(text: &str) {
    tracing::warn!(target: TUI, text = %safe(text), "notice");
}

/// An export of the logs page failed.
pub fn export_failed(reason: &str) {
    tracing::warn!(target: TUI, reason = %safe(reason), "export failed");
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
    /// The output the engine has open.
    open: Option<OpenOutput>,
    /// The last one it opened, for the reopening after a release.
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

/// The ALSA name of a sample format.
fn format_name(format: SampleFormat) -> &'static str {
    match format {
        SampleFormat::S16Le => "S16_LE",
        SampleFormat::S24Le => "S24_LE",
        SampleFormat::S24_3Le => "S24_3LE",
        SampleFormat::S32Le => "S32_LE",
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

    /// The quality asked for, as `playing` says it.
    pub fn set_requested(&mut self, requested: AudioQuality) {
        self.requested = requested;
    }

    fn opened(&mut self, output: OpenOutput) {
        if self.open.as_ref() != Some(&output) {
            tracing::info!(
                target: PLAYER,
                device = %output.device,
                format = format_name(output.format),
                rate = output.rate,
                channels = output.channels,
                bit_perfect = output.bit_perfect,
                "device opened"
            );
        }
        self.last = Some(output.clone());
        self.open = Some(output);
    }

    fn closed(&mut self) {
        if let Some(output) = self.open.take() {
            tracing::info!(target: PLAYER, device = %output.device, "device released");
        }
    }

    /// An engine event: the device opening and closing, and `playing`
    /// for the event that starts an entry (`playing` is what the player
    /// made of the event, `None` for a stale one).
    pub fn engine(&mut self, event: &EngineEvent, playing: Option<Playing<'_>>) {
        match event {
            EngineEvent::Started { output, .. } | EngineEvent::Transitioned { output, .. } => {
                self.opened(OpenOutput::of(output));
                if let Some(p) = playing {
                    tracing::info!(
                        target: PLAYER,
                        track = p.track,
                        title = %safe(p.title),
                        quality = %p.quality,
                        requested = %self.requested,
                        "playing"
                    );
                }
            }
            EngineEvent::OutputChanged(output) => self.opened(OpenOutput::of(output)),
            EngineEvent::DeviceFallback { device, .. } => {
                if let Some(last) = &mut self.last {
                    last.device.clone_from(device);
                }
            }
            // The engine reopens what it released.
            EngineEvent::Resumed => {
                if self.open.is_none()
                    && let Some(last) = self.last.clone()
                {
                    self.opened(last);
                }
            }
            EngineEvent::Released
            | EngineEvent::Stopped
            | EngineEvent::TrackEnded { .. }
            | EngineEvent::Error { .. } => self.closed(),
            EngineEvent::Position(_)
            | EngineEvent::Buffering
            | EngineEvent::Buffered
            | EngineEvent::Paused
            | EngineEvent::ResumeFailed(_) => {}
        }
    }

    /// The snapshot after an input: `stopped` when playback ended, and
    /// `message` when its text changed.
    pub fn snapshot(&mut self, snapshot: &PlayerSnapshot) {
        if self.state != PlaybackState::Stopped && snapshot.state == PlaybackState::Stopped {
            tracing::info!(target: PLAYER, "stopped");
        }
        self.state = snapshot.state;
        if snapshot.message != self.message {
            if let Some(text) = &snapshot.message {
                tracing::warn!(target: PLAYER, text = %safe(text), "message");
            }
            self.message.clone_from(&snapshot.message);
        }
    }
}

#[cfg(test)]
mod tests;
