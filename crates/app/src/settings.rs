//! The settings every mode starts from (spec 0003 "Settings", 0004
//! "Settings", 0008): the command line over the environment over
//! `app.toml` over the defaults, resolved once at start. Pure: the
//! environment is a function and the file is already parsed
//! ([`crate::config`]), so the precedence is tested without either.

use std::path::PathBuf;
use std::time::Duration;

use tidal_player_core::player::PlayerConfig;
use tidal_player_core::{AudioQuality, ParseQualityError};

use crate::config::{AppConfig, LibraryLayout};
use crate::player::runtime::LibrarySettings;

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
/// The lowest level the log keeps, over `log_level` in `app.toml` (spec
/// 0027).
pub const LOG_LEVEL_VAR: &str = "TIDAL_PLAYER_LOG_LEVEL";

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
    /// The lowest level the log keeps (spec 0027).
    pub log_level: tidal_player_core::log::LogSetting,
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
            log_level: tidal_player_core::log::LogSetting::default(),
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

#[cfg(test)]
mod tests;
