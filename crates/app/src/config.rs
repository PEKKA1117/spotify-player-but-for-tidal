//! The config directory and `app.toml` (spec 0008 "Where the files are",
//! "`app.toml`"), `keymap.toml`, and the TUI's `theme.toml` and theme
//! choice (spec 0017): pure functions over strings and a fake environment,
//! so the table tests need no files. Nothing is ever written.

use std::path::{Path, PathBuf};
use std::time::Duration;

use tidal_player_core::AudioQuality;
use tidal_player_core::ui::keymap::{self, KeymapFile};
use tidal_player_core::ui::theme::{self, ThemeFile};
use tidal_player_core::ui::{Keymap, Themes};

/// Overrides the config directory (below `--config-folder`).
pub const CONFIG_DIR_VAR: &str = "TIDAL_PLAYER_CONFIG_DIR";
/// The settings file's name in the config directory.
pub const APP_TOML: &str = "app.toml";
/// The keymap file's name in the config directory (read by the TUI).
pub const KEYMAP_TOML: &str = "keymap.toml";
/// The themes file's name in the config directory (read by the TUI only,
/// spec 0017 decision 5).
pub const THEME_TOML: &str = "theme.toml";
/// The TUI's theme, over `theme` in `app.toml` (spec 0017).
pub const THEME_VAR: &str = "TIDAL_PLAYER_THEME";

/// A config file that cannot be used (exit 2); the text already starts
/// with the file's path.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct ConfigError(pub String);

impl ConfigError {
    /// `<path>: <what>`.
    pub fn at(path: &Path, what: impl std::fmt::Display) -> Self {
        Self(format!("{}: {what}", path.display()))
    }
}

/// The library page's window widths (spec 0008 "The library layout"):
/// the TUI model's own type, which `tui_state` hands it.
pub use tidal_player_core::ui::LibraryLayout;

/// `player_event_hook_command` (spec 0025): the command run on each player
/// event, and the arguments that come before the event's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookCommand {
    pub command: String,
    pub args: Vec<String>,
}

/// What `app.toml` sets; `None`: not set there (the layer below the
/// environment falls through to the default).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppConfig {
    pub quality: Option<AudioQuality>,
    pub output_device: Option<String>,
    pub volume_step: Option<u8>,
    pub seek_duration_secs: Option<u64>,
    pub previous_restart_secs: Option<u64>,
    pub autoplay: Option<bool>,
    /// `Some(None)`: `"never"`.
    pub release_paused: Option<Option<Duration>>,
    pub page_size: Option<u32>,
    pub search_page_size: Option<u32>,
    pub hide_versions: Option<Vec<String>>,
    /// Spec 0024 "Hiding pages".
    pub hide_browse_pages: Option<Vec<String>>,
    pub layout: LibraryLayout,
    /// Spec 0009 "Turning it off".
    pub remember_playback: Option<bool>,
    /// Spec 0010 "Turning it off".
    pub mpris: Option<bool>,
    /// Spec 0010 "The cover cache".
    pub max_cover_arts: Option<u16>,
    /// Spec 0013: whether the TUI shows key-sequence hints.
    pub key_hints: Option<bool>,
    /// Spec 0013: how long a sequence is pending before its hint shows.
    pub key_hints_delay_ms: Option<u64>,
    /// Spec 0019: whether the TUI asks before removing.
    pub confirm_removals: Option<bool>,
    /// Spec 0025: the command run on each player event.
    pub player_event_hook_command: Option<HookCommand>,
    /// Spec 0016: whether the TUI draws the album cover.
    pub cover_img: Option<bool>,
    /// Spec 0016: the playback window's height with the cover.
    pub cover_img_rows: Option<u16>,
    /// Spec 0016: the graphics protocol.
    pub cover_img_protocol: Option<crate::tui::cover::ProtocolSetting>,
    /// Spec 0017: the TUI's theme, by name (checked against `theme.toml`
    /// by the TUI only).
    pub theme: Option<String>,
    /// Spec 0027: the lowest level the log keeps.
    pub log_level: Option<tidal_player_core::log::LogSetting>,
}

