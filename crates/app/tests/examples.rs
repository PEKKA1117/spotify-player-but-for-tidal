//! Spec 0008 AC18: `examples/app.toml` and `examples/keymap.toml` spell out
//! every default, so copying them changes nothing. Spec 0017 AC14:
//! `examples/theme.toml` loads, its `spotify_player` theme is
//! spotify-player's default look.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use tidal_player::config::{AppConfig, parse_app_toml, parse_keymap_toml, parse_theme_toml};
use tidal_player::play::{resolve_player_config_with, resolve_settings_with};
use tidal_player_core::ui::Keymap;
use tidal_player_core::ui::keymap::{Binding, defaults};

fn example(name: &str) -> (PathBuf, String) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples")
        .join(name);
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    (path, text)
}

/// AC18: every `app.toml` key is set, and to its default.
#[test]
fn ac18_example_app_toml_is_the_defaults() {
    let (path, text) = example("app.toml");
    let file = parse_app_toml(&path, &text).expect("examples/app.toml parses");
    let missing: Vec<&str> = [
        ("quality", file.quality.is_none()),
        ("output_device", file.output_device.is_none()),
        ("volume_step", file.volume_step.is_none()),
        ("seek_duration_secs", file.seek_duration_secs.is_none()),
        (
            "previous_restart_secs",
            file.previous_restart_secs.is_none(),
        ),
        ("autoplay", file.autoplay.is_none()),
        ("release_paused_secs", file.release_paused.is_none()),
        ("remember_playback", file.remember_playback.is_none()),
        ("page_size", file.page_size.is_none()),
        ("search_page_size", file.search_page_size.is_none()),
        ("hide_versions", file.hide_versions.is_none()),
        // Spec 0010.
        ("mpris", file.mpris.is_none()),
        ("max_cover_arts", file.max_cover_arts.is_none()),
        // Spec 0013.
        ("key_hints", file.key_hints.is_none()),
        ("key_hints_delay_ms", file.key_hints_delay_ms.is_none()),
        // Spec 0016 AC9.
        ("cover_img", file.cover_img.is_none()),
        ("cover_img_rows", file.cover_img_rows.is_none()),
        ("cover_img_protocol", file.cover_img_protocol.is_none()),
    ]
    .into_iter()
    .filter_map(|(key, missing)| missing.then_some(key))
    .collect();
    assert!(missing.is_empty(), "keys not set: {missing:?}");
    assert!(
        text.contains("[layout]")
            && text.contains("playlist_percent")
            && text.contains("album_percent"),
        "[layout] library not set"
    );

    let no_env = |_: &str| None;
    let defaults = AppConfig::default();
    assert_eq!(
        resolve_player_config_with(&file, no_env).unwrap(),
        resolve_player_config_with(&defaults, no_env).unwrap(),
        "player settings"
    );
    assert_eq!(
        resolve_settings_with(&file, None, None, no_env).unwrap(),
        resolve_settings_with(&defaults, None, None, no_env).unwrap(),
        "quality and device"
    );
    assert_eq!(file.layout, defaults.layout, "library layout");
}

