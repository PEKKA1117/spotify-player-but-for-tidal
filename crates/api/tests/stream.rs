//! Spec 0003, stream resolution over HTTP: AC1, AC5. No real network: a
//! `wiremock` server answers with the fixtures under `fixtures/stream/`.

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use tidal_player_api::auth::{
    AuthConfig, AuthError, Authenticator, ManualClock, MemoryStore, Session, SessionStore,
};
use tidal_player_api::stream::{ResolvedStream, StreamError, StreamResolver};
use tidal_player_core::AudioQuality;
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const TRACK: u64 = 1001;
const INFO_PATH: &str = "/v1/tracks/1001/playbackinfopostpaywall";
const TOKEN_PATH: &str = "/oauth2/token";

const HIRES: &str = include_str!("fixtures/stream/playbackinfo_hires_dash.json");
const HIGH: &str = include_str!("fixtures/stream/playbackinfo_high_bts.json");
const NOT_AVAILABLE: &str = include_str!("fixtures/stream/error_not_available_4005.json");
const NOT_FOUND: &str = include_str!("fixtures/stream/error_not_found_999.json");
const SERVER_500: &str = include_str!("fixtures/stream/error_server_500.json");
const TOKEN_401: &str = include_str!("fixtures/auth/api_unauthorized.json");
const REFRESHED: &str = include_str!("fixtures/auth/token_refreshed.json");
const INVALID_GRANT: &str = include_str!("fixtures/auth/refresh_invalid_grant.json");

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

fn json(status: u16, body: &str) -> ResponseTemplate {
    ResponseTemplate::new(status).set_body_raw(body.to_owned(), "application/json")
}

struct Setup {
    server: MockServer,
    resolver: StreamResolver,
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
        let resolver = StreamResolver::new(Arc::new(auth));
        Self { server, resolver }
    }

    /// How many requests went to `path`.
    async fn count(&self, path: &str) -> usize {
        self.server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .filter(|r| r.url.path() == path)
            .count()
    }
}

#[tokio::test]
async fn ac1_playbackinfo_request() {
    let s = Setup::new().await;
    Mock::given(method("GET"))
        .and(path(INFO_PATH))
        .and(query_param("audioquality", "HI_RES_LOSSLESS"))
        .and(query_param("playbackmode", "STREAM"))
        .and(query_param("assetpresentation", "FULL"))
        .and(query_param("countryCode", "FI"))
        .and(header("authorization", "Bearer FAKE-ACCESS"))
        .respond_with(json(200, HIRES))
        .expect(1)
        .mount(&s.server)
        .await;

    let result = s
        .resolver
        .resolve_stream(TRACK, AudioQuality::HiResLossless)
        .await;

    let requests = s.server.received_requests().await.unwrap();
    let queries: Vec<Vec<(String, String)>> = requests
        .iter()
        .map(|r| {
            let mut pairs: Vec<(String, String)> = r
                .url
                .query_pairs()
                .map(|(k, v)| (k.into_owned(), v.into_owned()))
                .collect();
            pairs.sort();
            pairs
        })
        .collect();
    let expected: Vec<(String, String)> = [
        ("assetpresentation", "FULL"),
        ("audioquality", "HI_RES_LOSSLESS"),
        ("countryCode", "FI"),
        ("playbackmode", "STREAM"),
    ]
    .iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    assert_eq!(
        queries,
        [expected],
        "exactly one request, exactly these params"
    );
    assert!(result.is_ok(), "{result:?}");
    s.server.verify().await;
}

/// What a row expects back.
enum Expect {
    Granted(AudioQuality),
    Error(StreamError),
    Transport,
}

#[tokio::test]
async fn ac5_one_request() {
    use AudioQuality::*;

    // (name, asked, response to the playbackinfo request, refresh answer,
    //  expected, expected /token requests). Error rows first: a ladder
    //  (tidalt) shows up there as extra requests.
    let rows: Vec<(&str, AudioQuality, ResponseTemplate, &str, Expect, usize)> = vec![
        (
            "503",
            HiResLossless,
            ResponseTemplate::new(503),
            REFRESHED,
            Expect::Error(StreamError::Auth(AuthError::Http {
                status: 503,
                code: None,
            })),
            0,
        ),
        (
            "grant equal",
            HiResLossless,
            json(200, HIRES),
            REFRESHED,
            Expect::Granted(HiResLossless),
            0,
        ),
        (
            "grant lower",
            HiResLossless,
            json(200, HIGH),
            REFRESHED,
            Expect::Granted(High),
            0,
        ),
        (
            "401/4005",
            HiResLossless,
            json(401, NOT_AVAILABLE),
            REFRESHED,
            Expect::Error(StreamError::NotAvailable),
            0,
        ),
        (
            "500/999",
            Lossless,
            json(500, NOT_FOUND),
            REFRESHED,
            Expect::Error(StreamError::NotFound),
            0,
        ),
        (
            "other 500",
            Lossless,
            json(500, SERVER_500),
            REFRESHED,
            Expect::Error(StreamError::Server(500)),
            0,
        ),
        (
            "429",
            HiResLossless,
            ResponseTemplate::new(429),
            REFRESHED,
            Expect::Error(StreamError::Auth(AuthError::Http {
                status: 429,
                code: None,
            })),
            0,
        ),
        (
            "timeout",
            HiResLossless,
            json(200, HIRES).set_delay(Duration::from_secs(5)),
            REFRESHED,
            Expect::Transport,
            0,
        ),
        (
            "LoginRequired",
            HiResLossless,
            json(401, TOKEN_401),
            INVALID_GRANT,
            Expect::Error(StreamError::Auth(AuthError::LoginRequired)),
            1,
        ),
    ];

    for (name, asked, response, refresh, expected, token_requests) in rows {
        let s = Setup::new().await;
        let resolver = s.resolver.clone().with_timeout(Duration::from_millis(200));
        Mock::given(method("GET"))
            .and(path(INFO_PATH))
            .respond_with(response)
            .mount(&s.server)
            .await;
        let refresh_status = if refresh == INVALID_GRANT { 400 } else { 200 };
        Mock::given(method("POST"))
            .and(path(TOKEN_PATH))
            .respond_with(json(refresh_status, refresh))
            .mount(&s.server)
            .await;

        let result = resolver.resolve_stream(TRACK, asked).await;

        assert_eq!(s.count(INFO_PATH).await, 1, "{name}: playbackinfo requests");
        assert_eq!(
            s.count(TOKEN_PATH).await,
            token_requests,
            "{name}: /token requests"
        );
        match expected {
            Expect::Granted(quality) => {
                let got = result.as_ref().map(|r: &ResolvedStream| r.quality);
                assert_eq!(got, Ok(quality), "{name}");
            }
            Expect::Error(error) => {
                assert_eq!(result, Err(error), "{name}");
            }
            Expect::Transport => assert!(
                matches!(result, Err(StreamError::Auth(AuthError::Transport(_)))),
                "{name}: {result:?}"
            ),
        }
    }
}
