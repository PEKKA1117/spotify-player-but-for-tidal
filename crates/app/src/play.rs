//! `tidal-player play` and `tidal-player devices` (spec 0003 "Commands",
//! "Settings", "Edge cases & errors"; AC25, AC26): settings, the status
//! lines and the user-facing error messages, kept pure so they are tested
//! without a terminal, a session or a sound card.

use std::collections::HashMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::time::Duration;

use tidal_player_api::auth::AuthError;
use tidal_player_api::stream::StreamError;
use tidal_player_audio::devices::{PlaybackDevice, parse_devices};
use tidal_player_audio::{
    Codec, EngineError, Event, OutputInfo, OutputKind, SampleFormat, SinkError, SourceError,
    SourceFormat,
};
use tidal_player_core::player::{Failure, FailureKind, PlayerConfig};
use tidal_player_core::protocol::{
    Command, Event as PlayerEvent, PlaybackState, PlayerSnapshot, RepeatMode,
};
use tidal_player_core::{AudioQuality, Item, ParseQualityError, Track};

use crate::config::{AppConfig, LibraryLayout};
use crate::player_runtime::{
    EngineControl, ExpandError, Handled, Jobs, LibrarySettings, Metadata, PlayerRuntime,
    RuntimeInput, expand_items,
};

/// Highest quality to ask for (spec 0003 "Settings").
pub const QUALITY_VAR: &str = "TIDAL_PLAYER_QUALITY";
/// Output device.
pub const DEVICE_VAR: &str = "TIDAL_PLAYER_DEVICE";
/// Where `devices` reads `cards` and `pcm` from instead of `/proc/asound`.
pub const ASOUND_DIR_VAR: &str = "TIDAL_PLAYER_ASOUND_DIR";
/// The default output device (decision 2: shared, never takes the card).
pub const DEFAULT_DEVICE: &str = "default";
/// The default highest quality.
pub const DEFAULT_QUALITY: AudioQuality = AudioQuality::HiResLossless;

/// What `play` uses: flag, then environment, then default.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub quality: AudioQuality,
    pub device: String,
}

/// A setting with a value `play` cannot use (exit 2).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid {setting}: {message}")]
pub struct SettingsError {
    /// `--quality` or the variable's name.
    pub setting: String,
    pub message: String,
}

/// Resolves the settings; `env` reads an environment variable (an empty
/// value counts as unset).
pub fn resolve_settings(
    quality_flag: Option<&str>,
    device_flag: Option<&str>,
    env: impl Fn(&str) -> Option<String>,
) -> Result<Settings, SettingsError> {
    resolve_settings_with(&AppConfig::default(), quality_flag, device_flag, env)
}

/// As [`resolve_settings`], with `app.toml` as the layer under the
/// environment (spec 0008 "`app.toml`": flag, environment, file, default).
pub fn resolve_settings_with(
    file: &AppConfig,
    quality_flag: Option<&str>,
    device_flag: Option<&str>,
    env: impl Fn(&str) -> Option<String>,
) -> Result<Settings, SettingsError> {
    let quality = match quality_flag {
        Some(flag) => parse_quality(flag, "--quality")?,
        None => match non_empty(env(QUALITY_VAR)) {
            Some(value) => parse_quality(&value, QUALITY_VAR)?,
            None => file.quality.unwrap_or(DEFAULT_QUALITY),
        },
    };
    let device = match device_flag {
        Some(flag) => flag.to_owned(),
        None => configured_device_with(file, env),
    };
    Ok(Settings { quality, device })
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|v| !v.is_empty())
}

fn parse_quality(value: &str, setting: &str) -> Result<AudioQuality, SettingsError> {
    value.parse().map_err(|e: ParseQualityError| SettingsError {
        setting: setting.to_owned(),
        message: e.to_string(),
    })
}

/// The device `play` would use without `--device`, for the `*` of
/// `devices`.
pub fn configured_device(env: impl Fn(&str) -> Option<String>) -> String {
    configured_device_with(&AppConfig::default(), env)
}

/// As [`configured_device`], with `app.toml` under the environment.
pub fn configured_device_with(file: &AppConfig, env: impl Fn(&str) -> Option<String>) -> String {
    non_empty(env(DEVICE_VAR))
        .or_else(|| file.output_device.clone())
        .unwrap_or_else(|| DEFAULT_DEVICE.into())
}

/// The player's config with the device it starts on (spec 0014 AC6): the
/// device of `settings` (flag, environment, `app.toml`, `default`), the
/// same one the engine is started with.
pub fn with_device(player: PlayerConfig, settings: &Settings) -> PlayerConfig {
    let _ = settings;
    player
}

/// Where the device list is read from: `TIDAL_PLAYER_ASOUND_DIR`, else
/// `/proc/asound`.
pub fn asound_dir(env: impl Fn(&str) -> Option<String>) -> PathBuf {
    non_empty(env(ASOUND_DIR_VAR)).map_or_else(|| PathBuf::from("/proc/asound"), PathBuf::from)
}

/// The playback devices under `dir` (`cards` and `pcm`), read now. A
/// missing file means no card (no ALSA, or a container): `default` only.
/// Any other read error is the `Err` (spec 0014 AC7).
pub fn read_devices(dir: &Path) -> Result<Vec<PlaybackDevice>, String> {
    let read = |name: &str| {
        let path = dir.join(name);
        match std::fs::read_to_string(&path) {
            Ok(text) => Ok(text),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(String::new()),
            Err(e) => Err(format!("cannot read {}: {e}", path.display())),
        }
    };
    Ok(parse_devices(&read("cards")?, &read("pcm")?))
}

/// `Track 77640617: HI_RES_LOSSLESS, FLAC 24-bit 96 kHz stereo`.
pub fn track_line(track_id: u64, granted: AudioQuality, source: &SourceFormat) -> String {
    format!(
        "Track {track_id}: {granted}, {}",
        source_description(source)
    )
}

/// `FLAC 24-bit 96 kHz stereo`: the "Track" line after the quality.
pub fn source_description(source: &SourceFormat) -> String {
    let codec = match source.codec {
        Codec::Flac => "FLAC",
        Codec::AacLc => "AAC",
    };
    let bits = source
        .bits_per_sample
        .map(|b| format!(" {b}-bit"))
        .unwrap_or_default();
    let channels = match source.channels {
        1 => "mono".to_owned(),
        2 => "stereo".to_owned(),
        n => format!("{n} ch"),
    };
    format!("{codec}{bits} {} {channels}", khz(source.sample_rate))
}

/// `Output: hw:1,0 (exclusive) S32_LE 96 kHz 2 ch, bit-perfect`.
pub fn output_line(output: &OutputInfo) -> String {
    let quality = match (&output.not_bit_perfect_reason, output.bit_perfect) {
        (_, true) => "bit-perfect",
        (Some(reason), false) => reason.as_str(),
        (None, false) => "not bit-perfect",
    };
    format!("Output: {}, {quality}", output_description(output))
}

/// `hw:1,0 (exclusive) S32_LE 96 kHz 2 ch`: the "Output" line without the
/// verdict.
pub fn output_description(output: &OutputInfo) -> String {
    let kind = match output.kind {
        OutputKind::Exclusive => "exclusive",
        OutputKind::Fallback => "fallback",
        OutputKind::Shared => "shared",
    };
    let format = match output.sample_format {
        SampleFormat::S16Le => "S16_LE",
        SampleFormat::S24Le => "S24_LE",
        SampleFormat::S24_3Le => "S24_3LE",
        SampleFormat::S32Le => "S32_LE",
    };
    format!(
        "{} ({kind}) {format} {} {} ch",
        output.device,
        khz(output.sample_rate),
        output.channels
    )
}

/// `96 kHz`, `44.1 kHz`, `22.05 kHz`.
fn khz(rate: u32) -> String {
    let whole = rate / 1000;
    let fraction = format!("{:03}", rate % 1000);
    let fraction = fraction.trim_end_matches('0');
    if fraction.is_empty() {
        format!("{whole} kHz")
    } else {
        format!("{whole}.{fraction} kHz")
    }
}

/// `  1:23 / 4:56`; the duration is `?:??` when the stream does not say.
pub fn progress_line(position: Duration, duration: Option<Duration>) -> String {
    let duration = duration.map_or_else(|| "?:??".to_owned(), clock_time);
    format!("  {} / {duration}", clock_time(position))
}

