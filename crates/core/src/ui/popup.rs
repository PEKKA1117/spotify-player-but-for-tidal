//! The popups over a page (spec 0006 "Actions popup", "Role filter"): the
//! actions list, *Add to playlist…*, the new-playlist name prompt, a `y/n`
//! question and the role filter. Pure data and the actions per item.

use crate::item::Item;
use crate::library::{AlbumSummary, FavoriteKind, MixSummary, PlaylistSummary};
use crate::protocol::DeviceEntry;
use crate::track::{ArtistRef, EntryId, Track};

use super::page::{PageKind, Rows, Window};

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
    /// A `y/n` question.
    Confirm { question: String, on_yes: Confirmed },
    /// The *All tracks* role filter: check boxes per
    /// [`super::page::ROLE_CATEGORIES`].
    Roles { checked: [bool; 4], cursor: usize },
    /// The output devices (spec 0014): the player's list, and the cursor
    /// over [`device_rows`].
    Devices { list: DeviceList, cursor: usize },
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
    let _ = (list, selected);
    Vec::new()
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
