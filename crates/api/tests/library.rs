//! Spec 0006, the library over HTTP: AC4-AC7. No real network: a `wiremock`
//! server answers with the fixtures under `fixtures/library/` (and tracks
//! cloned from `fixtures/metadata/track.json`).

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use serde_json::{Value, json};
use tidal_player_api::auth::{
    AuthConfig, AuthError, Authenticator, ManualClock, MemoryStore, Session, SessionStore,
};
use tidal_player_api::library::{LibraryClient, LibraryError};
use tidal_player_core::library::{
    AlbumKind, AlbumSummary, CreditedTrack, DEFAULT_HIDDEN_VERSIONS, FavoriteKind, LibraryRequest,
    LibraryResponse, ListItems, ListPage, ListRef, PageData, PageRequest, PlaylistSummary,
    RoleCategory,
};
use tidal_player_core::{AlbumRef, ArtistRef, Track, TrackId};
use wiremock::matchers::{method, path, query_param, query_param_is_missing};
use wiremock::{Mock, MockServer, ResponseTemplate};

const TRACK: &str = include_str!("fixtures/metadata/track.json");
const PLAYLISTS: &str = include_str!("fixtures/library/playlists.json");
const FAVORITE_ALBUMS: &str = include_str!("fixtures/library/favorite_albums.json");
const FAVORITE_ARTISTS: &str = include_str!("fixtures/library/favorite_artists.json");
const ALBUM: &str = include_str!("fixtures/library/album.json");
const PLAYLIST: &str = include_str!("fixtures/library/playlist.json");
const ARTIST: &str = include_str!("fixtures/library/artist.json");
const ALBUMS_PLAIN: &str = include_str!("fixtures/library/albums_plain.json");
const ALBUMS_EPS: &str = include_str!("fixtures/library/albums_eps_and_singles.json");
const ALBUMS_COMPILATIONS: &str = include_str!("fixtures/library/albums_compilations.json");
const ALBUMS_EMPTY: &str = include_str!("fixtures/library/albums_empty.json");
const PLAYLIST_ITEMS: &str = include_str!("fixtures/library/playlist_items_with_video.json");
const CONTRIBUTOR: &str = include_str!("fixtures/library/contributor_page.json");
const CONTRIBUTOR_NO_MODULE: &str =
    include_str!("fixtures/library/contributor_page_no_module.json");
const CONTRIBUTOR_DATA: &str = include_str!("fixtures/library/contributor_data_page.json");
const ARTIST_404: &str = include_str!("fixtures/library/error_artist_not_found_2001.json");
const CONTRIBUTOR_404: &str =
    include_str!("fixtures/library/error_contributor_not_found_2001.json");
const ETAG_7002: &str = include_str!("fixtures/library/error_playlist_etag_7002.json");
const CREATED: &str = include_str!("fixtures/library/created_playlist.json");
const ADD_OK: &str = include_str!("fixtures/library/add_items_ok.json");
const FAVORITES_IDS: &str = include_str!("fixtures/library/favorites_ids.json");
const ALBUM_404: &str = include_str!("fixtures/metadata/error_album_not_found_2001.json");
const PLAYLIST_404: &str = include_str!("fixtures/metadata/error_playlist_not_found_2001.json");
const SERVER_500: &str = include_str!("fixtures/metadata/error_server_500.json");
const TOKEN_401: &str = include_str!("fixtures/auth/api_unauthorized.json");
const INVALID_GRANT: &str = include_str!("fixtures/auth/refresh_invalid_grant.json");

const UUID: &str = "00000000-0000-4000-8000-0000000000a1";

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

fn words() -> Vec<String> {
    DEFAULT_HIDDEN_VERSIONS
        .iter()
        .map(|w| (*w).to_owned())
        .collect()
}

/// One request the server received.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Seen {
    method: String,
    path: String,
    /// Sorted.
    query: Vec<(String, String)>,
    if_none_match: Option<String>,
    body: String,
}

fn seen(
    method: &str,
    path: &str,
    query: &[(&str, &str)],
    if_none_match: Option<&str>,
    body: &str,
) -> Seen {
    Seen {
        method: method.into(),
        path: path.into(),
        query: sorted(query),
        if_none_match: if_none_match.map(Into::into),
        body: body.into(),
    }
}

fn sorted(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    v.sort();
    v
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

    async fn ask(&self, request: LibraryRequest) -> Result<LibraryResponse, LibraryError> {
        self.client.request(request, 100, &words()).await
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
                    method: r.method.to_string(),
                    path: r.url.path().to_owned(),
                    query,
                    if_none_match: r
                        .headers
                        .get("if-none-match")
                        .map(|v| v.to_str().unwrap().to_owned()),
                    body: String::from_utf8(r.body.clone()).unwrap(),
                }
            })
            .collect()
    }

    /// Mounts a `GET path` answering `status` and `body`; `filter` is the
    /// `filter` query parameter it needs (`Some("")`: none may be sent).
    async fn get(&self, p: &str, filter: Option<&str>, status: u16, body: &str) {
        let mock = Mock::given(method("GET")).and(path(p));
        let mock = match filter {
            Some("") => mock.and(query_param_is_missing("filter")),
            Some(f) => mock.and(query_param("filter", f)),
            None => mock,
        };
        mock.respond_with(json_body(status, body))
            .mount(&self.server)
            .await;
    }
}

