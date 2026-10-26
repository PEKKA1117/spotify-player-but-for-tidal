//! Spec 0008 AC17: `docs/config.md` documents every command and action of
//! the keymap, each command with its default keys, in the file's syntax.
//! Spec 0010 AC17 adds rows: MPRIS's settings in `docs/config.md`, the new
//! one-shot commands in `docs/daemon.md`, and `docs/mpris.md` with its
//! zh-TW copy. Spec 0013 AC7: the key hints' settings and their section
//! in `docs/tui.md`, in English and in the zh-TW copies. Spec 0017 AC15:
//! every theme component in `docs/config.md`.

use std::path::Path;

use tidal_player_core::ui::keymap::{ACTIONS, Binding, COMMANDS, defaults};

fn doc(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn config_md() -> String {
    doc("config.md")
}

/// The table rows (lines starting with `|`) whose first cell names
/// `name` in backticks.
fn rows<'a>(doc: &'a str, name: &str) -> Vec<&'a str> {
    let code = format!("`{name}`");
    doc.lines()
        .filter(|line| {
            line.starts_with('|')
                && line
                    .split('|')
                    .nth(1)
                    .is_some_and(|cell| cell.contains(&code))
        })
        .collect()
}

/// AC17: every command is in a table row of `docs/config.md` that lists
/// each of its default key sequences (as `` `label` ``); every action is in
/// a table row too.
#[test]
fn ac17_every_command_documented() {
    let doc = config_md();
    let defaults = defaults();
    for command in COMMANDS {
        let name = command.name();
        let labels: Vec<String> = defaults
            .iter()
            .filter(|(_, binding)| *binding == Binding::Command(command))
            .map(|(sequence, _)| format!("`{sequence}`"))
            .collect();
        let found = rows(&doc, name);
        assert!(
            found
                .iter()
                .any(|row| labels.iter().all(|label| row.contains(label.as_str()))),
            "{name}: no table row with {labels:?}: {found:?}"
        );
    }
    for action in ACTIONS {
        let name = action.name();
        assert!(!rows(&doc, name).is_empty(), "{name}: no table row");
    }
}

/// 0010 AC17: `mpris` and `max_cover_arts` in the `app.toml` table (with
/// their variables) and the cache directory; the new one-shot setters in
/// `docs/daemon.md`'s table; `docs/mpris.md` and its zh-TW copy.
#[test]
fn ac17_mpris_documented() {
    let config = config_md();
    for (key, var) in [
        ("mpris", "TIDAL_PLAYER_MPRIS"),
        ("max_cover_arts", "TIDAL_PLAYER_MAX_COVER_ARTS"),
    ] {
        let found = rows(&config, key);
        assert!(
            found.iter().any(|row| row.contains(var)),
            "{key}: no table row naming {var}: {found:?}"
        );
    }
    assert!(
        config.contains("TIDAL_PLAYER_CACHE_DIR"),
        "the cache directory"
    );

    let daemon = doc("daemon.md");
    for command in [
        "play",
        "pause",
        "stop",
        "shuffle on",
        "shuffle off",
        "repeat off",
        "repeat queue",
        "repeat track",
    ] {
        assert!(
            !rows(&daemon, command).is_empty(),
            "docs/daemon.md: no table row for `{command}`"
        );
    }
    assert!(daemon.contains("playerctl"), "docs/daemon.md: playerctl");

    let mpris = doc("mpris.md");
    for needed in [
        "org.mpris.MediaPlayer2.tidal_player",
        "playerctl",
        "mpris = false",
        "max_cover_arts",
        "bindsym XF86AudioPlay",
        "bindl = , XF86AudioPlay",
    ] {
        assert!(mpris.contains(needed), "docs/mpris.md: {needed}");
    }
    let zh = doc("zh-TW/mpris.md");
    assert!(
        zh.contains("org.mpris.MediaPlayer2.tidal_player"),
        "docs/zh-TW/mpris.md"
    );
    assert!(
        zh.contains("<a id=\"media-keys\"></a>"),
        "docs/zh-TW/mpris.md: anchors"
    );
}

