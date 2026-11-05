//! The library: pages, lists, summaries, the requests a client sends the
//! player about them and the answers (spec 0006 "Types", "Talking to the
//! player"), and the alternate-version filter of the artist's *All tracks*.
//! Pure types and functions: the player fetches, the client shows.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::lyrics::Lyrics;
use crate::track::{ArtistRef, Track, TrackId};

/// What kind of release an album is (`type` in Tidal's album object).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AlbumKind {
    Album,
    Ep,
    Single,
}

/// An album row (spec 0006 "Rows"): favorite albums, an artist's albums,
/// and the album page's header.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlbumSummary {
    /// Tidal's album ID, as in [`crate::Item::Album`].
    pub id: u64,
    pub title: String,
    pub artists: Vec<ArtistRef>,
    /// From `releaseDate`, when known.
    pub year: Option<u16>,
    pub kind: AlbumKind,
    /// `numberOfTracks`, when known.
    pub tracks: Option<u32>,
    pub duration: Option<Duration>,
}

/// A playlist row (spec 0006 "Rows") and the playlist page's header.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaylistSummary {
    pub uuid: String,
    pub title: String,
    /// `numberOfTracks`, when known.
    pub tracks: Option<u32>,
    pub duration: Option<Duration>,
    /// The user's own playlist (`USER_CREATED`); `false` for a favorite
    /// (followed) one.
    pub own: bool,
    /// Tidal's description; `None` when empty (spec 0022).
    pub description: Option<String>,
    /// `publicPlaylist` (spec 0022).
    pub public: bool,
}

/// One of the four role categories of Tidal's credits (`roleCategories`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RoleCategory {
    Performer,
    Songwriter,
    Producer,
    Engineer,
}

/// A row of the artist's *All tracks*: the track and the artist's role
/// categories on it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreditedTrack {
    pub track: Track,
    pub roles: Vec<RoleCategory>,
}

/// One page of a list, as Tidal returned it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListPage<T> {
    /// In the order Tidal returned them.
    pub items: Vec<T>,
    /// The `offset` this page was asked at.
    pub offset: u32,
    /// Tidal's `totalNumberOfItems` for the whole list.
    pub total: u32,
    /// Rows of this page left out by the player's filters (*All tracks*:
    /// Atmos-only copies and alternate versions); `0` for every other list.
    pub hidden: u32,
    /// Each item's position in Tidal's list, where items were dropped
    /// from the page (a playlist's videos). Empty: `offset` + index.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub positions: Vec<u32>,
}

impl<T> ListPage<T> {
    /// Item `index`'s position in Tidal's list.
    pub fn position(&self, index: usize) -> u32 {
        self.positions
            .get(index)
            .copied()
            .unwrap_or(self.offset + index as u32)
    }
}

/// One page of any list, tagged by its row type: the answer to
/// [`LibraryRequest::More`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ListItems {
    Tracks(ListPage<Track>),
    Albums(ListPage<AlbumSummary>),
    Playlists(ListPage<PlaylistSummary>),
    Artists(ListPage<ArtistRef>),
    Credits(ListPage<CreditedTrack>),
    /// A browse section's mixes (spec 0024).
    Mixes(ListPage<MixSummary>),
    /// A browse section's links.
    Links(ListPage<PageLink>),
    /// A browse section that holds rows of several kinds.
    Mixed(ListPage<MixedRow>),
}

/// Names one list (spec 0006 "Talking to the player").
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ListRef {
    FavoriteTracks,
    /// Own and favorite playlists.
    Playlists,
    FavoriteAlbums,
    FavoriteArtists,
    AlbumTracks(u64),
    PlaylistTracks(String),
    TopTracks(u64),
    /// Albums, then EPs and singles.
    ArtistAlbums(u64),
    ArtistAppearsOn(u64),
    /// The artist's *All tracks* (`Credits for <artist>`).
    Credits(u64),
    /// A search's tracks, albums, artists and playlists, by query (spec
    /// 0007).
    SearchTracks(String),
    SearchAlbums(String),
    SearchArtists(String),
    SearchPlaylists(String),
    /// A browse section's next page, by its `dataApiPath` (spec 0024).
    BrowseData(String),
    /// A browse section's whole list, by its `viewAll` path (spec 0024).
    BrowseViewAll(String),
}

