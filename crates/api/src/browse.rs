//! Browse pages and the personal home feed (spec 0024). A child of
//! `library` (see its `mod browse`), so it reads the client's HTTP helpers
//! and DTOs without widening their visibility.
//!
//! Choices the spec leaves open:
//!
//! - **The v2 base** is the sibling of the v1 base (`../v2/…` against
//!   `…/v1`), so tests and production point both at one place.
//! - **The root page** is titled *Browse*; the home feed *Home*; a v1 page
//!   without a title takes the last segment of its path.
//! - **A link** whose `apiPath` is not a `pages/…` path is dropped: it
//!   could not be followed.
//! - **A page of a v1 list** is told apart by the shape of its items (the
//!   answer names no module type): a `apiPath` is a link, a `uuid` a
//!   playlist, a string `id` a mix, a `trackNumber` a track, a `cover` or
//!   `releaseDate` an album, a `name` without a `title` an artist. An empty
//!   page is an empty `Mixed` list: its kind is not known.
//! - **A feed page after the first** that fails ends the feed there: the
//!   sections already read are kept.

use reqwest::Method;
use serde::Deserialize;
use serde_json::Value;
use tidal_player_core::library::{
    AlbumSummary, BrowseMore, BrowseRef, BrowseSection, ListItems, ListPage, MixSummary, MixedRow,
    PageData, PageLink, PlaylistSummary,
};
use tidal_player_core::{ArtistRef, Track};

use super::{
    AlbumDto, LibraryClient, LibraryError, MixDto, PAGES_QUERY, PlaylistDto, RawPage, Result,
    check, clamp_total, parse, stereo_track,
};
use crate::ApiResponse;
use crate::metadata::{ArtistDto, REQUEST_TIMEOUT, TrackDto};

/// The v2 feed, relative to the v1 base.
const FEED_PATH: &str = "home/feed/static";
/// Requests to the feed, the first included (spec 0024 "Requests").
const FEED_REQUESTS: usize = 5;
/// The largest page of a `dataApiPath`.
const LARGEST_DATA: u32 = 50;
/// Sent as `x-tidal-client-version`: Tidal only checks it is there.
const CLIENT_VERSION: &str = env!("CARGO_PKG_VERSION");
/// What the v2 endpoints are asked besides `countryCode`.
const V2_QUERY: [(&str, &str); 3] = [
    ("deviceType", "BROWSER"),
    ("locale", "en_US"),
    ("platform", "WEB"),
];

/// Which kind of path [`browse_path`] accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Source {
    /// A v1 `pages/…` path.
    V1,
    /// A v2 `home/pages/…` path.
    V2,
}

/// The path itself if it is of the form `source` follows: the prefix, then
/// something, and no `..` or `.` segment, plain or percent-encoded (spec
/// 0024 AC8). The query, if any, is not checked: it is not a segment.
fn browse_path(path: &str, source: Source) -> Result<&str> {
    let prefix = match source {
        Source::V1 => "pages/",
        Source::V2 => "home/pages/",
    };
    let base = path.split('?').next().unwrap_or_default();
    let lower = base.to_ascii_lowercase();
    let ok = base.len() > prefix.len()
        && base.starts_with(prefix)
        && !base.contains('\\')
        && !["%2e", "%2f", "%5c"].iter().any(|e| lower.contains(e))
        && !base
            .split('/')
            .any(|segment| segment == ".." || segment == ".");
    if ok {
        Ok(path)
    } else {
        Err(LibraryError::NotAPage(path.to_owned()))
    }
}

impl LibraryClient {
    /// A browse page: the root, the home feed or a v1 page (spec 0024).
    pub(super) async fn browse_page(&self, target: &BrowseRef) -> Result<PageData> {
        match target {
            BrowseRef::Root => self.browse_root().await,
            BrowseRef::Home => self.browse_home().await,
            BrowseRef::Page(path) => {
                let path = browse_path(path, Source::V1)?;
                let page = self.v1_page(path).await?;
                let sections = page.sections().map(|(_, section)| section).collect();
                let title = page
                    .title
                    .filter(|t| !t.trim().is_empty())
                    .unwrap_or_else(|| path.rsplit('/').next().unwrap_or(path).to_owned());
                Ok(PageData::Browse { title, sections })
            }
        }
    }

