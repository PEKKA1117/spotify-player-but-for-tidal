//! Spec 0007, search over HTTP: AC2-AC3. No real network: a `wiremock`
//! server answers with the fixtures under `fixtures/search/`, recorded from
//! the live probe (see its README).

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use serde_json::{Value, json};
use tidal_player_api::auth::{
    AuthConfig, AuthError, Authenticator, ManualClock, MemoryStore, Session, SessionStore,
};
use tidal_player_api::library::{LibraryClient, LibraryError};
use tidal_player_core::library::{
    AlbumKind, AlbumSummary, LibraryRequest, LibraryResponse, ListItems, ListPage, ListRef,
    PageData, PageRequest, PlaylistSummary, TopHit,
};
use tidal_player_core::{AlbumRef, ArtistRef, Track, TrackId};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const SEARCH_PAGE: &str = include_str!("fixtures/search/search_page.json");
const SEARCH_EMPTY: &str = include_str!("fixtures/search/search_empty.json");
const SEARCH_TRACKS: &str = include_str!("fixtures/search/search_tracks.json");
const SEARCH_ALBUMS: &str = include_str!("fixtures/search/search_albums.json");
const SEARCH_ARTISTS: &str = include_str!("fixtures/search/search_artists.json");
const SEARCH_PLAYLISTS: &str = include_str!("fixtures/search/search_playlists.json");
const SEARCH_PAST_END: &str = include_str!("fixtures/search/search_tracks_past_end.json");
const SERVER_500: &str = include_str!("fixtures/metadata/error_server_500.json");
const TOKEN_401: &str = include_str!("fixtures/auth/api_unauthorized.json");
const INVALID_GRANT: &str = include_str!("fixtures/auth/refresh_invalid_grant.json");

const TYPES: &str = "TRACKS,ALBUMS,ARTISTS,PLAYLISTS";
/// The search page size of these tests (the setting's default).
const SIZE: u32 = 20;

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
    method: String,
    path: String,
    /// Decoded and sorted.
    query: Vec<(String, String)>,
}

fn seen(path: &str, query: &[(&str, &str)]) -> Seen {
    let mut query: Vec<(String, String)> = query
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    query.sort();
    Seen {
        method: "GET".into(),
        path: path.into(),
        query,
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

    /// A `GET p` answering `status` and `body`.
    async fn get(&self, p: &str, status: u16, body: &str) {
        Mock::given(method("GET"))
            .and(path(p))
            .respond_with(json_body(status, body))
            .mount(&self.server)
            .await;
    }

    async fn ask(
        &self,
        request: LibraryRequest,
        page_size: u32,
    ) -> Result<LibraryResponse, LibraryError> {
        self.client.request(request, page_size, &[]).await
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
                }
            })
            .collect()
    }

    /// The raw query strings received, in order.
    async fn raw_queries(&self) -> Vec<String> {
        self.server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .map(|r| r.url.query().unwrap_or_default().to_owned())
            .collect()
    }
}

/// A client whose API base is a port nothing listens on.
fn unreachable_client() -> LibraryClient {
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let store = Arc::new(MemoryStore::default());
    store.save(&session()).unwrap();
    let config = AuthConfig::with_bases(
        format!("http://127.0.0.1:{port}/oauth2"),
        format!("http://127.0.0.1:{port}/v1"),
    );
    let clock = Arc::new(ManualClock::new(t0()));
    let auth = Authenticator::new(config, store, session(), clock).unwrap();
    LibraryClient::new(Arc::new(auth))
}