/// A page to open.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PageRequest {
    Library,
    FavoriteTracks,
    Album(u64),
    Playlist(String),
    Artist(u64),
    /// A search, by its trimmed query (spec 0007).
    Search(String),
    /// The user's mixes (spec 0011).
    Mixes,
    /// A mix, by Tidal's mix ID (spec 0011).
    Mix(String),
    /// A track's radio, by track ID (spec 0011).
    TrackRadio(u64),
    /// An artist's radio, by artist ID (spec 0011).
    ArtistRadio(u64),
    /// A track's lyrics, by track ID (spec 0021).
    Lyrics(u64),
    /// A browse page (spec 0024).
    Browse(BrowseRef),
}

/// What a browse page shows (spec 0024 "Talking to the player").
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BrowseRef {
    /// The root page, from `pages/explore`.
    Root,
    /// The personal home, from the v2 feed.
    Home,
    /// A Tidal page, by its `pages/…` path.
    Page(String),
}

/// A row that opens a browse page (spec 0024).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageLink {
    pub title: String,
    pub target: BrowseRef,
}

/// One row of a browse section of several kinds (spec 0024): the owned
/// form of the UI's `Row`. Not [`crate::Item`], which names a track, album
/// or playlist by ID.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MixedRow {
    Track(Track),
    Album(AlbumSummary),
    Playlist(PlaylistSummary),
    Artist(ArtistRef),
    Mix(MixSummary),
}

/// How a browse section gets the rest of its rows (spec 0024).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BrowseMore {
    /// It is whole.
    None,
    /// Paged, from this `dataApiPath` ([`ListRef::BrowseData`]).
    Data(String),
    /// One fetch, from this `viewAll` path ([`ListRef::BrowseViewAll`]),
    /// the first time its tab is shown.
    ViewAll(String),
}

/// One section of a browse page: a tab of its pane (spec 0024).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowseSection {
    pub title: String,
    pub rows: ListItems,
    pub more: BrowseMore,
}

/// A mix row and the mix page's header (spec 0011).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MixSummary {
    /// Tidal's mix ID (30 hex digits).
    pub id: String,
    pub title: String,
    /// Tidal's `subTitle`, when not empty.
    pub subtitle: Option<String>,
}

/// What a radio page is the radio of (spec 0011).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RadioSeed {
    Track(Track),
    Artist(ArtistRef),
}

/// Tidal's top hit of a search (spec 0007 decision 6): one item of any
/// kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TopHit {
    Track(Track),
    Album(AlbumSummary),
    Artist(ArtistRef),
    Playlist(PlaylistSummary),
}

/// A page's header and the first page of each of its lists (*All tracks*
/// excepted: fetched when first focused).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PageData {
    Library {
        playlists: ListPage<PlaylistSummary>,
        albums: ListPage<AlbumSummary>,
        artists: ListPage<ArtistRef>,
    },
    FavoriteTracks {
        tracks: ListPage<Track>,
    },
    Album {
        album: AlbumSummary,
        tracks: ListPage<Track>,
    },
    Playlist {
        playlist: PlaylistSummary,
        /// The `ETag` header of `GET /playlists/{uuid}`, sent back by
        /// [`LibraryRequest::RemoveFromPlaylist`].
        etag: Option<String>,
        tracks: ListPage<Track>,
    },
    Artist {
        artist: ArtistRef,
        top_tracks: ListPage<Track>,
        albums: ListPage<AlbumSummary>,
        appears_on: ListPage<AlbumSummary>,
    },
    /// A search's top hit and the first page of each result list.
    Search {
        /// Boxed: a track is large, and most pages carry none.
        top_hit: Option<Box<TopHit>>,
        tracks: ListPage<Track>,
        albums: ListPage<AlbumSummary>,
        artists: ListPage<ArtistRef>,
        playlists: ListPage<PlaylistSummary>,
    },
    /// The user's mixes, all of them (spec 0011: `total` is the rows kept).
    Mixes {
        mixes: ListPage<MixSummary>,
    },
    /// A mix and all its tracks.
    Mix {
        mix: MixSummary,
        tracks: ListPage<Track>,
    },
    /// A track's or an artist's radio, all its tracks.
    Radio {
        seed: RadioSeed,
        tracks: ListPage<Track>,
    },
    /// A browse page and its sections, in Tidal's order (spec 0024).
    Browse {
        title: String,
        sections: Vec<BrowseSection>,
    },
    /// A track's lyrics (spec 0021); `lyrics` is `None` when Tidal has
    /// none.
    Lyrics {
        track: u64,
        lyrics: Option<Lyrics>,
    },
}