    /// One more page of a v1 section, from its `dataApiPath`.
    pub(super) async fn browse_data(
        &self,
        data: &str,
        offset: u32,
        limit: u32,
    ) -> Result<ListItems> {
        let data = browse_path(data, Source::V1)?;
        let (limit, offset_text) = (limit.clamp(1, LARGEST_DATA).to_string(), offset.to_string());
        let mut query = PAGES_QUERY.to_vec();
        query.push(("limit", limit.as_str()));
        query.push(("offset", offset_text.as_str()));
        let response = self.fetch(data, &query).await?;
        let raw: RawPage<Value> = parse(&page_check(&response, data)?, "browse")?;
        let rows = raw.items.iter().filter_map(classify_v1).collect();
        Ok(assemble(rows, offset, Some(clamp_total(raw.total))))
    }

    /// A v2 section's whole list, asked with neither `limit` nor `offset`
    /// (the module may ignore them, spec 0024 "Facts").
    pub(super) async fn browse_view_all(&self, view_all: &str) -> Result<ListItems> {
        let view_all = browse_path(view_all, Source::V2)?;
        let response = self.fetch_v2(view_all, &[]).await?;
        let raw: ViewAll = parse(&page_check(&response, view_all)?, "browse")?;
        let rows = raw.items.iter().filter_map(v2_row).collect();
        Ok(assemble(rows, 0, None))
    }

    /// `GET /pages/explore` as four tabs, *Discover* first (spec 0024
    /// "The root page").
    async fn browse_root(&self) -> Result<PageData> {
        let page = self.v1_page("pages/explore").await?;
        let mut links = vec![
            link("Home", BrowseRef::Home),
            link("Staff picks", BrowseRef::Page("pages/home".into())),
            link("For you", BrowseRef::Page("pages/for_you".into())),
        ];
        let mut tabs = Vec::new();
        for (untitled, section) in page.sections() {
            match section.rows {
                ListItems::Links(list) if untitled => links.extend(list.items),
                _ => tabs.push(section),
            }
        }
        let discover = BrowseSection {
            title: "Discover".into(),
            rows: ListItems::Links(whole_page(links)),
            more: BrowseMore::None,
        };
        let mut sections = vec![discover];
        sections.extend(tabs);
        Ok(PageData::Browse {
            title: "Browse".into(),
            sections,
        })
    }

    /// `GET /v2/home/feed/static`, following `cursor` (spec 0024 AC6).
    async fn browse_home(&self) -> Result<PageData> {
        let mut sections = Vec::new();
        let mut cursor: Option<String> = None;
        for request in 0..FEED_REQUESTS {
            let extra: Vec<(&str, &str)> = cursor.iter().map(|c| ("cursor", c.as_str())).collect();
            let response = match self.fetch_v2(FEED_PATH, &extra).await {
                Ok(response) => response,
                Err(_) if request > 0 => break,
                Err(error) => return Err(error),
            };
            let status = response.status();
            if !status.is_success() {
                if request > 0 {
                    break;
                }
                // A `429` reads as anywhere else; the rest is Home's own.
                return Err(match check(&response, None) {
                    Err(error) if status.as_u16() == 429 => error,
                    _ => LibraryError::HomeUnavailable(status.as_u16()),
                });
            }
            let feed: Feed = match parse(&response, "home") {
                Ok(feed) => feed,
                Err(_) if request > 0 => break,
                Err(error) => return Err(error),
            };
            sections.extend(feed.items.into_iter().filter_map(FeedModule::section));
            cursor = feed.page.and_then(|p| p.cursor).filter(|c| !c.is_empty());
            if cursor.is_none() {
                break;
            }
        }
        Ok(PageData::Browse {
            title: "Home".into(),
            sections,
        })
    }

    /// A v1 page, its `404` the page's own not-found.
    async fn v1_page(&self, path: &str) -> Result<V1Page> {
        let response = self.fetch(path, &PAGES_QUERY).await?;
        parse(&page_check(&response, path)?, "browse")
    }

    /// `GET` against the v2 base, with `countryCode`, the v2 parameters and
    /// the client version header.
    async fn fetch_v2(&self, path: &str, extra: &[(&str, &str)]) -> Result<ApiResponse> {
        let (_, country) = self.auth.account().await;
        let mut query = vec![("countryCode", country.as_str())];
        query.extend_from_slice(&V2_QUERY);
        query.extend_from_slice(extra);
        Ok(self
            .auth
            .send(
                Method::GET,
                &format!("../v2/{path}"),
                &query,
                None,
                &[("x-tidal-client-version", CLIENT_VERSION)],
                Some(REQUEST_TIMEOUT),
            )
            .await?)
    }
}

/// A success as is; a `404` is the page's not-found; the rest as [`check`].
fn page_check(response: &ApiResponse, path: &str) -> Result<ApiResponse> {
    if response.status().as_u16() == 404 {
        return Err(LibraryError::PageNotFound(path.to_owned()));
    }
    check(response, None)
}

