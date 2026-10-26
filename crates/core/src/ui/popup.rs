//! The popups over a page (spec 0006 "Actions popup", "Role filter"): the
//! actions list, *Add to playlist…*, the new-playlist name prompt, a `y/n`
//! question and the role filter; the devices (spec 0014) and the themes
//! (spec 0017). Pure data and the actions per item.

use crate::item::Item;
use crate::library::{AlbumSummary, FavoriteKind, MixSummary, PlaylistSummary};
use crate::protocol::DeviceEntry;
use crate::track::{ArtistRef, EntryId, Track};

use super::page::{PageKind, Rows, Window};
use super::select::{Held, Origin};

/// The first row of *Add to playlist…*.
pub const NEW_PLAYLIST: &str = "New playlist…";
/// The name prompt's label.
pub const PLAYLIST_NAME: &str = "Playlist name: ";

/// The tracks an *Add to playlist…* adds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrackSource {
    /// Known tracks (a track row, a queue entry, the playing track).
    Tracks(Vec<Track>),
    /// An album's tracks, fetched whole first.
    Album(u64),
}

/// One row of the actions popup, with what it acts on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuAction {
    /// *Open* (an album, playlist or artist row).
    Open(PageKind),
    /// *Go to album*.
    GoToAlbum(u64),
    /// *Go to artist: <name>*.
    GoToArtist(ArtistRef),
    /// *Go to radio* (spec 0011): the page to open.
    GoToRadio(PageKind),
    /// *Add to queue*: `Open { [item], Some(End) }`.
    AddToQueue(Item),
    /// *Play next*: `Open { [item], Some(Next) }`.
    PlayNext(Item),
    /// *Add to favorites* (the ID as [`crate::library::LibraryRequest`]
    /// takes it).
    AddFavorite(FavoriteKind, String),
    /// *Remove from favorites*.
    RemoveFavorite(FavoriteKind, String),
    /// *Add to playlist…*: opens [`Popup::AddToPlaylist`].
    AddToPlaylist(TrackSource),
    /// *Remove from this playlist*: the row's position and the ETag the
    /// list was loaded with.
    RemoveFromPlaylist {
        uuid: String,
        title: String,
        index: u32,
        etag: Option<String>,
    },
    /// *Remove from queue*.
    RemoveFromQueue(EntryId),
    /// *Delete playlist*: asks first.
    DeletePlaylist { uuid: String, title: String },
}

impl MenuAction {
    /// The row's text.
    pub fn label(&self) -> String {
        match self {
            Self::Open(_) => "Open".into(),
            Self::GoToAlbum(_) => "Go to album".into(),
            Self::GoToArtist(artist) => format!("Go to artist: {}", artist.name),
            Self::GoToRadio(_) => "Go to radio".into(),
            Self::AddToQueue(_) => "Add to queue".into(),
            Self::PlayNext(_) => "Play next".into(),
            Self::AddFavorite(..) => "Add to favorites".into(),
            Self::RemoveFavorite(..) => "Remove from favorites".into(),
            Self::AddToPlaylist(_) => "Add to playlist…".into(),
            Self::RemoveFromPlaylist { .. } => "Remove from this playlist".into(),
            Self::RemoveFromQueue(_) => "Remove from queue".into(),
            Self::DeletePlaylist { .. } => "Delete playlist".into(),
        }
    }
}

/// What a `y` answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Confirmed {
    /// Add `tracks` to the playlist although some are in it already.
    AddToPlaylist {
        uuid: String,
        title: String,
        tracks: Vec<Track>,
    },
    DeletePlaylist {
        uuid: String,
        title: String,
    },
    /// Remove what the question named (spec 0019).
    Remove(Removal),
    /// Remove a window's or the queue's selection (spec 0020 "Removing
    /// the selection").
    RemoveSelection(SelectionRemoval),
}

/// What `d s` asks about (spec 0020): one [`Removal`] per list the rows
/// leave (two in the library's Playlists: the own ones deleted first, the
/// followed ones removed from favorites), each item with the row it was,
/// and the window or queue whose selection it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectionRemoval {
    pub origin: Origin,
    pub parts: Vec<SelectionPart>,
}

/// One list of a [`SelectionRemoval`]: the removal, and per item the
/// selected row it is (`held[i]` is `removal.items[i]`'s).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectionPart {
    pub removal: Removal,
    pub held: Vec<Held>,
}