/// The config directory: `--config-folder`, else `$TIDAL_PLAYER_CONFIG_DIR`,
/// else `$XDG_CONFIG_HOME/tidal-player`, else `~/.config/tidal-player`; an
/// empty variable counts as unset.
pub fn config_dir(
    flag: Option<&Path>,
    dir_var: Option<&str>,
    xdg_config_home: Option<&str>,
    home: Option<&Path>,
) -> PathBuf {
    fn set(v: Option<&str>) -> Option<&str> {
        v.filter(|v| !v.is_empty())
    }
    if let Some(flag) = flag {
        return flag.to_owned();
    }
    if let Some(dir) = set(dir_var) {
        return PathBuf::from(dir);
    }
    if let Some(xdg) = set(xdg_config_home) {
        return Path::new(xdg).join("tidal-player");
    }
    home.unwrap_or(Path::new("."))
        .join(".config")
        .join("tidal-player")
}

/// [`config_dir`] for this process's environment.
pub fn process_config_dir(flag: Option<&Path>) -> PathBuf {
    let home = directories::BaseDirs::new().map(|d| d.home_dir().to_owned());
    config_dir(
        flag,
        std::env::var(CONFIG_DIR_VAR).ok().as_deref(),
        std::env::var("XDG_CONFIG_HOME").ok().as_deref(),
        home.as_deref(),
    )
}

/// `<dir>/app.toml`.
pub fn app_toml_path(dir: &Path) -> PathBuf {
    dir.join(APP_TOML)
}

/// `<dir>/keymap.toml`.
pub fn keymap_toml_path(dir: &Path) -> PathBuf {
    dir.join(KEYMAP_TOML)
}

/// `<dir>/theme.toml`.
pub fn theme_toml_path(dir: &Path) -> PathBuf {
    dir.join(THEME_TOML)
}

