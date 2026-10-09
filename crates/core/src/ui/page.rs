//! Pages, their windows and rows (spec 0006 "Pages", "Lists load as you
//! scroll", "Lists and the cursor"): pure data and the text a renderer
//! shows for it (titles, empty-list messages). No key handling here.

use std::time::Duration;

use crate::item::Item;
use crate::library::{
    AlbumSummary, CreditedTrack, ListItems, ListPage, ListRef, PageData, PageRequest,
    PlaylistSummary, RoleCategory, TopHit,
};
use crate::track::{ArtistRef, Track};

use super::search::{Search, SearchFocus};

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
    /// The search page (spec 0007) and the query it last sent; empty
    /// until the first search.
    Search(String),
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
            Self::Search(query) if query.is_empty() => return None,
            Self::Search(query) => PageRequest::Search(query.clone()),
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
    /// The search page's result lists (spec 0007).
    SearchTracks,
    SearchAlbums,
    SearchArtists,
    SearchPlaylists,
}

impl WindowKind {
    /// The window's name in its title.
    pub fn name(self) -> &'static str {
        match self {
            Self::Playlists | Self::SearchPlaylists => "Playlists",
            Self::Albums | Self::ArtistAlbums | Self::SearchAlbums => "Albums",
            Self::Artists | Self::SearchArtists => "Artists",
            Self::FavoriteTracks => "Favorite tracks",
            Self::AlbumTracks | Self::PlaylistTracks | Self::SearchTracks => "Tracks",
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
            Self::SearchTracks => "No tracks found",
            Self::SearchAlbums => "No albums found",
            Self::SearchArtists => "No artists found",
            Self::SearchPlaylists => "No playlists found",
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
    /// The row a search's top hit acts as (spec 0007 decision 6).
    pub fn top_hit(hit: &'a TopHit) -> Self {
        match hit {
            TopHit::Track(t) => Self::Track(t),
            TopHit::Album(a) => Self::Album(a),
            TopHit::Artist(a) => Self::Artist(a),
            TopHit::Playlist(p) => Self::Playlist(p),
        }
    }

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
            WindowKind::Playlists | WindowKind::SearchPlaylists => Rows::Playlists(Vec::new()),
            WindowKind::Albums
            | WindowKind::ArtistAlbums
            | WindowKind::AppearsOn
            | WindowKind::SearchAlbums => Rows::Albums(Vec::new()),
            WindowKind::Artists | WindowKind::SearchArtists => Rows::Artists(Vec::new()),
            WindowKind::AllTracks => Rows::Credits(Vec::new()),
            WindowKind::FavoriteTracks
            | WindowKind::AlbumTracks
            | WindowKind::PlaylistTracks
            | WindowKind::TopTracks
            | WindowKind::SearchTracks => Rows::Tracks(Vec::new()),
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
    /// skipped (by ID; for a playlist's tracks by position, as a playlist
    /// can hold a track twice: spec 0006 "Bugs"); `next_offset` advances by
    /// what was asked. A page of another row type is ignored.
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
        fn add_by_position<T: Clone>(
            rows: &mut Vec<T>,
            positions: &mut Vec<u32>,
            page: &ListPage<T>,
        ) {
            for (i, item) in page.items.iter().enumerate() {
                let position = page.offset + i as u32;
                if !positions.contains(&position) {
                    rows.push(item.clone());
                    positions.push(position);
                }
            }
        }
        let by_position = matches!(self.list, ListRef::PlaylistTracks(_));
        let (offset, total, hidden) = match (&mut self.rows, &items) {
            (Rows::Tracks(rows), ListItems::Tracks(page)) if by_position => {
                add_by_position(rows, &mut self.positions, page);
                (page.offset, page.total, page.hidden)
            }
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

/// Whether `list` is one of a search's (sized by the search page size).
pub fn is_search_list(list: &ListRef) -> bool {
    matches!(
        list,
        ListRef::SearchTracks(_)
            | ListRef::SearchAlbums(_)
            | ListRef::SearchArtists(_)
            | ListRef::SearchPlaylists(_)
    )
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
    /// The panes, in `Tab` order: the window indices of each pane's tabs.
    /// Every page but the artist's has one pane per window.
    pub panes: Vec<Vec<usize>>,
    /// Each pane's active tab (an index into its `panes` entry).
    pub tabs: Vec<usize>,
    /// The search page's input, top hit and focus (spec 0007); `None` on
    /// every other page.
    pub search: Option<Search>,
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
            PageKind::Search(query) => vec![
                Window::new(
                    WindowKind::SearchTracks,
                    ListRef::SearchTracks(query.clone()),
                ),
                Window::new(
                    WindowKind::SearchAlbums,
                    ListRef::SearchAlbums(query.clone()),
                ),
                Window::new(
                    WindowKind::SearchArtists,
                    ListRef::SearchArtists(query.clone()),
                ),
                Window::new(
                    WindowKind::SearchPlaylists,
                    ListRef::SearchPlaylists(query.clone()),
                ),
            ],
        };
        // The artist page: Top tracks | All tracks, Albums | Appears on.
        let panes: Vec<Vec<usize>> = match &kind {
            PageKind::Artist(_) => vec![vec![0, 3], vec![1, 2]],
            _ => (0..windows.len()).map(|i| vec![i]).collect(),
        };
        let tabs = vec![0; panes.len()];
        let search = matches!(kind, PageKind::Search(_)).then(Search::default);
        Self {
            search,
            kind,
            header: None,
            load: Load::Idle,
            focus: 0,
            panes,
            tabs,
            windows,
        }
    }

    /// The focused pane.
    pub fn focused_pane(&self) -> usize {
        self.panes
            .iter()
            .position(|pane| pane.contains(&self.focus))
            .unwrap_or(0)
    }

    /// Focuses the next or previous pane, on its active tab, wrapping.
    pub fn cycle_pane(&mut self, forward: bool) {
        let count = self.panes.len();
        if count < 2 {
            return;
        }
        let at = self.focused_pane();
        let next = if forward {
            (at + 1) % count
        } else {
            (at + count - 1) % count
        };
        self.focus = self.panes[next][self.tabs[next]];
    }

    /// Shows the next or previous tab of the focused pane, wrapping.
    pub fn cycle_tab(&mut self, forward: bool) {
        let Some(at) = self.panes.iter().position(|p| p.contains(&self.focus)) else {
            return;
        };
        let count = self.panes[at].len();
        if count < 2 {
            return;
        }
        self.tabs[at] = if forward {
            (self.tabs[at] + 1) % count
        } else {
            (self.tabs[at] + count - 1) % count
        };
        self.focus = self.panes[at][self.tabs[at]];
    }

    /// The focused window; `None` on the queue, and on a search page
    /// while its input or top hit has the focus.
    pub fn focused(&self) -> Option<&Window> {
        if !self.windows_focused() {
            return None;
        }
        self.windows.get(self.focus)
    }

    /// Whether a window has the focus (always, but on a search page).
    pub fn windows_focused(&self) -> bool {
        self.search
            .as_ref()
            .is_none_or(|s| s.focus == SearchFocus::Windows)
    }

    /// The row under the focus on a loaded page: the focused window's row
    /// under its cursor, or a search's top hit.
    pub fn selected(&self) -> Option<Row<'_>> {
        if self.load != Load::Idle {
            return None;
        }
        if let Some(search) = &self.search
            && search.focus == SearchFocus::TopHit
        {
            return search.top_hit.as_ref().map(Row::top_hit);
        }
        self.focused()?.selected()
    }

