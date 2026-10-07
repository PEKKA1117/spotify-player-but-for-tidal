//! Pages, their windows and rows (spec 0006 "Pages", "Lists load as you
//! scroll", "Lists and the cursor"): pure data and the text a renderer
//! shows for it (titles, empty-list messages). No key handling here.

use std::time::Duration;

use crate::item::Item;
use crate::library::{
    AlbumSummary, CreditedTrack, ListItems, ListPage, ListRef, PageData, PageRequest,
    PlaylistSummary, RoleCategory,
};
use crate::track::{ArtistRef, Track};

/// The history keeps at most this many pages above the queue (the queue at
/// the bottom is never dropped).
pub const MAX_HISTORY: usize = 50;
/// `Enter` on a track refuses a list longer than this (spec 0006 "The
/// whole list is needed").
pub const MAX_WHOLE_LIST: u32 = 40_000;
/// The default page size of every list request (spec 0006 "Page size").
pub const DEFAULT_PAGE_SIZE: u32 = 100;
/// The role categories of the *All tracks* filter, in the popup's order.
pub const ROLE_CATEGORIES: [RoleCategory; 4] = [
    RoleCategory::Performer,
    RoleCategory::Songwriter,
    RoleCategory::Producer,
    RoleCategory::Engineer,
];

/// Which page: the queue or one of the library's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PageKind {
    Queue,
    Library,
    FavoriteTracks,
    Album(u64),
    Playlist(String),
    Artist(u64),
}

impl PageKind {
    /// The request that fetches this page; `None` for the queue (it comes
    /// with the player's snapshots).
    pub fn request(&self) -> Option<PageRequest> {
        Some(match self {
            Self::Queue => return None,
            Self::Library => PageRequest::Library,
            Self::FavoriteTracks => PageRequest::FavoriteTracks,
            Self::Album(id) => PageRequest::Album(*id),
            Self::Playlist(uuid) => PageRequest::Playlist(uuid.clone()),
            Self::Artist(id) => PageRequest::Artist(*id),
        })
    }
}

/// A fetch's state: of a whole page (`Page::load`) or of a window's next
/// list page (`Window::load`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Load {
    #[default]
    Idle,
    /// Waiting for the reply with this request ID.
    Loading { id: u64 },
    /// The fetch failed: the message to show.
    Failed(String),
}

/// What a page's header holds, once fetched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Header {
    Album(AlbumSummary),
    Playlist {
        playlist: PlaylistSummary,
        /// Sent back with a removal (spec 0006 "Playlist edits and the
        /// ETag").
        etag: Option<String>,
    },
    Artist(ArtistRef),
}

/// Which list a window shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowKind {
    /// The library's own and favorite playlists.
    Playlists,
    /// The library's favorite albums.
    Albums,
    /// The library's favorite artists.
    Artists,
    FavoriteTracks,
    AlbumTracks,
    PlaylistTracks,
    TopTracks,
    /// The artist's albums, then EPs and singles.
    ArtistAlbums,
    AppearsOn,
    /// The artist's credits (*All tracks*).
    AllTracks,
}

impl WindowKind {
    /// The window's name in its title.
    pub fn name(self) -> &'static str {
        match self {
            Self::Playlists => "Playlists",
            Self::Albums | Self::ArtistAlbums => "Albums",
            Self::Artists => "Artists",
            Self::FavoriteTracks => "Favorite tracks",
            Self::AlbumTracks | Self::PlaylistTracks => "Tracks",
            Self::TopTracks => "Top tracks",
            Self::AppearsOn => "Appears on",
            Self::AllTracks => "All tracks",
        }
    }

    /// What an empty window says (spec 0006 "Pages").
    pub fn empty_message(self) -> &'static str {
        match self {
            Self::Playlists => "No playlists yet",
            Self::Albums => "No favorite albums yet",
            Self::Artists => "No favorite artists yet",
            Self::FavoriteTracks => "No favorite tracks yet",
            Self::AlbumTracks => "This album has no tracks",
            Self::PlaylistTracks => "This playlist has no tracks",
            Self::TopTracks => "No top tracks",
            Self::ArtistAlbums | Self::AppearsOn => "No albums",
            Self::AllTracks => "No credits",
        }
    }
}

/// A window's loaded rows, in the order Tidal returned them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rows {
    Tracks(Vec<Track>),
    Albums(Vec<AlbumSummary>),
    Playlists(Vec<PlaylistSummary>),
    Artists(Vec<ArtistRef>),
    Credits(Vec<CreditedTrack>),
}