/// The question `d s` asks (spec 0020): 0019's text for one list
/// ([`question`]), `Delete N playlists and remove M from favorites? (y/n)`
/// for own and followed playlists together.
pub fn selection_question(removal: &SelectionRemoval) -> String {
    match removal.parts.as_slice() {
        [one] => question(&one.removal),
        parts => {
            let count = |from: fn(&RemoveFrom) -> bool| -> usize {
                parts
                    .iter()
                    .filter(|p| from(&p.removal.from))
                    .map(|p| p.removal.items.len())
                    .sum()
            };
            let deleted = count(|f| *f == RemoveFrom::OwnPlaylists);
            let removed = count(|f| matches!(f, RemoveFrom::Favorites(_)));
            let plural = if deleted == 1 { "" } else { "s" };
            format!("Delete {deleted} playlist{plural} and remove {removed} from favorites? (y/n)")
        }
    }
}

/// One line of the target list under a removal question (spec 0020): a
/// heading (`Delete:`, `Remove from favorites:`) or a target's title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetLine {
    pub text: String,
    pub heading: bool,
}

/// The target list a question shows under its first line, in the list's
/// order (spec 0020): every title of a [`Confirmed::RemoveSelection`],
/// under a heading per list when there are two; nothing for the other
/// questions.
pub fn target_lines(on_yes: &Confirmed) -> Vec<TargetLine> {
    let Confirmed::RemoveSelection(removal) = on_yes else {
        return Vec::new();
    };
    let headed = removal.parts.len() > 1;
    let mut lines = Vec::new();
    for part in &removal.parts {
        if headed {
            let heading = match part.removal.from {
                RemoveFrom::OwnPlaylists => "Delete:",
                _ => "Remove from favorites:",
            };
            lines.push(TargetLine {
                text: heading.to_owned(),
                heading: true,
            });
        }
        lines.extend(part.removal.items.iter().map(|item| TargetLine {
            text: item.title.clone(),
            heading: false,
        }));
    }
    lines
}

/// What a removal question asks about (spec 0019): the items hold what
/// to remove, not a cursor, so the answer is the same however the list
/// moved meanwhile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Removal {
    pub from: RemoveFrom,
    pub items: Vec<Removed>,
}

/// Where a [`Removal`] removes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoveFrom {
    /// The favorites of one kind.
    Favorites(FavoriteKind),
    /// An own playlist, with the ETag its page had when the question was
    /// asked.
    Playlist {
        uuid: String,
        title: String,
        etag: Option<String>,
    },
    /// The play queue.
    Queue,
    /// The user's own playlists, deleted (spec 0020: `d s` in the
    /// library's Playlists).
    OwnPlaylists,
}

/// One item of a [`Removal`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Removed {
    /// The row's title, as the list shows it.
    pub title: String,
    pub target: RemovedTarget,
}

/// What identifies a [`Removed`] item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemovedTarget {
    /// A favorite's ID, as [`crate::library::LibraryRequest`] takes it.
    Favorite(String),
    /// A playlist track's position.
    Position(u32),
    /// A queue entry.
    Entry(EntryId),
    /// A playlist's UUID (one deleted).
    Playlist(String),
}

/// The question a [`Removal`] asks (spec 0019 "Several items").
pub fn question(removal: &Removal) -> String {
    let (title, count) = match removal.items.as_slice() {
        [one] => (Some(one.title.as_str()), 1),
        items => (None, items.len()),
    };
    // `Remove <title>` for one item, `Remove N <things>` for several.
    let what = |things: &str| match title {
        Some(title) => title.to_owned(),
        None => format!("{count} {things}"),
    };
    match &removal.from {
        RemoveFrom::Favorites(kind) => {
            let things = match kind {
                FavoriteKind::Track => "tracks",
                FavoriteKind::Album => "albums",
                FavoriteKind::Artist => "artists",
                FavoriteKind::Playlist => "playlists",
            };
            format!("Remove {} from favorites? (y/n)", what(things))
        }
        RemoveFrom::Playlist {
            title: playlist, ..
        } => {
            format!("Remove {} from {playlist}? (y/n)", what("tracks"))
        }
        RemoveFrom::Queue => format!("Remove {} from the queue? (y/n)", what("tracks")),
        RemoveFrom::OwnPlaylists => format!("Delete {}? (y/n)", what("playlists")),
    }
}