// Rows -------------------------------------------------------------------

/// What an item of a list is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Track,
    Album,
    Playlist,
    Artist,
    Mix,
    Link,
}

/// One row, of any kind.
enum Item {
    Track(Track),
    Album(AlbumSummary),
    Playlist(PlaylistSummary),
    Artist(ArtistRef),
    Mix(MixSummary),
    Link(PageLink),
}

/// The row of `kind` that `value` is, or none for an item that cannot be
/// read or is left out (spec 0024 "Which rows are kept").
fn read(kind: Kind, value: &Value) -> Option<Item> {
    match kind {
        Kind::Track => stereo_track(TrackDto::deserialize(value).ok()?).map(Item::Track),
        Kind::Album => Some(Item::Album(AlbumDto::deserialize(value).ok()?.into())),
        Kind::Playlist => {
            let dto = PlaylistDto::deserialize(value).ok()?;
            (dto.tracks != Some(0)).then(|| Item::Playlist(dto.into_summary(false)))
        }
        Kind::Artist => Some(Item::Artist(ArtistRef::from(
            ArtistDto::deserialize(value).ok()?,
        ))),
        Kind::Mix => {
            let dto = MixDto::deserialize(value).ok()?;
            (!dto.mix_type.contains("VIDEO") && !dto.id.is_empty()).then(|| Item::Mix(dto.into()))
        }
        Kind::Link => {
            let dto = LinkDto::deserialize(value).ok()?;
            let followable =
                !dto.title.trim().is_empty() && browse_path(&dto.api_path, Source::V1).is_ok();
            followable.then_some(Item::Link(PageLink {
                title: dto.title,
                target: BrowseRef::Page(dto.api_path),
            }))
        }
    }
}

/// A v1 item whose module is not known (a `dataApiPath` answer), by its
/// shape; an item of no known shape is dropped.
fn classify_v1(value: &Value) -> Option<Item> {
    let value = value.get("item").unwrap_or(value);
    let has = |key: &str| value.get(key).is_some_and(|v| !v.is_null());
    let kind = if has("apiPath") {
        Kind::Link
    } else if has("uuid") {
        Kind::Playlist
    } else if value.get("id").is_some_and(Value::is_string) {
        Kind::Mix
    } else if has("trackNumber") {
        Kind::Track
    } else if has("title") && (has("cover") || has("numberOfTracks") || has("releaseDate")) {
        Kind::Album
    } else if has("name") && !has("title") {
        Kind::Artist
    } else {
        return None;
    };
    read(kind, value)
}

/// A v2 item (`{type, data}`): only the five kinds, the rest dropped.
fn v2_row(item: &Value) -> Option<Item> {
    let data = item.get("data")?;
    match item.get("type")?.as_str()? {
        "TRACK" => read(Kind::Track, data),
        "ALBUM" => read(Kind::Album, data),
        "PLAYLIST" => read(Kind::Playlist, data),
        "ARTIST" => read(Kind::Artist, data),
        "MIX" => {
            let dto = V2Mix::deserialize(data).ok()?;
            let title = dto
                .title
                .and_then(|t| t.text)
                .filter(|t| !t.trim().is_empty())?;
            let kept = !dto.kind.contains("VIDEO") && !dto.id.is_empty();
            kept.then(|| {
                Item::Mix(MixSummary {
                    id: dto.id,
                    title,
                    subtitle: dto
                        .subtitle
                        .and_then(|s| s.text)
                        .filter(|s| !s.trim().is_empty()),
                })
            })
        }
        _ => None,
    }
}

fn link(title: &str, target: BrowseRef) -> PageLink {
    PageLink {
        title: title.into(),
        target,
    }
}

fn whole_page<T>(items: Vec<T>) -> ListPage<T> {
    let total = clamp_total(items.len() as u64);
    paged(items, 0, total)
}

fn paged<T>(items: Vec<T>, offset: u32, total: u32) -> ListPage<T> {
    ListPage {
        items,
        offset,
        total,
        hidden: 0,
        positions: Vec::new(),
    }
}