impl Rows {
    /// The number of loaded rows.
    pub fn len(&self) -> usize {
        match self {
            Self::Tracks(v) => v.len(),
            Self::Albums(v) => v.len(),
            Self::Playlists(v) => v.len(),
            Self::Artists(v) => v.len(),
            Self::Credits(v) => v.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Row `index`.
    pub fn get(&self, index: usize) -> Option<Row<'_>> {
        Some(match self {
            Self::Tracks(v) => Row::Track(v.get(index)?),
            Self::Albums(v) => Row::Album(v.get(index)?),
            Self::Playlists(v) => Row::Playlist(v.get(index)?),
            Self::Artists(v) => Row::Artist(v.get(index)?),
            Self::Credits(v) => Row::Credit(v.get(index)?),
        })
    }

    /// An empty list of the same row type.
    fn emptied(&self) -> Self {
        match self {
            Self::Tracks(_) => Self::Tracks(Vec::new()),
            Self::Albums(_) => Self::Albums(Vec::new()),
            Self::Playlists(_) => Self::Playlists(Vec::new()),
            Self::Artists(_) => Self::Artists(Vec::new()),
            Self::Credits(_) => Self::Credits(Vec::new()),
        }
    }
}

/// One row, borrowed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row<'a> {
    Track(&'a Track),
    Credit(&'a CreditedTrack),
    Album(&'a AlbumSummary),
    Playlist(&'a PlaylistSummary),
    Artist(&'a ArtistRef),
}

impl<'a> Row<'a> {
    /// The track of a track or credit row.
    pub fn track(self) -> Option<&'a Track> {
        match self {
            Self::Track(t) => Some(t),
            Self::Credit(c) => Some(&c.track),
            _ => None,
        }
    }

    /// What `Z` and the popup's queue actions send for this row; `None`
    /// for an artist.
    pub fn item(self) -> Option<Item> {
        match self {
            Self::Track(t) => Some(Item::Track(t.id)),
            Self::Credit(c) => Some(Item::Track(c.track.id)),
            Self::Album(a) => Some(Item::Album(a.id)),
            Self::Playlist(p) => Some(Item::Playlist(p.uuid.clone())),
            Self::Artist(_) => None,
        }
    }

    /// The page `Enter` opens on this row; `None` for a track.
    pub fn page(self) -> Option<PageKind> {
        match self {
            Self::Track(_) | Self::Credit(_) => None,
            Self::Album(a) => Some(PageKind::Album(a.id)),
            Self::Playlist(p) => Some(PageKind::Playlist(p.uuid.clone())),
            Self::Artist(a) => Some(PageKind::Artist(a.id)),
        }
    }

    /// The ID that makes a row "already loaded" (spec 0006: track ID,
    /// album ID, artist ID or playlist UUID).
    fn key(self) -> RowKey {
        match self {
            Self::Track(t) => RowKey::Id(t.id.0),
            Self::Credit(c) => RowKey::Id(c.track.id.0),
            Self::Album(a) => RowKey::Id(a.id),
            Self::Artist(a) => RowKey::Id(a.id),
            Self::Playlist(p) => RowKey::Uuid(p.uuid.clone()),
        }
    }
}

#[derive(PartialEq, Eq, Hash)]
enum RowKey {
    Id(u64),
    Uuid(String),
}

/// One focusable list of a page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Window {
    pub kind: WindowKind,
    /// The list `More` asks for.
    pub list: ListRef,
    /// The loaded rows.
    pub rows: Rows,
    /// Each loaded row's position in Tidal's list (`offset` + index in its
    /// page): a playlist removal is addressed by it.
    pub positions: Vec<u32>,
    /// Tidal's total, known from the first page; `None` before it.
    pub total: Option<u32>,
    /// Rows the player left out (*All tracks*: Atmos-only copies and
    /// alternate versions), summed over the loaded pages.
    pub hidden: u32,
    /// The `offset` of the next `More`: advanced by the page size asked,
    /// not by the rows received.
    pub next_offset: u32,
    /// The cursor, by position among the visible rows.
    pub cursor: usize,
    /// The next list page's fetch.
    pub load: Load,
    /// The role filter (*All tracks* only), checked per
    /// [`ROLE_CATEGORIES`]; all checked by default.
    pub roles: [bool; 4],
}

impl Window {
    /// An empty window of `kind` over `list`.
    pub fn new(kind: WindowKind, list: ListRef) -> Self {
        let rows = match kind {
            WindowKind::Playlists => Rows::Playlists(Vec::new()),
            WindowKind::Albums | WindowKind::ArtistAlbums | WindowKind::AppearsOn => {
                Rows::Albums(Vec::new())
            }
            WindowKind::Artists => Rows::Artists(Vec::new()),
            WindowKind::AllTracks => Rows::Credits(Vec::new()),
            WindowKind::FavoriteTracks
            | WindowKind::AlbumTracks
            | WindowKind::PlaylistTracks
            | WindowKind::TopTracks => Rows::Tracks(Vec::new()),
        };
        Self {
            kind,
            list,
            rows,
            positions: Vec::new(),
            total: None,
            hidden: 0,
            next_offset: 0,
            cursor: 0,
            load: Load::Idle,
            roles: [true; 4],
        }
    }

    /// Whether the role filter hides anything.
    pub fn filtered(&self) -> bool {
        self.roles.iter().any(|checked| !checked)
    }

    /// The indices (into `rows`) of the rows shown: all of them, or in
    /// *All tracks* those with a checked role category.
    pub fn visible(&self) -> Vec<usize> {
        match &self.rows {
            Rows::Credits(credits) if self.filtered() => credits
                .iter()
                .enumerate()
                .filter(|(_, c)| {
                    c.roles.iter().any(|role| {
                        ROLE_CATEGORIES
                            .iter()
                            .zip(self.roles)
                            .any(|(category, checked)| checked && category == role)
                    })
                })
                .map(|(i, _)| i)
                .collect(),
            rows => (0..rows.len()).collect(),
        }
    }

    /// The number of visible rows.
    pub fn len(&self) -> usize {
        if self.filtered() {
            self.visible().len()
        } else {
            self.rows.len()
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Visible row `index`.
    pub fn row(&self, index: usize) -> Option<Row<'_>> {
        if self.filtered() {
            self.rows.get(*self.visible().get(index)?)
        } else {
            self.rows.get(index)
        }
    }

    /// The row under the cursor.
    pub fn selected(&self) -> Option<Row<'_>> {
        self.row(self.cursor)
    }

    /// The visible tracks, in list order (a track or credits window).
    pub fn tracks(&self) -> Vec<Track> {
        self.visible()
            .into_iter()
            .filter_map(|i| self.rows.get(i)?.track().cloned())
            .collect()
    }

    /// Whether every page of the list has been asked for and answered.
    pub fn complete(&self) -> bool {
        self.total.is_some_and(|total| self.next_offset >= total)
    }

    /// The window's title: its name and Tidal's total, and for *All
    /// tracks* the role filter and the hidden count (`All tracks (548 ·
    /// Performer, Songwriter · 37 hidden)`).
    pub fn title(&self) -> String {
        let name = self.kind.name();
        let Some(total) = self.total else {
            return name.to_owned();
        };
        let mut parts = vec![group(total)];
        if self.filtered() {
            let checked: Vec<String> = ROLE_CATEGORIES
                .iter()
                .zip(self.roles)
                .filter(|(_, checked)| *checked)
                .map(|(category, _)| format!("{category:?}"))
                .collect();
            parts.push(checked.join(", "));
        }
        if self.hidden > 0 {
            parts.push(format!("{} hidden", group(self.hidden)));
        }
        format!("{name} ({})", parts.join(" · "))
    }

    /// What the window says when it has no visible rows: its empty-list
    /// message, with the hidden count when rows were hidden (`No credits
    /// (37 hidden)`).
    pub fn empty_message(&self) -> String {
        let message = self.kind.empty_message();
        if self.hidden > 0 {
            format!("{message} ({} hidden)", group(self.hidden))
        } else {
            message.to_owned()
        }
    }

    /// Forgets the loaded rows (before a page is fetched again); the
    /// cursor and the role filter stay.
    pub(super) fn reset(&mut self) {
        self.rows = self.rows.emptied();
        self.positions.clear();
        self.total = None;
        self.hidden = 0;
        self.next_offset = 0;
        self.load = Load::Idle;
    }

    /// Appends one list page asked with `limit`: rows already loaded are
    /// skipped; `next_offset` advances by what was asked. A page of
    /// another row type is ignored.
    pub(super) fn append(&mut self, items: ListItems, limit: u32) {
        fn add<T: Clone>(
            rows: &mut Vec<T>,
            positions: &mut Vec<u32>,
            page: &ListPage<T>,
            key: impl Fn(&T) -> RowKey,
        ) {
            let mut known: std::collections::HashSet<RowKey> = rows.iter().map(&key).collect();
            for (i, item) in page.items.iter().enumerate() {
                if known.insert(key(item)) {
                    rows.push(item.clone());
                    positions.push(page.offset + i as u32);
                }
            }
        }
        let (offset, total, hidden) = match (&mut self.rows, &items) {
            (Rows::Tracks(rows), ListItems::Tracks(page)) => {
                add(rows, &mut self.positions, page, |t| Row::Track(t).key());
                (page.offset, page.total, page.hidden)
            }
            (Rows::Albums(rows), ListItems::Albums(page)) => {
                add(rows, &mut self.positions, page, |a| Row::Album(a).key());
                (page.offset, page.total, page.hidden)
            }
            (Rows::Playlists(rows), ListItems::Playlists(page)) => {
                add(rows, &mut self.positions, page, |p| Row::Playlist(p).key());
                (page.offset, page.total, page.hidden)
            }
            (Rows::Artists(rows), ListItems::Artists(page)) => {
                add(rows, &mut self.positions, page, |a| Row::Artist(a).key());
                (page.offset, page.total, page.hidden)
            }
            (Rows::Credits(rows), ListItems::Credits(page)) => {
                add(rows, &mut self.positions, page, |c| Row::Credit(c).key());
                (page.offset, page.total, page.hidden)
            }
            _ => return,
        };
        self.total = Some(total);
        self.hidden += hidden;
        self.next_offset = self.next_offset.max(offset.saturating_add(limit));
        self.load = Load::Idle;
        self.cursor = self.cursor.min(self.len().saturating_sub(1));
    }
}

/// The largest page each list's endpoint accepts (spec 0006 "What the
/// player fetches"): a request asks `min(page size, this)`.
pub fn largest_page(list: &ListRef) -> u32 {
    match list {
        ListRef::Playlists | ListRef::Credits(_) => 50,
        ListRef::PlaylistTracks(_) => 100,
        _ => 1000,
    }
}

/// One page of the history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    pub kind: PageKind,
    /// The album, playlist or artist, once fetched.
    pub header: Option<Header>,
    /// The page's own fetch (header and first pages): while `Loading` its
    /// windows show `Loading…`, when `Failed` the page shows the message.
    pub load: Load,
    /// The windows, in `Tab` order; none for the queue (drawn from the
    /// snapshot).
    pub windows: Vec<Window>,
    /// The focused window's index.
    pub focus: usize,
}

impl Page {
    /// A page of `kind` with empty windows, not yet fetched.
    pub fn new(kind: PageKind) -> Self {
        let windows = match &kind {
            PageKind::Queue => vec![],
            PageKind::Library => vec![
                Window::new(WindowKind::Playlists, ListRef::Playlists),
                Window::new(WindowKind::Albums, ListRef::FavoriteAlbums),
                Window::new(WindowKind::Artists, ListRef::FavoriteArtists),
            ],
            PageKind::FavoriteTracks => vec![Window::new(
                WindowKind::FavoriteTracks,
                ListRef::FavoriteTracks,
            )],
            PageKind::Album(id) => vec![Window::new(
                WindowKind::AlbumTracks,
                ListRef::AlbumTracks(*id),
            )],
            PageKind::Playlist(uuid) => vec![Window::new(
                WindowKind::PlaylistTracks,
                ListRef::PlaylistTracks(uuid.clone()),
            )],
            PageKind::Artist(id) => vec![
                Window::new(WindowKind::TopTracks, ListRef::TopTracks(*id)),
                Window::new(WindowKind::ArtistAlbums, ListRef::ArtistAlbums(*id)),
                Window::new(WindowKind::AppearsOn, ListRef::ArtistAppearsOn(*id)),
                Window::new(WindowKind::AllTracks, ListRef::Credits(*id)),
            ],
        };
        Self {
            kind,
            header: None,
            load: Load::Idle,
            windows,
            focus: 0,
        }
    }

    /// The focused window; `None` on the queue.
    pub fn focused(&self) -> Option<&Window> {
        self.windows.get(self.focus)
    }

    /// The window of `kind`, if the page has one.
    pub fn window(&self, kind: WindowKind) -> Option<&Window> {
        self.windows.iter().find(|w| w.kind == kind)
    }

    /// The playlist's ETag, on a playlist page.
    pub fn etag(&self) -> Option<&str> {
        match &self.header {
            Some(Header::Playlist { etag, .. }) => etag.as_deref(),
            _ => None,
        }
    }

    /// The page's own playlist, when it is the user's (`own`).
    pub fn own_playlist(&self) -> Option<&PlaylistSummary> {
        match &self.header {
            Some(Header::Playlist { playlist, .. }) if playlist.own => Some(playlist),
            _ => None,
        }
    }

    /// The title row (spec 0006 "Pages"): `Library`, `Favorite tracks ·
    /// 362 tracks`, `<title> · <artists> · <year> · 17 tracks · 1:02:15`,
    /// `<title> · 39 tracks · 2:41:07`, `<name>`; the parts not known yet
    /// are left out.
    pub fn title(&self) -> String {
        let total = self.windows.first().and_then(|w| w.total);
        let tracks = total.map(|n| match n {
            1 => "1 track".to_owned(),
            n => format!("{} tracks", group(n)),
        });
        let parts: Vec<String> = match (&self.kind, &self.header) {
            (PageKind::Queue, _) => vec!["Queue".into()],
            (PageKind::Library, _) => vec!["Library".into()],
            (PageKind::FavoriteTracks, _) => std::iter::once("Favorite tracks".into())
                .chain(tracks)
                .collect(),
            (_, Some(Header::Album(album))) => {
                let artists: Vec<&str> = album.artists.iter().map(|a| a.name.as_str()).collect();
                std::iter::once(album.title.clone())
                    .chain((!artists.is_empty()).then(|| artists.join(", ")))
                    .chain(album.year.map(|y| y.to_string()))
                    .chain(tracks.or(album.tracks.map(|n| format!("{n} tracks"))))
                    .chain(album.duration.map(clock))
                    .collect()
            }
            (_, Some(Header::Playlist { playlist, .. })) => std::iter::once(playlist.title.clone())
                .chain(tracks)
                .chain(playlist.duration.map(clock))
                .collect(),
            (_, Some(Header::Artist(artist))) => vec![artist.name.clone()],
            (PageKind::Album(_), None) => vec!["Album".into()],
            (PageKind::Playlist(_), None) => vec!["Playlist".into()],
            (PageKind::Artist(_), None) => vec!["Artist".into()],
        };
        parts.join(" · ")
    }

    /// Applies a fetched page: header and first list pages. Data of
    /// another page kind is ignored.
    pub(super) fn apply(&mut self, data: PageData, page_size: u32) {
        let lists: Vec<ListItems> = match (&self.kind, data) {
            (
                PageKind::Library,
                PageData::Library {
                    playlists,
                    albums,
                    artists,
                },
            ) => vec![
                ListItems::Playlists(playlists),
                ListItems::Albums(albums),
                ListItems::Artists(artists),
            ],
            (PageKind::FavoriteTracks, PageData::FavoriteTracks { tracks }) => {
                vec![ListItems::Tracks(tracks)]
            }
            (PageKind::Album(_), PageData::Album { album, tracks }) => {
                self.header = Some(Header::Album(album));
                vec![ListItems::Tracks(tracks)]
            }
            (
                PageKind::Playlist(_),
                PageData::Playlist {
                    playlist,
                    etag,
                    tracks,
                },
            ) => {
                self.header = Some(Header::Playlist { playlist, etag });
                vec![ListItems::Tracks(tracks)]
            }
            (
                PageKind::Artist(_),
                PageData::Artist {
                    artist,
                    top_tracks,
                    albums,
                    appears_on,
                },
            ) => {
                self.header = Some(Header::Artist(artist));
                vec![
                    ListItems::Tracks(top_tracks),
                    ListItems::Albums(albums),
                    ListItems::Albums(appears_on),
                ]
            }
            _ => return,
        };
        self.load = Load::Idle;
        for window in &mut self.windows {
            window.reset();
        }
        for (window, items) in self.windows.iter_mut().zip(lists) {
            let limit = page_size.min(largest_page(&window.list));
            window.append(items, limit);
        }
    }
}

/// `m:ss`, or `h:mm:ss` from an hour.
pub fn clock(duration: Duration) -> String {
    let secs = duration.as_secs();
    let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// A count with a space between thousands (`1 234`), as the spec writes
/// them.
pub fn group(n: u32) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(' ');
        }
        out.push(c);
    }
    out
}
