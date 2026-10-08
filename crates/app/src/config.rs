//! The config directory and `app.toml` (spec 0008 "Where the files are",
//! "`app.toml`"): pure functions over strings and a fake environment, so
//! the table tests need no files. Nothing is ever written.

use std::path::{Path, PathBuf};
use std::time::Duration;

use tidal_player_core::AudioQuality;
use tidal_player_core::ui::Keymap;
use tidal_player_core::ui::keymap::{self, KeymapFile};

/// Overrides the config directory (below `--config-folder`).
pub const CONFIG_DIR_VAR: &str = "TIDAL_PLAYER_CONFIG_DIR";
/// The settings file's name in the config directory.
pub const APP_TOML: &str = "app.toml";
/// The keymap file's name in the config directory (read by the TUI).
pub const KEYMAP_TOML: &str = "keymap.toml";

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
    pub layout: LibraryLayout,
    /// Spec 0009 "Turning it off".
    pub remember_playback: Option<bool>,
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
            "layout" => config.layout = layout(path, value)?,
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
mod tests {
    use super::*;
    use crate::play::{
        AUTOPLAY_VAR, DEVICE_VAR, HIDE_VERSIONS_VAR, PAGE_SIZE_VAR, PREVIOUS_RESTART_VAR,
        QUALITY_VAR, RELEASE_PAUSED_VAR, REMEMBER_PLAYBACK_VAR, SEARCH_PAGE_SIZE_VAR,
        SEEK_STEP_VAR, VOLUME_STEP_VAR, resolve_play_config_with, resolve_player_config_with,
        resolve_settings_with,
    };

    const PATH: &str = "/c/app.toml";

    /// AC9: the config directory: flag, variable, XDG, home; an empty
    /// variable counts as unset; the flag beats the variable.
    #[test]
    fn ac9_config_dir() {
        type Row = (
            &'static str,
            Option<&'static str>,
            Option<&'static str>,
            Option<&'static str>,
            Option<&'static str>,
            &'static str,
        );
        let rows: &[Row] = &[
            // name, flag, variable, XDG_CONFIG_HOME, home, want
            ("flag", Some("/f"), None, None, Some("/home/u"), "/f"),
            (
                "variable",
                None,
                Some("/v"),
                Some("/x"),
                Some("/home/u"),
                "/v",
            ),
            (
                "flag beats the variable",
                Some("/f"),
                Some("/v"),
                Some("/x"),
                Some("/home/u"),
                "/f",
            ),
            (
                "empty variable ignored",
                None,
                Some(""),
                Some("/x"),
                Some("/home/u"),
                "/x/tidal-player",
            ),
            (
                "XDG_CONFIG_HOME",
                None,
                None,
                Some("/x"),
                Some("/home/u"),
                "/x/tidal-player",
            ),
            (
                "empty XDG_CONFIG_HOME ignored",
                None,
                None,
                Some(""),
                Some("/home/u"),
                "/home/u/.config/tidal-player",
            ),
            (
                "home only",
                None,
                None,
                None,
                Some("/home/u"),
                "/home/u/.config/tidal-player",
            ),
        ];
        for (name, flag, var, xdg, home, want) in rows {
            let got = config_dir(flag.map(Path::new), *var, *xdg, home.map(Path::new));
            assert_eq!(got, PathBuf::from(want), "{name}");
        }
    }

    fn bounds(key: &str, min: u64, max: u64) -> Vec<(String, String, Result<(), String>)> {
        let ok = |n: u64| (format!("{key} = {n}"), Ok(()));
        let bad = |text: String, got: String| {
            let msg = format!(
                "{PATH}: invalid {key}: expected an integer from {min} to {max}, got {got}"
            );
            (text, Err(msg))
        };
        let mut rows = vec![ok(min), ok(max)];
        if min > 0 {
            rows.push(bad(format!("{key} = {}", min - 1), (min - 1).to_string()));
        } else {
            rows.push(bad(format!("{key} = -1"), "-1".into()));
        }
        rows.push(bad(format!("{key} = {}", max + 1), (max + 1).to_string()));
        rows.push(bad(format!("{key} = \"x\""), "\"x\"".into()));
        rows.push(bad(format!("{key} = 1.5"), "1.5".into()));
        rows.push(bad(format!("{key} = true"), "true".into()));
        rows.into_iter()
            .map(|(text, want)| (format!("{key} bounds"), text, want))
            .collect()
    }