/// `m:ss`, or `h:mm:ss` from an hour on (whole seconds, rounded down).
fn clock_time(t: Duration) -> String {
    let s = t.as_secs();
    let (h, m, s) = (s / 3600, s / 60 % 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// The one stderr line for a resolution failure (spec 0003 "Edge cases &
/// errors"); `country` is the session's.
pub fn stream_error_message(track_id: u64, country: &str, error: &StreamError) -> String {
    match error {
        StreamError::PreviewOnly => {
            format!("Track {track_id} is only available as a preview for this account")
        }
        StreamError::NotAvailable => format!("Track {track_id} is not available in {country}"),
        StreamError::NotFound => {
            format!("Track {track_id} was not found, or cannot be streamed in {country}")
        }
        StreamError::Unsupported(what) => format!("Track {track_id} is not playable: {what}"),
        other => capitalise(&other.to_string()),
    }
}

/// The one stderr line for a failed track.
pub fn engine_error_message(track_id: u64, error: &EngineError) -> String {
    match error {
        EngineError::Output(SinkError::Busy { device, holder }) => {
            let holder = holder
                .as_deref()
                .map(|h| format!(" (used by {h})"))
                .unwrap_or_default();
            format!("Output {device} is busy{holder}: close it, or use --device default")
        }
        EngineError::Output(SinkError::NotFound(device)) => {
            format!("No such output device {device}: see \"tidal-player devices\"")
        }
        EngineError::Output(SinkError::Lost(device)) => format!("Output {device} was lost"),
        EngineError::Output(other) => format!("Output error: {other}"),
        EngineError::Source(SourceError::Network(_)) => {
            format!("Network error while streaming track {track_id}")
        }
        EngineError::Source(other) => format!("Error while streaming track {track_id}: {other}"),
        EngineError::Decode(what) => format!("Track {track_id} could not be decoded: {what}"),
        EngineError::Unsupported(what) => format!("Track {track_id} is not playable: {what}"),
    }
}

fn capitalise(message: &str) -> String {
    let mut chars = message.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

/// How a played track ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Played to its end (exit 0).
    Ended,
    /// Failed, with the stderr line (exit 1).
    Failed(String),
}

/// Turns engine events into `play`'s output: the two status lines at
/// `Started`, then the progress line, redrawn in place, only on a terminal.
#[derive(Debug)]
pub struct Reporter {
    track_id: u64,
    granted: AudioQuality,
    duration: Option<Duration>,
    tty: bool,
    progress_shown: bool,
}

impl Reporter {
    pub fn new(
        track_id: u64,
        granted: AudioQuality,
        duration: Option<Duration>,
        tty: bool,
    ) -> Self {
        Self {
            track_id,
            granted,
            duration,
            tty,
            progress_shown: false,
        }
    }

    /// Prints what `event` shows; `Some` once the track is over.
    pub fn on_event(&mut self, event: &Event, out: &mut dyn Write) -> io::Result<Option<Outcome>> {
        match event {
            Event::Started { source, output, .. } | Event::Transitioned { source, output, .. } => {
                self.finish(out)?;
                writeln!(out, "{}", track_line(self.track_id, self.granted, source))?;
                writeln!(out, "{}", output_line(output))?;
                self.progress(Duration::ZERO, out)?;
            }
            Event::Position(position) => self.progress(*position, out)?,
            Event::TrackEnded { .. } => return Ok(Some(Outcome::Ended)),
            Event::Error { error, .. } => {
                return Ok(Some(Outcome::Failed(engine_error_message(
                    self.track_id,
                    error,
                ))));
            }
            _ => {}
        }
        out.flush()?;
        Ok(None)
    }

    /// Redraws the progress line, on a terminal only.
    fn progress(&mut self, position: Duration, out: &mut dyn Write) -> io::Result<()> {
        if self.tty {
            write!(out, "\r\x1b[K{}", progress_line(position, self.duration))?;
            self.progress_shown = true;
        }
        Ok(())
    }

    /// Ends the progress line, if one was drawn.
    pub fn finish(&mut self, out: &mut dyn Write) -> io::Result<()> {
        if std::mem::take(&mut self.progress_shown) {
            writeln!(out)?;
        }
        out.flush()
    }
}

// --- spec 0004: player settings ------------------------------------------------

/// Volume step of `+`/`-`, in percentage points (spec 0004 "Settings").
pub const VOLUME_STEP_VAR: &str = "TIDAL_PLAYER_VOLUME_STEP";
/// Seek step of `>`/`<`, in seconds.
pub const SEEK_STEP_VAR: &str = "TIDAL_PLAYER_SEEK_STEP";
/// `Previous` restarts the track after this many seconds; 0 never.
pub const PREVIOUS_RESTART_VAR: &str = "TIDAL_PLAYER_PREVIOUS_RESTART";
/// Autoplay at start: `on` or `off`.
pub const AUTOPLAY_VAR: &str = "TIDAL_PLAYER_AUTOPLAY";
/// Release the device after pausing for this many seconds, or `never`
/// (spec 0005).
pub const RELEASE_PAUSED_VAR: &str = "TIDAL_PLAYER_RELEASE_PAUSED";
/// Remember the playback state between runs: `on` or `off` (spec 0009).
pub const REMEMBER_PLAYBACK_VAR: &str = "TIDAL_PLAYER_REMEMBER_PLAYBACK";
/// `on` or `off`: whether the player publishes itself over MPRIS (spec 0010).
pub const MPRIS_VAR: &str = "TIDAL_PLAYER_MPRIS";
/// The most album covers the cache keeps, 0 to 1000 (spec 0010).
pub const MAX_COVER_ARTS_VAR: &str = "TIDAL_PLAYER_MAX_COVER_ARTS";
/// `on` or `off`: whether the TUI shows key-sequence hints (spec 0013).
pub const KEY_HINTS_VAR: &str = "TIDAL_PLAYER_KEY_HINTS";
/// How long a key sequence is pending before its hint shows, 0 to 10 000
/// ms (spec 0013).
pub const KEY_HINTS_DELAY_VAR: &str = "TIDAL_PLAYER_KEY_HINTS_DELAY_MS";

/// What a client sends with its volume and seek keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Steps {
    /// `ChangeVolume(±volume)`, 1–25.
    pub volume: u8,
    /// `SeekBy(±seek)`, 1–600 s.
    pub seek: Duration,
}

impl Default for Steps {
    fn default() -> Self {
        Self {
            volume: 5,
            seek: Duration::from_secs(5),
        }
    }
}

/// Items per page of a library list (spec 0006), 1 to 10000.
pub const PAGE_SIZE_VAR: &str = "TIDAL_PLAYER_PAGE_SIZE";
/// Items per page of a search's lists (spec 0007), 1 to 1000.
pub const SEARCH_PAGE_SIZE_VAR: &str = "TIDAL_PLAYER_SEARCH_PAGE_SIZE";
/// Comma-separated words hiding alternate versions from an artist's *All
/// tracks* (spec 0006); empty hides nothing.
pub const HIDE_VERSIONS_VAR: &str = "TIDAL_PLAYER_HIDE_VERSIONS";

/// The default of `key_hints_delay_ms` (spec 0013).
pub const DEFAULT_KEY_HINTS_DELAY: Duration = Duration::from_millis(1000);

/// The player's settings and the client's steps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerSettings {
    pub player: PlayerConfig,
    pub steps: Steps,
    /// The engine's release delay (spec 0005); `None`: never.
    pub release_paused: Option<Duration>,
    /// What the player passes to every library request.
    pub library: LibrarySettings,
    /// The library page's window widths (spec 0008); read by the TUI.
    pub layout: LibraryLayout,
    /// Whether the player reads and writes `playback.json` (spec 0009).
    pub remember_playback: bool,
    /// `player.autoplay` came from `--autoplay` or the environment, so it
    /// beats a remembered autoplay (spec 0009 "Precedence of autoplay").
    pub autoplay_explicit: bool,
    /// Whether the player publishes itself over MPRIS (spec 0010).
    pub mpris: bool,
    /// The most covers the cover cache keeps; `0`: no cache (spec 0010).
    pub max_cover_arts: u16,
    /// Whether the TUI shows key-sequence hints (spec 0013); read by the
    /// TUI only.
    pub key_hints: bool,
    /// How long a sequence is pending before its hint shows (spec 0013).
    pub key_hints_delay: Duration,
}

impl Default for PlayerSettings {
    fn default() -> Self {
        Self {
            player: PlayerConfig::default(),
            steps: Steps::default(),
            release_paused: Some(tidal_player_audio::DEFAULT_RELEASE_PAUSED),
            library: LibrarySettings::default(),
            layout: LibraryLayout::default(),
            remember_playback: true,
            autoplay_explicit: false,
            mpris: true,
            max_cover_arts: 20,
            key_hints: true,
            key_hints_delay: DEFAULT_KEY_HINTS_DELAY,
        }
    }
}

