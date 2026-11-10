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
use crate::player::runtime::{
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
    PlayerConfig {
        device: settings.device.clone(),
        ..player
    }
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
/// `on` or `off`: whether the TUI asks before removing (spec 0019).
pub const CONFIRM_REMOVALS_VAR: &str = "TIDAL_PLAYER_CONFIRM_REMOVALS";
/// `on` or `off`: whether the TUI draws the album cover (spec 0016).
pub const COVER_IMG_VAR: &str = "TIDAL_PLAYER_COVER_IMG";
/// The playback window's rows with the cover, 4 to 20 (spec 0016).
pub const COVER_IMG_ROWS_VAR: &str = "TIDAL_PLAYER_COVER_IMG_ROWS";
/// The cover's graphics protocol (spec 0016).
pub const COVER_IMG_PROTOCOL_VAR: &str = "TIDAL_PLAYER_COVER_IMG_PROTOCOL";

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

/// Comma-separated names of the browse pages whose links the TUI does not
/// show (spec 0024); empty hides nothing.
pub const HIDE_BROWSE_PAGES_VAR: &str = "TIDAL_PLAYER_HIDE_BROWSE_PAGES";
/// The default of `hide_browse_pages`: the two pages with nothing to show.
pub const DEFAULT_HIDE_BROWSE_PAGES: [&str; 2] = ["videos", "tidal_magazine"];

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
    /// Whether the TUI asks before removing from favorites, a playlist or
    /// the queue (spec 0019); read by the TUI only.
    pub confirm_removals: bool,
    /// The browse pages whose links the TUI does not show (spec 0024);
    /// read by the TUI only.
    pub hide_browse_pages: Vec<String>,
    /// Whether the TUI draws the album cover (spec 0016); read by the TUI
    /// only, as the two below.
    pub cover_img: bool,
    /// The playback window's rows while the cover is active.
    pub cover_img_rows: u16,
    /// The cover's graphics protocol.
    pub cover_img_protocol: crate::tui::cover::ProtocolSetting,
    /// The command run on each player event (spec 0025).
    pub hook: Option<crate::config::HookCommand>,
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
            confirm_removals: true,
            hide_browse_pages: DEFAULT_HIDE_BROWSE_PAGES
                .iter()
                .map(|name| (*name).to_owned())
                .collect(),
            cover_img: true,
            cover_img_rows: 6,
            cover_img_protocol: crate::tui::cover::ProtocolSetting::Auto,
            hook: None,
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
    // As `TIDAL_PLAYER_HIDE_VERSIONS`: an empty value hides nothing.
    if let Some(value) = env(HIDE_BROWSE_PAGES_VAR) {
        settings.hide_browse_pages = value
            .split(',')
            .map(str::trim)
            .filter(|name| !name.is_empty())
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
    if let Some(on) = on_off(get(CONFIRM_REMOVALS_VAR), CONFIRM_REMOVALS_VAR)? {
        settings.confirm_removals = on;
    }
    if let Some(value) = get(KEY_HINTS_DELAY_VAR) {
        settings.key_hints_delay =
            Duration::from_millis(int_in(&value, KEY_HINTS_DELAY_VAR, 0, 10_000)?);
    }
    if let Some(on) = on_off(get(COVER_IMG_VAR), COVER_IMG_VAR)? {
        settings.cover_img = on;
    }
    if let Some(value) = get(COVER_IMG_ROWS_VAR) {
        settings.cover_img_rows = int_in(&value, COVER_IMG_ROWS_VAR, 4, 20)? as u16;
    }
    if let Some(value) = get(COVER_IMG_PROTOCOL_VAR) {
        settings.cover_img_protocol = crate::tui::cover::ProtocolSetting::parse(&value)
            .ok_or_else(|| SettingsError {
                setting: COVER_IMG_PROTOCOL_VAR.into(),
                message: format!(
                    "expected {}, got \"{value}\"",
                    crate::tui::cover::PROTOCOL_NAMES
                ),
            })?;
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
    settings.hook.clone_from(&file.player_event_hook_command);
    if let Some(on) = file.key_hints {
        settings.key_hints = on;
    }
    if let Some(on) = file.confirm_removals {
        settings.confirm_removals = on;
    }
    if let Some(ms) = file.key_hints_delay_ms {
        settings.key_hints_delay = Duration::from_millis(ms);
    }
    if let Some(on) = file.cover_img {
        settings.cover_img = on;
    }
    if let Some(rows) = file.cover_img_rows {
        settings.cover_img_rows = rows;
    }
    if let Some(protocol) = file.cover_img_protocol {
        settings.cover_img_protocol = protocol;
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
    if let Some(names) = &file.hide_browse_pages {
        settings.hide_browse_pages = names.clone();
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
/// no new flag and no hook is queued as 0003 played it, without a metadata
/// request (its errors are 0003's, from the stream resolution); a hook needs
/// the track's title (spec 0025).
pub async fn play_tracks(
    meta: &dyn Metadata,
    items: &[Item],
    options: &PlayOptions,
    hooked: bool,
) -> Result<Vec<Track>, ExpandError> {
    if let [Item::Track(id)] = items
        && !options.has_new_flag()
        && !hooked
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
            runtime.hook_shutdown();
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
mod tests;