/// Parses the text of `app.toml` (`path` only names it in errors). Every
/// key is optional; an unknown key anywhere, a wrong type or a value out of
/// range is an error naming the key.
pub fn parse_app_toml(path: &Path, text: &str) -> Result<AppConfig, ConfigError> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let table: toml::Table = text
        .parse()
        .map_err(|e: toml::de::Error| syntax_error(path, text, &e))?;
    let invalid = |key: &str, what: String| ConfigError::at(path, format!("invalid {key}: {what}"));
    let mut config = AppConfig::default();
    for (key, value) in &table {
        match key.as_str() {
            "quality" => {
                config.quality = Some(match value.as_str() {
                    Some(name) => {
                        name.parse()
                            .map_err(|e: tidal_player_core::ParseQualityError| {
                                invalid(key, e.to_string())
                            })?
                    }
                    None => {
                        return Err(invalid(
                            key,
                            format!("expected hi-res, lossless or high, got {}", describe(value)),
                        ));
                    }
                });
            }
            "output_device" => match value.as_str().filter(|name| !name.is_empty()) {
                Some(name) => config.output_device = Some(name.to_owned()),
                None => {
                    return Err(invalid(
                        key,
                        format!("expected a device name, got {}", describe(value)),
                    ));
                }
            },
            "volume_step" => {
                config.volume_step = Some(int_in(path, key, value, 1, 25)? as u8);
            }
            "seek_duration_secs" => {
                config.seek_duration_secs = Some(int_in(path, key, value, 1, 600)?);
            }
            "previous_restart_secs" => {
                config.previous_restart_secs = Some(int_in(path, key, value, 0, 60)?);
            }
            "autoplay" => match value.as_bool() {
                Some(on) => config.autoplay = Some(on),
                None => {
                    return Err(invalid(
                        key,
                        format!("expected true or false, got {}", describe(value)),
                    ));
                }
            },
            "remember_playback" => match value.as_bool() {
                Some(on) => config.remember_playback = Some(on),
                None => {
                    return Err(invalid(
                        key,
                        format!("expected true or false, got {}", describe(value)),
                    ));
                }
            },
            "mpris" => match value.as_bool() {
                Some(on) => config.mpris = Some(on),
                None => {
                    return Err(invalid(
                        key,
                        format!("expected true or false, got {}", describe(value)),
                    ));
                }
            },
            "key_hints" => match value.as_bool() {
                Some(on) => config.key_hints = Some(on),
                None => {
                    return Err(invalid(
                        key,
                        format!("expected true or false, got {}", describe(value)),
                    ));
                }
            },
            "confirm_removals" => match value.as_bool() {
                Some(on) => config.confirm_removals = Some(on),
                None => {
                    return Err(invalid(
                        key,
                        format!("expected true or false, got {}", describe(value)),
                    ));
                }
            },
            "key_hints_delay_ms" => {
                config.key_hints_delay_ms = Some(int_in(path, key, value, 0, 10_000)?);
            }
            "cover_img" => match value.as_bool() {
                Some(on) => config.cover_img = Some(on),
                None => {
                    return Err(invalid(
                        key,
                        format!("expected true or false, got {}", describe(value)),
                    ));
                }
            },
            "cover_img_rows" => {
                config.cover_img_rows = Some(int_in(path, key, value, 4, 20)? as u16);
            }
            "cover_img_protocol" => {
                match value
                    .as_str()
                    .and_then(crate::tui::cover::ProtocolSetting::parse)
                {
                    Some(protocol) => config.cover_img_protocol = Some(protocol),
                    None => {
                        return Err(invalid(
                            key,
                            format!(
                                "expected {}, got {}",
                                crate::tui::cover::PROTOCOL_NAMES,
                                describe(value)
                            ),
                        ));
                    }
                }
            }
            "max_cover_arts" => {
                config.max_cover_arts = Some(int_in(path, key, value, 0, 1000)? as u16);
            }
            "release_paused_secs" => {
                config.release_paused = Some(if value.as_str() == Some("never") {
                    None
                } else {
                    let secs = int_in(path, key, value, 0, 3600).map_err(|_| {
                        invalid(
                            key,
                            format!(
                                "expected an integer from 0 to 3600 or never, got {}",
                                describe(value)
                            ),
                        )
                    })?;
                    Some(Duration::from_secs(secs))
                });
            }
            "page_size" => config.page_size = Some(int_in(path, key, value, 1, 10_000)? as u32),
            "search_page_size" => {
                config.search_page_size = Some(int_in(path, key, value, 1, 1000)? as u32);
            }
            "hide_versions" => config.hide_versions = Some(words(path, key, value)?),
            "hide_browse_pages" => config.hide_browse_pages = Some(words(path, key, value)?),
            "layout" => config.layout = layout(path, value)?,
            "player_event_hook_command" => {
                config.player_event_hook_command = Some(hook_command(path, key, value)?);
            }
            "log_level" => {}
            "theme" => match value.as_str() {
                Some(name) => config.theme = Some(name.to_owned()),
                None => {
                    return Err(invalid(
                        key,
                        format!("expected a string, got {}", describe(value)),
                    ));
                }
            },
            _ => return Err(unknown(path, key)),
        }
    }
    Ok(config)
}

/// Reads and validates `<dir>/app.toml`; a missing file or directory is
/// the defaults, a file that cannot be read is `<path>: <io error>`.
pub fn load_app_toml(dir: &Path) -> Result<AppConfig, ConfigError> {
    let path = app_toml_path(dir);
    match std::fs::read_to_string(&path) {
        Ok(text) => parse_app_toml(&path, &text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(AppConfig::default()),
        Err(e) => Err(ConfigError::at(&path, e)),
    }
}

/// Parses the text of `keymap.toml` (`path` only names it in errors) and
/// builds the keymap from the defaults and its entries (spec 0008
/// "`keymap.toml`").
pub fn parse_keymap_toml(path: &Path, text: &str) -> Result<Keymap, ConfigError> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let file: KeymapFile = toml::from_str(text).map_err(|e| syntax_error(path, text, &e))?;
    keymap::build(&file).map_err(|e| ConfigError::at(path, e))
}