    /// AC10: `app.toml`: every key at its bounds and one past, wrong
    /// types, the layout, unknown keys, syntax errors, a BOM and CRLF;
    /// each error message as in the spec.
    #[test]
    fn ac10_app_toml() {
        let path = Path::new(PATH);
        // Range-checked integers: the text must parse, or fail with the
        // message built by `bounds`.
        let mut rows: Vec<(String, String, Result<(), String>)> = Vec::new();
        for (key, min, max) in [
            ("volume_step", 1, 25),
            ("seek_duration_secs", 1, 600),
            ("previous_restart_secs", 0, 60),
            ("page_size", 1, 10_000),
            ("search_page_size", 1, 1000),
        ] {
            rows.extend(bounds(key, min, max));
        }
        for (name, text, want) in &rows {
            let got = parse_app_toml(path, text).map(|_| ()).map_err(|e| e.0);
            assert_eq!(&got, want, "{name}: {text}");
        }

        // Values: what each accepted text sets.
        type Check = fn(&AppConfig) -> String;
        let values: &[(&str, &str, Check, &str)] = &[
            ("empty file", "", |c| format!("{c:?}"), "{default}"),
            (
                "comments only",
                "# nothing\n",
                |c| format!("{c:?}"),
                "{default}",
            ),
            (
                "volume",
                "volume_step = 7",
                |c| format!("{:?}", c.volume_step),
                "Some(7)",
            ),
            (
                "seek",
                "seek_duration_secs = 600",
                |c| format!("{:?}", c.seek_duration_secs),
                "Some(600)",
            ),
            (
                "previous restart at 0",
                "previous_restart_secs = 0",
                |c| format!("{:?}", c.previous_restart_secs),
                "Some(0)",
            ),
            (
                "autoplay true",
                "autoplay = true",
                |c| format!("{:?}", c.autoplay),
                "Some(true)",
            ),
            (
                "autoplay false",
                "autoplay = false",
                |c| format!("{:?}", c.autoplay),
                "Some(false)",
            ),
            (
                "remember_playback true",
                "remember_playback = true",
                |c| format!("{:?}", c.remember_playback),
                "Some(true)",
            ),
            (
                "remember_playback false",
                "remember_playback = false",
                |c| format!("{:?}", c.remember_playback),
                "Some(false)",
            ),
            (
                "quality hi-res",
                "quality = \"hi-res\"",
                |c| format!("{:?}", c.quality),
                "Some(HiResLossless)",
            ),
            (
                "quality lossless",
                "quality = \"lossless\"",
                |c| format!("{:?}", c.quality),
                "Some(Lossless)",
            ),
            (
                "quality high",
                "quality = \"high\"",
                |c| format!("{:?}", c.quality),
                "Some(High)",
            ),
            (
                "device",
                "output_device = \"hw:1,0\"",
                |c| format!("{:?}", c.output_device),
                "Some(\"hw:1,0\")",
            ),
            (
                "release at once",
                "release_paused_secs = 0",
                |c| format!("{:?}", c.release_paused),
                "Some(Some(0ns))",
            ),
            (
                "release at an hour",
                "release_paused_secs = 3600",
                |c| format!("{:?}", c.release_paused),
                "Some(Some(3600s))",
            ),
            (
                "release never",
                "release_paused_secs = \"never\"",
                |c| format!("{:?}", c.release_paused),
                "Some(None)",
            ),
            (
                "hide nothing",
                "hide_versions = []",
                |c| format!("{:?}", c.hide_versions),
                "Some([])",
            ),
            (
                "hide words",
                "hide_versions = [\"live\", \"Karaoke\"]",
                |c| format!("{:?}", c.hide_versions),
                "Some([\"live\", \"Karaoke\"])",
            ),
            (
                "layout default",
                "[layout]\n",
                |c| format!("{:?}", c.layout),
                "LibraryLayout { playlist_percent: 40, album_percent: 40 }",
            ),
            (
                "layout lowest",
                "[layout]\nlibrary = { playlist_percent = 1, album_percent = 1 }",
                |c| format!("{:?}", c.layout),
                "LibraryLayout { playlist_percent: 1, album_percent: 1 }",
            ),
            (
                "layout highest",
                "[layout]\nlibrary = { playlist_percent = 98, album_percent = 1 }",
                |c| format!("{:?}", c.layout),
                "LibraryLayout { playlist_percent: 98, album_percent: 1 }",
            ),
            (
                "layout sum of 99",
                "[layout]\nlibrary = { playlist_percent = 50, album_percent = 49 }",
                |c| format!("{:?}", c.layout),
                "LibraryLayout { playlist_percent: 50, album_percent: 49 }",
            ),
            (
                "layout one part",
                "[layout]\nlibrary = { playlist_percent = 30 }",
                |c| format!("{:?}", c.layout),
                "LibraryLayout { playlist_percent: 30, album_percent: 40 }",
            ),
            (
                "UTF-8 BOM",
                "\u{feff}volume_step = 7\n",
                |c| format!("{:?}", c.volume_step),
                "Some(7)",
            ),
            (
                "CRLF line ends",
                "volume_step = 7\r\nautoplay = true\r\n[layout]\r\nlibrary = { playlist_percent = 30, album_percent = 30 }\r\n",
                |c| {
                    format!(
                        "{:?} {:?} {}",
                        c.volume_step, c.autoplay, c.layout.playlist_percent
                    )
                },
                "Some(7) Some(true) 30",
            ),
        ];
        for (name, text, check, want) in values {
            let got = parse_app_toml(path, text).unwrap_or_else(|e| panic!("{name}: {e}"));
            let want = if *want == "{default}" {
                format!("{:?}", AppConfig::default())
            } else {
                (*want).to_owned()
            };
            assert_eq!(check(&got), want, "{name}");
        }

        // Errors: the whole message.
        let errors: &[(&str, &str, &str)] = &[
            (
                "quality unknown",
                "quality = \"ultra\"",
                "/c/app.toml: invalid quality: unknown quality \"ultra\": expected hi-res, lossless or high",
            ),
            (
                "quality low",
                "quality = \"low\"",
                "/c/app.toml: invalid quality: quality \"low\" is not available: Tidal's LOW streams are HE-AAC, which is not supported (use high, lossless or hi-res)",
            ),
            (
                "quality type",
                "quality = 5",
                "/c/app.toml: invalid quality: expected hi-res, lossless or high, got 5",
            ),
            (
                "device type",
                "output_device = 5",
                "/c/app.toml: invalid output_device: expected a device name, got 5",
            ),
            (
                "device empty",
                "output_device = \"\"",
                "/c/app.toml: invalid output_device: expected a device name, got \"\"",
            ),
            (
                "autoplay type",
                "autoplay = \"yes\"",
                "/c/app.toml: invalid autoplay: expected true or false, got \"yes\"",
            ),
            (
                "remember_playback type",
                "remember_playback = \"off\"",
                "/c/app.toml: invalid remember_playback: expected true or false, got \"off\"",
            ),
            (
                "release over",
                "release_paused_secs = 3601",
                "/c/app.toml: invalid release_paused_secs: expected an integer from 0 to 3600 or never, got 3601",
            ),
            (
                "release text",
                "release_paused_secs = \"soon\"",
                "/c/app.toml: invalid release_paused_secs: expected an integer from 0 to 3600 or never, got \"soon\"",
            ),
            (
                "release negative",
                "release_paused_secs = -1",
                "/c/app.toml: invalid release_paused_secs: expected an integer from 0 to 3600 or never, got -1",
            ),
            (
                "hide type",
                "hide_versions = \"live\"",
                "/c/app.toml: invalid hide_versions: expected an array of strings, got \"live\"",
            ),
            (
                "hide element type",
                "hide_versions = [\"live\", 1]",
                "/c/app.toml: invalid hide_versions: expected an array of strings, got an array containing 1",
            ),
            (
                "layout over 98",
                "[layout]\nlibrary = { playlist_percent = 99, album_percent = 1 }",
                "/c/app.toml: invalid layout.library.playlist_percent: expected an integer from 1 to 98, got 99",
            ),
            (
                "layout zero",
                "[layout]\nlibrary = { playlist_percent = 40, album_percent = 0 }",
                "/c/app.toml: invalid layout.library.album_percent: expected an integer from 1 to 98, got 0",
            ),
            (
                "layout sum of 100",
                "[layout]\nlibrary = { playlist_percent = 50, album_percent = 50 }",
                "/c/app.toml: invalid layout.library: playlist_percent + album_percent must be at most 99, got 100",
            ),
            (
                "layout type",
                "layout = 5",
                "/c/app.toml: invalid layout: expected a table, got 5",
            ),
            (
                "unknown top-level key",
                "volum_step = 3",
                "/c/app.toml: unknown setting \"volum_step\"",
            ),
            (
                "unknown key under layout",
                "[layout]\nlibary = { playlist_percent = 30, album_percent = 30 }",
                "/c/app.toml: unknown setting \"layout.libary\"",
            ),
            (
                "unknown key under layout.library",
                "[layout]\nlibrary = { playlist_pct = 30 }",
                "/c/app.toml: unknown setting \"layout.library.playlist_pct\"",
            ),
            (
                "syntax: no value",
                "autoplay = true\nvolume_step = \n",
                "/c/app.toml: line 2, column 15:",
            ),
            (
                "syntax: after a BOM and CRLF",
                "\u{feff}autoplay = true\r\nvolume_step = = 3\r\n",
                "/c/app.toml: line 2, column 15:",
            ),
            (
                "syntax: unclosed string",
                "output_device = \"hw\n",
                "/c/app.toml: line 1, column 20:",
            ),
        ];
        for (name, text, want) in errors {
            let err = parse_app_toml(path, text).expect_err(name).0;
            if want.contains(": line ") {
                // The parser's own words follow the position.
                assert!(err.starts_with(want), "{name}: {err}");
                assert!(err.len() > want.len() + 1, "{name}: no message: {err}");
            } else {
                assert_eq!(&err, want, "{name}");
            }
        }
    }

