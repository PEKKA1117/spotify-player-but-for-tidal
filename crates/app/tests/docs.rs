//! Spec 0008 AC17: `docs/config.md` documents every command and action of
//! the keymap, each command with its default keys, in the file's syntax.
//! Spec 0010 AC17 adds rows: MPRIS's settings in `docs/config.md`, the new
//! one-shot commands in `docs/daemon.md`, and `docs/mpris.md` with its
//! zh-TW copy.

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