/// Reads and validates `<dir>/keymap.toml`; a missing file or directory is
/// the default keymap, a file that cannot be read is `<path>: <io error>`.
pub fn load_keymap_toml(dir: &Path) -> Result<Keymap, ConfigError> {
    let path = keymap_toml_path(dir);
    match std::fs::read_to_string(&path) {
        Ok(text) => parse_keymap_toml(&path, &text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Keymap::default()),
        Err(e) => Err(ConfigError::at(&path, e)),
    }
}

/// Parses the text of `theme.toml` (`path` only names it in errors) and
/// builds the theme list (spec 0017 "`theme.toml`").
pub fn parse_theme_toml(path: &Path, text: &str) -> Result<Themes, ConfigError> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let file: ThemeFile = toml::from_str(text).map_err(|e| syntax_error(path, text, &e))?;
    theme::build(&file).map_err(|e| ConfigError::at(path, e))
}

/// Reads and validates `<dir>/theme.toml`; a missing file or directory is
/// `default` alone, a file that cannot be read is `<path>: <io error>`.
pub fn load_theme_toml(dir: &Path) -> Result<Themes, ConfigError> {
    let path = theme_toml_path(dir);
    match std::fs::read_to_string(&path) {
        Ok(text) => parse_theme_toml(&path, &text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Themes::default()),
        Err(e) => Err(ConfigError::at(&path, e)),
    }
}

/// The theme in use (spec 0017 "Choosing the theme"): `--theme`, else
/// `TIDAL_PLAYER_THEME` (empty counts as unset), else `theme` in `app.toml`
/// (`app_toml` names it in errors), else `default`; a name no theme has is
/// an error naming where it came from and the themes there are.
pub fn select_theme(
    themes: &Themes,
    app_toml: &Path,
    file: Option<&str>,
    var: Option<&str>,
    flag: Option<&str>,
) -> Result<usize, ConfigError> {
    let (source, name) = match (flag, var.filter(|v| !v.is_empty()), file) {
        (Some(flag), _, _) => ("--theme".to_owned(), flag),
        (None, Some(var), _) => (THEME_VAR.to_owned(), var),
        (None, None, Some(file)) => (app_toml.display().to_string(), file),
        (None, None, None) => return Ok(0),
    };
    themes
        .find(name)
        .ok_or_else(|| ConfigError(format!("{source}: {}", themes.unknown(name))))
}

/// `<path>: line L, column C: <parser message>`.
fn syntax_error(path: &Path, text: &str, e: &toml::de::Error) -> ConfigError {
    let (line, column) = e
        .span()
        .map_or((1, 1), |span| line_column(text, span.start));
    ConfigError::at(
        path,
        format!("line {line}, column {column}: {}", e.message()),
    )
}

fn unknown(path: &Path, key: &str) -> ConfigError {
    ConfigError::at(path, format!("unknown setting \"{key}\""))
}

/// How a value reads in "got ...": strings quoted, scalars as written.
fn describe(value: &toml::Value) -> String {
    match value {
        toml::Value::String(s) => format!("{s:?}"),
        toml::Value::Integer(n) => n.to_string(),
        toml::Value::Float(f) => f.to_string(),
        toml::Value::Boolean(b) => b.to_string(),
        toml::Value::Datetime(d) => d.to_string(),
        toml::Value::Array(_) => "an array".into(),
        toml::Value::Table(_) => "a table".into(),
    }
}