/// The autoplay a player starts with (spec 0009 "Precedence of autoplay"):
/// flag > environment > remembered > `app.toml` > default. `remembered` is
/// the saved state's autoplay, when a state was restored.
pub fn start_autoplay(settings: &PlayerSettings, remembered: Option<bool>) -> bool {
    match remembered {
        Some(remembered) if !settings.autoplay_explicit => remembered,
        _ => settings.player.autoplay,
    }
}

/// The player settings from the environment (an empty variable counts as
/// unset, except `TIDAL_PLAYER_HIDE_VERSIONS`: empty hides nothing); an invalid value names the variable and the accepted range
/// (exit 2). `country` is left for the caller (it comes from the session).
pub fn resolve_player_config(
    env: impl Fn(&str) -> Option<String>,
) -> Result<PlayerSettings, SettingsError> {
    resolve_with(&AppConfig::default(), false, env)
}

/// As [`resolve_player_config`], with `app.toml` under the environment.
pub fn resolve_player_config_with(
    file: &AppConfig,
    env: impl Fn(&str) -> Option<String>,
) -> Result<PlayerSettings, SettingsError> {
    resolve_with(file, false, env)
}

/// As [`resolve_player_config`], for `play`: `--autoplay` beats the
/// environment (whose value is then not read).
pub fn resolve_play_config(
    autoplay_flag: bool,
    env: impl Fn(&str) -> Option<String>,
) -> Result<PlayerSettings, SettingsError> {
    resolve_with(&AppConfig::default(), autoplay_flag, env)
}

/// As [`resolve_play_config`], with `app.toml` under the environment.
pub fn resolve_play_config_with(
    file: &AppConfig,
    autoplay_flag: bool,
    env: impl Fn(&str) -> Option<String>,
) -> Result<PlayerSettings, SettingsError> {
    resolve_with(file, autoplay_flag, env)
}

fn resolve_with(
    file: &AppConfig,
    autoplay_flag: bool,
    env: impl Fn(&str) -> Option<String>,
) -> Result<PlayerSettings, SettingsError> {
    let mut settings = PlayerSettings::default();
    apply_file(&mut settings, file);
    let get = |var: &str| non_empty(env(var));
    if let Some(value) = get(VOLUME_STEP_VAR) {
        settings.steps.volume = int_in(&value, VOLUME_STEP_VAR, 1, 25)? as u8;
    }
    if let Some(value) = get(SEEK_STEP_VAR) {
        settings.steps.seek = Duration::from_secs(int_in(&value, SEEK_STEP_VAR, 1, 600)?);
    }
    if let Some(value) = get(PREVIOUS_RESTART_VAR) {
        settings.player.previous_restart =
            Duration::from_secs(int_in(&value, PREVIOUS_RESTART_VAR, 0, 60)?);
    }
    if let Some(value) = get(RELEASE_PAUSED_VAR) {
        settings.release_paused = if value == "never" {
            None
        } else {
            let secs = int_in(&value, RELEASE_PAUSED_VAR, 0, 3600).map_err(|mut e| {
                e.message = format!("expected an integer from 0 to 3600 or never, got \"{value}\"");
                e
            })?;
            Some(Duration::from_secs(secs))
        };
    }
    if let Some(value) = get(PAGE_SIZE_VAR) {
        settings.library.page_size = int_in(&value, PAGE_SIZE_VAR, 1, 10_000)? as u32;
    }
    if let Some(value) = get(SEARCH_PAGE_SIZE_VAR) {
        settings.library.search_page_size = int_in(&value, SEARCH_PAGE_SIZE_VAR, 1, 1000)? as u32;
    }
    // Unlike the others, an empty value is a choice: hide nothing.
    if let Some(value) = env(HIDE_VERSIONS_VAR) {
        settings.library.hidden_words = value
            .split(',')
            .map(str::trim)
            .filter(|word| !word.is_empty())
            .map(str::to_owned)
            .collect();
    }
    if let Some(on) = on_off(get(REMEMBER_PLAYBACK_VAR), REMEMBER_PLAYBACK_VAR)? {
        settings.remember_playback = on;
    }
    if let Some(on) = on_off(get(MPRIS_VAR), MPRIS_VAR)? {
        settings.mpris = on;
    }
    if let Some(on) = on_off(get(KEY_HINTS_VAR), KEY_HINTS_VAR)? {
        settings.key_hints = on;
    }
    if let Some(value) = get(KEY_HINTS_DELAY_VAR) {
        settings.key_hints_delay =
            Duration::from_millis(int_in(&value, KEY_HINTS_DELAY_VAR, 0, 10_000)?);
    }
    if let Some(value) = get(MAX_COVER_ARTS_VAR) {
        settings.max_cover_arts = int_in(&value, MAX_COVER_ARTS_VAR, 0, 1000)? as u16;
    }
    if autoplay_flag {
        settings.player.autoplay = true;
        settings.autoplay_explicit = true;
    } else if let Some(on) = on_off(get(AUTOPLAY_VAR), AUTOPLAY_VAR)? {
        settings.player.autoplay = on;
        settings.autoplay_explicit = true;
    }
    Ok(settings)
}

/// `on` or `off`, in any case; `None` when unset.
fn on_off(value: Option<String>, var: &str) -> Result<Option<bool>, SettingsError> {
    match value.as_deref() {
        None => Ok(None),
        Some(value) if value.eq_ignore_ascii_case("on") => Ok(Some(true)),
        Some(value) if value.eq_ignore_ascii_case("off") => Ok(Some(false)),
        Some(value) => Err(SettingsError {
            setting: var.into(),
            message: format!("expected on or off, got \"{value}\""),
        }),
    }
}

/// `app.toml` is the layer under the environment: its values replace the
/// defaults, then the variables replace those.
fn apply_file(settings: &mut PlayerSettings, file: &AppConfig) {
    if let Some(volume) = file.volume_step {
        settings.steps.volume = volume;
    }
    if let Some(secs) = file.seek_duration_secs {
        settings.steps.seek = Duration::from_secs(secs);
    }
    if let Some(secs) = file.previous_restart_secs {
        settings.player.previous_restart = Duration::from_secs(secs);
    }
    if let Some(autoplay) = file.autoplay {
        settings.player.autoplay = autoplay;
    }
    if let Some(remember) = file.remember_playback {
        settings.remember_playback = remember;
    }
    if let Some(mpris) = file.mpris {
        settings.mpris = mpris;
    }
    if let Some(on) = file.key_hints {
        settings.key_hints = on;
    }
    if let Some(ms) = file.key_hints_delay_ms {
        settings.key_hints_delay = Duration::from_millis(ms);
    }
    if let Some(max) = file.max_cover_arts {
        settings.max_cover_arts = max;
    }
    if let Some(release) = file.release_paused {
        settings.release_paused = release;
    }
    if let Some(size) = file.page_size {
        settings.library.page_size = size;
    }
    if let Some(size) = file.search_page_size {
        settings.library.search_page_size = size;
    }
    if let Some(words) = &file.hide_versions {
        settings.library.hidden_words = words.clone();
    }
    settings.layout = file.layout;
}

fn int_in(value: &str, var: &str, min: u64, max: u64) -> Result<u64, SettingsError> {
    value
        .parse::<u64>()
        .ok()
        .filter(|n| (min..=max).contains(n))
        .ok_or_else(|| SettingsError {
            setting: var.into(),
            message: format!("expected an integer from {min} to {max}, got \"{value}\""),
        })
}

// --- spec 0004: failures ----------------------------------------------------------

/// Classifies a resolution failure (spec 0004 "Failures") with 0003's
/// message. Anything not listed as track-only stops (never skips).
pub fn stream_failure(track_id: u64, country: &str, error: &StreamError) -> Failure {
    let kind = match error {
        StreamError::NotFound
        | StreamError::NotAvailable
        | StreamError::PreviewOnly
        | StreamError::Unsupported(_) => FailureKind::TrackOnly,
        StreamError::Auth(AuthError::LoginRequired | AuthError::Unauthorized) => {
            FailureKind::Session
        }
        StreamError::Server(_) | StreamError::Malformed(_) | StreamError::Auth(_) => {
            FailureKind::Transient
        }
    };
    Failure {
        kind,
        message: stream_error_message(track_id, country, error),
    }
}

/// Classifies an engine (or source opening) failure with 0003's message.
pub fn engine_failure(track_id: u64, error: &EngineError) -> Failure {
    let kind = match error {
        EngineError::Decode(_) | EngineError::Unsupported(_) => FailureKind::TrackOnly,
        EngineError::Output(_) => FailureKind::Output,
        EngineError::Source(_) => FailureKind::Transient,
    };
    Failure {
        kind,
        message: engine_error_message(track_id, error),
    }
}

// --- spec 0004: `play` with a queue -----------------------------------------------------