/// `rows` as the list of their kind, or `Mixed` when they hold several (or
/// none: the kind of an empty answer is not known). `total` is Tidal's, or
/// the rows kept when the list is whole.
fn assemble(rows: Vec<Item>, offset: u32, total: Option<u32>) -> ListItems {
    let total = total.unwrap_or_else(|| clamp_total(rows.len() as u64));
    fn pick<T>(
        rows: Vec<Item>,
        f: impl Fn(Item) -> Option<T>,
        offset: u32,
        total: u32,
    ) -> ListPage<T> {
        paged(rows.into_iter().filter_map(f).collect(), offset, total)
    }
    let same = rows
        .windows(2)
        .all(|w| std::mem::discriminant(&w[0]) == std::mem::discriminant(&w[1]));
    match rows.first() {
        Some(Item::Track(_)) if same => ListItems::Tracks(pick(
            rows,
            |i| {
                if let Item::Track(t) = i {
                    Some(t)
                } else {
                    None
                }
            },
            offset,
            total,
        )),
        Some(Item::Album(_)) if same => ListItems::Albums(pick(
            rows,
            |i| {
                if let Item::Album(a) = i {
                    Some(a)
                } else {
                    None
                }
            },
            offset,
            total,
        )),
        Some(Item::Playlist(_)) if same => ListItems::Playlists(pick(
            rows,
            |i| {
                if let Item::Playlist(p) = i {
                    Some(p)
                } else {
                    None
                }
            },
            offset,
            total,
        )),
        Some(Item::Artist(_)) if same => ListItems::Artists(pick(
            rows,
            |i| {
                if let Item::Artist(a) = i {
                    Some(a)
                } else {
                    None
                }
            },
            offset,
            total,
        )),
        Some(Item::Mix(_)) if same => ListItems::Mixes(pick(
            rows,
            |i| if let Item::Mix(m) = i { Some(m) } else { None },
            offset,
            total,
        )),
        Some(Item::Link(_)) if same => ListItems::Links(pick(
            rows,
            |i| if let Item::Link(l) = i { Some(l) } else { None },
            offset,
            total,
        )),
        _ => ListItems::Mixed(pick(
            rows,
            |i| match i {
                Item::Track(t) => Some(MixedRow::Track(t)),
                Item::Album(a) => Some(MixedRow::Album(a)),
                Item::Playlist(p) => Some(MixedRow::Playlist(p)),
                Item::Artist(a) => Some(MixedRow::Artist(a)),
                Item::Mix(m) => Some(MixedRow::Mix(m)),
                Item::Link(_) => None,
            },
            offset,
            total,
        )),
    }
}

/// The name of an untitled section: what it holds.
fn kind_name(rows: &ListItems) -> &'static str {
    match rows {
        ListItems::Tracks(_) => "Tracks",
        ListItems::Albums(_) => "Albums",
        ListItems::Playlists(_) => "Playlists",
        ListItems::Artists(_) => "Artists",
        ListItems::Mixes(_) => "Mixes",
        ListItems::Links(_) | ListItems::Mixed(_) | ListItems::Credits(_) => "More",
    }
}

fn titled(title: Option<String>, rows: &ListItems) -> String {
    title
        .map(|t| t.trim().to_owned())
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| kind_name(rows).to_owned())
}

// DTOs -------------------------------------------------------------------

/// `GET /pages/…`: modules are read one by one, so an unknown type or an
/// unreadable module never fails the page.
#[derive(Deserialize)]
struct V1Page {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    rows: Vec<V1Row>,
}

#[derive(Deserialize)]
struct V1Row {
    #[serde(default)]
    modules: Vec<Value>,
}

impl V1Page {
    /// Each module that becomes a section (spec 0024 "Which rows are
    /// kept"), with whether Tidal gave it no title.
    fn sections(&self) -> impl Iterator<Item = (bool, BrowseSection)> {
        self.rows
            .iter()
            .flat_map(|row| &row.modules)
            .filter_map(v1_section)
    }
}

/// A v1 module as a section; none for the modules that are skipped.
fn v1_section(module: &Value) -> Option<(bool, BrowseSection)> {
    let kind = match module.get("type")?.as_str()? {
        "TRACK_LIST" => Kind::Track,
        "ALBUM_LIST" => Kind::Album,
        "PLAYLIST_LIST" => Kind::Playlist,
        "ARTIST_LIST" => Kind::Artist,
        "MIX_LIST" => Kind::Mix,
        "PAGE_LINKS" | "PAGE_LINKS_CLOUD" => Kind::Link,
        _ => return None,
    };
    let list = V1List::deserialize(module.get("pagedList")?).ok()?;
    let rows: Vec<Item> = list.items.iter().filter_map(|v| read(kind, v)).collect();
    if rows.is_empty() {
        return None;
    }
    // Fewer rows than the total, and a path to ask for the rest.
    let path = list
        .data_api_path
        .filter(|p| browse_path(p, Source::V1).is_ok())
        .filter(|_| list.total > list.items.len() as u64);
    let rows = assemble(rows, 0, path.as_ref().map(|_| clamp_total(list.total)));
    let title = module
        .get("title")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let untitled = title.as_deref().is_none_or(|t| t.trim().is_empty());
    Some((
        untitled,
        BrowseSection {
            title: titled(title, &rows),
            rows,
            more: path.map_or(BrowseMore::None, BrowseMore::Data),
        },
    ))
}

