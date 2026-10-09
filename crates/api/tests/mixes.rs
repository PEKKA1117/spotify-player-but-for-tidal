//! Spec 0011, mixes and radio over HTTP: AC2-AC4. No real network: a
//! `wiremock` server answers with the fixtures under `fixtures/mixes/`,
//! written from the live probe's shapes (see its README).

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use serde_json::{Value, json};
use tidal_player_api::auth::{
    AuthConfig, AuthError, Authenticator, ManualClock, MemoryStore, Session, SessionStore,
};
use tidal_player_api::library::{LibraryClient, LibraryError};
use tidal_player_core::library::{
    LibraryRequest, LibraryResponse, ListPage, MixSummary, PageData, PageRequest, RadioSeed,
};
use tidal_player_core::{AlbumRef, ArtistRef, Track, TrackId};
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const MIXES_PAGE: &str = include_str!("fixtures/mixes/mixes_page.json");
const MIXES_FIRST: &str = include_str!("fixtures/mixes/mixes_page_first.json");
const MIXES_DATA_2: &str = include_str!("fixtures/mixes/mixes_data_2.json");
const MIXES_DATA_4: &str = include_str!("fixtures/mixes/mixes_data_4.json");
const MIXES_EMPTY: &str = include_str!("fixtures/mixes/mixes_page_empty.json");
const PAGES_MIX: &str = include_str!("fixtures/mixes/pages_mix.json");
const MIX_ITEMS: &str = include_str!("fixtures/mixes/mix_items.json");
const MIX_NOT_FOUND: &str = include_str!("fixtures/mixes/error_mix_not_found_2001.json");
const MIX_ITEMS_NOT_FOUND: &str =
    include_str!("fixtures/mixes/error_mix_items_not_found_2001.json");
const TRACK_HEADER: &str = include_str!("fixtures/mixes/track_header.json");
const TRACK_RADIO: &str = include_str!("fixtures/mixes/track_radio.json");
const ARTIST_HEADER: &str = include_str!("fixtures/mixes/artist_header.json");
const ARTIST_RADIO: &str = include_str!("fixtures/mixes/artist_radio.json");
const ARTIST_RADIO_404: &str = include_str!("fixtures/mixes/error_artist_radio_2001.json");
const TRACK_RADIO_404: &str = include_str!("fixtures/metadata/error_radio_not_found_2001.json");
const TRACK_404: &str = include_str!("fixtures/metadata/error_track_not_found_2001.json");
const ARTIST_404: &str = include_str!("fixtures/library/error_artist_not_found_2001.json");
const SERVER_500: &str = include_str!("fixtures/metadata/error_server_500.json");
const TOKEN_401: &str = include_str!("fixtures/auth/api_unauthorized.json");
const INVALID_GRANT: &str = include_str!("fixtures/auth/refresh_invalid_grant.json");

const MIX: &str = "00a1b2c3d4e5f60718293a4b5c6d7e";
const DATA_PATH: &str = "/v1/pages/data/00000000-0000-4000-8000-0000000000aa";
/// The page size of these tests: the lists are whole, it must not matter.
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

fn ask_page(request: PageRequest) -> LibraryRequest {
    LibraryRequest::Page(request)
}

fn page<T>(items: Vec<T>, total: u32) -> ListPage<T> {
    ListPage {
        items,
        offset: 0,
        total,
        hidden: 0,
    }
}

fn mix_id(n: u32) -> String {
    format!("{n:030x}")
}

fn mix(n: u32, title: &str, subtitle: Option<&str>) -> MixSummary {
    MixSummary {
        id: mix_id(n),
        title: title.into(),
        subtitle: subtitle.map(Into::into),
    }
}

fn artist(id: u64, name: &str) -> ArtistRef {
    ArtistRef {
        id,
        name: name.into(),
    }
}

/// A track as the fixtures' generator writes it.
fn track(id: u64, title: &str, by: ArtistRef) -> Track {
    Track {
        id: TrackId(id),
        title: title.into(),
        version: None,
        artists: vec![by],
        album: Some(AlbumRef {
            id: 9000 + id,
            title: format!("Album {title}"),
            cover: Some(format!("00000000-0000-4000-8000-{id:012}")),
        }),
        duration: Some(Duration::from_secs(200 + id % 100)),
        streamable: true,
    }
}

fn a() -> ArtistRef {
    artist(3001, "Artist A")
}

fn b() -> ArtistRef {
    artist(3002, "Artist B")
}