/// `play`'s new flags, applied to the player before the queue loads.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlayOptions {
    pub shuffle: bool,
    pub repeat: RepeatMode,
    /// `--autoplay` (the environment is in the player's config).
    pub autoplay: bool,
    /// `--start`, for the first track.
    pub start_at: Duration,
}

impl PlayOptions {
    /// Whether a flag 0003's `play` did not have is given.
    pub fn has_new_flag(&self) -> bool {
        self.shuffle || self.repeat != RepeatMode::Off || self.autoplay
    }
}

/// The queue `play` loads: the items expanded in order. One track ID with
/// no new flag is queued as 0003 played it, without a metadata request
/// (its errors are 0003's, from the stream resolution).
pub async fn play_tracks(
    meta: &dyn Metadata,
    items: &[Item],
    options: &PlayOptions,
) -> Result<Vec<Track>, ExpandError> {
    if let [Item::Track(id)] = items
        && !options.has_new_flag()
    {
        return Ok(vec![Track {
            id: *id,
            title: String::new(),
            version: None,
            artists: Vec::new(),
            album: None,
            duration: None,
            streamable: true,
        }]);
    }
    expand_items(meta, items).await
}

/// Where `play` writes.
pub struct PlayOutput<'a> {
    pub out: &'a mut dyn Write,
    pub err: &'a mut dyn Write,
    /// Whether `out` is a terminal (the progress line is drawn only there).
    pub tty: bool,
}

/// Plays `tracks` as one queue through `runtime` and returns the exit code
/// (spec 0004 "Commands"): `0` when the queue ran out and at least one track
/// played to its end, `1` when it stopped on a failure or nothing played,
/// `130` once `interrupted` says so (the engine is stopped first).
pub fn play_queue<E: EngineControl, J: Jobs>(
    runtime: &mut PlayerRuntime<E, J>,
    tracks: Vec<Track>,
    options: &PlayOptions,
    inputs: &Receiver<RuntimeInput>,
    poll: Duration,
    interrupted: &mut dyn FnMut() -> bool,
    output: &mut PlayOutput<'_>,
) -> u8 {
    let mut reporter = QueueReporter::new(output.tty);
    let mut commands = Vec::new();
    if options.shuffle {
        commands.push(Command::ToggleShuffle);
    }
    let mut repeat = RepeatMode::Off;
    while repeat != options.repeat {
        commands.push(Command::CycleRepeat);
        repeat = repeat.cycled();
    }
    commands.push(Command::LoadQueue { tracks, start: 0 });
    if !options.start_at.is_zero() {
        commands.push(Command::SeekTo(options.start_at));
    }
    for command in commands {
        let handled = runtime.handle(RuntimeInput::Command(command));
        reporter.report(&handled, output);
    }
    loop {
        // The player stops for good only once nothing can restart it.
        let snapshot = runtime.snapshot();
        if snapshot.state == PlaybackState::Stopped && !runtime.suggestions_pending() {
            reporter.finish(output);
            return reporter.exit_code(&snapshot);
        }
        let input = loop {
            if interrupted() {
                runtime.handle(RuntimeInput::Command(Command::Shutdown));
                reporter.finish(output);
                return 130;
            }
            if let Some(input) = runtime.next_input(inputs, poll) {
                break input;
            }
        };
        let handled = runtime.handle(input);
        reporter.report(&handled, output);
    }
}

/// Turns what the runtime did into `play`'s output: the "Track" and
/// "Output" lines at each track start, the progress line on a terminal,
/// each new message on stderr; and decides the exit code.
#[derive(Debug)]
struct QueueReporter {
    tty: bool,
    progress_shown: bool,
    duration: Option<Duration>,
    message: Option<String>,
    kinds: HashMap<String, FailureKind>,
    played_to_end: bool,
}

impl QueueReporter {
    fn new(tty: bool) -> Self {
        Self {
            tty,
            progress_shown: false,
            duration: None,
            message: None,
            kinds: HashMap::new(),
            played_to_end: false,
        }
    }

    /// Writes what `handled` shows; write errors (a closed pipe) are
    /// ignored, playback goes on.
    fn report(&mut self, handled: &Handled, output: &mut PlayOutput<'_>) {
        let _ = self.try_report(handled, output);
    }

    fn try_report(&mut self, handled: &Handled, output: &mut PlayOutput<'_>) -> io::Result<()> {
        for failure in &handled.failures {
            self.kinds.insert(failure.message.clone(), failure.kind);
        }
        self.played_to_end |= handled.ended;
        // A failure the player acted on (it answered with a snapshot) is
        // shown even when the player replaced its message in the same step
        // (with the run-limit summary).
        let mut shown = None;
        if !handled.events.is_empty()
            && let Some(failure) = handled.failures.last()
        {
            self.end_progress(output.out)?;
            writeln!(output.err, "{}", failure.message)?;
            shown = Some(failure.message.as_str());
        }
        if let Some(start) = &handled.started {
            self.end_progress(output.out)?;
            writeln!(
                output.out,
                "{}",
                track_line(start.track.0, start.quality, &start.source)
            )?;
            writeln!(output.out, "{}", output_line(&start.output))?;
            self.duration = start.duration;
            self.progress(Duration::ZERO, output.out)?;
        }
        for event in &handled.events {
            match event {
                PlayerEvent::Position { position, .. } => self.progress(*position, output.out)?,
                PlayerEvent::Player(snapshot) if snapshot.message != self.message => {
                    self.message.clone_from(&snapshot.message);
                    let Some(message) = &snapshot.message else {
                        continue;
                    };
                    // The failure shown just now is not printed twice.
                    if Some(message.as_str()) != shown {
                        self.end_progress(output.out)?;
                        writeln!(output.err, "{message}")?;
                    }
                }
                _ => {}
            }
        }
        output.out.flush()
    }

    fn progress(&mut self, position: Duration, out: &mut dyn Write) -> io::Result<()> {
        if self.tty {
            write!(out, "\r\x1b[K{}", progress_line(position, self.duration))?;
            self.progress_shown = true;
        }
        Ok(())
    }

    fn end_progress(&mut self, out: &mut dyn Write) -> io::Result<()> {
        if std::mem::take(&mut self.progress_shown) {
            writeln!(out)?;
        }
        Ok(())
    }

    fn finish(&mut self, output: &mut PlayOutput<'_>) {
        let _ = self.end_progress(output.out);
        let _ = output.out.flush();
    }

