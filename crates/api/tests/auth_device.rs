//! Spec 0002, device flow: AC1, AC2, AC19 (the post-login refresh). No real network, no real sleep.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, SystemTime};

use tidal_player_api::auth::{
    AuthConfig, AuthError, CLIENT_ID, CLIENT_SECRET, DEVICE_CODE_GRANT, DeviceCode, DeviceFlow,
    ManualClock, MemoryStore, PKCE_CLIENT_ID, PKCE_CLIENT_SECRET, RefreshClient, SCOPE, Session,
    SessionStore, StoreError,
};
use wiremock::matchers::{basic_auth, method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

fn t0() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000)
}

/// The decoded form fields of a request body.
fn form(request: &Request) -> Vec<(String, String)> {
    let body = String::from_utf8_lossy(&request.body);
    let url = reqwest::Url::parse(&format!("http://form.invalid/?{body}")).unwrap();
    url.query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect()
}

/// Matches a request whose form body has every one of `fields`.
fn form_has(fields: &'static [(&'static str, &'static str)]) -> impl Fn(&Request) -> bool {
    move |request: &Request| {
        let got = form(request);
        fields
            .iter()
            .all(|(k, v)| got.iter().any(|(gk, gv)| gk == k && gv == v))
    }
}

fn json(body: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_raw(body.to_owned(), "application/json")
}

/// Answers with each template in turn, repeating the last one.
struct Sequence {
    responses: Vec<ResponseTemplate>,
    next: AtomicUsize,
}

impl Respond for Sequence {
    fn respond(&self, _: &Request) -> ResponseTemplate {
        let i = self.next.fetch_add(1, Ordering::SeqCst);
        self.responses[i.min(self.responses.len() - 1)].clone()
    }
}

fn flow(server: &MockServer, clock: &Arc<ManualClock>) -> DeviceFlow {
    let config = AuthConfig::with_bases(server.uri(), format!("{}/v1", server.uri()));
    DeviceFlow::with_time(config, clock.clone(), clock.clone()).unwrap()
}

fn code(expires_in: u64) -> DeviceCode {
    DeviceCode {
        device_code: "FAKE-DEVICE-CODE".into(),
        user_code: "ABCDE".into(),
        verification_uri: "link.tidal.com".into(),
        verification_uri_complete: Some("link.tidal.com/ABCDE".into()),
        expires_in: Duration::from_secs(expires_in),
        interval: Duration::from_secs(2),
    }
}

#[tokio::test]
async fn ac1_start_device_flow() {
    let cases = [
        (
            include_str!("fixtures/auth/device_authorization.json"),
            Some("link.tidal.com/ABCDE"),
            "https://link.tidal.com/ABCDE",
        ),
        (
            include_str!("fixtures/auth/device_authorization_no_complete.json"),
            None,
            "https://link.tidal.com",
        ),
    ];
    for (fixture, complete, browser) in cases {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/device_authorization"))
            .and(form_has(&[("client_id", CLIENT_ID), ("scope", SCOPE)]))
            .respond_with(json(fixture))
            .expect(1)
            .mount(&server)
            .await;
        let clock = Arc::new(ManualClock::new(t0()));

        let result = flow(&server, &clock).start_device_flow().await;

        server.verify().await;
        let code = result.expect("device code");
        let expected = DeviceCode {
            device_code: "FAKE-DEVICE-CODE".into(),
            user_code: "ABCDE".into(),
            verification_uri: "link.tidal.com".into(),
            verification_uri_complete: complete.map(Into::into),
            expires_in: Duration::from_secs(300),
            interval: Duration::from_secs(2),
        };
        assert_eq!(code, expected);
        assert_eq!(code.login_link(), "https://link.tidal.com");
        assert_eq!(code.browser_link(), browser);
    }
}

#[tokio::test]
async fn ac2_poll_until_granted() {
    let server = MockServer::start().await;
    let pending = include_str!("fixtures/auth/token_pending.json");
    let slow_down = include_str!("fixtures/auth/token_slow_down.json");
    let granted = include_str!("fixtures/auth/token_granted.json");
    Mock::given(method("POST"))
        .and(path("/token"))
        .and(basic_auth(CLIENT_ID, CLIENT_SECRET))
        .and(form_has(&[
            ("client_id", CLIENT_ID),
            ("grant_type", DEVICE_CODE_GRANT),
            ("device_code", "FAKE-DEVICE-CODE"),
            ("scope", SCOPE),
        ]))
        .respond_with(Sequence {
            responses: vec![
                ResponseTemplate::new(400).set_body_raw(pending, "application/json"),
                ResponseTemplate::new(400).set_body_raw(slow_down, "application/json"),
                json(granted),
            ],
            next: AtomicUsize::new(0),
        })
        .expect(3)
        .mount(&server)
        .await;
    let clock = Arc::new(ManualClock::new(t0()));

    let session = flow(&server, &clock).wait_for_session(&code(300)).await;

    server.verify().await;
    let session = session.expect("session");
    assert_eq!(session.access_token, "FAKE-ACCESS");
    assert_eq!(session.refresh_token, "FAKE-REFRESH");
    // Same interval after `authorization_pending`, +5 s after `slow_down`.
    assert_eq!(
        clock.sleeps(),
        [Duration::from_secs(2), Duration::from_secs(7)]
    );
    assert_eq!(
        session.expires_at,
        t0() + Duration::from_secs(9) + Duration::from_secs(14_400)
    );
}