fn fixture(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

fn search(query: &str) -> LibraryRequest {
    LibraryRequest::Page(PageRequest::Search(query.into()))
}

fn more(list: ListRef, offset: u32, limit: u32) -> LibraryRequest {
    LibraryRequest::More {
        list,
        offset,
        limit,
    }
}

// Expected values, from the fixtures ------------------------------------

fn ptv() -> ArtistRef {
    ArtistRef {
        id: 4108155,
        name: "Pierce The Veil".into(),
    }
}

fn collide_ref() -> Option<AlbumRef> {
    Some(AlbumRef {
        id: 58758536,
        title: "Collide With The Sky".into(),
    })
}

fn king_for_a_day() -> Track {
    Track {
        id: TrackId(58758540),
        title: "King For A Day".into(),
        version: None,
        artists: vec![
            ptv(),
            ArtistRef {
                id: 4982923,
                name: "Kellin Quinn".into(),
            },
        ],
        album: collide_ref(),
        duration: Some(Duration::from_secs(236)),
        streamable: true,
    }
}

fn hell_above() -> Track {
    Track {
        id: TrackId(58758538),
        title: "Hell Above".into(),
        version: None,
        artists: vec![ptv()],
        album: collide_ref(),
        duration: Some(Duration::from_secs(224)),
        streamable: true,
    }
}

fn collide() -> AlbumSummary {
    AlbumSummary {
        id: 58758536,
        title: "Collide With The Sky".into(),
        artists: vec![ptv()],
        year: Some(2012),
        kind: AlbumKind::Album,
        tracks: Some(12),
        duration: Some(Duration::from_secs(2776)),
    }
}

fn jaws() -> AlbumSummary {
    AlbumSummary {
        id: 274869620,
        title: "The Jaws Of Life".into(),
        artists: vec![ptv()],
        year: Some(2023),
        kind: AlbumKind::Ep,
        tracks: Some(12),
        duration: Some(Duration::from_secs(2492)),
    }
}

fn sirens() -> ArtistRef {
    ArtistRef {
        id: 3668002,
        name: "Sleeping With Sirens".into(),
    }
}

/// Never own: search playlists are Tidal's (spec 0007 decision 5).
fn essentials() -> PlaylistSummary {
    PlaylistSummary {
        uuid: "1f11afd7-1a4e-4e4f-b49e-d61848c42ada".into(),
        title: "Pierce The Veil Essentials".into(),
        tracks: Some(15),
        duration: Some(Duration::from_secs(3509)),
        own: false,
    }
}

fn page<T>(items: Vec<T>, offset: u32, total: u32) -> ListPage<T> {
    ListPage {
        items,
        offset,
        total,
        hidden: 0,
    }
}

/// What `search_page.json` maps to, with `top_hit`: the Dolby-Atmos-only
/// "Hell Above" is not among the tracks.
fn search_page_data(top_hit: Option<TopHit>) -> PageData {
    PageData::Search {
        top_hit: top_hit.map(Box::new),
        tracks: page(vec![king_for_a_day(), hell_above()], 0, 223),
        albums: page(vec![collide(), jaws()], 0, 55),
        artists: page(vec![ptv(), sirens()], 0, 7),
        playlists: page(vec![essentials()], 0, 3),
    }
}

/// `search_page.json` with its `topHit` replaced (`None`: the key removed).
fn with_top_hit(top_hit: Option<Value>) -> String {
    let mut body = fixture(SEARCH_PAGE);
    let object = body.as_object_mut().unwrap();
    match top_hit {
        Some(hit) => {
            object.insert("topHit".into(), hit);
        }
        None => {
            object.remove("topHit");
        }
    }
    body.to_string()
}

/// The `n`th item of `list` in `search_page.json`.
fn item(list: &str, n: usize) -> Value {
    fixture(SEARCH_PAGE)[list]["items"][n].clone()
}

/// The parameters of the combined search.
fn search_query(q: &str, limit: &str) -> Vec<(&'static str, String)> {
    vec![
        ("countryCode", "FI".into()),
        ("limit", limit.into()),
        ("offset", "0".into()),
        ("query", q.into()),
        ("types", TYPES.into()),
    ]
}

fn seen_owned(p: &str, query: &[(&'static str, String)]) -> Seen {
    let pairs: Vec<(&str, &str)> = query.iter().map(|(k, v)| (*k, v.as_str())).collect();
    seen(p, &pairs)
}

// AC2 -------------------------------------------------------------------

/// AC2: `Page(Search(q))` is one `GET /search` with the query URL-encoded,
/// `types`, `countryCode`, the search page size and `offset=0`; the four
/// lists with their totals and the top hit (table over the `type`s); the
/// Atmos-only track dropped; an empty result; the `400` message; the
/// errors of 0006 unchanged.
#[tokio::test]
async fn ac2_search_page() {
    // The query is sent URL-encoded, decoded back unchanged.
    for q in ["pierce the veil", "AC/DC", "Sigur Rós", "a&b"] {
        let s = Setup::new().await;
        s.get("/v1/search", 200, SEARCH_PAGE).await;
        let got = s.ask(search(q), SIZE).await;
        assert_eq!(
            got,
            Ok(LibraryResponse::Page(search_page_data(Some(
                TopHit::Artist(ptv())
            )))),
            "{q}"
        );
        assert_eq!(
            s.seen().await,
            vec![seen_owned("/v1/search", &search_query(q, "20"))],
            "{q}"
        );
        for raw in s.raw_queries().await {
            assert!(
                raw.is_ascii() && !raw.contains(' ') && !raw.contains('/'),
                "{q}: not encoded: {raw}"
            );
        }
    }

    // The search page size is the limit, clamped to the endpoint's 1000.
    for (size, limit) in [(1, "1"), (100, "100"), (1000, "1000"), (5000, "1000")] {
        let s = Setup::new().await;
        s.get("/v1/search", 200, SEARCH_PAGE).await;
        s.ask(search("love"), size).await.unwrap();
        assert_eq!(
            s.seen().await,
            vec![seen_owned("/v1/search", &search_query("love", limit))],
            "{size}"
        );
    }

    // The top hit, by its `type`.
    let atmos_track = item("tracks", 2);
    assert_eq!(atmos_track["audioModes"], json!(["DOLBY_ATMOS"]));
    let table: Vec<(&str, Option<Value>, Option<TopHit>)> = vec![
        (
            "artist",
            Some(json!({"type": "ARTISTS", "value": item("artists", 0)})),
            Some(TopHit::Artist(ptv())),
        ),
        (
            "track",
            Some(json!({"type": "TRACKS", "value": item("tracks", 0)})),
            Some(TopHit::Track(king_for_a_day())),
        ),
        (
            "album",
            Some(json!({"type": "ALBUMS", "value": item("albums", 1)})),
            Some(TopHit::Album(jaws())),
        ),
        (
            "playlist",
            Some(json!({"type": "PLAYLISTS", "value": item("playlists", 0)})),
            Some(TopHit::Playlist(essentials())),
        ),
        (
            "atmos-only track",
            Some(json!({"type": "TRACKS", "value": atmos_track})),
            None,
        ),
        (
            "video",
            Some(json!({"type": "VIDEOS", "value": {"id": 1, "title": "A video"}})),
            None,
        ),
        (
            "unreadable value",
            Some(json!({"type": "TRACKS", "value": {"nothing": "here"}})),
            None,
        ),
        ("null", Some(Value::Null), None),
        ("missing", None, None),
    ];
    for (name, hit, want) in table {
        let s = Setup::new().await;
        s.get("/v1/search", 200, &with_top_hit(hit)).await;
        let got = s.ask(search("pierce the veil"), SIZE).await;
        assert_eq!(
            got,
            Ok(LibraryResponse::Page(search_page_data(want))),
            "{name}"
        );
    }

    // Nothing matches: four empty lists, no top hit.
    let s = Setup::new().await;
    s.get("/v1/search", 200, SEARCH_EMPTY).await;
    assert_eq!(
        s.ask(search("zzzzzzqqq"), SIZE).await,
        Ok(LibraryResponse::Page(PageData::Search {
            top_hit: None,
            tracks: page(vec![], 0, 0),
            albums: page(vec![], 0, 0),
            artists: page(vec![], 0, 0),
            playlists: page(vec![], 0, 0),
        }))
    );

    // A body that is not a search.
    let s = Setup::new().await;
    s.get("/v1/search", 200, "[1, 2]").await;
    let error = s.ask(search("x"), SIZE).await.unwrap_err();
    assert_eq!(error, LibraryError::Malformed("search"));

    // Tidal refuses the query.
    let s = Setup::new().await;
    s.get(
        "/v1/search",
        400,
        r#"{"status": 400, "subStatus": 1002, "userMessage": "Query is invalid"}"#,
    )
    .await;
    let error = s.ask(search("x"), SIZE).await.unwrap_err();
    assert_eq!(
        error.to_string(),
        "Tidal refused the search: Query is invalid"
    );
    // ... with no message to quote.
    let s = Setup::new().await;
    s.get("/v1/search", 400, "{}").await;
    let error = s.ask(search("x"), SIZE).await.unwrap_err();
    assert_eq!(error.to_string(), "Tidal answered 400");

    errors_unchanged(search("pierce the veil")).await;
}

/// 0006's errors, unchanged: `429`, `500`, the lost session and no
/// network.
async fn errors_unchanged(request: LibraryRequest) {
    for (status, message) in [
        (429, "Tidal answered 429: try again in a moment"),
        (500, "Tidal answered 500"),
    ] {
        let s = Setup::new().await;
        Mock::given(wiremock::matchers::any())
            .respond_with(json_body(status, SERVER_500))
            .mount(&s.server)
            .await;
        let error = s.ask(request.clone(), SIZE).await.unwrap_err();
        assert_eq!(error.to_string(), message, "{request:?} {status}");
        assert!(
            error.auth().is_some_and(AuthError::is_transient),
            "{request:?}"
        );
    }
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
    let error = s.ask(request.clone(), SIZE).await.unwrap_err();
    assert_eq!(
        error,
        LibraryError::Auth(AuthError::LoginRequired),
        "{request:?}"
    );
    let error = unreachable_client()
        .request(request.clone(), SIZE, &[])
        .await
        .unwrap_err();
    assert!(
        error.to_string().starts_with("Could not reach Tidal: "),
        "{request:?}: {error}"
    );
    assert!(
        error.auth().is_some_and(AuthError::is_transient),
        "{request:?}"
    );
}

// AC3 -------------------------------------------------------------------

/// AC3: `More` on each search list is one `GET /search/{type}` with the
/// query, the `limit` asked (clamped to 1000) and the `offset`; the bare
/// items mapped (the per-type track shape included), Atmos-only tracks
/// dropped, an offset past the end answering no items.
#[tokio::test]
async fn ac3_search_more() {
    let q = "pierce the veil";
    let table: Vec<(&str, ListRef, &str, u32, u32, &str, ListItems)> = vec![
        (
            "tracks",
            ListRef::SearchTracks(q.into()),
            "/v1/search/tracks",
            20,
            20,
            SEARCH_TRACKS,
            ListItems::Tracks(page(vec![hell_above(), king_for_a_day()], 20, 223)),
        ),
        (
            "albums",
            ListRef::SearchAlbums(q.into()),
            "/v1/search/albums",
            20,
            20,
            SEARCH_ALBUMS,
            ListItems::Albums(page(vec![collide(), jaws()], 20, 55)),
        ),
        (
            "artists",
            ListRef::SearchArtists(q.into()),
            "/v1/search/artists",
            0,
            20,
            SEARCH_ARTISTS,
            ListItems::Artists(page(vec![ptv(), sirens()], 0, 7)),
        ),
        (
            "playlists",
            ListRef::SearchPlaylists(q.into()),
            "/v1/search/playlists",
            0,
            20,
            SEARCH_PLAYLISTS,
            ListItems::Playlists(page(vec![essentials()], 0, 3)),
        ),
        (
            "past the end",
            ListRef::SearchTracks("love".into()),
            "/v1/search/tracks",
            300,
            50,
            SEARCH_PAST_END,
            ListItems::Tracks(page(vec![], 300, 295)),
        ),
    ];
    for (name, list, p, offset, limit, body, want) in table {
        let s = Setup::new().await;
        s.get(p, 200, body).await;
        let query = match &list {
            ListRef::SearchTracks(q)
            | ListRef::SearchAlbums(q)
            | ListRef::SearchArtists(q)
            | ListRef::SearchPlaylists(q) => q.clone(),
            other => unreachable!("{other:?}"),
        };
        let got = s.ask(more(list, offset, limit), SIZE).await;
        assert_eq!(got, Ok(LibraryResponse::Items(want)), "{name}");
        let (offset, limit) = (offset.to_string(), limit.to_string());
        assert_eq!(
            s.seen().await,
            vec![seen(
                p,
                &[
                    ("countryCode", "FI"),
                    ("limit", &limit),
                    ("offset", &offset),
                    ("query", &query),
                ]
            )],
            "{name}"
        );
    }

    // The query encoded; the limit clamped to 1000.
    for (list, p) in [
        (ListRef::SearchTracks("a&b/c ø".into()), "/v1/search/tracks"),
        (ListRef::SearchAlbums("a&b/c ø".into()), "/v1/search/albums"),
        (
            ListRef::SearchArtists("a&b/c ø".into()),
            "/v1/search/artists",
        ),
        (
            ListRef::SearchPlaylists("a&b/c ø".into()),
            "/v1/search/playlists",
        ),
    ] {
        let s = Setup::new().await;
        s.get(p, 200, SEARCH_PAST_END).await;
        s.ask(more(list, 40, 5000), SIZE).await.unwrap();
        assert_eq!(
            s.seen().await,
            vec![seen(
                p,
                &[
                    ("countryCode", "FI"),
                    ("limit", "1000"),
                    ("offset", "40"),
                    ("query", "a&b/c ø"),
                ]
            )],
            "{p}"
        );
        for raw in s.raw_queries().await {
            assert!(
                raw.is_ascii() && !raw.contains(' ') && !raw.contains('/'),
                "{p}: not encoded: {raw}"
            );
        }
    }

    errors_unchanged(more(ListRef::SearchTracks(q.into()), 20, 20)).await;
}
