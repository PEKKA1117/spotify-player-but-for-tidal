//! Spec 0021, lyrics over HTTP: AC3. No real network: a `wiremock` server
//! answers with the fixtures under `fixtures/lyrics/`, written from the
//! live probe's shapes (invented text).

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use tidal_player_api::auth::{
    AuthConfig, AuthError, Authenticator, ManualClock, MemoryStore, Session, SessionStore,
};
use tidal_player_api::library::{LibraryClient, LibraryError};
use tidal_player_core::library::{LibraryRequest, LibraryResponse, PageData, PageRequest};
use tidal_player_core::lyrics::{LyricLine, Lyrics};
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const SYNCED: &str = include_str!("fixtures/lyrics/lyrics_synced.json");
const PLAIN_ONLY: &str = include_str!("fixtures/lyrics/lyrics_plain_only.json");
const RTL: &str = include_str!("fixtures/lyrics/lyrics_rtl.json");
const NO_RTL_FIELD: &str = include_str!("fixtures/lyrics/lyrics_no_rtl_field.json");
const NOT_FOUND_PROBE: &str = include_str!("fixtures/lyrics/error_not_found_probe.json");
const NOT_FOUND_2001: &str = include_str!("fixtures/metadata/error_track_not_found_2001.json");
const SERVER_500: &str = include_str!("fixtures/metadata/error_server_500.json");
const TOKEN_401: &str = include_str!("fixtures/auth/api_unauthorized.json");
const INVALID_GRANT: &str = include_str!("fixtures/auth/refresh_invalid_grant.json");

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

fn client_at(base: &str) -> LibraryClient {
    let store = Arc::new(MemoryStore::default());
    store.save(&session()).unwrap();
    let config = AuthConfig::with_bases(format!("{base}/oauth2"), format!("{base}/v1"));
    let clock = Arc::new(ManualClock::new(t0()));
    let auth = Authenticator::new(config, store, session(), clock).unwrap();
    LibraryClient::new(Arc::new(auth))
}

fn ask(id: u64) -> LibraryRequest {
    LibraryRequest::Page(PageRequest::Lyrics(id))
}

fn line(ms: u64, text: &str) -> LyricLine {
    LyricLine {
        at: Duration::from_millis(ms),
        text: text.into(),
    }
}

fn lyrics(track: u64, lyrics: Option<Lyrics>) -> Result<LibraryResponse, LibraryError> {
    Ok(LibraryResponse::Page(PageData::Lyrics { track, lyrics }))
}

/// AC3: the request, the mapping of the answer, `404` (any body) as no
/// lyrics, and 0006's errors unchanged.
#[tokio::test]
async fn ac3_lyrics() {
    // The request: path, `countryCode`, the bearer token.
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/tracks/7001/lyrics"))
        .and(query_param("countryCode", "FI"))
        .and(header("authorization", "Bearer FAKE-ACCESS"))
        .respond_with(json_body(200, SYNCED))
        .expect(1)
        .mount(&server)
        .await;
    let got = client_at(&server.uri()).request(ask(7001), 20, &[]).await;
    assert_eq!(
        got,
        lyrics(
            7001,
            Some(Lyrics {
                synced: vec![
                    line(12_500, "First paper lantern"),
                    line(17_080, "Drifts over the quiet river"),
                    line(22_000, ""),
                    line(24_300, "Second verse begins"),
                    line(29_990, "Under an invented moon"),
                    line(35_120, ""),
                ],
                // CRLF -> LF, trailing blank lines dropped.
                plain: "First paper lantern\nDrifts over the quiet river\n\n\
                        Second verse begins\nUnder an invented moon"
                    .into(),
                right_to_left: false,
            })
        )
    );
    server.verify().await;

    // Answers and their mapping.
    let table = [
        (
            "empty subtitles: only plain",
            7002,
            200,
            PLAIN_ONLY,
            Some(Lyrics {
                synced: vec![],
                plain: "Only words here\nNo timing at all".into(),
                right_to_left: false,
            }),
        ),
        (
            "right to left",
            7003,
            200,
            RTL,
            Some(Lyrics {
                synced: vec![
                    line(5_000, "مرحبا بالفانوس"),
                    line(9_400, "النهر الهادئ"),
                    line(14_000, ""),
                ],
                plain: "مرحبا بالفانوس\nالنهر الهادئ".into(),
                right_to_left: true,
            }),
        ),
        (
            "isRightToLeft missing",
            7004,
            200,
            NO_RTL_FIELD,
            Some(Lyrics {
                synced: vec![line(1_000, "Words without a flag")],
                plain: "Words without a flag".into(),
                right_to_left: false,
            }),
        ),
        ("404, the probe's body", 7005, 404, NOT_FOUND_PROBE, None),
        ("404, Tidal's usual body", 7006, 404, NOT_FOUND_2001, None),
        ("404, no body", 7007, 404, "", None),
    ];
    for (name, id, status, body, want) in table {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(format!("/v1/tracks/{id}/lyrics")))
            .respond_with(json_body(status, body))
            .mount(&server)
            .await;
        let got = client_at(&server.uri()).request(ask(id), 20, &[]).await;
        assert_eq!(got, lyrics(id, want), "{name}");
    }

    // A `200` that is not lyrics is malformed, never quoted.
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/tracks/7008/lyrics"))
        .respond_with(json_body(200, "<html>not json</html>"))
        .mount(&server)
        .await;
    let error = client_at(&server.uri())
        .request(ask(7008), 20, &[])
        .await
        .unwrap_err();
    assert_eq!(error.to_string(), "malformed lyrics response from Tidal");

    // 0006's errors, unchanged: `429`, `500`, the lost session, no network.
    for (status, message) in [
        (429, "Tidal answered 429: try again in a moment"),
        (500, "Tidal answered 500"),
    ] {
        let server = MockServer::start().await;
        Mock::given(wiremock::matchers::any())
            .respond_with(json_body(status, SERVER_500))
            .mount(&server)
            .await;
        let error = client_at(&server.uri())
            .request(ask(7001), 20, &[])
            .await
            .unwrap_err();
        assert_eq!(error.to_string(), message, "{status}");
        assert!(
            error.auth().is_some_and(AuthError::is_transient),
            "{status}"
        );
    }
    let server = MockServer::start().await;
    Mock::given(wiremock::matchers::any())
        .and(wiremock::matchers::path_regex("^/v1/"))
        .respond_with(json_body(401, TOKEN_401))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/oauth2/token"))
        .respond_with(json_body(400, INVALID_GRANT))
        .mount(&server)
        .await;
    let error = client_at(&server.uri())
        .request(ask(7001), 20, &[])
        .await
        .unwrap_err();
    assert_eq!(error, LibraryError::Auth(AuthError::LoginRequired));

    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let error = client_at(&format!("http://127.0.0.1:{port}"))
        .request(ask(7001), 20, &[])
        .await
        .unwrap_err();
    assert!(error.to_string().starts_with("Could not reach Tidal: "));
    assert!(error.auth().is_some_and(AuthError::is_transient));
}