/// The open popup; while one is open no other key acts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Popup {
    /// The actions on one item.
    Actions {
        /// The item's title or name.
        title: String,
        actions: Vec<MenuAction>,
        cursor: usize,
    },
    /// *Add to playlist…*: row 0 is [`NEW_PLAYLIST`], then
    /// [`Popup::own_playlists`].
    AddToPlaylist {
        tracks: TrackSource,
        /// The user's playlists, fetched when the popup opens.
        playlists: Window,
        cursor: usize,
    },
    /// The one-row name prompt ([`PLAYLIST_NAME`]) of *New playlist…*.
    NewPlaylist { tracks: TrackSource, name: String },
    /// A `y/n` question; `scroll` is the first line of its target list
    /// shown ([`target_lines`], spec 0020).
    Confirm {
        question: String,
        on_yes: Confirmed,
        scroll: usize,
    },
    /// The *All tracks* role filter: check boxes per
    /// [`super::page::ROLE_CATEGORIES`].
    Roles { checked: [bool; 4], cursor: usize },
    /// The output devices (spec 0014): the player's list, and the cursor
    /// over [`device_rows`].
    Devices { list: DeviceList, cursor: usize },
    /// The themes (spec 0017): the cursor over [`super::State::themes`],
    /// and the theme in use when it opened (`●`), which `Esc` goes back to.
    Themes { cursor: usize, active: usize },
    /// The actions on the selection (spec 0020): titled `N selected`,
    /// on the selection at `origin`.
    SelectionActions {
        title: String,
        actions: Vec<SelectionAction>,
        cursor: usize,
        origin: Origin,
    },
}

/// One row of [`Popup::SelectionActions`] (spec 0020 "The selection
/// actions popup"), with what it acts on, in the list's order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectionAction {
    /// *Play* (`p`): `Open { items, at: None }`.
    Play(Vec<Item>),
    /// *Play next* (`N`): `Open { items, at: Some(Next) }`.
    PlayNext(Vec<Item>),
    /// *Add to queue* (`a`): `Open { items, at: Some(End) }`.
    AddToQueue(Vec<Item>),
    /// *Add to playlist…*: the picker for these tracks.
    AddToPlaylist(Vec<Track>),
    /// *Add to favorites*: the rows not known to be favorites, by ID.
    AddFavorites(FavoriteKind, Vec<(Held, String)>),
    /// *Remove from favorites*, as `d s`.
    RemoveFavorites,
    /// *Remove from this playlist*, as `d s`.
    RemoveFromPlaylist,
    /// *Remove from queue*, as `d s`.
    RemoveFromQueue,
}

impl SelectionAction {
    /// The row's text.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Play(_) => "Play",
            Self::PlayNext(_) => "Play next",
            Self::AddToQueue(_) => "Add to queue",
            Self::AddToPlaylist(_) => "Add to playlist…",
            Self::AddFavorites(..) => "Add to favorites",
            Self::RemoveFavorites => "Remove from favorites",
            Self::RemoveFromPlaylist => "Remove from this playlist",
            Self::RemoveFromQueue => "Remove from queue",
        }
    }

    /// The popup's own key for it (`p`, `N`, `a`), if it has one.
    pub fn key(&self) -> Option<char> {
        match self {
            Self::Play(_) => Some('p'),
            Self::PlayNext(_) => Some('N'),
            Self::AddToQueue(_) => Some('a'),
            _ => None,
        }
    }
}

/// The device list of [`Popup::Devices`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceList {
    /// Asked with this request ID (`Loading devices…`).
    Loading { id: u64 },
    /// The player's answer.
    Loaded(Vec<DeviceEntry>),
    /// Why there is no list (`Cannot list devices: <reason>`).
    Failed(String),
}

/// The text shown while the list is asked for.
pub const LOADING_DEVICES: &str = "Loading devices…";
/// The description of a selected device the list does not have.
pub const DEVICE_NOT_FOUND: &str = "not found";

impl DeviceList {
    /// What the popup shows instead of rows: `Loading devices…`, or
    /// `Cannot list devices: <reason>`; `None` once loaded.
    pub fn status(&self) -> Option<String> {
        match self {
            Self::Loading { .. } => Some(LOADING_DEVICES.to_owned()),
            Self::Loaded(_) => None,
            Self::Failed(reason) => Some(format!("Cannot list devices: {reason}")),
        }
    }
}

/// One row of [`Popup::Devices`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceRow {
    pub name: String,
    pub description: String,
    /// The player's selected device (`●`).
    pub selected: bool,
}

