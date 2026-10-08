//! Spec 0008 AC17: `docs/config.md` documents every command and action of
//! the keymap, each command with its default keys, in the file's syntax.

use std::path::Path;

use tidal_player_core::ui::keymap::{ACTIONS, Binding, COMMANDS, defaults};

fn config_md() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/config.md");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
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