fn fixture(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

fn artist(id: u64, name: &str) -> ArtistRef {
    ArtistRef {
        id,
        name: name.into(),
    }
}

/// What `track.json` maps to, with another ID, title and version.
fn trk(id: u64, title: &str, version: Option<&str>) -> Track {
    Track {
        id: TrackId(id),
        title: title.into(),
        version: version.map(Into::into),
        artists: vec![artist(3001, "Artist A"), artist(3002, "Artist B")],
        album: Some(AlbumRef {
            id: 2001,
            title: "Album One".into(),
        }),
        duration: Some(Duration::from_secs(291)),
        streamable: true,
    }
}

/// `track.json` with another ID and title.
fn track_json(id: u64) -> Value {
    let mut t = fixture(TRACK);
    t["id"] = json!(id);
    t["title"] = json!(format!("Track {id}"));
    t
}

fn named(id: u64) -> Track {
    trk(id, &format!("Track {id}"), None)
}

/// A list page of tracks `ids`, wrapped as the endpoint wraps them.
fn tracks_body(total: u64, ids: &[u64], wrap: &str) -> String {
    let items: Vec<Value> = ids
        .iter()
        .map(|&id| match wrap {
            "item" => json!({"created": "2026-03-02T10:00:00.000+0000", "item": track_json(id)}),
            "typed" => json!({"type": "track", "item": track_json(id)}),
            _ => track_json(id),
        })
        .collect();
    json!({"limit": 100, "offset": 0, "totalNumberOfItems": total, "items": items}).to_string()
}

fn track_page(items: &[u64], offset: u32, total: u32) -> ListItems {
    ListItems::Tracks(ListPage {
        items: items.iter().map(|&id| named(id)).collect(),
        offset,
        total,
        hidden: 0,
    })
}

fn album(id: u64, title: &str, year: u16, kind: AlbumKind, tracks: u32, secs: u64) -> AlbumSummary {
    AlbumSummary {
        id,
        title: title.into(),
        artists: vec![artist(3001, "Artist A")],
        year: Some(year),
        kind,
        tracks: Some(tracks),
        duration: Some(Duration::from_secs(secs)),
    }
}

fn album_one() -> AlbumSummary {
    album(2001, "Album One", 2014, AlbumKind::Album, 12, 2400)
}
fn album_three() -> AlbumSummary {
    album(2003, "Album Three", 2016, AlbumKind::Album, 10, 2000)
}
fn ep_four() -> AlbumSummary {
    album(2004, "EP Four", 2018, AlbumKind::Ep, 5, 1000)
}
fn single_two() -> AlbumSummary {
    album(2002, "Single Two", 2020, AlbumKind::Single, 1, 200)
}

fn album_page(items: Vec<AlbumSummary>, offset: u32, total: u32) -> ListItems {
    ListItems::Albums(ListPage {
        items,
        offset,
        total,
        hidden: 0,
    })
}

fn playlist_summary(uuid: &str, title: &str, tracks: u32, secs: u64, own: bool) -> PlaylistSummary {
    PlaylistSummary {
        uuid: uuid.into(),
        title: title.into(),
        tracks: Some(tracks),
        duration: Some(Duration::from_secs(secs)),
        own,
    }
}

fn running() -> PlaylistSummary {
    playlist_summary(UUID, "Running", 42, 9000, true)
}

fn late_night() -> PlaylistSummary {
    playlist_summary(
        "00000000-0000-4000-8000-0000000000a2",
        "Late night",
        17,
        3600,
        false,
    )
}

fn playlist_page(offset: u32, total: u32) -> ListPage<PlaylistSummary> {
    ListPage {
        items: vec![running(), late_night()],
        offset,
        total,
        hidden: 0,
    }
}

fn artist_page(offset: u32, total: u32) -> ListPage<ArtistRef> {
    ListPage {
        items: vec![artist(3001, "Artist A"), artist(3002, "Artist B")],
        offset,
        total,
        hidden: 0,
    }
}

fn more(list: ListRef, offset: u32, limit: u32) -> LibraryRequest {
    LibraryRequest::More {
        list,
        offset,
        limit,
    }
}

/// `countryCode=FI`, the favorites order and a page.
fn ordered(limit: &str, offset: &str) -> Vec<(&'static str, String)> {
    vec![
        ("countryCode", "FI".into()),
        ("order", "DATE".into()),
        ("orderDirection", "DESC".into()),
        ("limit", limit.into()),
        ("offset", offset.into()),
    ]
}

fn q(pairs: &[(&str, String)]) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect();
    v.sort();
    v
}