/// AC18 (and spec 0011 AC9): one `[[keymaps]]` entry per default command
/// binding and one `[[actions]]` entry per default action binding, nothing
/// else, and the file builds to the default keymap.
#[test]
fn ac18_example_keymap_is_the_defaults() {
    let (path, text) = example("keymap.toml");
    let value: toml::Table = text.parse().expect("examples/keymap.toml is TOML");
    let entries_of = |section: &str, name: &str| -> Vec<(String, String, String)> {
        value
            .get(section)
            .and_then(toml::Value::as_array)
            .map(|entries| {
                entries
                    .iter()
                    .map(|e| {
                        let field = |k: &str| e.get(k).and_then(toml::Value::as_str);
                        (
                            field(name).unwrap().to_owned(),
                            field("key_sequence").unwrap().to_owned(),
                            field("target").unwrap_or("SelectedItem").to_owned(),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    for (section, name) in [("keymaps", "command"), ("actions", "action")] {
        let entries = entries_of(section, name);
        let listed: BTreeSet<(String, String, String)> = entries.iter().cloned().collect();
        assert_eq!(listed.len(), entries.len(), "{section}: listed twice");
        let expected: BTreeSet<(String, String, String)> = defaults()
            .into_iter()
            .filter_map(|(sequence, binding)| match (section, binding) {
                ("keymaps", Binding::Command(command)) => Some((
                    command.name().to_owned(),
                    sequence.to_string(),
                    "SelectedItem".to_owned(),
                )),
                ("actions", Binding::Action(action)) => Some((
                    action.action.name().to_owned(),
                    sequence.to_string(),
                    format!("{:?}", action.target),
                )),
                _ => None,
            })
            .collect();
        assert_eq!(listed, expected, "{section} vs the default bindings");
    }

    let keymap = parse_keymap_toml(&path, &text).expect("examples/keymap.toml builds");
    assert_eq!(keymap, Keymap::default());
}

/// Spec 0017 AC14: `examples/theme.toml` loads with no error and no
/// notice; its first theme, `spotify_player`, sets every component to
/// spotify-player's default ("spotify-player's default" column), followed
/// by spotify-player's sample themes with their credit; `examples/app.toml`
/// sets `theme = "default"`.
#[test]
fn ac14_example_theme_toml_loads() {
    use tidal_player_core::ui::theme::{COMPONENTS, Color, Component, Modifiers, Style};

    let (path, text) = example("theme.toml");
    let themes = parse_theme_toml(&path, &text).expect("examples/theme.toml loads");
    assert_eq!(themes.notice(), None, "{:?}", themes.skipped());
    let names: Vec<&str> = themes.list().iter().map(|t| t.name.as_str()).collect();
    assert_eq!(
        names.get(..2),
        Some(&["default", "spotify_player"][..]),
        "{names:?}"
    );
    for sample in ["dracula", "gruvbox_dark", "catppuccin_mocha", "tokyonight"] {
        assert!(names.contains(&sample), "{sample}: {names:?}");
    }
    assert!(
        text.contains("aome510/spotify-player") && text.contains("MIT"),
        "the credit line"
    );

    // The column, through the default palette (the terminal's colours).
    use Component::*;
    let none = Style::new();
    let cyan_bold = Style::new().fg(Color::Cyan).add(Modifiers::BOLD);
    let column: [(Component, Style); 18] = [
        (BlockTitle, Style::new().fg(Color::Magenta)),
        (Border, none),
        (PlaybackStatus, cyan_bold),
        (PlaybackTrack, cyan_bold),
        (PlaybackArtists, cyan_bold),
        (PlaybackAlbum, Style::new().fg(Color::Yellow)),
        (PlaybackMetadata, Style::new().fg(Color::DarkGray)),
        (
            PlaybackProgressBar,
            Style::new().fg(Color::Green).bg(Color::DarkGray),
        ),
        (
            PlaybackProgressBarUnfilled,
            Style::new().bg(Color::DarkGray),
        ),
        (
            CurrentPlaying,
            Style::new().fg(Color::Green).add(Modifiers::BOLD),
        ),
        (PageDesc, cyan_bold),
        (
            PlaylistDesc,
            Style::new().fg(Color::DarkGray).add(Modifiers::DIM),
        ),
        (TableHeader, Style::new().fg(Color::Blue)),
        (
            Selection,
            Style::new().add(Modifiers::REVERSED | Modifiers::BOLD),
        ),
        (SecondaryRow, none),
        (Like, none),
        (Error, Style::new().fg(Color::Red)),
        (Prompt, Style::new().add(Modifiers::BOLD)),
    ];
    let theme = &themes.list()[1];
    assert_eq!(theme.screen, Style::new());
    for (component, want) in column {
        assert_eq!(theme.style(component), want, "{}", component.name());
    }
    // Every component is spelled out in the block, so it reads as the
    // whole list to copy.
    let block = text
        .split("[[themes]]")
        .find(|t| t.contains("name = \"spotify_player\""))
        .expect("the spotify_player block");
    for component in COMPONENTS {
        assert!(
            block.contains(&format!("\n{} = ", component.name())),
            "{} not in the spotify_player block",
            component.name()
        );
    }

    let (path, text) = example("app.toml");
    let file = parse_app_toml(&path, &text).unwrap();
    assert_eq!(file.theme.as_deref(), Some("default"));
}
