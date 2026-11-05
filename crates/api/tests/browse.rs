//! Spec 0024, browse pages and the home feed over HTTP: AC2-AC8, AC12. No
//! real network: a `wiremock` server answers with the fixtures under
//! `fixtures/browse/`, written from the live probes' shapes ("Facts").

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use tidal_player_api::auth::{
    AuthConfig, Authenticator, ManualClock, MemoryStore, Session, SessionStore,
};
use tidal_player_api::library::{LibraryClient, LibraryError};
use tidal_player_core::library::{
    BrowseMore, BrowseRef, BrowseSection, LibraryRequest, LibraryResponse, ListItems, ListPage,
    ListRef, MixSummary, MixedRow, PageData, PageRequest,
};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const EXPLORE: &str = include_str!("fixtures/browse/explore.json");
const PAGE_MODULES: &str = include_str!("fixtures/browse/page_mixed_modules.json");
const PAGE_SKIPPED: &str = include_str!("fixtures/browse/page_only_skipped.json");
const PAGE_UNTITLED: &str = include_str!("fixtures/browse/page_untitled.json");
const DATA_ALBUMS: &str = include_str!("fixtures/browse/data_albums.json");
const DATA_TRACKS: &str = include_str!("fixtures/browse/data_tracks.json");
const DATA_MIXES: &str = include_str!("fixtures/browse/data_mixes.json");
const DATA_LINKS: &str = include_str!("fixtures/browse/data_links.json");
const DATA_PLAYLISTS: &str = include_str!("fixtures/browse/data_playlists.json");
const DATA_ARTISTS: &str = include_str!("fixtures/browse/data_artists.json");
const DATA_EMPTY: &str = include_str!("fixtures/browse/data_empty.json");
const FEED_0: &str = include_str!("fixtures/browse/feed_0.json");
const FEED_1: &str = include_str!("fixtures/browse/feed_1.json");
const FEED_LOOP: &str = include_str!("fixtures/browse/feed_loop.json");
const VIEW_ALL_MIXED: &str = include_str!("fixtures/browse/view_all_mixed.json");
const VIEW_ALL_MIXES: &str = include_str!("fixtures/browse/view_all_mixes.json");
const NOT_FOUND: &str = include_str!("fixtures/browse/error_page_not_found.json");
const V2_BAD_REQUEST: &str = include_str!("fixtures/browse/error_v2_bad_request.json");

const DATA: &str = "pages/data/00000000-0000-4000-8000-0000000000";
const SIZE: u32 = 20;
/// The version tidal-player-api sends in `x-tidal-client-version`.
const VERSION: &str = env!("CARGO_PKG_VERSION");

fn t0() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000)
}

/// Valid for an hour, in Finland, user 1.
fn session() -> Session {
    Session {
        access_token: "FAKE-ACCESS".into(),
        refresh_token: "FAKE-REFRESH".into(),
        expires_at: t0() + Duration::from_secs(3600),
        user_id: 1,
        country_code: "FI".into(),
    }
}

fn json_body(status: u16, body: &str) -> ResponseTemplate {
    ResponseTemplate::new(status).set_body_raw(body.to_owned(), "application/json")
}

/// One request the server received.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Seen {
    path: String,
    /// Decoded and sorted.
    query: Vec<(String, String)>,
    /// `x-tidal-client-version`, when sent.
    version: Option<String>,
}

fn seen(path: &str, query: &[(&str, &str)], version: Option<&str>) -> Seen {
    let mut query: Vec<(String, String)> = query
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    query.sort();
    Seen {
        path: path.into(),
        query,
        version: version.map(Into::into),
    }
}

struct Setup {
    server: MockServer,
    client: LibraryClient,
}

impl Setup {
    async fn new() -> Self {
        let server = MockServer::start().await;
        let store = Arc::new(MemoryStore::default());
        store.save(&session()).unwrap();
        let config = AuthConfig::with_bases(
            format!("{}/oauth2", server.uri()),
            format!("{}/v1", server.uri()),
        );
        let clock = Arc::new(ManualClock::new(t0()));
        let auth = Authenticator::new(config, store, session(), clock).unwrap();
        let client = LibraryClient::new(Arc::new(auth));
        Self { server, client }
    }

    /// A `GET p` answering `status` and `body`, for any query.
    async fn get(&self, p: &str, status: u16, body: &str) {
        Mock::given(method("GET"))
            .and(path(p))
            .respond_with(json_body(status, body))
            .mount(&self.server)
            .await;
    }