/// The rows of the devices popup for `list` (spec 0014 "Listing
/// devices"): the selected device first as [`DEVICE_NOT_FOUND`] when the
/// list does not have it, then the list in its order; none until loaded.
pub fn device_rows(list: &DeviceList, selected: Option<&str>) -> Vec<DeviceRow> {
    let DeviceList::Loaded(devices) = list else {
        return Vec::new();
    };
    let missing = selected.filter(|s| !devices.iter().any(|d| d.name == *s));
    missing
        .map(|name| DeviceRow {
            name: name.to_owned(),
            description: DEVICE_NOT_FOUND.to_owned(),
            selected: true,
        })
        .into_iter()
        .chain(devices.iter().map(|d| DeviceRow {
            name: d.name.clone(),
            description: d.description.clone(),
            selected: Some(d.name.as_str()) == selected,
        }))
        .collect()
}

impl Popup {
    /// The own playlists *Add to playlist…* lists, in Tidal's order.
    pub fn own_playlists(&self) -> Vec<&PlaylistSummary> {
        match self {
            Self::AddToPlaylist { playlists, .. } => own(playlists),
            _ => Vec::new(),
        }
    }
}

/// The own playlists among a playlists window's rows.
pub(super) fn own(playlists: &Window) -> Vec<&PlaylistSummary> {
    match &playlists.rows {
        Rows::Playlists(rows) => rows.iter().filter(|p| p.own).collect(),
        _ => Vec::new(),
    }
}

/// The actions on a track of a browse page (spec 0006 "Actions popup"):
/// `own_playlist` is the playlist page's own playlist with the row's
/// position and the page's ETag.
pub fn track_actions(
    track: &Track,
    own_playlist: Option<(&PlaylistSummary, u32, Option<&str>)>,
) -> Vec<MenuAction> {
    let item = Item::Track(track.id);
    let mut actions = go_to(track);
    actions.extend([
        MenuAction::AddToQueue(item.clone()),
        MenuAction::PlayNext(item),
    ]);
    actions.extend(favorites(FavoriteKind::Track, track.id.to_string()));
    actions.push(MenuAction::AddToPlaylist(TrackSource::Tracks(vec![
        track.clone(),
    ])));
    if let Some((playlist, index, etag)) = own_playlist {
        actions.push(MenuAction::RemoveFromPlaylist {
            uuid: playlist.uuid.clone(),
            title: playlist.title.clone(),
            index,
            etag: etag.map(Into::into),
        });
    }
    actions
}

/// The actions on a queue entry, or on the playing track (`playing`: no
/// *Play next*).
pub fn entry_actions(track: &Track, entry: EntryId, playing: bool) -> Vec<MenuAction> {
    let mut actions = go_to(track);
    if !playing {
        actions.push(MenuAction::PlayNext(Item::Track(track.id)));
    }
    actions.push(MenuAction::RemoveFromQueue(entry));
    actions.extend(favorites(FavoriteKind::Track, track.id.to_string()));
    actions.push(MenuAction::AddToPlaylist(TrackSource::Tracks(vec![
        track.clone(),
    ])));
    actions
}

/// The actions on an album row.
pub fn album_actions(album: &AlbumSummary) -> Vec<MenuAction> {
    let item = Item::Album(album.id);
    let mut actions = vec![MenuAction::Open(PageKind::Album(album.id))];
    actions.extend(album.artists.iter().cloned().map(MenuAction::GoToArtist));
    actions.extend([
        MenuAction::AddToQueue(item.clone()),
        MenuAction::PlayNext(item),
    ]);
    actions.extend(favorites(FavoriteKind::Album, album.id.to_string()));
    actions.push(MenuAction::AddToPlaylist(TrackSource::Album(album.id)));
    actions
}

/// The actions on a playlist row.
pub fn playlist_actions(playlist: &PlaylistSummary) -> Vec<MenuAction> {
    let item = Item::Playlist(playlist.uuid.clone());
    let mut actions = vec![
        MenuAction::Open(PageKind::Playlist(playlist.uuid.clone())),
        MenuAction::AddToQueue(item.clone()),
        MenuAction::PlayNext(item),
    ];
    if playlist.own {
        actions.push(MenuAction::DeletePlaylist {
            uuid: playlist.uuid.clone(),
            title: playlist.title.clone(),
        });
    } else {
        actions.extend(favorites(FavoriteKind::Playlist, playlist.uuid.clone()));
    }
    actions
}