#[derive(Deserialize)]
struct V1List {
    #[serde(default, rename = "dataApiPath")]
    data_api_path: Option<String>,
    #[serde(default, rename = "totalNumberOfItems")]
    total: u64,
    #[serde(default)]
    items: Vec<Value>,
}

#[derive(Deserialize)]
struct LinkDto {
    #[serde(default)]
    title: String,
    #[serde(rename = "apiPath")]
    api_path: String,
}

/// `GET /v2/home/feed/static`.
#[derive(Deserialize)]
struct Feed {
    #[serde(default)]
    items: Vec<FeedModule>,
    #[serde(default)]
    page: Option<FeedCursor>,
}

#[derive(Deserialize)]
struct FeedCursor {
    #[serde(default)]
    cursor: Option<String>,
}

#[derive(Deserialize)]
struct FeedModule {
    #[serde(default)]
    title: Option<String>,
    #[serde(default, rename = "viewAll")]
    view_all: Option<String>,
    #[serde(default)]
    items: Vec<Value>,
}

impl FeedModule {
    /// Every module is a section, whatever its type; none if no item is kept.
    fn section(self) -> Option<BrowseSection> {
        let rows: Vec<Item> = self.items.iter().filter_map(v2_row).collect();
        if rows.is_empty() {
            return None;
        }
        let rows = assemble(rows, 0, None);
        Some(BrowseSection {
            title: titled(self.title, &rows),
            rows,
            more: self
                .view_all
                .filter(|p| !p.is_empty())
                .map_or(BrowseMore::None, BrowseMore::ViewAll),
        })
    }
}

/// `GET /v2/home/pages/…/view-all`.
#[derive(Deserialize)]
struct ViewAll {
    #[serde(default)]
    items: Vec<Value>,
}

/// A v2 mix: its type is `type`, its texts are in `…TextInfo`.
#[derive(Deserialize)]
struct V2Mix {
    #[serde(default)]
    id: String,
    #[serde(default, rename = "type")]
    kind: String,
    #[serde(default, rename = "titleTextInfo")]
    title: Option<TextInfo>,
    #[serde(default, rename = "subtitleTextInfo")]
    subtitle: Option<TextInfo>,
}

#[derive(Deserialize)]
struct TextInfo {
    #[serde(default)]
    text: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// AC8: a path is followed only if it starts with `pages/` (v1) or
    /// `home/pages/` (v2) and has no `..` segment, plain or percent-encoded.
    #[test]
    fn ac8_browse_paths() {
        use Source::{V1, V2};
        let table = [
            (V1, "pages/home", true),
            (
                V1,
                "pages/data/00000000-0000-4000-8000-0000000000aa?albumId=10581228",
                true,
            ),
            (V1, "pages/single-module-page/f7bc/7/02f1/1", true),
            (V1, "pages/a..b", true),
            (V1, "pages/x?back=../", true),
            (V1, "home/pages/DAILY_MIXES/view-all", false),
            (V1, "pages", false),
            (V1, "pages/", false),
            (V1, "", false),
            (V1, "/pages/home", false),
            (V1, "pagesx/home", false),
            (V1, "Pages/home", false),
            (V1, "pages/../tracks/1", false),
            (V1, "pages/./home", false),
            (V1, "pages/a/..", false),
            (V1, "pages/%2e%2e/tracks/1", false),
            (V1, "pages/%2E./tracks/1", false),
            (V1, "pages\\..\\x", false),
            (V1, "https://example.com/pages/home", false),
            (V2, "home/pages/DAILY_MIXES/view-all", true),
            (
                V2,
                "home/pages/X/view-all?itemId=heavy%20alternative&rankingToken=1",
                true,
            ),
            (V2, "pages/home", false),
            (V2, "home/feed/static", false),
            (V2, "home/pages/../feed/static", false),
            (V2, "home/pages/%2e%2e/x", false),
            (V2, "", false),
        ];
        for (source, path, ok) in table {
            let got = browse_path(path, source);
            if ok {
                assert_eq!(got, Ok(path), "{source:?} {path}");
            } else {
                assert_eq!(
                    got,
                    Err(LibraryError::NotAPage(path.to_owned())),
                    "{source:?} {path}"
                );
            }
        }
    }
}