    async fn browse(&self, target: BrowseRef) -> Result<PageData, LibraryError> {
        match self
            .client
            .request(LibraryRequest::Page(PageRequest::Browse(target)), SIZE, &[])
            .await?
        {
            LibraryResponse::Page(page) => Ok(page),
            other => panic!("not a page: {other:?}"),
        }
    }

    async fn more(
        &self,
        list: ListRef,
        offset: u32,
        limit: u32,
    ) -> Result<ListItems, LibraryError> {
        match self
            .client
            .request(
                LibraryRequest::More {
                    list,
                    offset,
                    limit,
                },
                SIZE,
                &[],
            )
            .await?
        {
            LibraryResponse::Items(items) => Ok(items),
            other => panic!("not items: {other:?}"),
        }
    }

    /// Every request received, in order.
    async fn seen(&self) -> Vec<Seen> {
        self.server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .map(|r| {
                let mut query: Vec<(String, String)> = r
                    .url
                    .query_pairs()
                    .map(|(k, v)| (k.into_owned(), v.into_owned()))
                    .collect();
                query.sort();
                Seen {
                    path: r.url.path().to_owned(),
                    query,
                    version: r
                        .headers
                        .get("x-tidal-client-version")
                        .and_then(|v| v.to_str().ok())
                        .map(str::to_owned),
                }
            })
            .collect()
    }
}

/// What the v1 endpoints are asked, plus `extra`.
fn v1_query<'a>(extra: &[(&'a str, &'a str)]) -> Vec<(&'a str, &'a str)> {
    let mut query = vec![
        ("countryCode", "FI"),
        ("deviceType", "BROWSER"),
        ("locale", "en_US"),
    ];
    query.extend_from_slice(extra);
    query
}

/// What the v2 endpoints are asked, plus `extra`.
fn v2_query<'a>(extra: &[(&'a str, &'a str)]) -> Vec<(&'a str, &'a str)> {
    let mut query = v1_query(&[("platform", "WEB")]);
    query.extend_from_slice(extra);
    query
}

/// A row as `kind:key`, or `link:title>target`.
fn label(row: &MixedRow) -> String {
    match row {
        MixedRow::Track(t) => format!("track:{}", t.id.0),
        MixedRow::Album(a) => format!("album:{}", a.id),
        MixedRow::Playlist(p) => format!("playlist:{}", p.uuid),
        MixedRow::Artist(a) => format!("artist:{}", a.id),
        MixedRow::Mix(m) => format!("mix:{}", m.id),
    }
}

/// The kind of a list, its rows as `key`s (a link as `title>target`), and
/// its page's `(offset, total)`.
fn rows(items: &ListItems) -> (&'static str, Vec<String>, (u32, u32)) {
    fn keys<T, K: ToString>(p: &ListPage<T>, f: impl Fn(&T) -> K) -> (Vec<String>, (u32, u32)) {
        (
            p.items.iter().map(|i| f(i).to_string()).collect(),
            (p.offset, p.total),
        )
    }
    match items {
        ListItems::Tracks(p) => {
            let (k, c) = keys(p, |t| t.id.0);
            ("tracks", k, c)
        }
        ListItems::Albums(p) => {
            let (k, c) = keys(p, |a| a.id);
            ("albums", k, c)
        }
        ListItems::Playlists(p) => {
            let (k, c) = keys(p, |p| p.uuid.clone());
            ("playlists", k, c)
        }
        ListItems::Artists(p) => {
            let (k, c) = keys(p, |a| a.id);
            ("artists", k, c)
        }
        ListItems::Mixes(p) => {
            let (k, c) = keys(p, |m| m.id.clone());
            ("mixes", k, c)
        }
        ListItems::Links(p) => {
            let (k, c) = keys(p, |l| format!("{}>{:?}", l.title, l.target));
            ("links", k, c)
        }
        ListItems::Mixed(p) => {
            let (k, c) = keys(p, label);
            ("mixed", k, c)
        }
        ListItems::Credits(_) => panic!("no credits in browse"),
    }
}

