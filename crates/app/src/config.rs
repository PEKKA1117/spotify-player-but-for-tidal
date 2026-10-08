//! The config directory and `app.toml` (spec 0008 "Where the files are",
//! "`app.toml`"): pure functions over strings and a fake environment, so
//! the table tests need no files. Nothing is ever written.

use std::path::{Path, PathBuf};
use std::time::Duration;

use tidal_player_core::AudioQuality;

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
/// *Playlists* and *Albums*; *Artists* takes the rest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LibraryLayout {
    pub playlist_percent: u16,
    pub album_percent: u16,
}

impl Default for LibraryLayout {
    fn default() -> Self {
        Self {
            playlist_percent: 40,
            album_percent: 40,
        }
    }
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
    pub layout: LibraryLayout,
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
    let _ = (flag, dir_var, xdg_config_home);
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

/// Parses the text of `app.toml` (`path` only names it in errors).
pub fn parse_app_toml(path: &Path, text: &str) -> Result<AppConfig, ConfigError> {
    let _ = (path, text);
    Ok(AppConfig::default())
}

/// Reads and validates `<dir>/app.toml`; a missing file or directory is
/// the defaults.
pub fn load_app_toml(dir: &Path) -> Result<AppConfig, ConfigError> {
    let _ = dir;
    Ok(AppConfig::default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::play::{
        AUTOPLAY_VAR, DEVICE_VAR, HIDE_VERSIONS_VAR, PAGE_SIZE_VAR, PREVIOUS_RESTART_VAR,
        QUALITY_VAR, RELEASE_PAUSED_VAR, SEARCH_PAGE_SIZE_VAR, SEEK_STEP_VAR, VOLUME_STEP_VAR,
        resolve_play_config_with, resolve_player_config_with, resolve_settings_with,
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
