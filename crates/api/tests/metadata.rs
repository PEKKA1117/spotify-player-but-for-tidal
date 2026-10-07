//! Spec 0004, metadata over HTTP: AC17, AC27. No real network: a `wiremock`
//! server answers with the fixtures under `fixtures/metadata/`.

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use serde_json::{Value, json};
use tidal_player_api::auth::{
    AuthConfig, AuthError, Authenticator, ManualClock, MemoryStore, Session, SessionStore,
};
use tidal_player_api::metadata::{MetadataClient, MetadataError};
use tidal_player_core::{Item, Track, TrackId};
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const TRACK: &str = include_str!("fixtures/metadata/track.json");
const TRACK_NO_ALLOW: &str = include_str!("fixtures/metadata/track_unstreamable_allow.json");
const TRACK_NO_READY: &str = include_str!("fixtures/metadata/track_unstreamable_ready.json");
const TRACK_NO_DURATION: &str = include_str!("fixtures/metadata/track_no_duration.json");
const ALBUM_PAGE: &str = include_str!("fixtures/metadata/album_page.json");
const PLAYLIST_PAGE: &str = include_str!("fixtures/metadata/playlist_page_with_video.json");
const RADIO_PAGE: &str = include_str!("fixtures/metadata/radio_page.json");
const TRACK_404: &str = include_str!("fixtures/metadata/error_track_not_found_2001.json");
const ALBUM_404: &str = include_str!("fixtures/metadata/error_album_not_found_2001.json");
const PLAYLIST_404: &str = include_str!("fixtures/metadata/error_playlist_not_found_2001.json");
const RADIO_404: &str = include_str!("fixtures/metadata/error_radio_not_found_2001.json");
const SERVER_500: &str = include_str!("fixtures/metadata/error_server_500.json");
const TOKEN_401: &str = include_str!("fixtures/auth/api_unauthorized.json");
const INVALID_GRANT: &str = include_str!("fixtures/auth/refresh_invalid_grant.json");

const UUID: &str = "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d";

fn t0() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000)
}

/// Valid for an hour, in Finland.
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

struct Setup {
    server: MockServer,
    client: MetadataClient,
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
        let client = MetadataClient::new(Arc::new(auth));
        Self { server, client }
    }

    /// The `(path, sorted query)` of every request received, in order.
    async fn requests(&self) -> Vec<(String, Vec<(String, String)>)> {
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
                (r.url.path().to_owned(), query)
            })
            .collect()
    }
}

fn query(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    v.sort();
    v
}