/// The actions on an artist row.
pub fn artist_actions(artist: &ArtistRef) -> Vec<MenuAction> {
    let mut actions = vec![
        MenuAction::Open(PageKind::Artist(artist.id)),
        MenuAction::GoToRadio(PageKind::ArtistRadio(artist.id)),
    ];
    actions.extend(favorites(FavoriteKind::Artist, artist.id.to_string()));
    actions
}

/// The actions on a mix row (spec 0011): a mix is not an item and is not
/// favorited, so only *Open*.
pub fn mix_actions(mix: &MixSummary) -> Vec<MenuAction> {
    vec![MenuAction::Open(PageKind::Mix(mix.id.clone()))]
}

/// *Go to album* (when the track has one), *Go to artist* per artist and
/// *Go to radio* (spec 0011).
fn go_to(track: &Track) -> Vec<MenuAction> {
    track
        .album
        .as_ref()
        .map(|album| MenuAction::GoToAlbum(album.id))
        .into_iter()
        .chain(track.artists.iter().cloned().map(MenuAction::GoToArtist))
        .chain([MenuAction::GoToRadio(PageKind::TrackRadio(track.id.0))])
        .collect()
}

/// Both favorite actions: whether the item is a favorite is not known
/// (spec 0006 "Favorite or not": `favorites/ids` is not verified).
fn favorites(kind: FavoriteKind, id: String) -> [MenuAction; 2] {
    [
        MenuAction::AddFavorite(kind, id.clone()),
        MenuAction::RemoveFavorite(kind, id),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items(n: u32, target: impl Fn(u32) -> RemovedTarget) -> Vec<Removed> {
        (1..=n)
            .map(|i| Removed {
                title: format!("T{i}"),
                target: target(i),
            })
            .collect()
    }

    /// Spec 0019 AC5: the question for one and several items, for the
    /// favorites of each kind, a playlist and the queue.
    #[test]
    fn ac5_removal_question() {
        let favorites = |kind| RemoveFrom::Favorites(kind);
        let favorite = |i: u32| RemovedTarget::Favorite(i.to_string());
        let playlist = || RemoveFrom::Playlist {
            uuid: "p1".into(),
            title: "Mine".into(),
            etag: Some("e1".into()),
        };
        let cases = [
            (
                favorites(FavoriteKind::Track),
                1,
                "Remove T1 from favorites? (y/n)",
            ),
            (
                favorites(FavoriteKind::Album),
                1,
                "Remove T1 from favorites? (y/n)",
            ),
            (
                favorites(FavoriteKind::Artist),
                1,
                "Remove T1 from favorites? (y/n)",
            ),
            (
                favorites(FavoriteKind::Playlist),
                1,
                "Remove T1 from favorites? (y/n)",
            ),
            (
                favorites(FavoriteKind::Track),
                3,
                "Remove 3 tracks from favorites? (y/n)",
            ),
            (
                favorites(FavoriteKind::Album),
                3,
                "Remove 3 albums from favorites? (y/n)",
            ),
            (
                favorites(FavoriteKind::Artist),
                3,
                "Remove 3 artists from favorites? (y/n)",
            ),
            (
                favorites(FavoriteKind::Playlist),
                3,
                "Remove 3 playlists from favorites? (y/n)",
            ),
            (playlist(), 1, "Remove T1 from Mine? (y/n)"),
            (playlist(), 3, "Remove 3 tracks from Mine? (y/n)"),
            (RemoveFrom::Queue, 1, "Remove T1 from the queue? (y/n)"),
            (
                RemoveFrom::Queue,
                3,
                "Remove 3 tracks from the queue? (y/n)",
            ),
            // Spec 0020: deleting own playlists (`d s` in the library).
            (RemoveFrom::OwnPlaylists, 1, "Delete T1? (y/n)"),
            (RemoveFrom::OwnPlaylists, 3, "Delete 3 playlists? (y/n)"),
        ];
        for (from, n, want) in cases {
            let target = match &from {
                RemoveFrom::Favorites(_) => favorite,
                RemoveFrom::Playlist { .. } => RemovedTarget::Position,
                RemoveFrom::Queue => |i| RemovedTarget::Entry(EntryId(u64::from(i))),
                RemoveFrom::OwnPlaylists => |i: u32| RemovedTarget::Playlist(format!("p{i}")),
            };
            let removal = Removal {
                items: items(n, target),
                from,
            };
            assert_eq!(question(&removal), want, "{:?} x{n}", removal.from);
        }
    }
}