fn paged(limit: &str, offset: &str, extra: &[(&'static str, &str)]) -> Vec<(&'static str, String)> {
    let mut v: Vec<(&'static str, String)> = vec![
        ("countryCode", "FI".into()),
        ("limit", limit.into()),
        ("offset", offset.into()),
    ];
    v.extend(extra.iter().map(|(k, val)| (*k, val.to_string())));
    v
}

/// One `GET` the stub server answers, and the request it must receive.
struct Call {
    path: String,
    filter: Option<&'static str>,
    body: String,
    want_query: Vec<(&'static str, String)>,
}

fn call(
    p: &str,
    filter: Option<&'static str>,
    body: &str,
    want_query: Vec<(&'static str, String)>,
) -> Call {
    Call {
        path: p.into(),
        filter,
        body: body.into(),
        want_query,
    }
}

/// AC4: for every `ListRef` one request per `More` (two for the artist's
/// albums, see below) with the path, `countryCode`, the order parameters
/// where listed, `limit = min(asked, largest page)` and the given `offset`;
/// the envelope unwrapped; `total` from `totalNumberOfItems`.
#[tokio::test]
async fn ac4_lists() {
    struct Row {
        name: &'static str,
        list: ListRef,
        offset: u32,
        limit: u32,
        calls: Vec<Call>,
        want: ListItems,
    }
    let u = format!("playlists/{UUID}/items");
    let _ = u;
    let rows = vec![
        Row {
            name: "favorite tracks: clamped to 1000, offset kept, order sent",
            list: ListRef::FavoriteTracks,
            offset: 100,
            limit: 5000,
            calls: vec![call(
                "/v1/users/1/favorites/tracks",
                None,
                &tracks_body(362, &[1001, 1002], "item"),
                ordered("1000", "100"),
            )],
            want: track_page(&[1001, 1002], 100, 362),
        },
        Row {
            name: "playlists: 100 asked, 50 sent; own from the entry type",
            list: ListRef::Playlists,
            offset: 0,
            limit: 100,
            calls: vec![call(
                "/v1/users/1/playlistsAndFavoritePlaylists",
                None,
                PLAYLISTS,
                ordered("50", "0"),
            )],
            want: ListItems::Playlists(playlist_page(0, 12)),
        },
        Row {
            name: "playlists: a smaller ask is kept",
            list: ListRef::Playlists,
            offset: 50,
            limit: 20,
            calls: vec![call(
                "/v1/users/1/playlistsAndFavoritePlaylists",
                None,
                PLAYLISTS,
                ordered("20", "50"),
            )],
            want: ListItems::Playlists(playlist_page(50, 12)),
        },
        Row {
            name: "favorite albums: year, kind, tracks, duration",
            list: ListRef::FavoriteAlbums,
            offset: 0,
            limit: 100,
            calls: vec![call(
                "/v1/users/1/favorites/albums",
                None,
                FAVORITE_ALBUMS,
                ordered("100", "0"),
            )],
            want: ListItems::Albums(ListPage {
                items: vec![album_one(), single_two()],
                offset: 0,
                total: 14,
                hidden: 0,
            }),
        },
        Row {
            name: "favorite artists",
            list: ListRef::FavoriteArtists,
            offset: 0,
            limit: 1000,
            calls: vec![call(
                "/v1/users/1/favorites/artists",
                None,
                FAVORITE_ARTISTS,
                ordered("1000", "0"),
            )],
            want: ListItems::Artists(artist_page(0, 196)),
        },
        Row {
            name: "album tracks: bare tracks, no order",
            list: ListRef::AlbumTracks(2001),
            offset: 0,
            limit: 100,
            calls: vec![call(
                "/v1/albums/2001/tracks",
                None,
                &tracks_body(2, &[1001, 1002], "bare"),
                paged("100", "0", &[]),
            )],
            want: track_page(&[1001, 1002], 0, 2),
        },
        Row {
            name: "playlist tracks: 100 is the largest page, videos dropped",
            list: ListRef::PlaylistTracks(UUID.into()),
            offset: 100,
            limit: 500,
            calls: vec![call(
                &format!("/v1/playlists/{UUID}/items"),
                None,
                PLAYLIST_ITEMS,
                paged("100", "100", &[]),
            )],
            want: track_page(&[1001, 1002], 100, 3),
        },
        Row {
            name: "top tracks",
            list: ListRef::TopTracks(3001),
            offset: 0,
            limit: 100,
            calls: vec![call(
                "/v1/artists/3001/toptracks",
                None,
                &tracks_body(91, &[1001, 1002], "bare"),
                paged("100", "0", &[]),
            )],
            want: track_page(&[1001, 1002], 0, 91),
        },
        Row {
            name: "appears on: the COMPILATIONS filter",
            list: ListRef::ArtistAppearsOn(3001),
            offset: 0,
            limit: 100,
            calls: vec![call(
                "/v1/artists/3001/albums",
                Some("COMPILATIONS"),
                ALBUMS_COMPILATIONS,
                paged("100", "0", &[("filter", "COMPILATIONS")]),
            )],
            want: ListItems::Albums(ListPage {
                items: vec![AlbumSummary {
                    id: 2005,
                    title: "Hits".into(),
                    artists: vec![artist(3999, "Various Artists")],
                    year: Some(2012),
                    // Not an EP or a single: shown as an album.
                    kind: AlbumKind::Album,
                    tracks: Some(20),
                    duration: Some(Duration::from_secs(4000)),
                }],
                offset: 0,
                total: 30,
                hidden: 0,
            }),
        },
        // The artist's albums are the plain list, then EPs and singles. Both
        // lists are paged by the page size asked: the plain list (12 albums)
        // takes the offsets below ceil(12 / limit) * limit, the EPs and
        // singles the offsets after, counted from 0. A plain page costs a
        // second request that only learns the EP total (`limit=1`).
        Row {
            name: "artist albums: the plain list, total the sum",
            list: ListRef::ArtistAlbums(3001),
            offset: 0,
            limit: 100,
            calls: vec![
                call(
                    "/v1/artists/3001/albums",
                    Some(""),
                    ALBUMS_PLAIN,
                    paged("100", "0", &[]),
                ),
                call(
                    "/v1/artists/3001/albums",
                    Some("EPSANDSINGLES"),
                    ALBUMS_EPS,
                    paged("1", "0", &[("filter", "EPSANDSINGLES")]),
                ),
            ],
            want: album_page(vec![album_one(), album_three()], 0, 78),
        },
        Row {
            name: "artist albums: the plain list is exhausted, EPs and singles from 0",
            list: ListRef::ArtistAlbums(3001),
            offset: 100,
            limit: 100,
            calls: vec![
                call(
                    "/v1/artists/3001/albums",
                    Some(""),
                    ALBUMS_EMPTY
                        .replace("\"totalNumberOfItems\": 0", "\"totalNumberOfItems\": 12")
                        .as_str(),
                    paged("100", "100", &[]),
                ),
                call(
                    "/v1/artists/3001/albums",
                    Some("EPSANDSINGLES"),
                    ALBUMS_EPS,
                    paged("100", "0", &[("filter", "EPSANDSINGLES")]),
                ),
            ],
            want: album_page(vec![ep_four(), single_two()], 100, 78),
        },
        Row {
            name: "artist albums: the second EP page",
            list: ListRef::ArtistAlbums(3001),
            offset: 200,
            limit: 100,
            calls: vec![
                call(
                    "/v1/artists/3001/albums",
                    Some(""),
                    ALBUMS_EMPTY
                        .replace("\"totalNumberOfItems\": 0", "\"totalNumberOfItems\": 12")
                        .as_str(),
                    paged("100", "200", &[]),
                ),
                call(
                    "/v1/artists/3001/albums",
                    Some("EPSANDSINGLES"),
                    ALBUMS_EPS,
                    paged("100", "100", &[("filter", "EPSANDSINGLES")]),
                ),
            ],
            want: album_page(vec![ep_four(), single_two()], 200, 78),
        },
    ];
    for row in rows {
        let s = Setup::new().await;
        for c in &row.calls {
            s.get(&c.path, c.filter, 200, &c.body).await;
        }
        let got = s
            .client
            .request(more(row.list.clone(), row.offset, row.limit), 100, &words())
            .await;
        assert_eq!(got, Ok(LibraryResponse::Items(row.want)), "{}", row.name);
        let mut got_seen = s.seen().await;
        let mut want_seen: Vec<Seen> = row
            .calls
            .iter()
            .map(|c| Seen {
                method: "GET".into(),
                path: c.path.clone(),
                query: q(&c.want_query),
                if_none_match: None,
                body: String::new(),
            })
            .collect();
        got_seen.sort_by(|a, b| a.query.cmp(&b.query));
        want_seen.sort_by(|a, b| a.query.cmp(&b.query));
        assert_eq!(got_seen, want_seen, "{}: requests", row.name);
    }
}

/// AC4: a list's own `404`/`2001` is the item's not-found message, other
/// failures pass through, and a malformed body is named.
#[tokio::test]
async fn ac4_list_errors() {
    let rows: [(&str, ListRef, u16, &str, &str); 5] = [
        (
            "album",
            ListRef::AlbumTracks(1),
            404,
            ALBUM_404,
            "Album 1 was not found",
        ),
        (
            "playlist",
            ListRef::PlaylistTracks(UUID.into()),
            404,
            PLAYLIST_404,
            &format!("Playlist {UUID} was not found"),
        ),
        (
            "artist top tracks",
            ListRef::TopTracks(1),
            404,
            ARTIST_404,
            "Artist 1 was not found",
        ),
        (
            "429",
            ListRef::FavoriteTracks,
            429,
            "{}",
            "Tidal answered 429: try again in a moment",
        ),
        (
            "500",
            ListRef::FavoriteAlbums,
            500,
            SERVER_500,
            "Tidal answered 500",
        ),
    ];
    for (name, list, status, body, message) in rows {
        let s = Setup::new().await;
        Mock::given(method("GET"))
            .respond_with(json_body(status, body))
            .mount(&s.server)
            .await;
        let got = s.ask(more(list, 0, 100)).await;
        assert_eq!(got.unwrap_err().to_string(), message, "{name}");
    }
    let s = Setup::new().await;
    s.get("/v1/users/1/favorites/tracks", None, 200, "{\"items\": 3}")
        .await;
    let got = s.ask(more(ListRef::FavoriteTracks, 0, 100)).await;
    assert!(matches!(got, Err(LibraryError::Malformed(_))), "{got:?}");
}

/// AC5: a page is its header and the first page of each list, from
/// parallel requests; the playlist's `ETag` is kept; the first page asks
/// `min(page size, largest)`.
#[tokio::test]
async fn ac5_pages() {
    // Library: three lists.
    let s = Setup::new().await;
    s.get(
        "/v1/users/1/playlistsAndFavoritePlaylists",
        None,
        200,
        PLAYLISTS,
    )
    .await;
    s.get("/v1/users/1/favorites/albums", None, 200, FAVORITE_ALBUMS)
        .await;
    s.get("/v1/users/1/favorites/artists", None, 200, FAVORITE_ARTISTS)
        .await;
    let got = s
        .client
        .request(LibraryRequest::Page(PageRequest::Library), 100, &words())
        .await
        .unwrap();
    let LibraryResponse::Page(PageData::Library {
        playlists,
        albums,
        artists,
    }) = got
    else {
        panic!("not a library page: {got:?}");
    };
    assert_eq!(playlists, playlist_page(0, 12));
    assert_eq!(
        (albums.items, albums.total),
        (vec![album_one(), single_two()], 14)
    );
    assert_eq!(artists, artist_page(0, 196));
    let mut limits: Vec<(String, String)> = s
        .seen()
        .await
        .iter()
        .map(|r| {
            let limit = r
                .query
                .iter()
                .find(|(k, _)| k == "limit")
                .unwrap()
                .1
                .clone();
            (r.path.clone(), limit)
        })
        .collect();
    limits.sort();
    assert_eq!(
        limits,
        [
            ("/v1/users/1/favorites/albums".to_owned(), "100".to_owned()),
            ("/v1/users/1/favorites/artists".to_owned(), "100".to_owned()),
            (
                "/v1/users/1/playlistsAndFavoritePlaylists".to_owned(),
                "50".to_owned()
            ),
        ],
        "page size 100, 50 for playlists"
    );

    // Favorite tracks: the first page at the page size.
    let s = Setup::new().await;
    s.get(
        "/v1/users/1/favorites/tracks",
        None,
        200,
        &tracks_body(362, &[1001, 1002], "item"),
    )
    .await;
    let got = s
        .client
        .request(
            LibraryRequest::Page(PageRequest::FavoriteTracks),
            7,
            &words(),
        )
        .await;
    let LibraryResponse::Page(PageData::FavoriteTracks { tracks }) = got.unwrap() else {
        panic!("not favorite tracks");
    };
    assert_eq!(tracks.total, 362);
    assert_eq!(tracks.items, vec![named(1001), named(1002)]);
    assert_eq!(
        s.seen().await[0].query,
        sorted(&[
            ("countryCode", "FI"),
            ("order", "DATE"),
            ("orderDirection", "DESC"),
            ("limit", "7"),
            ("offset", "0"),
        ])
    );

    // Album.
    let s = Setup::new().await;
    s.get("/v1/albums/2001", None, 200, ALBUM).await;
    s.get(
        "/v1/albums/2001/tracks",
        None,
        200,
        &tracks_body(12, &[1001, 1002], "bare"),
    )
    .await;
    let got = s
        .ask(LibraryRequest::Page(PageRequest::Album(2001)))
        .await
        .unwrap();
    assert_eq!(
        got,
        LibraryResponse::Page(PageData::Album {
            album: album_one(),
            tracks: ListPage {
                items: vec![named(1001), named(1002)],
                offset: 0,
                total: 12,
                hidden: 0
            },
        })
    );

    // Playlist: the ETag header is kept.
    let s = Setup::new().await;
    Mock::given(method("GET"))
        .and(path(format!("/v1/playlists/{UUID}")))
        .respond_with(json_body(200, PLAYLIST).insert_header("ETag", "\"1790000000000\""))
        .mount(&s.server)
        .await;
    s.get(
        &format!("/v1/playlists/{UUID}/items"),
        None,
        200,
        PLAYLIST_ITEMS,
    )
    .await;
    let got = s
        .ask(LibraryRequest::Page(PageRequest::Playlist(UUID.into())))
        .await
        .unwrap();
    assert_eq!(
        got,
        LibraryResponse::Page(PageData::Playlist {
            playlist: running(),
            etag: Some("\"1790000000000\"".into()),
            tracks: ListPage {
                items: vec![named(1001), named(1002)],
                offset: 0,
                total: 3,
                hidden: 0
            },
        })
    );
    let s = Setup::new().await;
    s.get(&format!("/v1/playlists/{UUID}"), None, 200, PLAYLIST)
        .await;
    s.get(
        &format!("/v1/playlists/{UUID}/items"),
        None,
        200,
        PLAYLIST_ITEMS,
    )
    .await;
    let got = s
        .ask(LibraryRequest::Page(PageRequest::Playlist(UUID.into())))
        .await
        .unwrap();
    let LibraryResponse::Page(PageData::Playlist { etag, .. }) = got else {
        panic!("not a playlist page");
    };
    assert_eq!(etag, None, "no ETag header, none kept");

    // Artist: header, top tracks, albums, appears on (not the credits).
    let s = Setup::new().await;
    s.get("/v1/artists/3001", None, 200, ARTIST).await;
    s.get(
        "/v1/artists/3001/toptracks",
        None,
        200,
        &tracks_body(91, &[1001], "bare"),
    )
    .await;
    s.get("/v1/artists/3001/albums", Some(""), 200, ALBUMS_PLAIN)
        .await;
    s.get(
        "/v1/artists/3001/albums",
        Some("EPSANDSINGLES"),
        200,
        ALBUMS_EPS,
    )
    .await;
    s.get(
        "/v1/artists/3001/albums",
        Some("COMPILATIONS"),
        200,
        ALBUMS_COMPILATIONS,
    )
    .await;
    let got = s
        .ask(LibraryRequest::Page(PageRequest::Artist(3001)))
        .await
        .unwrap();
    let LibraryResponse::Page(PageData::Artist {
        artist: header,
        top_tracks,
        albums,
        appears_on,
    }) = got
    else {
        panic!("not an artist page");
    };
    assert_eq!(header, artist(3001, "Artist A"));
    assert_eq!(
        (top_tracks.items, top_tracks.total),
        (vec![named(1001)], 91)
    );
    assert_eq!(albums.items, vec![album_one(), album_three()]);
    assert_eq!(albums.total, 78);
    assert_eq!((appears_on.items.len(), appears_on.total), (1, 30));
    assert!(
        s.seen()
            .await
            .iter()
            .all(|r| !r.path.contains("contributor")),
        "the credits are fetched when first focused"
    );
}

/// AC5: errors. `404`/`2001` on the item's own resource is its not-found
/// message, an unknown artist whose `/albums` is a `200` with no items still
/// fails from `/artists/{id}`, and any failing call fails the page.
#[tokio::test]
async fn ac5_page_errors() {
    // Unknown album, playlist.
    let rows: [(&str, PageRequest, &str, &str, String); 2] = [
        (
            "album",
            PageRequest::Album(1),
            "/v1/albums/1",
            ALBUM_404,
            "Album 1 was not found".into(),
        ),
        (
            "playlist",
            PageRequest::Playlist(UUID.into()),
            &format!("/v1/playlists/{UUID}"),
            PLAYLIST_404,
            format!("Playlist {UUID} was not found"),
        ),
    ];
    for (name, request, header_path, body, message) in rows {
        let s = Setup::new().await;
        s.get(header_path, None, 404, body).await;
        // The list answers something else: the header still decides.
        Mock::given(method("GET"))
            .and(path_regex_items())
            .respond_with(json_body(200, &tracks_body(0, &[], "typed")))
            .mount(&s.server)
            .await;
        let got = s.ask(LibraryRequest::Page(request)).await;
        assert_eq!(got.unwrap_err().to_string(), message, "{name}");
    }

    // Unknown artist: the header 404s while /albums is a 200 with no items.
    let s = Setup::new().await;
    s.get("/v1/artists/9", None, 404, ARTIST_404).await;
    s.get("/v1/artists/9/albums", None, 200, ALBUMS_EMPTY).await;
    s.get(
        "/v1/artists/9/toptracks",
        None,
        200,
        &tracks_body(0, &[], "bare"),
    )
    .await;
    let got = s.ask(LibraryRequest::Page(PageRequest::Artist(9))).await;
    assert_eq!(
        got,
        Err(LibraryError::NotFound(
            tidal_player_api::library::Subject::Artist(9)
        ))
    );
    assert_eq!(got.unwrap_err().to_string(), "Artist 9 was not found");

    // Any failing call fails the page: the header is fine, a list is not.
    let s = Setup::new().await;
    s.get("/v1/artists/3001", None, 200, ARTIST).await;
    s.get("/v1/artists/3001/toptracks", None, 500, SERVER_500)
        .await;
    s.get("/v1/artists/3001/albums", None, 200, ALBUMS_PLAIN)
        .await;
    let got = s.ask(LibraryRequest::Page(PageRequest::Artist(3001))).await;
    assert_eq!(got.unwrap_err().to_string(), "Tidal answered 500");

    let s = Setup::new().await;
    s.get("/v1/albums/2001", None, 200, ALBUM).await;
    s.get("/v1/albums/2001/tracks", None, 429, "{}").await;
    let got = s.ask(LibraryRequest::Page(PageRequest::Album(2001))).await;
    assert_eq!(
        got.unwrap_err().to_string(),
        "Tidal answered 429: try again in a moment"
    );

    // The library page: one failing list fails it.
    let s = Setup::new().await;
    s.get(
        "/v1/users/1/playlistsAndFavoritePlaylists",
        None,
        200,
        PLAYLISTS,
    )
    .await;
    s.get("/v1/users/1/favorites/albums", None, 200, FAVORITE_ALBUMS)
        .await;
    s.get("/v1/users/1/favorites/artists", None, 500, SERVER_500)
        .await;
    let got = s.ask(LibraryRequest::Page(PageRequest::Library)).await;
    assert!(got.is_err(), "{got:?}");
}

fn path_regex_items() -> wiremock::matchers::PathRegexMatcher {
    wiremock::matchers::path_regex(r"^/v1/(playlists|albums)/[^/]+/(items|tracks)$")
}

const CONTRIBUTOR_QUERY: [(&str, &str); 4] = [
    ("artistId", "3001"),
    ("countryCode", "FI"),
    ("deviceType", "BROWSER"),
    ("locale", "en_US"),
];

fn credited(t: Track, roles: &[RoleCategory]) -> CreditedTrack {
    CreditedTrack {
        track: t,
        roles: roles.to_vec(),
    }
}

fn fixture_track(id: u64, title: &str, version: Option<&str>) -> Track {
    trk(id, title, version)
}

/// AC6: the first page from `/pages/contributor`, role categories mapped
/// (unknown ids dropped), tracks without `STEREO` dropped, alternate versions
/// hidden by the words, the hidden count reported.
#[tokio::test]
async fn ac6_credits() {
    let s = Setup::new().await;
    s.get("/v1/pages/contributor", None, 200, CONTRIBUTOR).await;
    let got = s.ask(more(ListRef::Credits(3001), 0, 100)).await.unwrap();
    assert_eq!(
        got,
        LibraryResponse::Items(ListItems::Credits(ListPage {
            items: vec![
                credited(
                    fixture_track(1001, "Track One", None),
                    &[RoleCategory::Performer, RoleCategory::Songwriter]
                ),
                // STEREO and Atmos: kept; role 99 is unknown.
                credited(
                    fixture_track(1004, "Track Four", None),
                    &[RoleCategory::Producer, RoleCategory::Engineer]
                ),
                credited(
                    fixture_track(1005, "Track Five", Some("Acoustic")),
                    &[RoleCategory::Songwriter]
                ),
            ],
            offset: 0,
            total: 548,
            // The Atmos-only copy and the instrumental.
            hidden: 2,
        }))
    );
    assert_eq!(
        s.seen().await,
        [seen(
            "GET",
            "/v1/pages/contributor",
            &CONTRIBUTOR_QUERY,
            None,
            ""
        )]
    );

    // An empty word list hides only the Atmos-only copy.
    let got = s
        .client
        .request(more(ListRef::Credits(3001), 0, 100), 100, &[])
        .await
        .unwrap();
    let LibraryResponse::Items(ListItems::Credits(page)) = got else {
        panic!("not credits");
    };
    assert_eq!(
        page.items.iter().map(|c| c.track.id.0).collect::<Vec<_>>(),
        [1001, 1003, 1004, 1005]
    );
    assert_eq!(page.hidden, 1);

    // A smaller ask than Tidal's 50 keeps the first rows only.
    let got = s
        .client
        .request(more(ListRef::Credits(3001), 0, 2), 100, &words())
        .await
        .unwrap();
    let LibraryResponse::Items(ListItems::Credits(page)) = got else {
        panic!("not credits");
    };
    assert_eq!(
        page.items.iter().map(|c| c.track.id.0).collect::<Vec<_>>(),
        [1001],
        "rows 1 and 2 of the page, the second being Atmos-only"
    );
    assert_eq!(page.hidden, 1);
}

/// AC6: later pages come from the `dataApiPath` with `limit` (at most 50)
/// and `offset` appended; the path is read once per artist (cached), and
/// read again once when the cached one fails.
#[tokio::test]
async fn ac6_credits_data_pages() {
    let data_query = |offset: &'static str| -> Vec<(&'static str, &'static str)> {
        vec![
            ("artistId", "3001"),
            ("countryCode", "FI"),
            ("deviceType", "BROWSER"),
            ("locale", "en_US"),
            ("limit", "50"),
            ("offset", offset),
        ]
    };
    let data_path = "/v1/pages/data/00000000-0000-4000-8000-0000000000c1";
    let page_two = LibraryResponse::Items(ListItems::Credits(ListPage {
        items: vec![credited(
            fixture_track(1007, "Track Seven", None),
            &[RoleCategory::Songwriter, RoleCategory::Producer],
        )],
        offset: 50,
        total: 548,
        // TV Size.
        hidden: 1,
    }));

    // A fresh client at offset 50: the contributor page for the path first.
    let s = Setup::new().await;
    s.get("/v1/pages/contributor", None, 200, CONTRIBUTOR).await;
    s.get(data_path, None, 200, CONTRIBUTOR_DATA).await;
    let got = s.ask(more(ListRef::Credits(3001), 50, 100)).await.unwrap();
    assert_eq!(got, page_two);
    assert_eq!(
        s.seen().await,
        [
            seen("GET", "/v1/pages/contributor", &CONTRIBUTOR_QUERY, None, ""),
            seen("GET", data_path, &data_query("50"), None, ""),
        ]
    );

    // The path is cached: the next page reads no contributor page.
    let got = s.ask(more(ListRef::Credits(3001), 100, 100)).await.unwrap();
    let LibraryResponse::Items(ListItems::Credits(page)) = got else {
        panic!("not credits");
    };
    assert_eq!(page.offset, 100);
    let requests = s.seen().await;
    assert_eq!(requests.len(), 3, "{requests:?}");
    assert_eq!(
        requests[2],
        seen("GET", data_path, &data_query("100"), None, "")
    );

    // A cached path that fails (404) is read again once.
    let s = Setup::new().await;
    Mock::given(method("GET"))
        .and(path("/v1/pages/contributor"))
        .respond_with(json_body(
            200,
            &CONTRIBUTOR.replace("0000000000c1", "0000000000c2"),
        ))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&s.server)
        .await;
    s.get("/v1/pages/contributor", None, 200, CONTRIBUTOR).await;
    s.get(
        "/v1/pages/data/00000000-0000-4000-8000-0000000000c2",
        None,
        404,
        CONTRIBUTOR_404,
    )
    .await;
    s.get(data_path, None, 200, CONTRIBUTOR_DATA).await;
    let got = s.ask(more(ListRef::Credits(3001), 50, 100)).await.unwrap();
    assert_eq!(got, page_two, "the path is re-read when the first one 404s");

    // A page with no such module: no credits.
    let s = Setup::new().await;
    s.get("/v1/pages/contributor", None, 200, CONTRIBUTOR_NO_MODULE)
        .await;
    for offset in [0, 50] {
        let got = s
            .ask(more(ListRef::Credits(3001), offset, 100))
            .await
            .unwrap();
        assert_eq!(
            got,
            LibraryResponse::Items(ListItems::Credits(ListPage {
                items: vec![],
                offset,
                total: 0,
                hidden: 0
            })),
            "offset {offset}"
        );
    }

    // An unknown artist.
    let s = Setup::new().await;
    s.get("/v1/pages/contributor", None, 404, CONTRIBUTOR_404)
        .await;
    let got = s.ask(more(ListRef::Credits(3001), 0, 100)).await;
    assert_eq!(got.unwrap_err().to_string(), "Artist 3001 was not found");
    let got = s
        .ask(LibraryRequest::More {
            list: ListRef::Credits(3001),
            offset: 50,
            limit: 100,
        })
        .await;
    assert_eq!(got.unwrap_err().to_string(), "Artist 3001 was not found");
}

fn id_of(kind: FavoriteKind) -> (&'static str, &'static str, &'static str) {
    // (path segment, form field, an ID)
    match kind {
        FavoriteKind::Track => ("tracks", "trackIds", "1001"),
        FavoriteKind::Album => ("albums", "albumIds", "2001"),
        FavoriteKind::Artist => ("artists", "artistIds", "3001"),
        FavoriteKind::Playlist => ("playlists", "uuids", UUID),
    }
}

/// How a write test's server answers one endpoint, in order.
struct Answer {
    method: &'static str,
    path: String,
    status: u16,
    body: &'static str,
    etag: Option<&'static str>,
}

fn answer(
    method: &'static str,
    p: &str,
    status: u16,
    body: &'static str,
    etag: Option<&'static str>,
) -> Answer {
    Answer {
        method,
        path: p.into(),
        status,
        body,
        etag,
    }
}

/// Mounts `answers`; for one method and path the first is used first.
async fn mount_in_order(s: &Setup, answers: &[Answer]) {
    for (i, a) in answers.iter().enumerate() {
        let mut response = json_body(a.status, a.body);
        if let Some(etag) = a.etag {
            response = response.insert_header("ETag", etag);
        }
        let later = answers[i + 1..]
            .iter()
            .any(|b| b.method == a.method && b.path == a.path);
        let mock = Mock::given(method(a.method))
            .and(path(a.path.clone()))
            .respond_with(response);
        if later {
            mock.up_to_n_times(1).mount(&s.server).await;
        } else {
            mock.mount(&s.server).await;
        }
    }
}

fn add_body(first: u64, n: u64, dupes: bool) -> String {
    let ids: Vec<String> = (first..first + n).map(|i| i.to_string()).collect();
    let mut body = format!("trackIds={}", ids.join("%2C"));
    if dupes {
        body.push_str("&onDupes=ADD");
    }
    body
}

fn ids(first: u64, n: u64) -> Vec<TrackId> {
    (first..first + n).map(TrackId).collect()
}

const CC: [(&str, &str); 1] = [("countryCode", "FI")];
const E1: &str = "\"1790000000001\"";
const E2: &str = "\"1790000000002\"";
const E3: &str = "\"1790000000003\"";
const E4: &str = "\"1790000000004\"";

/// AC7: every write, its request on the wire and its result.
#[tokio::test]
async fn ac7_writes() {
    struct Row {
        name: String,
        request: LibraryRequest,
        answers: Vec<Answer>,
        want_requests: Vec<Seen>,
        want: Result<LibraryResponse, String>,
    }
    let items_path = format!("/v1/playlists/{UUID}/items");
    let playlist_path = format!("/v1/playlists/{UUID}");
    let mut rows: Vec<Row> = vec![];

    for kind in [
        FavoriteKind::Track,
        FavoriteKind::Album,
        FavoriteKind::Artist,
        FavoriteKind::Playlist,
    ] {
        let (segment, field, id) = id_of(kind);
        let collection = format!("/v1/users/1/favorites/{segment}");
        rows.push(Row {
            name: format!("add favorite {kind:?}"),
            request: LibraryRequest::AddFavorite(kind, id.into()),
            answers: vec![answer("POST", &collection, 200, "", Some(E1))],
            want_requests: vec![seen(
                "POST",
                &collection,
                &CC,
                None,
                &format!("{field}={id}"),
            )],
            want: Ok(LibraryResponse::Done),
        });
        rows.push(Row {
            name: format!("remove favorite {kind:?}"),
            request: LibraryRequest::RemoveFavorite(kind, id.into()),
            answers: vec![answer(
                "DELETE",
                &format!("{collection}/{id}"),
                200,
                "",
                None,
            )],
            want_requests: vec![seen("DELETE", &format!("{collection}/{id}"), &CC, None, "")],
            want: Ok(LibraryResponse::Done),
        });
    }
    rows.push(Row {
        name: "favorite of an unknown track".into(),
        request: LibraryRequest::AddFavorite(FavoriteKind::Track, "1".into()),
        answers: vec![answer(
            "POST",
            "/v1/users/1/favorites/tracks",
            404,
            ALBUM_404,
            None,
        )],
        want_requests: vec![seen(
            "POST",
            "/v1/users/1/favorites/tracks",
            &CC,
            None,
            "trackIds=1",
        )],
        want: Err("Track 1 was not found".into()),
    });
    rows.push(Row {
        name: "a track ID that is not a number".into(),
        request: LibraryRequest::AddFavorite(FavoriteKind::Track, "x".into()),
        answers: vec![],
        want_requests: vec![],
        want: Err("Not a valid ID: x".into()),
    });
    rows.push(Row {
        name: "create a playlist".into(),
        request: LibraryRequest::CreatePlaylist {
            title: "My mix".into(),
        },
        answers: vec![answer(
            "POST",
            "/v1/users/1/playlists",
            201,
            CREATED,
            Some(E1),
        )],
        want_requests: vec![seen(
            "POST",
            "/v1/users/1/playlists",
            &CC,
            None,
            "title=My+mix&description=",
        )],
        want: Ok(LibraryResponse::Created(PlaylistSummary {
            uuid: "00000000-0000-4000-8000-0000000000b1".into(),
            title: "My mix".into(),
            tracks: Some(0),
            duration: Some(Duration::ZERO),
            own: true,
        })),
    });
    rows.push(Row {
        name: "add 3 tracks: ETag read, sent, no onDupes".into(),
        request: LibraryRequest::AddToPlaylist {
            uuid: UUID.into(),
            tracks: ids(1, 3),
            allow_duplicates: false,
        },
        answers: vec![
            answer("GET", &playlist_path, 200, PLAYLIST, Some(E1)),
            answer("POST", &items_path, 200, ADD_OK, Some(E2)),
        ],
        want_requests: vec![
            seen("GET", &playlist_path, &CC, None, ""),
            seen("POST", &items_path, &CC, Some(E1), &add_body(1, 3, false)),
        ],
        want: Ok(LibraryResponse::Done),
    });
    rows.push(Row {
        name: "add with duplicates allowed".into(),
        request: LibraryRequest::AddToPlaylist {
            uuid: UUID.into(),
            tracks: ids(1, 2),
            allow_duplicates: true,
        },
        answers: vec![
            answer("GET", &playlist_path, 200, PLAYLIST, Some(E1)),
            answer("POST", &items_path, 200, ADD_OK, Some(E2)),
        ],
        want_requests: vec![
            seen("GET", &playlist_path, &CC, None, ""),
            seen("POST", &items_path, &CC, Some(E1), &add_body(1, 2, true)),
        ],
        want: Ok(LibraryResponse::Done),
    });
    rows.push(Row {
        name: "250 tracks: 3 requests, each with the ETag the previous answered".into(),
        request: LibraryRequest::AddToPlaylist {
            uuid: UUID.into(),
            tracks: ids(1, 250),
            allow_duplicates: false,
        },
        answers: vec![
            answer("GET", &playlist_path, 200, PLAYLIST, Some(E1)),
            answer("POST", &items_path, 200, ADD_OK, Some(E2)),
            answer("POST", &items_path, 200, ADD_OK, Some(E3)),
            answer("POST", &items_path, 200, ADD_OK, Some(E4)),
        ],
        want_requests: vec![
            seen("GET", &playlist_path, &CC, None, ""),
            seen("POST", &items_path, &CC, Some(E1), &add_body(1, 100, false)),
            seen(
                "POST",
                &items_path,
                &CC,
                Some(E2),
                &add_body(101, 100, false),
            ),
            seen(
                "POST",
                &items_path,
                &CC,
                Some(E3),
                &add_body(201, 50, false),
            ),
        ],
        want: Ok(LibraryResponse::Done),
    });
    rows.push(Row {
        name: "no tracks: nothing sent".into(),
        request: LibraryRequest::AddToPlaylist {
            uuid: UUID.into(),
            tracks: vec![],
            allow_duplicates: false,
        },
        answers: vec![],
        want_requests: vec![],
        want: Ok(LibraryResponse::Done),
    });
    rows.push(Row {
        name: "one 412: the ETag is read again and the add retried once".into(),
        request: LibraryRequest::AddToPlaylist {
            uuid: UUID.into(),
            tracks: ids(1, 2),
            allow_duplicates: false,
        },
        answers: vec![
            answer("GET", &playlist_path, 200, PLAYLIST, Some(E1)),
            answer("GET", &playlist_path, 200, PLAYLIST, Some(E2)),
            answer("POST", &items_path, 412, ETAG_7002, None),
            answer("POST", &items_path, 200, ADD_OK, Some(E3)),
        ],
        want_requests: vec![
            seen("GET", &playlist_path, &CC, None, ""),
            seen("POST", &items_path, &CC, Some(E1), &add_body(1, 2, false)),
            seen("GET", &playlist_path, &CC, None, ""),
            seen("POST", &items_path, &CC, Some(E2), &add_body(1, 2, false)),
        ],
        want: Ok(LibraryResponse::Done),
    });
    rows.push(Row {
        name: "a second 412: the changed-playlist message".into(),
        request: LibraryRequest::AddToPlaylist {
            uuid: UUID.into(),
            tracks: ids(1, 2),
            allow_duplicates: false,
        },
        answers: vec![
            answer("GET", &playlist_path, 200, PLAYLIST, Some(E1)),
            answer("GET", &playlist_path, 200, PLAYLIST, Some(E2)),
            answer("POST", &items_path, 412, ETAG_7002, None),
            answer("POST", &items_path, 412, ETAG_7002, None),
        ],
        want_requests: vec![
            seen("GET", &playlist_path, &CC, None, ""),
            seen("POST", &items_path, &CC, Some(E1), &add_body(1, 2, false)),
            seen("GET", &playlist_path, &CC, None, ""),
            seen("POST", &items_path, &CC, Some(E2), &add_body(1, 2, false)),
        ],
        want: Err("The playlist changed: nothing was changed, try again".into()),
    });
    rows.push(Row {
        name: "add to an unknown playlist".into(),
        request: LibraryRequest::AddToPlaylist {
            uuid: UUID.into(),
            tracks: ids(1, 2),
            allow_duplicates: false,
        },
        answers: vec![answer("GET", &playlist_path, 404, PLAYLIST_404, None)],
        want_requests: vec![seen("GET", &playlist_path, &CC, None, "")],
        want: Err(format!("Playlist {UUID} was not found")),
    });
    rows.push(Row {
        name: "remove: the given position and ETag".into(),
        request: LibraryRequest::RemoveFromPlaylist {
            uuid: UUID.into(),
            index: 3,
            etag: E1.into(),
        },
        answers: vec![answer(
            "DELETE",
            &format!("{items_path}/3"),
            200,
            "",
            Some(E2),
        )],
        want_requests: vec![seen(
            "DELETE",
            &format!("{items_path}/3"),
            &CC,
            Some(E1),
            "",
        )],
        want: Ok(LibraryResponse::Done),
    });
    rows.push(Row {
        name: "remove on 412: the message, no retry".into(),
        request: LibraryRequest::RemoveFromPlaylist {
            uuid: UUID.into(),
            index: 3,
            etag: E1.into(),
        },
        answers: vec![answer(
            "DELETE",
            &format!("{items_path}/3"),
            412,
            ETAG_7002,
            None,
        )],
        want_requests: vec![seen(
            "DELETE",
            &format!("{items_path}/3"),
            &CC,
            Some(E1),
            "",
        )],
        want: Err("The playlist changed: nothing was changed, try again".into()),
    });
    rows.push(Row {
        name: "delete: ETag read, then sent".into(),
        request: LibraryRequest::DeletePlaylist { uuid: UUID.into() },
        answers: vec![
            answer("GET", &playlist_path, 200, PLAYLIST, Some(E1)),
            answer("DELETE", &playlist_path, 204, "", None),
        ],
        want_requests: vec![
            seen("GET", &playlist_path, &CC, None, ""),
            seen("DELETE", &playlist_path, &CC, Some(E1), ""),
        ],
        want: Ok(LibraryResponse::Done),
    });
    rows.push(Row {
        name: "delete on 412: the message, no retry".into(),
        request: LibraryRequest::DeletePlaylist { uuid: UUID.into() },
        answers: vec![
            answer("GET", &playlist_path, 200, PLAYLIST, Some(E1)),
            answer("DELETE", &playlist_path, 412, ETAG_7002, None),
        ],
        want_requests: vec![
            seen("GET", &playlist_path, &CC, None, ""),
            seen("DELETE", &playlist_path, &CC, Some(E1), ""),
        ],
        want: Err("The playlist changed: nothing was changed, try again".into()),
    });

    for row in rows {
        let s = Setup::new().await;
        mount_in_order(&s, &row.answers).await;
        let got = s.ask(row.request).await.map_err(|e| e.to_string());
        assert_eq!(got, row.want, "{}", row.name);
        assert_eq!(s.seen().await, row.want_requests, "{}: requests", row.name);
    }
}

/// AC7: `IsFavorite` reads `/users/{user}/favorites/ids` (a shape that is
/// assumed, see the fixtures README).
#[tokio::test]
async fn ac7_is_favorite() {
    let rows = [
        (FavoriteKind::Track, "1001", true),
        (FavoriteKind::Track, "1003", false),
        (FavoriteKind::Album, "2001", true),
        (FavoriteKind::Album, "1001", false),
        (FavoriteKind::Artist, "3001", true),
        (
            FavoriteKind::Playlist,
            "00000000-0000-4000-8000-0000000000a2",
            true,
        ),
        (FavoriteKind::Playlist, UUID, false),
    ];
    for (kind, id, want) in rows {
        let s = Setup::new().await;
        s.get("/v1/users/1/favorites/ids", None, 200, FAVORITES_IDS)
            .await;
        let got = s
            .ask(LibraryRequest::IsFavorite(kind, id.into()))
            .await
            .unwrap();
        assert_eq!(got, LibraryResponse::Favorite(want), "{kind:?} {id}");
        assert_eq!(
            s.seen().await,
            [seen("GET", "/v1/users/1/favorites/ids", &CC, None, "")]
        );
    }
}

/// AC7: `LoginRequired` and transient errors come back unchanged from every
/// call (table over the requests that write and read).
#[tokio::test]
async fn ac7_errors_pass_through() {
    let requests = || -> Vec<(&'static str, LibraryRequest)> {
        vec![
            (
                "add favorite",
                LibraryRequest::AddFavorite(FavoriteKind::Track, "1".into()),
            ),
            (
                "remove favorite",
                LibraryRequest::RemoveFavorite(FavoriteKind::Album, "1".into()),
            ),
            (
                "is favorite",
                LibraryRequest::IsFavorite(FavoriteKind::Track, "1".into()),
            ),
            (
                "create",
                LibraryRequest::CreatePlaylist { title: "t".into() },
            ),
            (
                "add to playlist",
                LibraryRequest::AddToPlaylist {
                    uuid: UUID.into(),
                    tracks: ids(1, 1),
                    allow_duplicates: false,
                },
            ),
            (
                "remove from playlist",
                LibraryRequest::RemoveFromPlaylist {
                    uuid: UUID.into(),
                    index: 0,
                    etag: E1.into(),
                },
            ),
            (
                "delete",
                LibraryRequest::DeletePlaylist { uuid: UUID.into() },
            ),
            ("page", LibraryRequest::Page(PageRequest::FavoriteTracks)),
            ("more", more(ListRef::Credits(1), 0, 10)),
        ]
    };
    for (name, request) in requests() {
        // 429 and 500.
        for (status, message) in [
            (429, "Tidal answered 429: try again in a moment"),
            (500, "Tidal answered 500"),
        ] {
            let s = Setup::new().await;
            Mock::given(wiremock::matchers::any())
                .respond_with(json_body(status, SERVER_500))
                .mount(&s.server)
                .await;
            let error = s.ask(request.clone()).await.unwrap_err();
            assert_eq!(error.to_string(), message, "{name} {status}");
            assert!(error.auth().is_some_and(AuthError::is_transient), "{name}");
        }
        // The session is lost.
        let s = Setup::new().await;
        Mock::given(wiremock::matchers::any())
            .and(wiremock::matchers::path_regex("^/v1/"))
            .respond_with(json_body(401, TOKEN_401))
            .mount(&s.server)
            .await;
        Mock::given(method("POST"))
            .and(path("/oauth2/token"))
            .respond_with(json_body(400, INVALID_GRANT))
            .mount(&s.server)
            .await;
        let error = s.ask(request.clone()).await.unwrap_err();
        assert_eq!(
            error,
            LibraryError::Auth(AuthError::LoginRequired),
            "{name}"
        );
        // Nothing reaches Tidal: the server is gone.
        let s = Setup::new().await;
        let Setup { server, client } = s;
        drop(server);
        let error = client.request(request, 100, &words()).await.unwrap_err();
        assert!(
            error.to_string().starts_with("Could not reach Tidal: "),
            "{name}: {error}"
        );
        assert!(error.auth().is_some_and(AuthError::is_transient), "{name}");
    }
}