fn pages_query(extra: &[(&'static str, &str)]) -> Vec<(&'static str, String)> {
    let mut query = vec![
        ("countryCode", "FI".to_owned()),
        ("deviceType", "BROWSER".to_owned()),
        ("locale", "en_US".to_owned()),
    ];
    query.extend(extra.iter().map(|(k, v)| (*k, (*v).to_owned())));
    query
}

fn seen_owned(p: &str, query: &[(&'static str, String)]) -> Seen {
    let pairs: Vec<(&str, &str)> = query.iter().map(|(k, v)| (*k, v.as_str())).collect();
    seen(p, &pairs)
}

/// The requests received, sorted (the calls of a page run in parallel).
async fn seen_sorted(s: &Setup) -> Vec<Seen> {
    let mut all = s.seen().await;
    all.sort_by(|x, y| (&x.path, &x.query).cmp(&(&y.path, &y.query)));
    all
}

// AC2 -------------------------------------------------------------------

/// AC2: `Page(Mixes)` is one `GET /pages/my_collection_my_mixes` when the
/// module holds its total, else `dataApiPath` requests with the player's
/// own `offset` until the total (each answer says `offset: 0`); Tidal's
/// order; video mixes, repeated and empty IDs dropped; the errors of 0006.
#[tokio::test]
async fn ac2_mixes_list() {
    let list = || ask_page(PageRequest::Mixes);
    let first = || seen_owned("/v1/pages/my_collection_my_mixes", &pages_query(&[]));

    // The module holds its total: one request. The video mix, the repeated
    // ID and the empty ID are dropped; an empty `subTitle` is none.
    let s = Setup::new().await;
    s.get("/v1/pages/my_collection_my_mixes", 200, MIXES_PAGE)
        .await;
    assert_eq!(
        s.ask(list(), SIZE).await,
        Ok(LibraryResponse::Page(PageData::Mixes {
            mixes: page(
                vec![
                    mix(
                        0xd1,
                        "My Daily Discovery",
                        Some("Songs by new and familiar artists inspired by your listening.")
                    ),
                    mix(
                        0xa1,
                        "My Mix 1",
                        Some("Artist A, Artist B, Artist C and more")
                    ),
                    mix(0xa2, "My Mix 2", None),
                ],
                3
            )
        }))
    );
    assert_eq!(s.seen().await, vec![first()]);

    // The module holds 2 of 5: the rest from its `dataApiPath`, `offset` 2
    // then 4, `limit` the module's; the video mix among them dropped.
    let s = Setup::new().await;
    s.get("/v1/pages/my_collection_my_mixes", 200, MIXES_FIRST)
        .await;
    for (offset, body) in [("2", MIXES_DATA_2), ("4", MIXES_DATA_4)] {
        Mock::given(method("GET"))
            .and(path(DATA_PATH))
            .and(query_param("offset", offset))
            .respond_with(json_body(200, body))
            .expect(1)
            .mount(&s.server)
            .await;
    }
    let sub = Some("Artist A and more");
    assert_eq!(
        s.ask(list(), SIZE).await,
        Ok(LibraryResponse::Page(PageData::Mixes {
            mixes: page(
                vec![
                    mix(0xa1, "My Mix 1", sub),
                    mix(0xa2, "My Mix 2", sub),
                    mix(0xa3, "My Mix 3", sub),
                    mix(0xa5, "My Mix 5", sub),
                ],
                4
            )
        }))
    );
    let data = |offset: &str| {
        seen_owned(
            DATA_PATH,
            &pages_query(&[("limit", "2"), ("offset", offset)]),
        )
    };
    assert_eq!(s.seen().await, vec![first(), data("2"), data("4")]);

    // No mixes.
    let s = Setup::new().await;
    s.get("/v1/pages/my_collection_my_mixes", 200, MIXES_EMPTY)
        .await;
    assert_eq!(
        s.ask(list(), SIZE).await,
        Ok(LibraryResponse::Page(PageData::Mixes {
            mixes: page(vec![], 0)
        }))
    );

    // A body that is not a page of mixes.
    let s = Setup::new().await;
    s.get("/v1/pages/my_collection_my_mixes", 200, "[1, 2]")
        .await;
    assert_eq!(
        s.ask(list(), SIZE).await.unwrap_err().to_string(),
        "malformed mixes response from Tidal"
    );

    errors_unchanged(list()).await;
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

/// AC3: `Page(Mix(id))` is `GET /pages/mix` and exactly one `GET
/// /mixes/{id}/items` with neither `limit` nor `offset`; `video` items and
/// Atmos-only tracks dropped, `total` the tracks kept; a `404`/`2001` from
/// either is the mix's not-found; any failing call fails the page.
#[tokio::test]
async fn ac3_mix_page() {
    let request = || ask_page(PageRequest::Mix(MIX.into()));
    let header_path = "/v1/pages/mix";
    let items_path = format!("/v1/mixes/{MIX}/items");

    let s = Setup::new().await;
    s.get(header_path, 200, PAGES_MIX).await;
    Mock::given(method("GET"))
        .and(path(items_path.as_str()))
        .respond_with(json_body(200, MIX_ITEMS))
        .expect(1)
        .mount(&s.server)
        .await;
    assert_eq!(
        s.ask(request(), SIZE).await,
        Ok(LibraryResponse::Page(PageData::Mix {
            mix: MixSummary {
                id: MIX.into(),
                title: "My Daily Discovery".into(),
                subtitle: Some(
                    "Songs by new and familiar artists inspired by your listening.".into()
                ),
            },
            tracks: page(
                vec![
                    track(5001, "Song One", a()),
                    track(5002, "Song Two", b()),
                    track(5004, "Song Four", a()),
                ],
                3
            ),
        }))
    );
    let mut want = vec![
        seen_owned(header_path, &pages_query(&[("mixId", MIX)])),
        seen(&items_path, &[("countryCode", "FI")]),
    ];
    want.sort_by(|x, y| (&x.path, &x.query).cmp(&(&y.path, &y.query)));
    assert_eq!(seen_sorted(&s).await, want);
    // The server checks `expect(1)` when it drops.
    drop(s);

    // (header answer, items answer) -> the page's error.
    let table: Vec<(&str, (u16, &str), (u16, &str), &str)> = vec![
        (
            "header not found",
            (404, MIX_NOT_FOUND),
            (200, MIX_ITEMS),
            "Mix 00a1b2c3d4e5f60718293a4b5c6d7e was not found",
        ),
        (
            "items not found",
            (200, PAGES_MIX),
            (404, MIX_ITEMS_NOT_FOUND),
            "Mix 00a1b2c3d4e5f60718293a4b5c6d7e was not found",
        ),
        (
            "header 500",
            (500, SERVER_500),
            (200, MIX_ITEMS),
            "Tidal answered 500",
        ),
        (
            "items 500",
            (200, PAGES_MIX),
            (500, SERVER_500),
            "Tidal answered 500",
        ),
        (
            "header without a mix",
            (200, r#"{"rows": []}"#),
            (200, MIX_ITEMS),
            "malformed mix response from Tidal",
        ),
    ];
    for (name, header, items, want) in table {
        let s = Setup::new().await;
        s.get(header_path, header.0, header.1).await;
        s.get(&items_path, items.0, items.1).await;
        let error = s.ask(request(), SIZE).await.unwrap_err();
        assert_eq!(error.to_string(), want, "{name}");
    }

    errors_unchanged(request()).await;
}

// AC4 -------------------------------------------------------------------

/// The radio fixture's body with `n` copies of its first track (IDs 1..=n)
/// and `total` as Tidal says it.
fn radio_of(n: u64, total: u64) -> String {
    let template = fixture(TRACK_RADIO)["items"][0].clone();
    let items: Vec<Value> = (1..=n)
        .map(|id| {
            let mut t = template.clone();
            t["id"] = json!(id);
            t["title"] = json!(format!("Radio {id}"));
            t
        })
        .collect();
    json!({"limit": 100, "offset": 0, "totalNumberOfItems": total, "items": items}).to_string()
}

/// AC4: the radio calls and their headers; the seed track kept first;
/// Atmos-only tracks dropped; `total` the tracks kept; a radio `404`/`2001`
/// an empty list; a header `404` its not-found.
#[tokio::test]
async fn ac4_radio() {
    let seed = track(5001, "Song One", a());
    let radio_queries = [("countryCode", "FI"), ("limit", "100")];
    let country = [("countryCode", "FI")];

    // Track radio.
    let s = Setup::new().await;
    s.get("/v1/tracks/5001", 200, TRACK_HEADER).await;
    s.get("/v1/tracks/5001/radio", 200, TRACK_RADIO).await;
    assert_eq!(
        s.ask(ask_page(PageRequest::TrackRadio(5001)), SIZE).await,
        Ok(LibraryResponse::Page(PageData::Radio {
            seed: RadioSeed::Track(seed.clone()),
            tracks: page(
                vec![
                    seed.clone(),
                    track(6002, "Radio Two", b()),
                    track(6004, "Radio Four", a()),
                ],
                3
            ),
        }))
    );
    let mut want = vec![
        seen("/v1/tracks/5001", &country),
        seen("/v1/tracks/5001/radio", &radio_queries),
    ];
    want.sort_by(|x, y| (&x.path, &x.query).cmp(&(&y.path, &y.query)));
    assert_eq!(seen_sorted(&s).await, want);

    // The probe's 98: Tidal says 100, 98 rows are kept, 98 is the total.
    let s = Setup::new().await;
    s.get("/v1/tracks/5001", 200, TRACK_HEADER).await;
    s.get("/v1/tracks/5001/radio", 200, &radio_of(98, 100))
        .await;
    let got = s.ask(ask_page(PageRequest::TrackRadio(5001)), SIZE).await;
    let Ok(LibraryResponse::Page(PageData::Radio { tracks, .. })) = got else {
        panic!("{got:?}");
    };
    assert_eq!((tracks.items.len(), tracks.total), (98, 98));

    // Artist radio.
    let s = Setup::new().await;
    s.get("/v1/artists/3001", 200, ARTIST_HEADER).await;
    s.get("/v1/artists/3001/radio", 200, ARTIST_RADIO).await;
    assert_eq!(
        s.ask(ask_page(PageRequest::ArtistRadio(3001)), SIZE).await,
        Ok(LibraryResponse::Page(PageData::Radio {
            seed: RadioSeed::Artist(a()),
            tracks: page(
                vec![
                    track(7001, "Artist Radio One", a()),
                    track(7002, "Artist Radio Two", b()),
                ],
                2
            ),
        }))
    );
    let mut want = vec![
        seen("/v1/artists/3001", &country),
        seen("/v1/artists/3001/radio", &radio_queries),
    ];
    want.sort_by(|x, y| (&x.path, &x.query).cmp(&(&y.path, &y.query)));
    assert_eq!(seen_sorted(&s).await, want);

    // A radio Tidal cannot generate: an empty list, the header kept.
    let s = Setup::new().await;
    s.get("/v1/tracks/5001", 200, TRACK_HEADER).await;
    s.get("/v1/tracks/5001/radio", 404, TRACK_RADIO_404).await;
    assert_eq!(
        s.ask(ask_page(PageRequest::TrackRadio(5001)), SIZE).await,
        Ok(LibraryResponse::Page(PageData::Radio {
            seed: RadioSeed::Track(seed),
            tracks: page(vec![], 0),
        }))
    );
    let s = Setup::new().await;
    s.get("/v1/artists/3001", 200, ARTIST_HEADER).await;
    s.get("/v1/artists/3001/radio", 404, ARTIST_RADIO_404).await;
    assert_eq!(
        s.ask(ask_page(PageRequest::ArtistRadio(3001)), SIZE).await,
        Ok(LibraryResponse::Page(PageData::Radio {
            seed: RadioSeed::Artist(a()),
            tracks: page(vec![], 0),
        }))
    );

    // Errors: a header that is gone, a failing radio.
    type Case<'a> = (
        &'a str,
        PageRequest,
        &'a str,
        (u16, &'a str),
        (u16, &'a str),
        &'a str,
    );
    let table: Vec<Case> = vec![
        (
            "track header 404",
            PageRequest::TrackRadio(5001),
            "tracks/5001",
            (404, TRACK_404),
            (200, TRACK_RADIO),
            "Track 5001 was not found",
        ),
        (
            "artist header 404",
            PageRequest::ArtistRadio(3001),
            "artists/3001",
            (404, ARTIST_404),
            (200, ARTIST_RADIO),
            "Artist 3001 was not found",
        ),
        (
            "track radio 500",
            PageRequest::TrackRadio(5001),
            "tracks/5001",
            (200, TRACK_HEADER),
            (500, SERVER_500),
            "Tidal answered 500",
        ),
        (
            "artist radio 500",
            PageRequest::ArtistRadio(3001),
            "artists/3001",
            (200, ARTIST_HEADER),
            (500, SERVER_500),
            "Tidal answered 500",
        ),
    ];
    for (name, request, header_path, header, radio, want) in table {
        let s = Setup::new().await;
        s.get(&format!("/v1/{header_path}"), header.0, header.1)
            .await;
        s.get(&format!("/v1/{header_path}/radio"), radio.0, radio.1)
            .await;
        let error = s.ask(ask_page(request), SIZE).await.unwrap_err();
        assert_eq!(error.to_string(), want, "{name}");
    }

    errors_unchanged(ask_page(PageRequest::TrackRadio(5001))).await;
    errors_unchanged(ask_page(PageRequest::ArtistRadio(3001))).await;
}