/// What a favorite request is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FavoriteKind {
    Track,
    Album,
    Artist,
    Playlist,
}

/// A client's request about the library (spec 0006 "Talking to the
/// player"), sent as [`crate::protocol::ClientMessage::Library`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LibraryRequest {
    /// → [`LibraryResponse::Page`].
    Page(PageRequest),
    /// → [`LibraryResponse::Items`].
    More {
        list: ListRef,
        offset: u32,
        limit: u32,
    },
    /// → [`LibraryResponse::Favorite`]. `id` is the track, album or artist
    /// ID in decimal, or the playlist UUID.
    IsFavorite(FavoriteKind, String),
    /// → [`LibraryResponse::Done`]; `id` as for `IsFavorite`.
    AddFavorite(FavoriteKind, String),
    /// → [`LibraryResponse::Done`]; `id` as for `IsFavorite`.
    RemoveFavorite(FavoriteKind, String),
    /// Adds `tracks` at the end of an own playlist → [`LibraryResponse::Done`].
    AddToPlaylist {
        uuid: String,
        tracks: Vec<TrackId>,
        allow_duplicates: bool,
    },
    /// Removes the tracks at positions `indices` (0-based, as the list was
    /// loaded: one write, spec 0020), checked against the `etag` the
    /// client's list was loaded with → [`LibraryResponse::Done`].
    RemoveFromPlaylist {
        uuid: String,
        indices: Vec<u32>,
        etag: String,
    },
    /// Creates a private playlist → [`LibraryResponse::Created`].
    CreatePlaylist { title: String, description: String },
    /// Moves the track at position `from` to the final position `to`
    /// (spec 0022), checked against `etag` → [`LibraryResponse::Moved`].
    MovePlaylistItem {
        uuid: String,
        from: u32,
        to: u32,
        etag: String,
    },
    /// Changes the fields given (spec 0022) → [`LibraryResponse::Done`].
    EditPlaylist {
        uuid: String,
        title: Option<String>,
        description: Option<String>,
    },
    /// → [`LibraryResponse::Done`].
    SetPlaylistPublic { uuid: String, public: bool },
    /// → [`LibraryResponse::Done`].
    DeletePlaylist { uuid: String },
}

/// The player's answer to a [`LibraryRequest`], sent as
/// [`crate::protocol::ServerMessage::LibraryReply`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LibraryResponse {
    Page(PageData),
    Items(ListItems),
    Favorite(bool),
    Created(PlaylistSummary),
    /// A move's answer: the playlist's new ETag (spec 0022).
    Moved {
        etag: Option<String>,
    },
    Done,
}

/// The default hidden-version words of *All tracks* (spec 0006 "The
/// artist's *All tracks*").
pub const DEFAULT_HIDDEN_VERSIONS: &[&str] = &[
    "instrumental",
    "inst",
    "off vocal",
    "karaoke",
    "tv version",
    "tv ver",
    "tv size",
    "tv edit",
    "sped up",
    "speed up",
    "nightcore",
    "slowed",
    "slowed + reverb",
    "reverb",
    "8d",
    "8d audio",
];

/// Whether `track` is an alternate version to leave out of *All tracks*
/// (spec 0006 AC3): its `version`, a bracketed part (`(…)`, `[…]`) of its
/// title or a ` - …` suffix of its title equals or starts with one of
/// `words`, both normalised (lower-cased, without spaces, hyphens,
/// underscores, dots and `+`). An empty list hides nothing; a
/// word that normalises to nothing matches nothing.
pub fn hidden_version(track: &Track, words: &[String]) -> bool {
    let words: Vec<String> = words
        .iter()
        .map(|w| normalise(w))
        .filter(|w| !w.is_empty())
        .collect();
    if words.is_empty() {
        return false;
    }
    version_parts(track)
        .map(normalise)
        .filter(|part| !part.is_empty())
        .any(|part| words.iter().any(|w| part.starts_with(w.as_str())))
}