/// 0013 AC7: `key_hints` and `key_hints_delay_ms` in the `app.toml` table
/// of `docs/config.md` (with their variables) and of its zh-TW copy; the
/// "Key hints" section of `docs/tui.md` and its zh-TW copy.
#[test]
fn ac7_key_hints_documented() {
    for name in ["config.md", "zh-TW/config.md"] {
        let config = doc(name);
        for (key, var) in [
            ("key_hints", "TIDAL_PLAYER_KEY_HINTS"),
            ("key_hints_delay_ms", "TIDAL_PLAYER_KEY_HINTS_DELAY_MS"),
        ] {
            let found = rows(&config, key);
            assert!(
                found.iter().any(|row| row.contains(var)),
                "docs/{name}: {key}: no table row naming {var}: {found:?}"
            );
        }
    }
    let tui = doc("tui.md");
    assert!(
        tui.contains("## Key hints"),
        "docs/tui.md: Key hints section"
    );
    for needed in ["key_hints", "key_hints_delay_ms", "g …"] {
        assert!(tui.contains(needed), "docs/tui.md: {needed}");
    }
    let zh = doc("zh-TW/tui.md");
    assert!(
        zh.contains("<a id=\"key-hints\"></a>"),
        "docs/zh-TW/tui.md: the key-hints anchor"
    );
    assert!(zh.contains("key_hints_delay_ms"), "docs/zh-TW/tui.md");
}

/// 0019 AC8: `confirm_removals` (with its variable) in the `app.toml`
/// table of `docs/config.md` and of its zh-TW copy; `docs/tui.md` and its
/// zh-TW copy describe the question and `d d`, and no longer bind `d`.
#[test]
fn ac8_confirm_removals_documented() {
    for name in ["config.md", "zh-TW/config.md"] {
        let config = doc(name);
        let found = rows(&config, "confirm_removals");
        assert!(
            found
                .iter()
                .any(|row| row.contains("TIDAL_PLAYER_CONFIRM_REMOVALS")),
            "docs/{name}: confirm_removals: no table row naming its variable: {found:?}"
        );
        let remove = rows(&config, "RemoveSelectedItem");
        assert!(
            remove.iter().any(|row| row.contains("`d d`")),
            "docs/{name}: RemoveSelectedItem is not `d d`: {remove:?}"
        );
        let queue = rows(&config, "RemoveFromQueue");
        assert!(
            !queue.is_empty() && queue.iter().all(|row| !row.contains("`d d`")),
            "docs/{name}: RemoveFromQueue still has a default key: {queue:?}"
        );
    }
    for name in ["tui.md", "zh-TW/tui.md"] {
        let tui = doc(name);
        for needed in ["confirm_removals", "`d d`", "(y/n)", "RemoveSelectedItem"] {
            assert!(tui.contains(needed), "docs/{name}: {needed}");
        }
        assert!(
            !tui.lines().any(|line| line.starts_with("| `d` |")),
            "docs/{name}: a key table still binds `d`"
        );
    }
    // AC9: the keymap section says a file binding beats a default it
    // conflicts with, and what the notice says.
    for name in ["config.md", "zh-TW/config.md"] {
        let config = doc(name);
        for needed in ["replaces the default", "`d d`"] {
            assert!(config.contains(needed), "docs/{name}: {needed}");
        }
    }
    for name in ["playback.md", "zh-TW/playback.md"] {
        let playback = doc(name);
        assert!(
            playback.contains("TIDAL_PLAYER_CONFIRM_REMOVALS"),
            "docs/{name}: the settings table"
        );
    }
}