    /// What a search's reply focuses (spec 0007 "Sending"): the top hit,
    /// else the first non-empty window; `None` when there is no result
    /// (or the page is loading or failed).
    pub fn result_focus(&self) -> Option<(SearchFocus, usize)> {
        let search = self.search.as_ref()?;
        if self.load != Load::Idle {
            return None;
        }
        if search.top_hit.is_some() {
            return Some((SearchFocus::TopHit, self.focus));
        }
        let window = self.windows.iter().position(|w| !w.rows.is_empty())?;
        Some((SearchFocus::Windows, window))
    }

    /// Gives the focus to `focus` (`window` when it is a window).
    pub(super) fn focus_on(&mut self, focus: SearchFocus, window: usize) {
        if let Some(search) = self.search.as_mut() {
            search.focus = focus;
        }
        self.focus = window;
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
            (PageKind::Search(query), _) if query.is_empty() => vec!["Search".into()],
            (PageKind::Search(query), _) => vec!["Search".into(), format!("\"{query}\"")],
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
            (
                PageKind::Search(_),
                PageData::Search {
                    top_hit,
                    tracks,
                    albums,
                    artists,
                    playlists,
                },
            ) => {
                if let Some(search) = self.search.as_mut() {
                    search.top_hit = top_hit.map(|hit| *hit);
                }
                vec![
                    ListItems::Tracks(tracks),
                    ListItems::Albums(albums),
                    ListItems::Artists(artists),
                    ListItems::Playlists(playlists),
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
        // A search focuses its result, else its input (spec 0007).
        if self.search.is_some() {
            let (focus, window) = self
                .result_focus()
                .unwrap_or((SearchFocus::Input, self.focus));
            self.focus_on(focus, window);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::track::{AlbumRef, TrackId};

    fn track(id: u64) -> Track {
        Track {
            id: TrackId(id),
            title: format!("T{id}"),
            version: None,
            artists: vec![ArtistRef {
                id: 20,
                name: "A".into(),
            }],
            album: Some(AlbumRef {
                id: 10,
                title: "Al".into(),
                cover: None,
            }),
            duration: None,
            streamable: true,
        }
    }

    fn page(offset: u32, ids: &[u64]) -> ListItems {
        ListItems::Tracks(ListPage {
            items: ids.iter().map(|&id| track(id)).collect(),
            offset,
            total: 3,
            hidden: 0,
        })
    }

    fn ids(window: &Window) -> Vec<u64> {
        match &window.rows {
            Rows::Tracks(rows) => rows.iter().map(|t| t.id.0).collect(),
            other => panic!("not tracks: {other:?}"),
        }
    }

    /// Spec 0006 "Bugs": a playlist keeps a track it holds twice (skipped
    /// by position); other lists still skip a repeated ID.
    #[test]
    fn ac10_playlist_duplicates_kept() {
        let mut playlist = Window::new(
            WindowKind::PlaylistTracks,
            ListRef::PlaylistTracks("p".into()),
        );
        playlist.append(page(0, &[7, 7]), 2);
        playlist.append(page(0, &[7, 7]), 2); // the same page again
        playlist.append(page(2, &[8]), 2);
        assert_eq!(ids(&playlist), vec![7, 7, 8]);
        assert_eq!(playlist.positions, vec![0, 1, 2]);

        let mut favorites = Window::new(WindowKind::FavoriteTracks, ListRef::FavoriteTracks);
        favorites.append(page(0, &[7, 7]), 2);
        favorites.append(page(2, &[7]), 2);
        assert_eq!(ids(&favorites), vec![7]);
    }
}
