//! Links and the clipboard (spec 0030): what *Copy link*'s result and
//! the clipboard's text do. The TUI copies and reads; this model only asks
//! ([`Effect::Copy`], [`Effect::ReadClipboard`]) and takes the answers.

use crate::item::{Item, Link, parse_link};
use crate::protocol::{Command, InsertAt};

use super::{Effect, PageKind, State, browse};

/// How much of a text that is not a link the message row quotes.
const QUOTED: usize = 60;

/// How a link was copied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopiedVia {
    /// A clipboard tool (`wl-copy`, `xclip`, `xsel`).
    Tool,
    /// The terminal's OSC 52 sequence.
    Osc52,
}

/// The message row after copying `link`.
pub(super) fn copied(state: &mut State, link: &str, result: Result<CopiedVia, String>) {
    state.message = Some(match result {
        Ok(CopiedVia::Tool) => format!("Link copied: {link}"),
        Ok(CopiedVia::Osc52) => format!("Link copied (OSC 52): {link}"),
        Err(reason) => reason,
    });
}

/// `OpenSpotifyLinkFromClipboard` on a page: ask for the clipboard.
pub(super) fn open(state: &mut State) -> Vec<Effect> {
    if covered(state) {
        return Vec::new();
    }
    vec![Effect::ReadClipboard]
}

/// Whether something is over the page: a popup, a prompt, the keys help
/// or a whole-list load.
fn covered(state: &State) -> bool {
    state.popup.is_some()
        || state.prompt.is_some()
        || state.help.is_some()
        || state.whole_list.is_some()
}

/// The clipboard's text (or why it could not be read).
/// Dropped when something is over the page by the time it comes.
pub(super) fn read(state: &mut State, result: Result<String, String>) -> Vec<Effect> {
    if covered(state) {
        return Vec::new();
    }
    let text = match result {
        Ok(text) => text,
        Err(reason) => {
            state.message = Some(reason);
            return Vec::new();
        }
    };
    let page = match parse_link(&text) {
        Some(Link::Item(Item::Track(id))) => {
            return vec![Effect::Send(Command::Open {
                items: vec![Item::Track(id)],
                at: Some(InsertAt::End),
            })];
        }
        Some(Link::Item(Item::Album(id))) => PageKind::Album(id),
        Some(Link::Item(Item::Playlist(uuid))) => PageKind::Playlist(uuid),
        Some(Link::Artist(id)) => PageKind::Artist(id),
        Some(Link::Mix(id)) => PageKind::Mix(id),
        None => {
            state.message = Some(not_a_link(&text));
            return Vec::new();
        }
    };
    browse::open(state, page)
}

/// The message for a clipboard that holds no link: the text on one line,
/// cut to [`QUOTED`] characters.
fn not_a_link(text: &str) -> String {
    let text = text.trim();
    if text.is_empty() {
        return "The clipboard is empty".into();
    }
    let line: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let mut quoted: String = line.chars().take(QUOTED).collect();
    if line.chars().count() > QUOTED {
        quoted.push('…');
    }
    format!("Not a Tidal link: {quoted}")
}

#[cfg(test)]
mod tests;
