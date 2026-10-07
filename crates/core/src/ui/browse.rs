//! Browsing the library (spec 0006 "Client model"): the page history,
//! window focus and cursors, scrolling loads, the whole-list load,
//! playing and queueing from a page, the popups and the library writes.

use crate::library::{FavoriteKind, LibraryResponse};

use super::page::{PageKind, Window};
use super::popup::TrackSource;
use super::{Effect, Key, State};

/// The message of a removal refused because the playlist changed (`412`),
/// as the player words it (spec 0006 "Talking to the player").
pub const PLAYLIST_CHANGED: &str = "The playlist changed: nothing was changed, try again";

/// A whole-list load in progress (spec 0006 "The whole list is needed").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WholeList {
    pub source: WholeListSource,
    pub purpose: Purpose,
}

/// The list being loaded whole.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WholeListSource {
    /// A window of a page in the history.
    Window { page: usize, window: usize },
    /// A list no page shows (an album row's tracks).
    Detached(Window),
}

/// What happens once the list is whole.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Purpose {
    /// `LoadQueue` from the visible row `start`.
    Play { start: usize },
    /// `AddToPlaylist` to `uuid`, asking first when a track is in a loaded
    /// copy of it (`check_duplicates`).
    AddToPlaylist {
        uuid: String,
        title: String,
        check_duplicates: bool,
    },
}

/// A library write waiting for its reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Write {
    Favorite {
        add: bool,
        kind: FavoriteKind,
    },
    /// Then adds `tracks` to the created playlist.
    Create {
        title: String,
        tracks: TrackSource,
    },
    AddToPlaylist {
        uuid: String,
        title: String,
        count: usize,
    },
    RemoveFromPlaylist {
        uuid: String,
        title: String,
    },
    DeletePlaylist {
        uuid: String,
        title: String,
    },
}

pub(super) fn open(_state: &mut State, _kind: PageKind) -> Vec<Effect> {
    Vec::new()
}

pub(super) fn back(_state: &mut State) -> Vec<Effect> {
    Vec::new()
}

pub(super) fn cycle_focus(_state: &mut State, _forward: bool) -> Vec<Effect> {
    Vec::new()
}

pub(super) fn move_window_cursor(
    _state: &mut State,
    _to: impl Fn(usize, usize) -> usize,
) -> Vec<Effect> {
    Vec::new()
}

pub(super) fn browse_key(_state: &mut State, _key: Key) -> Option<Vec<Effect>> {
    None
}

pub(super) fn actions_on_selected(_state: &mut State) -> Vec<Effect> {
    Vec::new()
}

pub(super) fn actions_on_playing(_state: &mut State) -> Vec<Effect> {
    Vec::new()
}

pub(super) fn popup_key(_state: &mut State, _key: Key) -> Vec<Effect> {
    Vec::new()
}

pub(super) fn whole_list_key(_state: &mut State, _key: Key) -> Vec<Effect> {
    Vec::new()
}

pub(super) fn reply(
    _state: &mut State,
    _id: u64,
    _result: Result<LibraryResponse, String>,
) -> Vec<Effect> {
    Vec::new()
}

pub(super) fn disconnected(_state: &mut State) {}

pub(super) fn reconnected(_state: &mut State) -> Vec<Effect> {
    Vec::new()
}

#[cfg(test)]
mod tests;
