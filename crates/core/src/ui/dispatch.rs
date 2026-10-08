//! Keys to commands (spec 0008): stub.

use super::keymap::UiCommand;
use super::{Effect, State};

/// Runs `command` where the UI is (stub: nothing).
pub(crate) fn run_command(_state: &mut State, _command: UiCommand) -> Vec<Effect> {
    Vec::new()
}