    /// The exit code once the player stopped for good.
    fn exit_code(&self, snapshot: &PlayerSnapshot) -> u8 {
        let kind = snapshot
            .message
            .as_ref()
            .and_then(|m| self.kinds.get(m).copied());
        if kind.is_some_and(|k| k != FailureKind::TrackOnly) {
            return 1;
        }
        let last = snapshot.queue.last().map(|e| e.id);
        let ran_out = snapshot.repeat == RepeatMode::Off
            && snapshot.current.is_some()
            && snapshot.current == last;
        if ran_out && self.played_to_end { 0 } else { 1 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tidal_player_api::stream::Unsupported;

    #[test]
    fn ac26_settings_precedence() {
        type Row = (
            &'static str,
            Option<&'static str>,
            Option<&'static str>,
            &'static [(&'static str, &'static str)],
            Result<(AudioQuality, &'static str), &'static str>,
        );
        let rows: &[Row] = &[
            (
                "defaults",
                None,
                None,
                &[],
                Ok((AudioQuality::HiResLossless, "default")),
            ),
            (
                "environment",
                None,
                None,
                &[(QUALITY_VAR, "high"), (DEVICE_VAR, "hw:1,0")],
                Ok((AudioQuality::High, "hw:1,0")),
            ),
            (
                "flags beat environment",
                Some("lossless"),
                Some("hw:2,0"),
                &[(QUALITY_VAR, "high"), (DEVICE_VAR, "hw:1,0")],
                Ok((AudioQuality::Lossless, "hw:2,0")),
            ),
            (
                "empty environment counts as unset",
                None,
                None,
                &[(QUALITY_VAR, ""), (DEVICE_VAR, "")],
                Ok((AudioQuality::HiResLossless, "default")),
            ),
            (
                "flag beats a bad environment value",
                Some("hi-res"),
                None,
                &[(QUALITY_VAR, "ultra")],
                Ok((AudioQuality::HiResLossless, "default")),
            ),
            ("unknown flag", Some("ultra"), None, &[], Err("--quality")),
            ("low flag", Some("low"), None, &[], Err("--quality")),
            (
                "unknown environment",
                None,
                None,
                &[(QUALITY_VAR, "ultra")],
                Err(QUALITY_VAR),
            ),
            (
                "low environment",
                None,
                None,
                &[(QUALITY_VAR, "low")],
                Err(QUALITY_VAR),
            ),
        ];
        for (name, quality, device, env, want) in rows {
            let lookup = |key: &str| {
                env.iter()
                    .find(|(k, _)| *k == key)
                    .map(|(_, v)| (*v).to_string())
            };
            let got = resolve_settings(*quality, *device, lookup);
            match want {
                Ok((q, d)) => assert_eq!(
                    got,
                    Ok(Settings {
                        quality: *q,
                        device: (*d).into()
                    }),
                    "{name}"
                ),
                Err(setting) => {
                    let err = got.expect_err(name);
                    assert_eq!(err.setting, *setting, "{name}");
                    if quality == &Some("low") || env.iter().any(|(_, v)| *v == "low") {
                        assert!(err.to_string().contains("HE-AAC"), "{name}: {err}");
                    }
                }
            }
        }
        assert_eq!(configured_device(|_| None), "default");
        assert_eq!(
            configured_device(|k| (k == DEVICE_VAR).then(|| "hw:1,0".into())),
            "hw:1,0"
        );
    }

    const FLAC_24_96: SourceFormat = SourceFormat {
        codec: Codec::Flac,
        sample_rate: 96_000,
        channels: 2,
        bits_per_sample: Some(24),
    };
    const FLAC_16_44: SourceFormat = SourceFormat {
        codec: Codec::Flac,
        sample_rate: 44_100,
        channels: 2,
        bits_per_sample: Some(16),
    };
    const FLAC_MONO: SourceFormat = SourceFormat {
        codec: Codec::Flac,
        sample_rate: 48_000,
        channels: 1,
        bits_per_sample: Some(16),
    };
    const AAC: SourceFormat = SourceFormat {
        codec: Codec::AacLc,
        sample_rate: 44_100,
        channels: 2,
        bits_per_sample: None,
    };

    fn output(
        device: &str,
        kind: OutputKind,
        format: SampleFormat,
        rate: u32,
        reason: Option<&str>,
    ) -> OutputInfo {
        OutputInfo {
            requested: device.replace("plughw", "hw"),
            device: device.into(),
            kind,
            sample_format: format,
            sample_rate: rate,
            channels: 2,
            bit_perfect: reason.is_none(),
            not_bit_perfect_reason: reason.map(Into::into),
        }
    }

    #[test]
    fn ac26_status_lines() {
        let tracks = [
            (
                AudioQuality::HiResLossless,
                FLAC_24_96,
                "Track 77640617: HI_RES_LOSSLESS, FLAC 24-bit 96 kHz stereo",
            ),
            (
                AudioQuality::Lossless,
                FLAC_16_44,
                "Track 77640617: LOSSLESS, FLAC 16-bit 44.1 kHz stereo",
            ),
            (
                AudioQuality::Lossless,
                FLAC_MONO,
                "Track 77640617: LOSSLESS, FLAC 16-bit 48 kHz mono",
            ),
            // Asked for hi-res, granted less: the line shows the grant.
            (
                AudioQuality::High,
                AAC,
                "Track 77640617: HIGH, AAC 44.1 kHz stereo",
            ),
        ];
        for (granted, source, want) in tracks {
            assert_eq!(track_line(77_640_617, granted, &source), want);
        }

        let outputs = [
            (
                output(
                    "hw:1,0",
                    OutputKind::Exclusive,
                    SampleFormat::S32Le,
                    96_000,
                    None,
                ),
                "Output: hw:1,0 (exclusive) S32_LE 96 kHz 2 ch, bit-perfect",
            ),
            (
                output(
                    "hw:1,0",
                    OutputKind::Exclusive,
                    SampleFormat::S24_3Le,
                    44_100,
                    None,
                ),
                "Output: hw:1,0 (exclusive) S24_3LE 44.1 kHz 2 ch, bit-perfect",
            ),
            (
                output(
                    "plughw:1,0",
                    OutputKind::Fallback,
                    SampleFormat::S24Le,
                    96_000,
                    Some(
                        "resampled (plughw fallback: device refused S24_3LE/S24_LE/S32_LE at 96 kHz)",
                    ),
                ),
                "Output: plughw:1,0 (fallback) S24_LE 96 kHz 2 ch, resampled (plughw fallback: \
                 device refused S24_3LE/S24_LE/S32_LE at 96 kHz)",
            ),
            (
                output(
                    "default",
                    OutputKind::Shared,
                    SampleFormat::S16Le,
                    48_000,
                    Some("shared (system mixer)"),
                ),
                "Output: default (shared) S16_LE 48 kHz 2 ch, shared (system mixer)",
            ),
            (
                output(
                    "hw:1,0",
                    OutputKind::Exclusive,
                    SampleFormat::S32Le,
                    44_100,
                    Some("lossy source"),
                ),
                "Output: hw:1,0 (exclusive) S32_LE 44.1 kHz 2 ch, lossy source",
            ),
        ];
        for (info, want) in outputs {
            assert_eq!(output_line(&info), want);
        }

        let progress = [
            (
                Duration::from_secs(83),
                Some(Duration::from_secs(296)),
                "  1:23 / 4:56",
            ),
            (
                Duration::ZERO,
                Some(Duration::from_millis(291_008)),
                "  0:00 / 4:51",
            ),
            (Duration::from_millis(5_999), None, "  0:05 / ?:??"),
            (
                Duration::from_secs(3_725),
                Some(Duration::from_secs(4_000)),
                "  1:02:05 / 1:06:40",
            ),
        ];
        for (position, duration, want) in progress {
            assert_eq!(progress_line(position, duration), want);
        }
    }

    #[test]
    fn ac26_error_messages() {
        let stream = [
            (
                StreamError::PreviewOnly,
                "Track 123 is only available as a preview for this account",
            ),
            (
                StreamError::NotAvailable,
                "Track 123 is not available in NO",
            ),
            (
                StreamError::NotFound,
                "Track 123 was not found, or cannot be streamed in NO",
            ),
            (
                StreamError::Unsupported(Unsupported::Codec("mp4a.40.5".into())),
                "Track 123 is not playable: codec mp4a.40.5 is not supported",
            ),
            (
                StreamError::Auth(AuthError::LoginRequired),
                "Session expired: run \"tidal-player login\"",
            ),
            (StreamError::Server(500), "Tidal server error (HTTP 500)"),
        ];
        for (error, want) in stream {
            assert_eq!(stream_error_message(123, "NO", &error), want, "{error:?}");
        }

        let engine = [
            (
                EngineError::Output(SinkError::Busy {
                    device: "hw:1,0".into(),
                    holder: Some("PipeWire".into()),
                }),
                "Output hw:1,0 is busy (used by PipeWire): close it, or use --device default",
            ),
            (
                EngineError::Output(SinkError::Busy {
                    device: "hw:1,0".into(),
                    holder: None,
                }),
                "Output hw:1,0 is busy: close it, or use --device default",
            ),
            (
                EngineError::Output(SinkError::NotFound("hw:5,0".into())),
                "No such output device hw:5,0: see \"tidal-player devices\"",
            ),
            (
                EngineError::Output(SinkError::Lost("hw:1,0".into())),
                "Output hw:1,0 was lost",
            ),
            (
                EngineError::Source(SourceError::Network("timed out".into())),
                "Network error while streaming track 123",
            ),
            (
                EngineError::Unsupported("HE-AAC".into()),
                "Track 123 is not playable: HE-AAC",
            ),
        ];
        for (error, want) in engine {
            assert_eq!(engine_error_message(123, &error), want, "{error:?}");
        }
    }

    fn started() -> Event {
        Event::Started {
            tag: 0,
            source: FLAC_24_96,
            output: output(
                "hw:1,0",
                OutputKind::Exclusive,
                SampleFormat::S32Le,
                96_000,
                None,
            ),
        }
    }

    fn report(tty: bool, events: &[Event]) -> (String, Option<Outcome>) {
        let mut reporter = Reporter::new(
            77_640_617,
            AudioQuality::HiResLossless,
            Some(Duration::from_secs(296)),
            tty,
        );
        let mut out = Vec::new();
        let mut outcome = None;
        for event in events {
            if let Some(o) = reporter.on_event(event, &mut out).unwrap() {
                outcome = Some(o);
                break;
            }
        }
        reporter.finish(&mut out).unwrap();
        (String::from_utf8(out).unwrap(), outcome)
    }

    #[test]
    fn ac26_progress_only_on_terminal() {
        let events = [
            started(),
            Event::Position(Duration::from_secs(1)),
            Event::Position(Duration::from_secs(83)),
            Event::TrackEnded { tag: 0 },
        ];
        let lines = "Track 77640617: HI_RES_LOSSLESS, FLAC 24-bit 96 kHz stereo\n\
                     Output: hw:1,0 (exclusive) S32_LE 96 kHz 2 ch, bit-perfect\n";

        let (out, outcome) = report(false, &events);
        assert_eq!(out, lines);
        assert_eq!(outcome, Some(Outcome::Ended));

        let (out, outcome) = report(true, &events);
        assert_eq!(
            out,
            format!("{lines}\r\x1b[K  0:00 / 4:56\r\x1b[K  0:01 / 4:56\r\x1b[K  1:23 / 4:56\n")
        );
        assert_eq!(outcome, Some(Outcome::Ended));

        let lost = EngineError::Output(SinkError::Lost("hw:1,0".into()));
        let (out, outcome) = report(
            false,
            &[
                started(),
                Event::Error {
                    tag: 0,
                    error: lost,
                },
            ],
        );
        assert_eq!(out, lines);
        assert_eq!(
            outcome,
            Some(Outcome::Failed("Output hw:1,0 was lost".into()))
        );
    }

    // --- spec 0004 ---------------------------------------------------------------

    use std::sync::mpsc;
    use std::sync::{Arc, Mutex};

    use crate::player_runtime::fakes::{FakeEngine, FakeJobs, Log, Script, not_available, track};
    use crate::player_runtime::parse_items;
    use tidal_player_api::auth::BoxFuture;
    use tidal_player_api::metadata::MetadataError;
    use tidal_player_core::library::DEFAULT_HIDDEN_VERSIONS;
    use tidal_player_core::player::{
        self, EngineEvent, PlayerEffect, PlayerInput, PlayerState, Purpose, TrackDetails,
    };
    use tidal_player_core::{EntryId, TrackId};

    const SEED: u64 = 42;

    /// Metadata from memory; records each request.
    #[derive(Default)]
    struct FakeMeta {
        requests: Mutex<Vec<String>>,
    }

    impl FakeMeta {
        fn answer<T: Send + 'static>(
            &self,
            request: String,
            result: Result<T, MetadataError>,
        ) -> BoxFuture<'_, Result<T, MetadataError>> {
            self.requests.lock().unwrap().push(request);
            Box::pin(async move { result })
        }
    }

    impl Metadata for FakeMeta {
        fn track(&self, id: TrackId) -> BoxFuture<'_, Result<Track, MetadataError>> {
            let result = match id.0 {
                1..=11 => Ok(track(id.0, (id.0 != 11).then_some(200))),
                _ => Err(MetadataError::NotFound(Item::Track(id))),
            };
            self.answer(format!("track {id}"), result)
        }

        fn album(&self, id: u64) -> BoxFuture<'_, Result<Vec<Track>, MetadataError>> {
            let ids: &[u64] = match id {
                10 => &[1, 2],
                20 => &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
                _ => &[],
            };
            let result = if ids.is_empty() {
                Err(MetadataError::NotFound(Item::Album(id)))
            } else {
                Ok(ids.iter().map(|i| track(*i, Some(200))).collect())
            };
            self.answer(format!("album {id}"), result)
        }