#[tokio::test]
async fn ac2_poll_gives_up_after_expiry() {
    let server = MockServer::start().await;
    let pending = include_str!("fixtures/auth/token_pending.json");
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(400).set_body_raw(pending, "application/json"))
        .mount(&server)
        .await;
    let clock = Arc::new(ManualClock::new(t0()));

    let result = flow(&server, &clock).wait_for_session(&code(0)).await;

    assert_eq!(result, Err(AuthError::CodeExpired));
}

/// No server answering (a transport error on every poll) still ends with
/// `CodeExpired`, not a transport error.
#[tokio::test]
async fn ac2_transport_errors_keep_polling() {
    // A port nothing listens on: every request is refused.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);
    let config = AuthConfig::with_bases(base.clone(), base);
    let clock = Arc::new(ManualClock::new(t0()));
    let flow = DeviceFlow::with_time(config, clock.clone(), clock.clone()).unwrap();

    let result = flow.wait_for_session(&code(6)).await;

    assert_eq!(result, Err(AuthError::CodeExpired));
    assert_eq!(clock.sleeps(), [Duration::from_secs(2); 3]);
}

/// A [`MemoryStore`] that counts saves.
#[derive(Default)]
struct CountingStore {
    inner: MemoryStore,
    saves: AtomicUsize,
}

impl SessionStore for CountingStore {
    fn load(&self) -> Result<Option<Session>, StoreError> {
        self.inner.load()
    }

    fn save(&self, session: &Session) -> Result<(), StoreError> {
        self.saves.fetch_add(1, Ordering::SeqCst);
        self.inner.save(session)
    }

    fn delete(&self) -> Result<(), StoreError> {
        self.inner.delete()
    }
}

/// AC19: right after the grant, `login` refreshes once under the PKCE client
/// and stores that token; when that refresh fails for any reason, the
/// device-flow grant is stored as is and reported as such (for the warning).
#[tokio::test]
async fn ac19_login_refreshes_under_pkce() {
    let granted = include_str!("fixtures/auth/token_granted.json");
    let refreshed = include_str!("fixtures/auth/token_refreshed.json");
    let invalid_client = include_str!("fixtures/auth/refresh_invalid_client.json");
    let invalid_grant = include_str!("fixtures/auth/refresh_invalid_grant.json");
    let cases = [
        (
            "PKCE refresh succeeds",
            json(refreshed),
            "FAKE-ACCESS-2",
            RefreshClient::Pkce,
        ),
        (
            "PKCE client rejected",
            ResponseTemplate::new(401).set_body_raw(invalid_client, "application/json"),
            "FAKE-ACCESS",
            RefreshClient::DeviceFlow,
        ),
        (
            "refresh token rejected",
            ResponseTemplate::new(400).set_body_raw(invalid_grant, "application/json"),
            "FAKE-ACCESS",
            RefreshClient::DeviceFlow,
        ),
        (
            "server error",
            ResponseTemplate::new(503),
            "FAKE-ACCESS",
            RefreshClient::DeviceFlow,
        ),
    ];
    for (name, refresh_response, expected_access, expected_client) in cases {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .and(form_has(&[("grant_type", DEVICE_CODE_GRANT)]))
            .respond_with(json(granted))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .and(form_has(&[("grant_type", "refresh_token")]))
            .respond_with(refresh_response)
            .expect(1)
            .mount(&server)
            .await;
        let clock = Arc::new(ManualClock::new(t0()));
        let store = CountingStore::default();

        let login = flow(&server, &clock)
            .complete_login(&code(300), &store)
            .await
            .unwrap_or_else(|e| panic!("{name}: {e}"));

        server.verify().await;
        let requests = server.received_requests().await.unwrap();
        let mut refresh_form = form(&requests[1]);
        refresh_form.sort();
        let expected_form: Vec<(String, String)> = [
            ("client_id", PKCE_CLIENT_ID),
            ("client_secret", PKCE_CLIENT_SECRET),
            ("grant_type", "refresh_token"),
            ("refresh_token", "FAKE-REFRESH"),
            ("scope", SCOPE),
        ]
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        assert_eq!(refresh_form, expected_form, "{name}: refresh form");
        assert_eq!(
            requests[1].headers.get("authorization"),
            None,
            "{name}: no basic auth"
        );
        assert_eq!(login.client, expected_client, "{name}");
        assert_eq!(login.session.access_token, expected_access, "{name}");
        assert_eq!(login.session.refresh_token, "FAKE-REFRESH", "{name}");
        assert_eq!(
            store.saves.load(Ordering::SeqCst),
            1,
            "{name}: stored once, after the refresh"
        );
        assert_eq!(store.inner.load(), Ok(Some(login.session)), "{name}");
    }
}
