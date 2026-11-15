//! Links and the clipboard (spec 0030): what *Copy link*'s result and
//! the clipboard's text do. The TUI copies and reads; this model only asks
//! ([`Effect::Copy`], [`Effect::ReadClipboard`]) and takes the answers.

use super::{Effect, State};

/// How a link was copied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopiedVia {
    /// A clipboard tool (`wl-copy`, `xclip`, `xsel`).
    Tool,
    /// The terminal's OSC 52 sequence.
    Osc52,
}

/// The message row after copying `link`.
pub(super) fn copied(_state: &mut State, _link: &str, _result: Result<CopiedVia, String>) {}

/// `OpenSpotifyLinkFromClipboard` on a page: ask for the clipboard.
pub(super) fn open(_state: &mut State) -> Vec<Effect> {
    Vec::new()
}

/// The clipboard's text (or why it could not be read).
pub(super) fn read(_state: &mut State, _result: Result<String, String>) -> Vec<Effect> {
    Vec::new()
}

#[cfg(test)]
mod tests;