/// Lower-cased, without whitespace, hyphens, underscores, dots and `+`.
fn normalise(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .filter(|c| !c.is_whitespace() && !matches!(c, '-' | '_' | '.' | '+'))
        .collect()
}

/// The parts of a track that may name its version: `version`, each
/// bracketed part of the title, and what follows each ` - ` in the title.
fn version_parts(track: &Track) -> impl Iterator<Item = &str> {
    let title = track.title.as_str();
    let brackets = [('(', ')'), ('[', ']')]
        .into_iter()
        .flat_map(move |(open, close)| {
            title
                .split(open)
                .skip(1)
                .filter_map(move |rest| rest.split_once(close).map(|(inside, _)| inside))
        });
    let suffixes = title.split(" - ").skip(1);
    track
        .version
        .as_deref()
        .into_iter()
        .chain(brackets)
        .chain(suffixes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::track::TrackId;

    fn track(title: &str, version: Option<&str>) -> Track {
        Track {
            id: TrackId(1),
            title: title.into(),
            version: version.map(Into::into),
            artists: vec![],
            album: None,
            duration: None,
            streamable: true,
        }
    }

    fn words(list: &[&str]) -> Vec<String> {
        list.iter().map(|w| (*w).to_owned()).collect()
    }

    /// AC3: hidden or kept, per the spec's table; default words unless a
    /// row names its own.
    #[test]
    fn ac3_hidden_version() {
        let defaults = words(DEFAULT_HIDDEN_VERSIONS);
        let mut extended = defaults.clone();
        extended.push("slowed & reverb".into());
        let only_ampersand = words(&["slowed & reverb"]);
        let none: Vec<String> = vec![];
        let blank = words(&[" + "]);
        let rows: Vec<(&str, Option<&str>, &[String], bool)> = vec![
            // Hidden by `version`.
            ("X", Some("Instrumental"), &defaults, true),
            ("X", Some("TV Size"), &defaults, true),
            ("X", Some("TV-size ver."), &defaults, true),
            ("X", Some("TV Size ver."), &defaults, true),
            ("X", Some("Sped Up"), &defaults, true),
            ("X", Some("Slowed + Reverb"), &defaults, true),
            ("X", Some("Off Vocal"), &defaults, true),
            ("X", Some("Nightcore"), &defaults, true),
            // Hidden by the title.
            ("X (Instrumental)", None, &defaults, true),
            ("X [TV Size]", None, &defaults, true),
            ("X - Sped Up Version", None, &defaults, true),
            ("X (feat. Y) [Instrumental]", None, &defaults, true),
            ("X - Off Vocal", None, &defaults, true),
            ("X (slowed & reverb)", None, &extended, true),
            ("X (slowed & reverb)", None, &only_ampersand, true),
            // Kept.
            ("X", Some("Acoustic"), &defaults, false),
            ("X", Some("Live"), &defaults, false),
            ("X", Some("Remix"), &defaults, false),
            ("X", Some("Tiësto Remix"), &defaults, false),
            ("X", Some("Remastered 2011"), &defaults, false),
            ("Instrumentally Yours", None, &defaults, false),
            ("X (feat. Y)", None, &defaults, false),
            ("Rock-a-bye", None, &defaults, false),
            ("X", None, &defaults, false),
            ("X (reverb)", None, &only_ampersand, false),
            // An empty word list hides nothing.
            ("X (Instrumental)", Some("Instrumental"), &none, false),
            // A word that normalises to nothing matches nothing.
            ("X", Some("Live"), &blank, false),
        ];
        for (title, version, list, want) in rows {
            assert_eq!(
                hidden_version(&track(title, version), list),
                want,
                "{title:?} version {version:?} words {list:?}"
            );
        }
    }
}