fn fixture(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

/// `n` bare tracks with IDs `first..first + n`, cloned from `track.json`.
fn tracks(first: u64, n: u64) -> Vec<Value> {
    (first..first + n)
        .map(|id| {
            let mut t = fixture(TRACK);
            t["id"] = json!(id);
            t["title"] = json!(format!("Track {id}"));
            t
        })
        .collect()
}

fn page(total: u64, offset: u64, items: Vec<Value>) -> String {
    json!({"limit": 100, "offset": offset, "totalNumberOfItems": total, "items": items}).to_string()
}

fn ids(tracks: &[Track]) -> Vec<u64> {
    tracks.iter().map(|t| t.id.0).collect()
}

fn track_one() -> Track {
    Track {
        id: TrackId(1001),
        title: "Track One".into(),
        artists: vec!["Artist A".into(), "Artist B".into()],
        album: Some("Album One".into()),
        duration: Some(Duration::from_secs(291)),
        streamable: true,
    }
}

#[tokio::test]
async fn ac17_get_track() {
    let rows: [(&str, &str, Track); 4] = [
        ("full", TRACK, track_one()),
        (
            "allowStreaming false",
            TRACK_NO_ALLOW,
            Track {
                id: TrackId(1002),
                title: "Track Two".into(),
                artists: vec!["Artist A".into()],
                streamable: false,
                ..track_one()
            },
        ),
        (
            "streamReady false",
            TRACK_NO_READY,
            Track {
                id: TrackId(1003),
                title: "Track Three".into(),
                artists: vec!["Artist A".into()],
                streamable: false,
                ..track_one()
            },
        ),
        (
            "no duration",
            TRACK_NO_DURATION,
            Track {
                id: TrackId(1004),
                title: "Track Four".into(),
                artists: vec!["Artist A".into()],
                duration: None,
                ..track_one()
            },
        ),
    ];
    for (name, body, want) in rows {
        let s = Setup::new().await;
        let p = format!("/v1/tracks/{}", want.id.0);
        Mock::given(method("GET"))
            .and(path(p))
            .and(query_param("countryCode", "FI"))
            .and(header("authorization", "Bearer FAKE-ACCESS"))
            .respond_with(json_body(200, body))
            .expect(1)
            .mount(&s.server)
            .await;

        let got = s.client.get_track(want.id).await;

        assert_eq!(got, Ok(want.clone()), "{name}");
        assert_eq!(
            s.requests().await,
            [(
                format!("/v1/tracks/{}", want.id.0),
                query(&[("countryCode", "FI")])
            )],
            "{name}: one request, only countryCode"
        );
        s.server.verify().await;
    }
}

#[tokio::test]
async fn ac17_album_pages() {
    // (name, total, page sizes served in order, expected request offsets)
    let rows: [(&str, u64, &[u64], &[u64]); 4] = [
        ("one page", 3, &[3], &[0]),
        ("250 items", 250, &[100, 100, 50], &[0, 100, 200]),
        ("100 items is one page", 100, &[100], &[0]),
        ("empty page ends the walk", 250, &[100, 0], &[0, 100]),
    ];
    for (name, total, sizes, offsets) in rows {
        let s = Setup::new().await;
        let mut served = 0;
        for (i, size) in sizes.iter().enumerate() {
            let offset = offsets[i];
            let body = if name == "one page" {
                ALBUM_PAGE.to_owned()
            } else {
                page(total, offset, tracks(2000 + offset, *size))
            };
            served += size;
            Mock::given(method("GET"))
                .and(path("/v1/albums/77/tracks"))
                .and(query_param("offset", offset.to_string()))
                .respond_with(json_body(200, &body))
                .expect(1)
                .mount(&s.server)
                .await;
        }

        let got = s.client.get_album_tracks(77).await.unwrap();

        assert_eq!(got.len() as u64, served, "{name}: every item returned");
        let expected: Vec<u64> = if name == "one page" {
            vec![1001, 1002, 1003]
        } else {
            sizes
                .iter()
                .zip(offsets)
                .flat_map(|(n, o)| 2000 + o..2000 + o + n)
                .collect()
        };
        assert_eq!(ids(&got), expected, "{name}: in order");
        let want: Vec<_> = offsets
            .iter()
            .map(|o| {
                (
                    "/v1/albums/77/tracks".to_owned(),
                    query(&[
                        ("countryCode", "FI"),
                        ("limit", "100"),
                        ("offset", &o.to_string()),
                    ]),
                )
            })
            .collect();
        assert_eq!(s.requests().await, want, "{name}: requests");
        s.server.verify().await;
    }
}

fn playlist_item(track: Value) -> Value {
    json!({"item": track, "type": "track", "cut": null})
}

#[tokio::test]
async fn ac17_playlist_pages_and_videos() {
    // One page with a video among the tracks: the video is dropped.
    let s = Setup::new().await;
    let p = format!("/v1/playlists/{UUID}/items");
    Mock::given(method("GET"))
        .and(path(p.clone()))
        .and(query_param("countryCode", "FI"))
        .and(query_param("limit", "100"))
        .and(query_param("offset", "0"))
        .respond_with(json_body(200, PLAYLIST_PAGE))
        .expect(1)
        .mount(&s.server)
        .await;
    let got = s.client.get_playlist_tracks(UUID).await.unwrap();
    assert_eq!(ids(&got), [1001, 1002, 1003], "video left out, order kept");
    let mut first = track_one();
    first.artists = vec!["Artist A".into()];
    first.title = "Track One".into();
    first.duration = Some(Duration::from_secs(200));
    assert_eq!(got[0], first, "mapped as a bare track");
    s.server.verify().await;

    // 250 entries over three pages, the video entries counting towards the
    // total and the offset but not the result.
    let s = Setup::new().await;
    let mut entries: Vec<Value> = tracks(3000, 250).into_iter().map(playlist_item).collect();
    for i in [5, 150, 249] {
        entries[i] = json!({"item": {"id": 9000 + i, "album": null}, "type": "video", "cut": null});
    }
    for (offset, chunk) in entries.chunks(100).enumerate() {
        Mock::given(method("GET"))
            .and(path(p.clone()))
            .and(query_param("offset", (offset * 100).to_string()))
            .respond_with(json_body(
                200,
                &page(250, offset as u64 * 100, chunk.to_vec()),
            ))
            .expect(1)
            .mount(&s.server)
            .await;
    }
    let got = s.client.get_playlist_tracks(UUID).await.unwrap();
    let expected: Vec<u64> = (3000..3250)
        .filter(|id| ![3005, 3150, 3249].contains(id))
        .collect();
    assert_eq!(ids(&got), expected, "tracks of all pages, videos dropped");
    let offsets: Vec<String> = s
        .requests()
        .await
        .into_iter()
        .map(|(p, q)| {
            assert!(q.contains(&("limit".into(), "100".into())), "{p} {q:?}");
            q.into_iter().find(|(k, _)| k == "offset").unwrap().1
        })
        .collect();
    assert_eq!(offsets, ["0", "100", "200"]);
    s.server.verify().await;

    // An empty page ends the walk before totalNumberOfItems.
    let s = Setup::new().await;
    Mock::given(method("GET"))
        .and(path(p.clone()))
        .and(query_param("offset", "0"))
        .respond_with(json_body(
            200,
            &page(
                500,
                0,
                tracks(4000, 2).into_iter().map(playlist_item).collect(),
            ),
        ))
        .expect(1)
        .mount(&s.server)
        .await;
    Mock::given(method("GET"))
        .and(path(p))
        .and(query_param("offset", "2"))
        .respond_with(json_body(200, &page(500, 2, vec![])))
        .expect(1)
        .mount(&s.server)
        .await;
    let got = s.client.get_playlist_tracks(UUID).await.unwrap();
    assert_eq!(ids(&got), [4000, 4001]);
    assert_eq!(s.requests().await.len(), 2);
    s.server.verify().await;
}

#[tokio::test]
async fn ac17_errors() {
    #[derive(Clone, Copy)]
    enum Call {
        Track,
        Album,
        Playlist,
    }
    let playlist_path = format!("/v1/playlists/{UUID}/items");
    let rows: Vec<(&str, Call, String, u16, &str, MetadataError)> = vec![
        (
            "track 404",
            Call::Track,
            "/v1/tracks/1".into(),
            404,
            TRACK_404,
            MetadataError::NotFound(Item::Track(TrackId(1))),
        ),
        (
            "album 404",
            Call::Album,
            "/v1/albums/1/tracks".into(),
            404,
            ALBUM_404,
            MetadataError::NotFound(Item::Album(1)),
        ),
        (
            "playlist 404",
            Call::Playlist,
            playlist_path.clone(),
            404,
            PLAYLIST_404,
            MetadataError::NotFound(Item::Playlist(UUID.into())),
        ),
        (
            "track 500",
            Call::Track,
            "/v1/tracks/1".into(),
            500,
            SERVER_500,
            MetadataError::Auth(AuthError::Http {
                status: 500,
                code: None,
            }),
        ),
        (
            "album 429",
            Call::Album,
            "/v1/albums/1/tracks".into(),
            429,
            "{}",
            MetadataError::Auth(AuthError::Http {
                status: 429,
                code: None,
            }),
        ),
        (
            "playlist 404 without a subStatus",
            Call::Playlist,
            playlist_path,
            404,
            "{}",
            MetadataError::Auth(AuthError::Http {
                status: 404,
                code: None,
            }),
        ),
        (
            "track 200 with a bad body",
            Call::Track,
            "/v1/tracks/1".into(),
            200,
            "{\"nope\":1}",
            MetadataError::Malformed("track"),
        ),
    ];
    for (name, call, p, status, body, want) in rows {
        let s = Setup::new().await;
        Mock::given(method("GET"))
            .and(path(p))
            .respond_with(json_body(status, body))
            .expect(1)
            .mount(&s.server)
            .await;
        let got = match call {
            Call::Track => s.client.get_track(TrackId(1)).await.map(|_| ()),
            Call::Album => s.client.get_album_tracks(1).await.map(|_| ()),
            Call::Playlist => s.client.get_playlist_tracks(UUID).await.map(|_| ()),
        };
        assert_eq!(got, Err(want), "{name}");
        s.server.verify().await;
    }

    // The not-found message names the item.
    assert_eq!(
        MetadataError::NotFound(Item::Album(1)).to_string(),
        "Album 1 was not found"
    );
}

#[tokio::test]
async fn ac17_login_required() {
    // The access token is rejected and the refresh is refused: LoginRequired,
    // after which no metadata request is made.
    let s = Setup::new().await;
    Mock::given(method("GET"))
        .and(path("/v1/tracks/1"))
        .respond_with(json_body(401, TOKEN_401))
        .mount(&s.server)
        .await;
    Mock::given(method("POST"))
        .and(path("/oauth2/token"))
        .respond_with(json_body(400, INVALID_GRANT))
        .mount(&s.server)
        .await;
    let got = s.client.get_track(TrackId(1)).await;
    assert_eq!(got, Err(MetadataError::Auth(AuthError::LoginRequired)));
    let again = s.client.get_album_tracks(1).await;
    assert_eq!(again, Err(MetadataError::Auth(AuthError::LoginRequired)));
}

#[tokio::test]
async fn ac27_suggestions() {
    // The seed comes back first and stays.
    let s = Setup::new().await;
    Mock::given(method("GET"))
        .and(path("/v1/tracks/1001/radio"))
        .and(query_param("countryCode", "FI"))
        .and(query_param("limit", "100"))
        .and(header("authorization", "Bearer FAKE-ACCESS"))
        .respond_with(json_body(200, RADIO_PAGE))
        .expect(1)
        .mount(&s.server)
        .await;
    let got = s.client.get_suggestions(TrackId(1001)).await.unwrap();
    assert_eq!(ids(&got), [1001, 1011, 1012], "in order, seed included");
    assert_eq!(got[0], track_one());
    assert_eq!(got[1].album.as_deref(), Some("Album Two"));
    assert_eq!(
        s.requests().await,
        [(
            "/v1/tracks/1001/radio".to_owned(),
            query(&[("countryCode", "FI"), ("limit", "100")])
        )],
        "exactly one request"
    );
    s.server.verify().await;

    // Errors.
    let rows: [(&str, u16, &str, Result<usize, MetadataError>); 3] = [
        ("404 2001 is an empty list", 404, RADIO_404, Ok(0)),
        (
            "500 is unchanged",
            500,
            SERVER_500,
            Err(MetadataError::Auth(AuthError::Http {
                status: 500,
                code: None,
            })),
        ),
        (
            "429 is unchanged",
            429,
            "{}",
            Err(MetadataError::Auth(AuthError::Http {
                status: 429,
                code: None,
            })),
        ),
    ];
    for (name, status, body, want) in rows {
        let s = Setup::new().await;
        Mock::given(method("GET"))
            .and(path("/v1/tracks/1/radio"))
            .respond_with(json_body(status, body))
            .expect(1)
            .mount(&s.server)
            .await;
        let got = s.client.get_suggestions(TrackId(1)).await.map(|t| t.len());
        assert_eq!(got, want, "{name}");
        s.server.verify().await;
    }

    // LoginRequired is returned unchanged.
    let s = Setup::new().await;
    Mock::given(method("GET"))
        .and(path("/v1/tracks/1/radio"))
        .respond_with(json_body(401, TOKEN_401))
        .mount(&s.server)
        .await;
    Mock::given(method("POST"))
        .and(path("/oauth2/token"))
        .respond_with(json_body(400, INVALID_GRANT))
        .mount(&s.server)
        .await;
    let got = s.client.get_suggestions(TrackId(1)).await;
    assert_eq!(got, Err(MetadataError::Auth(AuthError::LoginRequired)));
}