fn int_in(
    path: &Path,
    key: &str,
    value: &toml::Value,
    min: u64,
    max: u64,
) -> Result<u64, ConfigError> {
    value
        .as_integer()
        .and_then(|n| u64::try_from(n).ok())
        .filter(|n| (min..=max).contains(n))
        .ok_or_else(|| {
            ConfigError::at(
                path,
                format!(
                    "invalid {key}: expected an integer from {min} to {max}, got {}",
                    describe(value)
                ),
            )
        })
}

/// `player_event_hook_command` (spec 0025 AC1).
fn hook_command(path: &Path, key: &str, value: &toml::Value) -> Result<HookCommand, ConfigError> {
    let Some(table) = value.as_table() else {
        return Err(ConfigError::at(
            path,
            format!(
                "invalid {key}: expected a table with command and args, got {}",
                describe(value)
            ),
        ));
    };
    if let Some(field) = table
        .keys()
        .find(|k| !matches!(k.as_str(), "command" | "args"))
    {
        return Err(unknown(path, &format!("{key}.{field}")));
    }
    let command = table.get("command");
    let Some(name) = command
        .and_then(toml::Value::as_str)
        .filter(|s| !s.is_empty())
    else {
        let got = command.map_or_else(|| "nothing".to_owned(), describe);
        return Err(ConfigError::at(
            path,
            format!("invalid {key}.command: expected a non-empty string, got {got}"),
        ));
    };
    let args = match table.get("args") {
        Some(args) => words(path, &format!("{key}.args"), args)?,
        None => Vec::new(),
    };
    Ok(HookCommand {
        command: name.to_owned(),
        args,
    })
}

fn words(path: &Path, key: &str, value: &toml::Value) -> Result<Vec<String>, ConfigError> {
    let wrong = |got: String| {
        ConfigError::at(
            path,
            format!("invalid {key}: expected an array of strings, got {got}"),
        )
    };
    let items = value.as_array().ok_or_else(|| wrong(describe(value)))?;
    items
        .iter()
        .map(|item| {
            item.as_str()
                .map(str::to_owned)
                .ok_or_else(|| wrong(format!("an array containing {}", describe(item))))
        })
        .collect()
}

/// `[layout] library = { playlist_percent, album_percent }`.
fn layout(path: &Path, value: &toml::Value) -> Result<LibraryLayout, ConfigError> {
    let not_table = |key: &str, value: &toml::Value| {
        ConfigError::at(
            path,
            format!("invalid {key}: expected a table, got {}", describe(value)),
        )
    };
    let table = value.as_table().ok_or_else(|| not_table("layout", value))?;
    let mut layout = LibraryLayout::default();
    for (key, value) in table {
        if key != "library" {
            return Err(unknown(path, &format!("layout.{key}")));
        }
        let library = value
            .as_table()
            .ok_or_else(|| not_table("layout.library", value))?;
        for (key, value) in library {
            let name = format!("layout.library.{key}");
            match key.as_str() {
                "playlist_percent" => {
                    layout.playlist_percent = int_in(path, &name, value, 1, 98)? as u16;
                }
                "album_percent" => layout.album_percent = int_in(path, &name, value, 1, 98)? as u16,
                _ => return Err(unknown(path, &name)),
            }
        }
        let sum = layout.playlist_percent + layout.album_percent;
        if sum > 99 {
            return Err(ConfigError::at(
                path,
                format!(
                    "invalid layout.library: playlist_percent + album_percent must be at most 99, got {sum}"
                ),
            ));
        }
    }
    Ok(layout)
}

/// 1-based line and column (in characters) of a byte offset.
fn line_column(text: &str, offset: usize) -> (usize, usize) {
    let mut end = offset.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let before = &text[..end];
    let line = before.matches('\n').count() + 1;
    let start = before.rfind('\n').map_or(0, |i| i + 1);
    (line, before[start..].chars().count() + 1)
}

#[cfg(test)]
mod tests;
