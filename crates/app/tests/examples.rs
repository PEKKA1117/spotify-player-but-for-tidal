//! Spec 0008 AC18: `examples/app.toml` and `examples/keymap.toml` spell out
//! every default, so copying them changes nothing.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use tidal_player::config::{AppConfig, parse_app_toml, parse_keymap_toml};
use tidal_player::play::{resolve_player_config_with, resolve_settings_with};
use tidal_player_core::ui::Keymap;
use tidal_player_core::ui::keymap::defaults;

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

/// AC18: one `[[keymaps]]` entry per default binding, nothing else, and
/// the file builds to the default keymap.
#[test]
fn ac18_example_keymap_is_the_defaults() {
    let (path, text) = example("keymap.toml");
    let value: toml::Table = text.parse().expect("examples/keymap.toml is TOML");
    let entries: Vec<(String, String)> = value
        .get("keymaps")
        .and_then(toml::Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .map(|e| {
                    let field =
                        |k: &str| e.get(k).and_then(toml::Value::as_str).unwrap().to_owned();
                    (field("command"), field("key_sequence"))
                })
                .collect()
        })
        .unwrap_or_default();
    let listed: BTreeSet<(String, String)> = entries.iter().cloned().collect();
    assert_eq!(listed.len(), entries.len(), "an entry is listed twice");
    let expected: BTreeSet<(String, String)> = defaults()
        .into_iter()
        .map(|(sequence, binding)| (binding.name().to_owned(), sequence.to_string()))
        .collect();
    assert_eq!(listed, expected, "entries vs the default bindings");
    assert!(value.get("actions").is_none(), "no default [[actions]]");

    let keymap = parse_keymap_toml(&path, &text).expect("examples/keymap.toml builds");
    assert_eq!(keymap, Keymap::default());
}