/// 0016 AC10: the cover's three settings (with their variables) in the
/// `app.toml` table of `docs/config.md` and of its zh-TW copy; the "Album
/// cover" section of `docs/tui.md` with tmux's `allow-passthrough`, in both
/// copies; `docs/mpris.md` says the player caches covers with MPRIS off.
#[test]
fn ac10_cover_documented() {
    for name in ["config.md", "zh-TW/config.md"] {
        let config = doc(name);
        for (key, var) in [
            ("cover_img", "TIDAL_PLAYER_COVER_IMG"),
            ("cover_img_rows", "TIDAL_PLAYER_COVER_IMG_ROWS"),
            ("cover_img_protocol", "TIDAL_PLAYER_COVER_IMG_PROTOCOL"),
        ] {
            let found = rows(&config, key);
            assert!(
                found.iter().any(|row| row.contains(var)),
                "docs/{name}: no {key} row with {var}: {found:?}"
            );
        }
    }
    let tui = doc("tui.md");
    assert!(
        tui.contains("### Album cover"),
        "docs/tui.md: Album cover section"
    );
    for needed in [
        "allow-passthrough",
        "cover_img_protocol",
        "halfblocks",
        "max_cover_arts",
    ] {
        assert!(tui.contains(needed), "docs/tui.md: {needed}");
    }
    let zh = doc("zh-TW/tui.md");
    assert!(
        zh.contains("<a id=\"album-cover\"></a>"),
        "docs/zh-TW/tui.md: the album-cover anchor"
    );
    assert!(zh.contains("allow-passthrough"), "docs/zh-TW/tui.md");
    for name in ["mpris.md", "zh-TW/mpris.md"] {
        let mpris = doc(name);
        assert!(
            mpris.contains("mpris = false") && mpris.contains("0016"),
            "docs/{name}: covers cached with MPRIS off (0016)"
        );
    }
}

/// 0014 AC12: the output device is documented: `D` and its popup in
/// `docs/tui.md`, `SwitchDevice` in `docs/config.md`, `playback device` in
/// `docs/daemon.md` and `docs/playback.md`, in English and in the zh-TW
/// copies, and the README mentions it.
#[test]
fn ac12_device_selection_documented() {
    for prefix in ["", "zh-TW/"] {
        let tui = doc(&format!("{prefix}tui.md"));
        for needed in ["`D`", "SwitchDevice", "playback device", "Devices"] {
            assert!(tui.contains(needed), "docs/{prefix}tui.md: {needed}");
        }
        let config = doc(&format!("{prefix}config.md"));
        assert!(
            !rows(&config, "SwitchDevice").is_empty(),
            "docs/{prefix}config.md: a SwitchDevice row"
        );
        assert!(
            config.contains("output_device"),
            "docs/{prefix}config.md: output_device"
        );
        let daemon = doc(&format!("{prefix}daemon.md"));
        assert!(
            !rows(&daemon, "device").is_empty() && daemon.contains("playback device"),
            "docs/{prefix}daemon.md: playback device"
        );
        let playback = doc(&format!("{prefix}playback.md"));
        for needed in ["playback device", "tidal-player devices", "output_device"] {
            assert!(
                playback.contains(needed),
                "docs/{prefix}playback.md: {needed}"
            );
        }
    }
    for readme in ["README.md", "README.zh-TW.md"] {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(readme);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("playback device"), "{readme}");
    }
    if let Some(row) = rows(&doc("tui.md"), "D").first() {
        assert!(row.contains("Devices") || row.contains("device"), "{row}");
    }
}

/// 0012 AC10: the filter in `docs/tui.md` ("Filtering a list", the `/`
/// row of the keys table) and its zh-TW copy (the section's anchor), and
/// `Search`'s new text in the command table of `docs/config.md` and its
/// zh-TW copy.
#[test]
fn ac10_filter_documented() {
    let tui = doc("tui.md");
    assert!(
        tui.contains("## Filtering a list"),
        "docs/tui.md: Filtering a list section"
    );
    let slash = rows(&tui, "/");
    assert!(
        slash.iter().any(|row| row.contains("filter")),
        "docs/tui.md: the `/` row: {slash:?}"
    );
    for needed in ["No rows match", "matches)", "Loading more…"] {
        assert!(tui.contains(needed), "docs/tui.md: {needed}");
    }
    let zh = doc("zh-TW/tui.md");
    assert!(
        zh.contains("<a id=\"filtering-a-list\"></a>"),
        "docs/zh-TW/tui.md: the filtering-a-list anchor"
    );
    assert!(zh.contains("No rows match"), "docs/zh-TW/tui.md");
    for (name, text) in [("config.md", "filter the rows"), ("zh-TW/config.md", "/")] {
        let config = doc(name);
        let found = rows(&config, "Search");
        assert!(
            found
                .iter()
                .any(|row| row.contains(text) && !row.contains("search input")),
            "docs/{name}: the Search row: {found:?}"
        );
    }
}