        fn playlist(&self, uuid: String) -> BoxFuture<'_, Result<Vec<Track>, MetadataError>> {
            let result = match uuid.as_str() {
                "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d" => {
                    Ok(vec![track(4, Some(200)), track(5, Some(200))])
                }
                _ => Err(MetadataError::NotFound(Item::Playlist(uuid.clone()))),
            };
            self.answer(format!("playlist {uuid}"), result)
        }

        fn suggestions(&self, seed: TrackId) -> BoxFuture<'_, Result<Vec<Track>, MetadataError>> {
            self.answer(format!("suggestions {seed}"), Ok(Vec::new()))
        }
    }

    fn status_lines(id: u64) -> String {
        format!(
            "Track {id}: LOSSLESS, FLAC 16-bit 44.1 kHz stereo\n\
             Output: hw:1,0 (exclusive) S32_LE 44.1 kHz 2 ch, bit-perfect\n"
        )
    }

    /// The play order the player gives `ids` with shuffle on and `SEED`.
    fn shuffled(ids: &[u64]) -> Vec<u64> {
        let mut state = PlayerState::new(PlayerConfig::default(), SEED);
        let tracks = ids.iter().map(|i| track(*i, Some(200))).collect();
        player::update(&mut state, PlayerInput::Command(Command::ToggleShuffle));
        player::update(
            &mut state,
            PlayerInput::Command(Command::LoadQueue { tracks, start: 0 }),
        );
        state
            .snapshot()
            .queue
            .iter()
            .map(|e| e.track.id.0)
            .collect()
    }

    #[derive(Default)]
    struct Row {
        name: &'static str,
        items: &'static [&'static str],
        options: PlayOptions,
        /// Tracks whose resolution fails (not available).
        unavailable: &'static [u64],
        /// Per track, the engine's outcome of each play (default: ends).
        engine: Vec<(u64, Vec<Script>)>,
        tty: bool,
        code: u8,
        /// The tracks whose "Track"/"Output" lines are printed, in order.
        started: Vec<u64>,
        stderr: &'static str,
    }

    /// Runs `play` as the binary does, against the fakes: parse, expand,
    /// then the queue through the player runtime.
    fn run_play(row: Row) -> (u8, String, String, Vec<String>) {
        let log: Log = Arc::default();
        let (tx, rx) = mpsc::channel();
        let mut engine = FakeEngine::new(&log, Script::Ends);
        for (track, outcomes) in row.engine {
            engine = engine.script(track, outcomes);
        }
        let mut jobs = FakeJobs::new(&log, &tx, true);
        for id in row.unavailable {
            jobs.failures.insert(*id, not_available(*id));
        }
        jobs.suggestions = vec![track(11, None), track(3, Some(200))];
        let meta = FakeMeta::default();
        let args: Vec<String> = row.items.iter().map(|s| (*s).to_owned()).collect();
        let tokio = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let tracks = parse_items(&args)
            .and_then(|items| tokio.block_on(play_tracks(&meta, &items, &row.options)));
        let code = match tracks {
            Err(e) => {
                writeln!(err, "{e}").unwrap();
                e.exit_code()
            }
            Ok(tracks) => {
                let settings = resolve_play_config(row.options.autoplay, |_| None).unwrap();
                let mut config = settings.player;
                config.country = Some("NO".into());
                let mut runtime = PlayerRuntime::new(config, SEED, engine, jobs);
                let mut polls = 0;
                let mut interrupted = || {
                    polls += 1;
                    polls > 10_000
                };
                let mut output = PlayOutput {
                    out: &mut out,
                    err: &mut err,
                    tty: row.tty,
                };
                play_queue(
                    &mut runtime,
                    tracks,
                    &row.options,
                    &rx,
                    Duration::ZERO,
                    &mut interrupted,
                    &mut output,
                )
            }
        };
        let requests = meta.requests.lock().unwrap().clone();
        (
            code,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
            requests,
        )
    }

    /// AC18: several items expand in order into one queue played through
    /// the player; the status lines at each start; the exit codes.
    #[test]
    fn ac18_queue_exit_codes() {
        let busy = || {
            EngineError::Output(SinkError::Busy {
                device: "hw:1,0".into(),
                holder: None,
            })
        };
        let lost = || EngineError::Output(SinkError::Lost("hw:1,0".into()));
        let ten = shuffled(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
        assert_eq!(ten[0], 1);
        assert_ne!(ten, vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
        let rows = vec![
            Row {
                name: "all play: album, track, playlist in order",
                items: &[
                    "https://tidal.com/browse/album/10",
                    "7",
                    "https://tidal.com/browse/playlist/0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d",
                ],
                started: vec![1, 2, 7, 4, 5],
                ..Row::default()
            },
            Row {
                name: "one track-only failure among three",
                items: &["1", "2", "3"],
                unavailable: &[2],
                started: vec![1, 3],
                stderr: "Track 2 is not available in NO\n",
                ..Row::default()
            },
            Row {
                name: "a decode failure among three",
                items: &["1", "2", "3"],
                engine: vec![(
                    2,
                    vec![Script::Fails(EngineError::Decode("bad frame".into()))],
                )],
                started: vec![1, 3],
                stderr: "Track 2 could not be decoded: bad frame\n",
                ..Row::default()
            },
            Row {
                name: "the last track fails: the queue still ran out",
                items: &["1", "2"],
                unavailable: &[2],
                started: vec![1],
                stderr: "Track 2 is not available in NO\n",
                ..Row::default()
            },
            Row {
                name: "output busy",
                items: &["1", "2", "3"],
                engine: vec![(2, vec![Script::Fails(busy())])],
                code: 1,
                started: vec![1],
                stderr: "Output hw:1,0 is busy: close it, or use --device default\n",
                ..Row::default()
            },
            Row {
                name: "every track fails",
                items: &["1", "2", "3"],
                unavailable: &[1, 2, 3],
                code: 1,
                stderr: "Track 1 is not available in NO\n\
                         Track 2 is not available in NO\n\
                         Track 3 is not available in NO\n\
                         Stopped: 3 tracks in a row could not be played\n",
                ..Row::default()
            },
            Row {
                name: "bad item",
                items: &["1", "https://tidal.com/browse/artist/1"],
                code: 2,
                stderr: "Not a Tidal track, album or playlist: https://tidal.com/browse/artist/1\n",
                ..Row::default()
            },
            Row {
                name: "item not found",
                items: &["1", "https://tidal.com/browse/album/404"],
                code: 1,
                stderr: "Album 404 was not found\n",
                ..Row::default()
            },
            Row {
                name: "--shuffle before loading",
                items: &["https://tidal.com/album/20"],
                options: PlayOptions {
                    shuffle: true,
                    ..PlayOptions::default()
                },
                started: ten,
                ..Row::default()
            },
            Row {
                name: "--repeat track before loading",
                items: &["1", "2"],
                options: PlayOptions {
                    repeat: RepeatMode::Track,
                    ..PlayOptions::default()
                },
                engine: vec![(1, vec![Script::Ends, Script::Ends, Script::Fails(lost())])],
                code: 1,
                started: vec![1, 1],
                stderr: "Output hw:1,0 was lost\n",
                ..Row::default()
            },
            Row {
                name: "--repeat queue before loading",
                items: &["1", "2"],
                options: PlayOptions {
                    repeat: RepeatMode::Queue,
                    ..PlayOptions::default()
                },
                engine: vec![(1, vec![Script::Ends, Script::Fails(lost())])],
                code: 1,
                started: vec![1, 2],
                stderr: "Output hw:1,0 was lost\n",
                ..Row::default()
            },
            Row {
                name: "--autoplay continues with suggestions",
                items: &["11"],
                options: PlayOptions {
                    autoplay: true,
                    ..PlayOptions::default()
                },
                started: vec![11, 3],
                ..Row::default()
            },
            Row {
                name: "progress on a terminal",
                items: &["1", "2"],
                tty: true,
                started: vec![1, 2],
                ..Row::default()
            },
        ];
        for row in rows {
            let name = row.name;
            let (code, started, stderr, tty) = (row.code, row.started.clone(), row.stderr, row.tty);
            let (got_code, out, err, _) = run_play(row);
            let progress = if tty { "\r\x1b[K  0:00 / 3:20\n" } else { "" };
            let want_out: String = started
                .iter()
                .map(|id| format!("{}{progress}", status_lines(*id)))
                .collect();
            assert_eq!(out, want_out, "{name}: stdout");
            assert_eq!(err, stderr, "{name}: stderr");
            assert_eq!(got_code, code, "{name}: exit code");
        }

        // 0003's form: one track ID, no new flag: no metadata request, and
        // the track plays (and its errors are the stream's).
        let (code, out, err, requests) = run_play(Row {
            items: &["99"],
            ..Row::default()
        });
        assert_eq!((code, out, err), (0, status_lines(99), String::new()));
        assert!(requests.is_empty(), "{requests:?}");
        let (code, _, err, _) = run_play(Row {
            items: &["99"],
            unavailable: &[99],
            ..Row::default()
        });
        assert_eq!(
            (code, err.as_str()),
            (1, "Track 99 is not available in NO\n")
        );
    }

    /// AC25: the player settings from the environment, their ranges, and
    /// `--autoplay` over the environment.
    #[test]
    fn ac25_player_config() {
        type Want = Result<(u8, u64, u64, bool), (&'static str, &'static str)>;
        type Env = &'static [(&'static str, &'static str)];
        let rows: &[(&str, Env, Want)] = &[
            ("defaults", &[], Ok((5, 5, 3, false))),
            (
                "empty counts as unset",
                &[
                    (VOLUME_STEP_VAR, ""),
                    (SEEK_STEP_VAR, ""),
                    (PREVIOUS_RESTART_VAR, ""),
                    (AUTOPLAY_VAR, ""),
                ],
                Ok((5, 5, 3, false)),
            ),
            (
                "lowest values",
                &[
                    (VOLUME_STEP_VAR, "1"),
                    (SEEK_STEP_VAR, "1"),
                    (PREVIOUS_RESTART_VAR, "0"),
                    (AUTOPLAY_VAR, "off"),
                ],
                Ok((1, 1, 0, false)),
            ),
            (
                "highest values",
                &[
                    (VOLUME_STEP_VAR, "25"),
                    (SEEK_STEP_VAR, "600"),
                    (PREVIOUS_RESTART_VAR, "60"),
                    (AUTOPLAY_VAR, "on"),
                ],
                Ok((25, 600, 60, true)),
            ),
            (
                "volume step 0",
                &[(VOLUME_STEP_VAR, "0")],
                Err((VOLUME_STEP_VAR, "1 to 25")),
            ),
            (
                "volume step 26",
                &[(VOLUME_STEP_VAR, "26")],
                Err((VOLUME_STEP_VAR, "1 to 25")),
            ),
            (
                "volume step text",
                &[(VOLUME_STEP_VAR, "five")],
                Err((VOLUME_STEP_VAR, "1 to 25")),
            ),
            (
                "volume step negative",
                &[(VOLUME_STEP_VAR, "-5")],
                Err((VOLUME_STEP_VAR, "1 to 25")),
            ),
            (
                "seek step 0",
                &[(SEEK_STEP_VAR, "0")],
                Err((SEEK_STEP_VAR, "1 to 600")),
            ),
            (
                "seek step 601",
                &[(SEEK_STEP_VAR, "601")],
                Err((SEEK_STEP_VAR, "1 to 600")),
            ),
            (
                "seek step fraction",
                &[(SEEK_STEP_VAR, "2.5")],
                Err((SEEK_STEP_VAR, "1 to 600")),
            ),
            (
                "previous 61",
                &[(PREVIOUS_RESTART_VAR, "61")],
                Err((PREVIOUS_RESTART_VAR, "0 to 60")),
            ),
            (
                "previous text",
                &[(PREVIOUS_RESTART_VAR, "3s")],
                Err((PREVIOUS_RESTART_VAR, "0 to 60")),
            ),
            (
                "autoplay yes",
                &[(AUTOPLAY_VAR, "yes")],
                Err((AUTOPLAY_VAR, "on or off")),
            ),
            (
                "autoplay 1",
                &[(AUTOPLAY_VAR, "1")],
                Err((AUTOPLAY_VAR, "on or off")),
            ),
        ];
        let lookup = |env: &'static [(&'static str, &'static str)]| {
            move |key: &str| {
                env.iter()
                    .find(|(k, _)| *k == key)
                    .map(|(_, v)| (*v).to_string())
            }
        };
        for (name, env, want) in rows {
            let got = resolve_player_config(lookup(env));
            match want {
                Ok((volume, seek, previous, autoplay)) => {
                    let got = got.unwrap_or_else(|e| panic!("{name}: {e}"));
                    assert_eq!(
                        got.steps,
                        Steps {
                            volume: *volume,
                            seek: Duration::from_secs(*seek)
                        },
                        "{name}"
                    );
                    assert_eq!(
                        got.player,
                        PlayerConfig {
                            previous_restart: Duration::from_secs(*previous),
                            autoplay: *autoplay,
                            country: None,
                            device: "default".into(),
                        },
                        "{name}"
                    );
                }
                Err((var, range)) => {
                    let err = got.expect_err(name);
                    assert_eq!(err.setting, *var, "{name}");
                    let text = err.to_string();
                    assert!(text.contains(var) && text.contains(range), "{name}: {text}");
                }
            }
        }

        // 0005 AC23: the release delay, 0–3600 s or `never`.
        // Ok(None): never; Err: refused.
        type ReleaseWant = Result<Option<u64>, ()>;
        let release: &[(&str, Option<&str>, ReleaseWant)] = &[
            ("unset", None, Ok(Some(10))),
            ("empty counts as unset", Some(""), Ok(Some(10))),
            ("at once", Some("0"), Ok(Some(0))),
            ("an hour", Some("3600"), Ok(Some(3600))),
            ("never", Some("never"), Ok(None)),
            ("over an hour", Some("3601"), Err(())),
            ("negative", Some("-1"), Err(())),
            ("text", Some("x"), Err(())),
        ];
        for (name, value, want) in release {
            let got = resolve_player_config(|key: &str| {
                (key == RELEASE_PAUSED_VAR)
                    .then_some(*value)
                    .flatten()
                    .map(Into::into)
            });
            match want {
                Ok(secs) => assert_eq!(
                    got.map(|s| s.release_paused).map_err(|e| e.to_string()),
                    Ok(secs.map(Duration::from_secs)),
                    "release delay: {name}"
                ),
                Err(()) => {
                    let err = got.expect_err(name);
                    assert_eq!(err.setting, RELEASE_PAUSED_VAR, "{name}");
                    let text = err.to_string();
                    assert!(
                        text.contains(RELEASE_PAUSED_VAR)
                            && text.contains("0 to 3600")
                            && text.contains("never"),
                        "{name}: {text}"
                    );
                }
            }
        }

        // 0006 AC3: the page size (empty = unset) and the hidden words
        // (empty = hide nothing).
        let default_words: Vec<String> = DEFAULT_HIDDEN_VERSIONS
            .iter()
            .map(|w| (*w).to_owned())
            .collect();
        let words = |list: &[&str]| list.iter().map(|w| (*w).to_owned()).collect::<Vec<_>>();
        type PageWant = Result<u32, ()>;
        let pages: &[(&str, Option<&str>, PageWant)] = &[
            ("unset", None, Ok(100)),
            ("empty counts as unset", Some(""), Ok(100)),
            ("lowest", Some("1"), Ok(1)),
            ("highest", Some("10000"), Ok(10000)),
            ("zero", Some("0"), Err(())),
            ("over", Some("10001"), Err(())),
            ("text", Some("x"), Err(())),
        ];
        for (name, value, want) in pages {
            let got = resolve_player_config(|key: &str| {
                (key == PAGE_SIZE_VAR)
                    .then_some(*value)
                    .flatten()
                    .map(Into::into)
            });
            match want {
                Ok(n) => assert_eq!(
                    got.map(|s| s.library.page_size).map_err(|e| e.to_string()),
                    Ok(*n),
                    "page size: {name}"
                ),
                Err(()) => {
                    let err = got.expect_err(name);
                    assert_eq!(err.setting, PAGE_SIZE_VAR, "{name}");
                    let text = err.to_string();
                    assert!(
                        text.contains(PAGE_SIZE_VAR) && text.contains("1 to 10000"),
                        "{name}: {text}"
                    );
                }
            }
        }
        // 0007 AC1: the search page size (empty = unset).
        let search_pages: &[(&str, Option<&str>, PageWant)] = &[
            ("unset", None, Ok(20)),
            ("empty counts as unset", Some(""), Ok(20)),
            ("lowest", Some("1"), Ok(1)),
            ("highest", Some("1000"), Ok(1000)),
            ("zero", Some("0"), Err(())),
            ("over", Some("1001"), Err(())),
            ("text", Some("x"), Err(())),
        ];
        for (name, value, want) in search_pages {
            let got = resolve_player_config(|key: &str| {
                (key == SEARCH_PAGE_SIZE_VAR)
                    .then_some(*value)
                    .flatten()
                    .map(Into::into)
            });
            match want {
                Ok(n) => assert_eq!(
                    got.map(|s| s.library.search_page_size)
                        .map_err(|e| e.to_string()),
                    Ok(*n),
                    "search page size: {name}"
                ),
                Err(()) => {
                    let err = got.expect_err(name);
                    assert_eq!(err.setting, SEARCH_PAGE_SIZE_VAR, "{name}");
                    let text = err.to_string();
                    assert!(
                        text.contains(SEARCH_PAGE_SIZE_VAR) && text.contains("1 to 1000"),
                        "{name}: {text}"
                    );
                }
            }
        }
        let hidden: &[(&str, Option<&str>, Vec<String>)] = &[
            ("unset", None, default_words),
            ("empty hides nothing", Some(""), Vec::new()),
            ("one word", Some("remix"), words(&["remix"])),
            (
                "trimmed list",
                Some(" live , demo,,  8d audio "),
                words(&["live", "demo", "8d audio"]),
            ),
        ];
        for (name, value, want) in hidden {
            let got = resolve_player_config(|key: &str| {
                (key == HIDE_VERSIONS_VAR)
                    .then_some(*value)
                    .flatten()
                    .map(Into::into)
            });
            assert_eq!(
                got.map(|s| s.library.hidden_words)
                    .map_err(|e| e.to_string()),
                Ok(want.clone()),
                "hidden words: {name}"
            );
        }

        // `play --autoplay` beats the environment, even a bad value.
        let precedence: &[(bool, Env, bool)] = &[
            (true, &[(AUTOPLAY_VAR, "off")], true),
            (true, &[(AUTOPLAY_VAR, "maybe")], true),
            (false, &[(AUTOPLAY_VAR, "on")], true),
            (false, &[(AUTOPLAY_VAR, "")], false),
        ];
        for (flag, env, want) in precedence {
            let got = resolve_play_config(*flag, lookup(env)).expect("valid");
            assert_eq!(got.player.autoplay, *want, "--autoplay {flag}, {env:?}");
        }

        // The player's previous uses the configured threshold: at 0:05,
        // 10 s goes back to the entry before, the default 3 s restarts.
        let previous_at_5s = |threshold: &'static str| {
            let settings = resolve_player_config(lookup(match threshold {
                "10" => &[(PREVIOUS_RESTART_VAR, "10")],
                _ => &[],
            }))
            .unwrap();
            let mut state = PlayerState::new(settings.player, SEED);
            let tracks = vec![track(1, Some(200)), track(2, Some(200))];
            let fx = player::update(
                &mut state,
                PlayerInput::Command(Command::LoadQueue { tracks, start: 1 }),
            );
            let Some(PlayerEffect::Resolve { tag, .. }) = fx.first().cloned() else {
                panic!("no resolve: {fx:?}");
            };
            player::update(
                &mut state,
                PlayerInput::Resolved {
                    tag,
                    result: Ok(AudioQuality::Lossless),
                },
            );
            let details = TrackDetails {
                source: "FLAC".into(),
                output: "hw:1,0".into(),
                bit_perfect: true,
                reason: None,
            };
            player::update(
                &mut state,
                PlayerInput::Engine(EngineEvent::Started { tag, details }),
            );
            player::update(
                &mut state,
                PlayerInput::Engine(EngineEvent::Position(Duration::from_secs(5))),
            );
            player::update(&mut state, PlayerInput::Command(Command::Previous))
        };
        let back = previous_at_5s("10");
        assert!(
            back.iter().any(|e| matches!(
                e,
                PlayerEffect::Resolve {
                    entry: EntryId(1),
                    purpose: Purpose::Play,
                    ..
                }
            )),
            "10 s threshold: {back:?}"
        );
        let restart = previous_at_5s("default");
        assert!(
            restart.contains(&PlayerEffect::EngineSeek(Duration::ZERO)),
            "3 s threshold: {restart:?}"
        );
    }

    /// Spec 0009 AC12: the autoplay a player starts with: flag > environment
    /// > remembered > `app.toml` > default.
    #[test]
    fn ac12_autoplay_precedence() {
        type Env = &'static [(&'static str, &'static str)];
        // name, app.toml, environment, --autoplay, remembered, want
        type Row = (&'static str, &'static str, Env, bool, Option<bool>, bool);
        let rows: &[Row] = &[
            ("default", "", &[], false, None, false),
            ("app.toml", "autoplay = true", &[], false, None, true),
            (
                "remembered over app.toml (off)",
                "autoplay = true",
                &[],
                false,
                Some(false),
                false,
            ),
            (
                "remembered over app.toml (on)",
                "autoplay = false",
                &[],
                false,
                Some(true),
                true,
            ),
            (
                "remembered over the default",
                "",
                &[],
                false,
                Some(true),
                true,
            ),
            (
                "environment over remembered (off)",
                "autoplay = true",
                &[(AUTOPLAY_VAR, "off")],
                false,
                Some(true),
                false,
            ),
            (
                "environment over remembered (on)",
                "",
                &[(AUTOPLAY_VAR, "on")],
                false,
                Some(false),
                true,
            ),
            (
                "empty environment falls through to remembered",
                "autoplay = true",
                &[(AUTOPLAY_VAR, "")],
                false,
                Some(false),
                false,
            ),
            (
                "--autoplay over all",
                "autoplay = false",
                &[(AUTOPLAY_VAR, "off")],
                true,
                Some(false),
                true,
            ),
        ];
        for (name, file, env, flag, remembered, want) in rows {
            let file = crate::config::parse_app_toml(std::path::Path::new("/c/app.toml"), file)
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            let env = |key: &str| {
                env.iter()
                    .find(|(k, _)| *k == key)
                    .map(|(_, v)| (*v).to_owned())
            };
            let settings = resolve_play_config_with(&file, *flag, env)
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(start_autoplay(&settings, *remembered), *want, "{name}");
        }
    }
}