    /// AC10 (reading): a missing directory or file is the defaults, a
    /// directory or an unreadable file is `<path>: <io error>`.
    #[test]
    fn ac10_load_app_toml() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(
            load_app_toml(&tmp.path().join("missing")),
            Ok(AppConfig::default())
        );
        assert_eq!(load_app_toml(tmp.path()), Ok(AppConfig::default()));
        let path = tmp.path().join(APP_TOML);
        std::fs::write(&path, "volume_step = 9").unwrap();
        assert_eq!(load_app_toml(tmp.path()).unwrap().volume_step, Some(9));
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        let err = load_app_toml(tmp.path()).unwrap_err().0;
        assert!(
            err.starts_with(&format!("{}: ", path.display()))
                && err.len() > path.as_os_str().len() + 2,
            "{err}"
        );
        std::fs::remove_dir(&path).unwrap();
        std::fs::write(&path, [0xff, 0xfe, 0x00]).unwrap();
        let err = load_app_toml(tmp.path()).unwrap_err().0;
        assert!(err.starts_with(&format!("{}: ", path.display())), "{err}");
    }

    struct Row {
        name: &'static str,
        file: &'static str,
        env: &'static [(&'static str, &'static str)],
        quality_flag: Option<&'static str>,
        device_flag: Option<&'static str>,
        autoplay_flag: bool,
        /// What `observe` prints, or a part of the error.
        want: Result<&'static str, &'static str>,
    }

    impl Row {
        const fn new(
            name: &'static str,
            file: &'static str,
            env: &'static [(&'static str, &'static str)],
            want: Result<&'static str, &'static str>,
        ) -> Self {
            Self {
                name,
                file,
                env,
                quality_flag: None,
                device_flag: None,
                autoplay_flag: false,
                want,
            }
        }
        const fn flags(
            mut self,
            quality: Option<&'static str>,
            device: Option<&'static str>,
            autoplay: bool,
        ) -> Self {
            self.quality_flag = quality;
            self.device_flag = device;
            self.autoplay_flag = autoplay;
            self
        }
    }

    /// AC11: the winning value per setting and source: default, file,
    /// environment over file, flag over both; an empty variable falls
    /// through to the file; an invalid variable is an error even when the
    /// file is valid.
    #[test]
    fn ac11_precedence() {
        type Observe = fn(&crate::play::Settings, &crate::play::PlayerSettings) -> String;
        let settings: &[(&str, Observe, Vec<Row>)] = &[
            (
                "quality",
                |s, _| format!("{:?}", s.quality),
                vec![
                    Row::new("default", "", &[], Ok("HiResLossless")),
                    Row::new("file", "quality = \"high\"", &[], Ok("High")),
                    Row::new(
                        "env over file",
                        "quality = \"high\"",
                        &[(QUALITY_VAR, "lossless")],
                        Ok("Lossless"),
                    ),
                    Row::new(
                        "empty env falls through",
                        "quality = \"high\"",
                        &[(QUALITY_VAR, "")],
                        Ok("High"),
                    ),
                    Row::new(
                        "invalid env, valid file",
                        "quality = \"high\"",
                        &[(QUALITY_VAR, "ultra")],
                        Err(QUALITY_VAR),
                    ),
                    Row::new(
                        "flag over both",
                        "quality = \"high\"",
                        &[(QUALITY_VAR, "lossless")],
                        Ok("HiResLossless"),
                    )
                    .flags(Some("hi-res"), None, false),
                ],
            ),
            (
                "device",
                |s, _| s.device.clone(),
                vec![
                    Row::new("default", "", &[], Ok("default")),
                    Row::new("file", "output_device = \"hw:1,0\"", &[], Ok("hw:1,0")),
                    Row::new(
                        "env over file",
                        "output_device = \"hw:1,0\"",
                        &[(DEVICE_VAR, "hw:2,0")],
                        Ok("hw:2,0"),
                    ),
                    Row::new(
                        "empty env falls through",
                        "output_device = \"hw:1,0\"",
                        &[(DEVICE_VAR, "")],
                        Ok("hw:1,0"),
                    ),
                    Row::new(
                        "flag over both",
                        "output_device = \"hw:1,0\"",
                        &[(DEVICE_VAR, "hw:2,0")],
                        Ok("hw:3,0"),
                    )
                    .flags(None, Some("hw:3,0"), false),
                ],
            ),
            (
                "volume_step",
                |_, p| p.steps.volume.to_string(),
                ranged("volume_step", VOLUME_STEP_VAR, ("5", "10", "15")),
            ),
            (
                "seek_duration_secs",
                |_, p| p.steps.seek.as_secs().to_string(),
                ranged("seek_duration_secs", SEEK_STEP_VAR, ("5", "10", "15")),
            ),
            (
                "previous_restart_secs",
                |_, p| p.player.previous_restart.as_secs().to_string(),
                ranged(
                    "previous_restart_secs",
                    PREVIOUS_RESTART_VAR,
                    ("3", "10", "15"),
                ),
            ),
            (
                "page_size",
                |_, p| p.library.page_size.to_string(),
                ranged("page_size", PAGE_SIZE_VAR, ("100", "10", "15")),
            ),
            (
                "search_page_size",
                |_, p| p.library.search_page_size.to_string(),
                ranged("search_page_size", SEARCH_PAGE_SIZE_VAR, ("20", "10", "15")),
            ),
            (
                "autoplay",
                |_, p| p.player.autoplay.to_string(),
                vec![
                    Row::new("default", "", &[], Ok("false")),
                    Row::new("file", "autoplay = true", &[], Ok("true")),
                    Row::new(
                        "env over file",
                        "autoplay = true",
                        &[(AUTOPLAY_VAR, "off")],
                        Ok("false"),
                    ),
                    Row::new(
                        "empty env falls through",
                        "autoplay = true",
                        &[(AUTOPLAY_VAR, "")],
                        Ok("true"),
                    ),
                    Row::new(
                        "invalid env, valid file",
                        "autoplay = true",
                        &[(AUTOPLAY_VAR, "maybe")],
                        Err(AUTOPLAY_VAR),
                    ),
                    Row::new(
                        "flag over both",
                        "autoplay = false",
                        &[(AUTOPLAY_VAR, "off")],
                        Ok("true"),
                    )
                    .flags(None, None, true),
                ],
            ),
            (
                "remember_playback",
                |_, p| p.remember_playback.to_string(),
                vec![
                    Row::new("default", "", &[], Ok("true")),
                    Row::new("file true", "remember_playback = true", &[], Ok("true")),
                    Row::new("file false", "remember_playback = false", &[], Ok("false")),
                    Row::new(
                        "env over file",
                        "remember_playback = true",
                        &[(REMEMBER_PLAYBACK_VAR, "off")],
                        Ok("false"),
                    ),
                    Row::new(
                        "env on over file",
                        "remember_playback = false",
                        &[(REMEMBER_PLAYBACK_VAR, "ON")],
                        Ok("true"),
                    ),
                    Row::new(
                        "empty env falls through",
                        "remember_playback = false",
                        &[(REMEMBER_PLAYBACK_VAR, "")],
                        Ok("false"),
                    ),
                    Row::new(
                        "invalid env, valid file",
                        "remember_playback = true",
                        &[(REMEMBER_PLAYBACK_VAR, "maybe")],
                        Err(REMEMBER_PLAYBACK_VAR),
                    ),
                ],
            ),
            (
                "release_paused_secs",
                |_, p| format!("{:?}", p.release_paused.map(|d| d.as_secs())),
                vec![
                    Row::new("default", "", &[], Ok("Some(10)")),
                    Row::new("file", "release_paused_secs = 30", &[], Ok("Some(30)")),
                    Row::new(
                        "file never",
                        "release_paused_secs = \"never\"",
                        &[],
                        Ok("None"),
                    ),
                    Row::new(
                        "env over file",
                        "release_paused_secs = \"never\"",
                        &[(RELEASE_PAUSED_VAR, "20")],
                        Ok("Some(20)"),
                    ),
                    Row::new(
                        "env never over file",
                        "release_paused_secs = 30",
                        &[(RELEASE_PAUSED_VAR, "never")],
                        Ok("None"),
                    ),
                    Row::new(
                        "empty env falls through",
                        "release_paused_secs = 30",
                        &[(RELEASE_PAUSED_VAR, "")],
                        Ok("Some(30)"),
                    ),
                    Row::new(
                        "invalid env, valid file",
                        "release_paused_secs = 30",
                        &[(RELEASE_PAUSED_VAR, "9999")],
                        Err(RELEASE_PAUSED_VAR),
                    ),
                ],
            ),
            (
                "hide_versions",
                |_, p| format!("{:?}", p.library.hidden_words),
                vec![
                    Row::new("file", "hide_versions = [\"live\"]", &[], Ok("[\"live\"]")),
                    Row::new("file hides nothing", "hide_versions = []", &[], Ok("[]")),
                    Row::new(
                        "env over file",
                        "hide_versions = [\"live\"]",
                        &[(HIDE_VERSIONS_VAR, "demo, remix")],
                        Ok("[\"demo\", \"remix\"]"),
                    ),
                    Row::new(
                        "empty env hides nothing (0006)",
                        "hide_versions = [\"live\"]",
                        &[(HIDE_VERSIONS_VAR, "")],
                        Ok("[]"),
                    ),
                ],
            ),
            (
                "layout",
                |_, p| format!("{}/{}", p.layout.playlist_percent, p.layout.album_percent),
                vec![
                    Row::new("default", "", &[], Ok("40/40")),
                    Row::new(
                        "file",
                        "[layout]\nlibrary = { playlist_percent = 30, album_percent = 50 }",
                        &[],
                        Ok("30/50"),
                    ),
                ],
            ),
        ];
        for (setting, observe, rows) in settings {
            for row in rows {
                let file = parse_app_toml(Path::new(PATH), row.file)
                    .unwrap_or_else(|e| panic!("{setting}: {}: {e}", row.name));
                let env = |key: &str| {
                    row.env
                        .iter()
                        .find(|(k, _)| *k == key)
                        .map(|(_, v)| (*v).to_owned())
                };
                let got = resolve_settings_with(&file, row.quality_flag, row.device_flag, env)
                    .map_err(|e| e.to_string())
                    .and_then(|s| {
                        resolve_play_config_with(&file, row.autoplay_flag, env)
                            .map(|p| observe(&s, &p))
                            .map_err(|e| e.to_string())
                    });
                match row.want {
                    Ok(want) => assert_eq!(got.as_deref(), Ok(want), "{setting}: {}", row.name),
                    Err(var) => {
                        let err = got.expect_err(row.name);
                        assert!(err.contains(var), "{setting}: {}: {err}", row.name);
                    }
                }
            }
        }
        // The player config without a flag reads the same file.
        let file = parse_app_toml(Path::new(PATH), "volume_step = 9").unwrap();
        assert_eq!(
            resolve_player_config_with(&file, |_| None)
                .unwrap()
                .steps
                .volume,
            9
        );
    }

    /// The rows every integer setting shares: `(default, file, env)`; the
    /// file says `10`, the environment `15`.
    fn ranged(key: &'static str, var: &'static str, vals: (&str, &str, &str)) -> Vec<Row> {
        let leak = |s: String| -> &'static str { Box::leak(s.into_boxed_str()) };
        let file = leak(format!("{key} = {}", vals.1));
        let env_file: &'static [(&str, &str)] = Box::leak(Box::new([(var, leak(vals.2.into()))]));
        let empty_env: &'static [(&str, &str)] = Box::leak(Box::new([(var, "")]));
        let bad_env: &'static [(&str, &str)] = Box::leak(Box::new([(var, "x")]));
        let leak_ok = |s: &str| -> Result<&'static str, &'static str> { Ok(leak(s.into())) };
        vec![
            Row::new("default", "", &[], leak_ok(vals.0)),
            Row::new("file", file, &[], leak_ok(vals.1)),
            Row::new("env over file", file, env_file, leak_ok(vals.2)),
            Row::new("empty env falls through", file, empty_env, leak_ok(vals.1)),
            Row::new("invalid env, valid file", file, bad_env, Err(var)),
        ]
    }
}