/// 0015 AC6: where a user is sent to the docs, it is the wiki: both
/// READMEs, the daemon's unit, `daemon unit --help` and the "no
/// passphrase" error. None of them names a user doc in `docs/`.
#[test]
fn ac6_pointers_to_wiki() {
    const WIKI: &str = "https://github.com/PEKKA1117/spotify-player-but-for-tidal/wiki";
    let help = std::process::Command::new(env!("CARGO_BIN_EXE_tidal-player"))
        .args(["daemon", "unit", "--help"])
        .output()
        .expect("run tidal-player");
    let mut texts = vec![
        (
            "daemon unit --help".to_string(),
            String::from_utf8_lossy(&help.stdout).into_owned(),
        ),
        (
            "unit".to_string(),
            tidal_player::daemon::unit(Path::new("/usr/bin/tidal-player")),
        ),
        (
            "passphrase error".to_string(),
            tidal_player::passphrase::PassphraseError::Unavailable.to_string(),
        ),
    ];
    for readme in ["README.md", "README.zh-TW.md"] {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(readme);
        texts.push((readme.to_string(), std::fs::read_to_string(&path).unwrap()));
    }
    let user_docs = ["tui", "config", "playback", "login", "daemon", "mpris"]
        .map(|page| format!("docs/{page}.md"));
    for (name, text) in &texts {
        assert!(text.contains(WIKI), "{name}: no link to the wiki:\n{text}");
        for doc in user_docs.iter().map(String::as_str).chain(["docs/zh-TW"]) {
            assert!(!text.contains(doc), "{name}: still names {doc}:\n{text}");
        }
    }
}

/// 0017 AC15: every component of a theme is named in `docs/config.md`
/// (as `` `name` ``), with spotify-player's four skipped names,
/// `TIDAL_PLAYER_THEME` and `--theme`.
#[test]
fn ac15_every_component_documented() {
    use tidal_player_core::ui::theme::{COMPONENTS, SKIPPED_COMPONENTS};
    let config = config_md();
    for name in COMPONENTS
        .iter()
        .map(|c| c.name())
        .chain(SKIPPED_COMPONENTS)
        .chain(["theme.toml", "TIDAL_PLAYER_THEME", "--theme"])
    {
        assert!(
            config.contains(&format!("`{name}`")),
            "docs/config.md: `{name}` not documented"
        );
    }
}

/// 0020 AC13: the six selection commands and `selected_row` in
/// `docs/config.md`, "Selecting rows" in `docs/tui.md`, and the zh-TW
/// copies, `README.md` and `CLAUDE.md` say so.
#[test]
fn ac13_selection_documented() {
    for name in ["config.md", "zh-TW/config.md"] {
        let config = doc(name);
        for (command, keys) in [
            ("SelectCurrent", "`s c`"),
            ("SelectDown", "`s j`"),
            ("SelectUp", "`s k`"),
            ("SelectMotion", "`s m`"),
            ("SelectionActions", "`s a`"),
            ("RemoveSelection", "`d s`"),
        ] {
            let found = rows(&config, command);
            assert!(
                found.iter().any(|row| row.contains(keys)),
                "docs/{name}: {command}: no table row with {keys}: {found:?}"
            );
        }
        assert!(
            !rows(&config, "selected_row").is_empty(),
            "docs/{name}: selected_row: no table row"
        );
    }
    for name in ["tui.md", "zh-TW/tui.md"] {
        let tui = doc(name);
        assert!(
            tui.contains("<a id=\"selecting-rows\"></a>") || tui.contains("## Selecting rows"),
            "docs/{name}: no \"Selecting rows\" section"
        );
        for needed in [
            "`s c`",
            "`s j`",
            "`s k`",
            "`s m`",
            "`s a`",
            "`d s`",
            "motion",
            "selected_row",
        ] {
            assert!(tui.contains(needed), "docs/{name}: {needed}");
        }
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for name in ["README.md", "README.zh-TW.md"] {
        let readme = std::fs::read_to_string(root.join(name)).unwrap();
        assert!(readme.contains("tui.md#selecting-rows"), "{name}: link");
    }
    let claude = std::fs::read_to_string(root.join("CLAUDE.md")).unwrap();
    assert!(
        claude.contains("0020-multi-select.md") && claude.contains("tui.md#selecting-rows"),
        "CLAUDE.md: Features entry"
    );
}