/// A section as `(title, kind, row keys, more)`.
type Shown = (String, &'static str, Vec<String>, BrowseMore);

/// The page's title and its sections as [`Shown`].
fn sections(page: PageData) -> (String, Vec<Shown>) {
    let PageData::Browse { title, sections } = page else {
        panic!("not a browse page");
    };
    let sections = sections
        .into_iter()
        .map(|s: BrowseSection| {
            let (kind, keys, _) = rows(&s.rows);
            (s.title, kind, keys, s.more)
        })
        .collect();
    (title, sections)
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

fn data_path(n: &str) -> String {
    format!("{DATA}{n}")
}

fn page_link(path: &str) -> BrowseRef {
    BrowseRef::Page(path.into())
}

const DISCOVERY: &str = "016214541b7a008d226272cf9d0964";
const MIX_1: &str = DISCOVERY;
const RADIO: &str = "000c9493c96d7c4ff25f1bf7da4852";
const SKILLET: &str = "51b42ddc-1789-4706-9570-c6b51c14b489";
const THORO: &str = "34c543c9-bb74-4b79-91a8-feb6d815f43c";
const ESSENTIALS: &str = "285d6293-8f77-4dc1-8dab-a262f3d0cb43";

// AC2 -------------------------------------------------------------------

/// AC2: a v1 page's modules become sections in Tidal's order; the video,
/// promotion and unknown modules are skipped and an item that cannot be
/// read is dropped without failing the page.
#[tokio::test]
async fn ac2_v1_sections() {
    let s = Setup::new().await;
    s.get("/v1/pages/genre_hip_hop", 200, PAGE_MODULES).await;
    let (title, got) = sections(s.browse(page_link("pages/genre_hip_hop")).await.unwrap());
    assert_eq!(title, "Hip-Hop");
    let expected = vec![
        (
            "Playlists".to_owned(),
            "playlists",
            strings(&[THORO, ESSENTIALS]),
            BrowseMore::Data(data_path("01")),
        ),
        (
            "New Tracks".to_owned(),
            "tracks",
            strings(&["563860770", "563860772"]),
            BrowseMore::Data(data_path("02")),
        ),
        (
            "Albums".to_owned(),
            "albums",
            strings(&["563860751", "35738574"]),
            BrowseMore::None,
        ),
        (
            "Top Artists".to_owned(),
            "artists",
            strings(&["17275", "7804"]),
            BrowseMore::Data(data_path("05")),
        ),
        (
            "Mixes".to_owned(),
            "mixes",
            strings(&[DISCOVERY]),
            BrowseMore::None,
        ),
        (
            "More to explore".to_owned(),
            "links",
            strings(&[
                "Social>Page(\"pages/social_justice\")",
                "Classics>Page(\"pages/classics\")",
            ]),
            BrowseMore::None,
        ),
        (
            "Same title".to_owned(),
            "albums",
            strings(&["1"]),
            BrowseMore::None,
        ),
        (
            "Same title".to_owned(),
            "albums",
            strings(&["2"]),
            BrowseMore::None,
        ),
    ];
    assert_eq!(got, expected);
    assert_eq!(
        s.seen().await,
        vec![seen("/v1/pages/genre_hip_hop", &v1_query(&[]), None)]
    );

    // A page of skipped modules only is a page without sections.
    let s = Setup::new().await;
    s.get("/v1/pages/videos", 200, PAGE_SKIPPED).await;
    let (title, got) = sections(s.browse(page_link("pages/videos")).await.unwrap());
    assert_eq!((title.as_str(), got.len()), ("Videos", 0));
}

/// AC2: a section keeps Tidal's total and its page; a section that holds
/// its total arrives whole.
#[tokio::test]
async fn ac2_section_totals() {
    let s = Setup::new().await;
    s.get("/v1/pages/genre_hip_hop", 200, PAGE_MODULES).await;
    let PageData::Browse { sections, .. } =
        s.browse(page_link("pages/genre_hip_hop")).await.unwrap()
    else {
        panic!("not a browse page");
    };
    let totals: Vec<(u32, u32)> = sections.iter().map(|s| rows(&s.rows).2).collect();
    // Paged sections say Tidal's total (27, 439, 122); whole ones the rows
    // kept (the album list drops one of its three).
    assert_eq!(
        totals,
        vec![
            (0, 27),
            (0, 439),
            (0, 2),
            (0, 122),
            (0, 1),
            (0, 2),
            (0, 1),
            (0, 1)
        ]
    );
}

/// AC4: rows left out of a paged section's first page count as `hidden`,
/// so the next `offset` follows Tidal's list, not the rows kept.
#[tokio::test]
async fn ac4_first_page_counts_rows_left_out() {
    let s = Setup::new().await;
    // Three tracks, one of them Atmos only.
    let tracks: serde_json::Value = serde_json::from_str(DATA_TRACKS).unwrap();
    let page = serde_json::json!({
        "title": "New",
        "rows": [{"modules": [{
            "type": "TRACK_LIST",
            "title": "New Tracks",
            "pagedList": {
                "dataApiPath": "pages/data/00000000-0000-4000-8000-000000000001",
                "limit": 3,
                "offset": 0,
                "totalNumberOfItems": 10,
                "items": tracks["items"],
            },
        }]}],
    });
    s.get("/v1/pages/explore_new_music", 200, &page.to_string())
        .await;
    let PageData::Browse { sections, .. } = s
        .browse(page_link("pages/explore_new_music"))
        .await
        .unwrap()
    else {
        panic!("not a browse page");
    };
    let ListItems::Tracks(first) = &sections[0].rows else {
        panic!("not tracks: {:?}", sections[0].rows);
    };
    assert_eq!((first.items.len(), first.hidden), (2, 1));
}

/// AC5: `pages/home` is *Staff picks*, whatever Tidal titles it: its own
/// title would clash with the personal *Home*.
#[tokio::test]
async fn ac5_staff_picks_title() {
    let s = Setup::new().await;
    s.get("/v1/pages/home", 200, PAGE_MODULES).await;
    let PageData::Browse { title, .. } = s.browse(page_link("pages/home")).await.unwrap() else {
        panic!("not a browse page");
    };
    assert_eq!(title, "Staff picks");
}

// AC3 -------------------------------------------------------------------

/// AC3: rows left out in v1 and v2, sections left empty dropped.
#[tokio::test]
async fn ac3_rows_left_out() {
    // v1: the Atmos-only track, the video mix, the video playlist and the
    // empty-titled link (all of `ac2_v1_sections`'s expectations); a
    // section left with no rows is gone.
    let s = Setup::new().await;
    s.get("/v1/pages/genre_hip_hop", 200, PAGE_MODULES).await;
    let (_, got) = sections(s.browse(page_link("pages/genre_hip_hop")).await.unwrap());
    let titles: Vec<&str> = got.iter().map(|s| s.0.as_str()).collect();
    assert!(!titles.contains(&"New Video Playlists"), "{titles:?}");
    assert!(!titles.contains(&"Only empty"), "{titles:?}");
    let by = |t: &str| got.iter().find(|s| s.0 == t).unwrap().2.clone();
    assert_eq!(by("New Tracks"), strings(&["563860770", "563860772"]));
    assert_eq!(by("Mixes"), strings(&[DISCOVERY]));
    assert_eq!(by("Playlists"), strings(&[THORO, ESSENTIALS]));
    assert_eq!(by("More to explore").len(), 2);

    // v2: the Atmos-only track, the DEEP_LINK, the video mix and the
    // playlist with no tracks; the module of deep links only is gone.
    let s = Setup::new().await;
    mount_feed(&s).await;
    let (_, got) = sections(s.browse(BrowseRef::Home).await.unwrap());
    let titles: Vec<&str> = got.iter().map(|s| s.0.as_str()).collect();
    assert!(!titles.contains(&"Only deep links"), "{titles:?}");
    assert!(!titles.contains(&"Video mixes and empty"), "{titles:?}");
    let by = |t: &str| got.iter().find(|s| s.0 == t).unwrap().2.clone();
    assert_eq!(
        by("Recently played"),
        strings(&[
            &format!("mix:{MIX_1}"),
            &format!("playlist:{SKILLET}"),
            "track:233573504"
        ])
    );
}

/// AC3: an untitled module takes its kind's name.
#[tokio::test]
async fn ac3_untitled_modules() {
    let s = Setup::new().await;
    s.get("/v1/pages/names", 200, PAGE_UNTITLED).await;
    let (_, got) = sections(s.browse(page_link("pages/names")).await.unwrap());
    let titles: Vec<&str> = got.iter().map(|s| s.0.as_str()).collect();
    assert_eq!(
        titles,
        vec!["Tracks", "Albums", "Playlists", "Artists", "Mixes", "More"]
    );
}

// AC4 -------------------------------------------------------------------

/// AC4: `dataApiPath` is asked with `limit` (at most 50) and `offset`, the
/// path's own query kept; rows left out stay out and the page keeps the
/// offset it was asked at and Tidal's total.
#[tokio::test]
async fn ac4_v1_paging() {
    struct Case {
        name: &'static str,
        list: String,
        offset: u32,
        limit: u32,
        body: &'static str,
        /// The path as the server sees it and its query besides the
        /// pages' own.
        server_path: String,
        asked: Vec<(&'static str, &'static str)>,
        kind: &'static str,
        keys: Vec<&'static str>,
        total: u32,
    }
    let table = [
        Case {
            name: "limit capped at 50, the path's own query kept",
            list: format!("{}?albumId=10581228", data_path("aa")),
            offset: 100,
            limit: 100,
            body: DATA_ALBUMS,
            server_path: format!("/v1/{}", data_path("aa")),
            asked: vec![("albumId", "10581228"), ("limit", "50"), ("offset", "100")],
            kind: "albums",
            keys: vec!["35738574", "563860751"],
            total: 465,
        },
        Case {
            name: "a small limit stays, an Atmos-only track is left out",
            list: data_path("02"),
            offset: 5,
            limit: 5,
            body: DATA_TRACKS,
            server_path: format!("/v1/{}", data_path("02")),
            asked: vec![("limit", "5"), ("offset", "5")],
            kind: "tracks",
            keys: vec!["563860770", "563860772"],
            total: 439,
        },
        Case {
            name: "a video mix is left out",
            list: data_path("03"),
            offset: 0,
            limit: 6,
            body: DATA_MIXES,
            server_path: format!("/v1/{}", data_path("03")),
            asked: vec![("limit", "6"), ("offset", "0")],
            kind: "mixes",
            keys: vec![DISCOVERY],
            total: 8,
        },
        Case {
            name: "a link with no title is left out",
            list: data_path("04"),
            offset: 15,
            limit: 15,
            body: DATA_LINKS,
            server_path: format!("/v1/{}", data_path("04")),
            asked: vec![("limit", "15"), ("offset", "15")],
            kind: "links",
            keys: vec!["Focus>Page(\"pages/mood_focus\")"],
            total: 19,
        },
        Case {
            name: "a video playlist is left out",
            list: data_path("05"),
            offset: 15,
            limit: 15,
            body: DATA_PLAYLISTS,
            server_path: format!("/v1/{}", data_path("05")),
            asked: vec![("limit", "15"), ("offset", "15")],
            kind: "playlists",
            keys: vec![THORO],
            total: 27,
        },
        Case {
            name: "artists",
            list: data_path("06"),
            offset: 15,
            limit: 15,
            body: DATA_ARTISTS,
            server_path: format!("/v1/{}", data_path("06")),
            asked: vec![("limit", "15"), ("offset", "15")],
            kind: "artists",
            keys: vec!["17275"],
            total: 122,
        },
    ];
    for case in table {
        let s = Setup::new().await;
        s.get(&case.server_path, 200, case.body).await;
        let items = s
            .more(
                ListRef::BrowseData(case.list.clone()),
                case.offset,
                case.limit,
            )
            .await
            .unwrap();
        let (kind, keys, (offset, total)) = rows(&items);
        assert_eq!(kind, case.kind, "{}", case.name);
        assert_eq!(keys, case.keys, "{}", case.name);
        assert_eq!(
            (offset, total),
            (case.offset, case.total),
            "{}: the page keeps the offset it was asked at and Tidal's total",
            case.name
        );
        assert_eq!(
            s.seen().await,
            vec![seen(&case.server_path, &v1_query(&case.asked), None)],
            "{}",
            case.name
        );
    }
}

/// AC4: an empty page past the end is an empty list, not an error.
#[tokio::test]
async fn ac4_empty_page() {
    let s = Setup::new().await;
    s.get(&format!("/v1/{}", data_path("07")), 200, DATA_EMPTY)
        .await;
    let items = s
        .more(ListRef::BrowseData(data_path("07")), 30, 15)
        .await
        .unwrap();
    let (_, keys, (offset, total)) = rows(&items);
    assert_eq!((keys.len(), offset, total), (0, 30, 27));
}

// AC5 -------------------------------------------------------------------

/// AC5: the root page is `pages/explore` as four tabs; its promotions are
/// not shown.
#[tokio::test]
async fn ac5_root() {
    let s = Setup::new().await;
    s.get("/v1/pages/explore", 200, EXPLORE).await;
    let (title, got) = sections(s.browse(BrowseRef::Root).await.unwrap());
    assert_eq!(title, "Browse");
    let titles: Vec<&str> = got.iter().map(|s| s.0.as_str()).collect();
    assert_eq!(
        titles,
        vec!["Discover", "Genres", "Moods & Activities", "Decades"]
    );
    // Discover: Home, Staff picks, For you, then explore's untitled links.
    assert_eq!(got[0].1, "links");
    assert_eq!(
        got[0].2,
        strings(&[
            "Home>Home",
            "Staff picks>Page(\"pages/home\")",
            "For you>Page(\"pages/for_you\")",
            "New>Page(\"pages/explore_new_music\")",
            "Top>Page(\"pages/explore_top_music\")",
            "HiRes>Page(\"pages/hires\")",
        ])
    );
    assert_eq!(got[0].3, BrowseMore::None);
    assert_eq!(
        got[1].2,
        strings(&[
            "Hip-Hop>Page(\"pages/genre_hip_hop\")",
            "Pop>Page(\"pages/genre_pop\")"
        ])
    );
    assert_eq!(got[1].3, BrowseMore::None);
    // Moods & Activities holds 2 of 4: the rest pages.
    assert_eq!(got[2].2.len(), 2);
    assert_eq!(got[2].3, BrowseMore::Data(data_path("c2")));
    assert_eq!(got[3].2, strings(&["1950s>Page(\"pages/m_1950s\")"]));
    assert_eq!(
        s.seen().await,
        vec![seen("/v1/pages/explore", &v1_query(&[]), None)]
    );
}

// AC6 -------------------------------------------------------------------

async fn mount_feed(s: &Setup) {
    // The first request has no cursor, the second has.
    Mock::given(method("GET"))
        .and(path("/v2/home/feed/static"))
        .and(wiremock::matchers::query_param_is_missing("cursor"))
        .respond_with(json_body(200, FEED_0))
        .mount(&s.server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v2/home/feed/static"))
        .and(wiremock::matchers::query_param(
            "cursor",
            "QkFTRURfT05fWU9VUl9JTlRFUkVTVFNfMg",
        ))
        .respond_with(json_body(200, FEED_1))
        .mount(&s.server)
        .await;
}

fn mix(id: &str, title: &str, subtitle: &str) -> MixSummary {
    MixSummary {
        id: id.into(),
        title: title.into(),
        subtitle: Some(subtitle.into()),
    }
}

/// AC6: the feed is asked with its parameters and the version header,
/// follows `cursor`, and every module becomes a section whose `TRACK`,
/// `ALBUM`, `PLAYLIST`, `ARTIST` and `MIX` items are read from `data`.
#[tokio::test]
async fn ac6_feed() {
    let s = Setup::new().await;
    mount_feed(&s).await;
    let page = s.browse(BrowseRef::Home).await.unwrap();

    // The mix's title and subtitle come from the text infos.
    let PageData::Browse { sections: raw, .. } = &page else {
        panic!("not a browse page");
    };
    assert_eq!(
        raw[1].rows,
        ListItems::Mixes(ListPage {
            items: vec![
                mix(MIX_1, "My Mix 1", "TK and more"),
                mix(RADIO, "Set It Off", "Artist Radio"),
            ],
            offset: 0,
            total: 2,
            hidden: 0,
            positions: Vec::new(),
        })
    );

    let (title, got) = sections(page);
    assert_eq!(title, "Home");
    let heavy = "home/pages/BASED_ON_YOUR_INTERESTS_1/view-all?itemId=heavy%20alternative&rankingToken=6320907525";
    let expected = vec![
        (
            "Shortcuts".to_owned(),
            "mixed",
            strings(&[&format!("mix:{DISCOVERY}"), &format!("playlist:{SKILLET}")]),
            BrowseMore::None,
        ),
        (
            "Custom mixes".to_owned(),
            "mixes",
            strings(&[MIX_1, RADIO]),
            BrowseMore::ViewAll("home/pages/DAILY_MIXES/view-all".into()),
        ),
        (
            "Recently played".to_owned(),
            "mixed",
            strings(&[
                &format!("mix:{MIX_1}"),
                &format!("playlist:{SKILLET}"),
                "track:233573504",
            ]),
            BrowseMore::ViewAll("home/pages/CONTINUE_LISTEN_TO/view-all".into()),
        ),
        (
            "Heavy Alternative & more".to_owned(),
            "mixed",
            strings(&[
                "album:35757649",
                "artist:5839856",
                &format!("playlist:{SKILLET}"),
            ]),
            BrowseMore::ViewAll(heavy.into()),
        ),
        (
            "Suggested new albums for you".to_owned(),
            "albums",
            strings(&["35738574"]),
            BrowseMore::ViewAll("home/pages/NEW_ALBUM_SUGGESTIONS/view-all".into()),
        ),
        (
            "Tracks".to_owned(),
            "tracks",
            strings(&["233573504"]),
            BrowseMore::None,
        ),
    ];
    assert_eq!(got, expected);

    // Two requests: the first without `cursor`, the second with it; both
    // with the version header.
    assert_eq!(
        s.seen().await,
        vec![
            seen("/v2/home/feed/static", &v2_query(&[]), Some(VERSION)),
            seen(
                "/v2/home/feed/static",
                &v2_query(&[("cursor", "QkFTRURfT05fWU9VUl9JTlRFUkVTVFNfMg")]),
                Some(VERSION)
            ),
        ]
    );
}

/// AC6: a feed whose cursor never ends is read in 5 requests.
#[tokio::test]
async fn ac6_feed_request_cap() {
    let s = Setup::new().await;
    s.get("/v2/home/feed/static", 200, FEED_LOOP).await;
    let (_, got) = sections(s.browse(BrowseRef::Home).await.unwrap());
    assert_eq!(s.seen().await.len(), 5);
    assert_eq!(got.len(), 5, "every module of every page is a section");
}

// AC7 -------------------------------------------------------------------

/// AC7: a `viewAll` is asked with neither `limit` nor `offset`, whatever
/// the caller asked, its own query kept; the answer is whole, one kind or
/// `Mixed` by the rows kept.
#[tokio::test]
async fn ac7_view_all() {
    struct Case {
        name: &'static str,
        view_all: &'static str,
        server_path: &'static str,
        own_query: Vec<(&'static str, &'static str)>,
        body: &'static str,
        kind: &'static str,
        keys: Vec<String>,
    }
    let table = [
        Case {
            name: "several kinds: Mixed, with what is left out gone",
            view_all: "home/pages/CONTINUE_LISTEN_TO/view-all",
            server_path: "/v2/home/pages/CONTINUE_LISTEN_TO/view-all",
            own_query: vec![],
            body: VIEW_ALL_MIXED,
            kind: "mixed",
            keys: strings(&[
                &format!("mix:{MIX_1}"),
                "track:233573504",
                "album:35757649",
                &format!("playlist:{SKILLET}"),
                "artist:5839856",
            ]),
        },
        Case {
            name: "one kind: that kind's list, the query of the path kept",
            view_all: "home/pages/DAILY_MIXES/view-all?itemId=heavy%20alternative&rankingToken=6320907525",
            server_path: "/v2/home/pages/DAILY_MIXES/view-all",
            own_query: vec![
                ("itemId", "heavy alternative"),
                ("rankingToken", "6320907525"),
            ],
            body: VIEW_ALL_MIXES,
            kind: "mixes",
            keys: strings(&[MIX_1, RADIO]),
        },
    ];
    for case in table {
        let s = Setup::new().await;
        s.get(case.server_path, 200, case.body).await;
        // The offset and limit a caller passes are not forwarded.
        let items = s
            .more(ListRef::BrowseViewAll(case.view_all.into()), 15, 50)
            .await
            .unwrap();
        let (kind, keys, (offset, total)) = rows(&items);
        assert_eq!((kind, &keys), (case.kind, &case.keys), "{}", case.name);
        assert_eq!(
            (offset, total as usize),
            (0, keys.len()),
            "{}: the list is whole",
            case.name
        );
        assert_eq!(
            s.seen().await,
            vec![seen(
                case.server_path,
                &v2_query(&case.own_query),
                Some(VERSION)
            )],
            "{}",
            case.name
        );
    }
}

// AC8 -------------------------------------------------------------------

/// AC8: a path of neither known form answers `Not a Tidal page: <path>`
/// and sends no request.
#[tokio::test]
async fn ac8_refused_paths_send_nothing() {
    let table = [
        ("a v2 path as a page", page_request("home/pages/X/view-all")),
        (
            "a dot segment in a page",
            page_request("pages/../v2/home/feed/static"),
        ),
        (
            "an encoded dot segment",
            page_request("pages/%2e%2e/tracks/1"),
        ),
        (
            "an absolute URL",
            page_request("https://example.com/pages/home"),
        ),
        ("an empty page", page_request("")),
        (
            "a data path off pages/",
            more_request(ListRef::BrowseData("tracks/1".into())),
        ),
        (
            "a data path with a dot segment",
            more_request(ListRef::BrowseData("pages/data/../../x".into())),
        ),
        (
            "a v1 path as a view-all",
            more_request(ListRef::BrowseViewAll("pages/home".into())),
        ),
        (
            "a view-all with a dot segment",
            more_request(ListRef::BrowseViewAll("home/pages/../feed/static".into())),
        ),
    ];
    for (name, (request, bad)) in table {
        let s = Setup::new().await;
        let err = s.client.request(request, SIZE, &[]).await.expect_err(name);
        assert_eq!(
            err.to_string(),
            format!("Not a Tidal page: {bad}"),
            "{name}"
        );
        assert_eq!(s.seen().await, vec![], "{name}: nothing was sent");
    }
}

fn page_request(path: &str) -> (LibraryRequest, String) {
    (
        LibraryRequest::Page(PageRequest::Browse(BrowseRef::Page(path.into()))),
        path.to_owned(),
    )
}

fn more_request(list: ListRef) -> (LibraryRequest, String) {
    let path = match &list {
        ListRef::BrowseData(p) | ListRef::BrowseViewAll(p) => p.clone(),
        other => panic!("{other:?}"),
    };
    (
        LibraryRequest::More {
            list,
            offset: 0,
            limit: 15,
        },
        path,
    )
}

// AC12 ------------------------------------------------------------------

/// AC12: the failure of a page, a list and the home feed, in words.
#[tokio::test]
async fn ac12_failures() {
    enum Ask {
        Page(&'static str),
        Home,
        Data(&'static str),
        ViewAll(&'static str),
    }
    let table = [
        (
            "a page that does not exist",
            Ask::Page("pages/nope"),
            "/v1/pages/nope",
            404,
            NOT_FOUND,
            "Page pages/nope was not found",
        ),
        (
            "a data path that does not exist",
            Ask::Data("pages/data/gone"),
            "/v1/pages/data/gone",
            404,
            NOT_FOUND,
            "Page pages/data/gone was not found",
        ),
        (
            "a server error",
            Ask::Page("pages/home"),
            "/v1/pages/home",
            500,
            "{}",
            "Tidal answered 500",
        ),
        (
            "too many requests",
            Ask::Page("pages/home"),
            "/v1/pages/home",
            429,
            "{}",
            "Tidal answered 429: try again in a moment",
        ),
        (
            "a body that is not a page",
            Ask::Page("pages/home"),
            "/v1/pages/home",
            200,
            "\"not a page\"",
            "malformed browse response from Tidal",
        ),
        (
            "a home Tidal refuses",
            Ask::Home,
            "/v2/home/feed/static",
            400,
            V2_BAD_REQUEST,
            "Home is not available: Tidal answered 400",
        ),
        (
            "a home that is down",
            Ask::Home,
            "/v2/home/feed/static",
            503,
            "{}",
            "Home is not available: Tidal answered 503",
        ),
        (
            "a view-all that fails",
            Ask::ViewAll("home/pages/CONTINUE_LISTEN_TO/view-all"),
            "/v2/home/pages/CONTINUE_LISTEN_TO/view-all",
            400,
            V2_BAD_REQUEST,
            "Tidal answered 400",
        ),
    ];
    for (name, ask, server_path, status, body, message) in table {
        let s = Setup::new().await;
        s.get(server_path, status, body).await;
        let err = match ask {
            Ask::Page(p) => s.browse(page_link(p)).await.map(drop),
            Ask::Home => s.browse(BrowseRef::Home).await.map(drop),
            Ask::Data(p) => s.more(ListRef::BrowseData(p.into()), 0, 15).await.map(drop),
            Ask::ViewAll(p) => s
                .more(ListRef::BrowseViewAll(p.into()), 0, 15)
                .await
                .map(drop),
        }
        .expect_err(name);
        assert_eq!(err.to_string(), message, "{name}");
    }
}

/// AC12: a failed *Home* leaves the rest of browse usable.
#[tokio::test]
async fn ac12_home_fails_alone() {
    let s = Setup::new().await;
    s.get("/v2/home/feed/static", 400, V2_BAD_REQUEST).await;
    s.get("/v1/pages/home", 200, PAGE_MODULES).await;
    s.get("/v1/pages/explore", 200, EXPLORE).await;
    assert!(s.browse(BrowseRef::Home).await.is_err());
    let (title, got) = sections(s.browse(page_link("pages/home")).await.unwrap());
    assert_eq!((title.as_str(), got.is_empty()), ("Hip-Hop", false));
    assert!(s.browse(BrowseRef::Root).await.is_ok());
}
