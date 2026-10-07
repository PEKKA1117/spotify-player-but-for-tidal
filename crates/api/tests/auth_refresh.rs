//! Spec 0002, session use and refresh: AC5-AC9. No real network; time is a
//! `ManualClock`.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use serde_json::Value;
use tidal_player_api::auth::{
    AuthConfig, AuthError, AuthStatus, Authenticator, CLIENT_ID, CLIENT_SECRET, ManualClock,
    MemoryStore, Session, SessionStore, StoreError,
};
use wiremock::matchers::{basic_auth, header, method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

const API_PATH: &str = "/v1/thing";
const REFRESHED: &str = include_str!("fixtures/auth/token_refreshed.json");
const ROTATED: &str = include_str!("fixtures/auth/token_refreshed_rotated.json");
const INVALID_GRANT: &str = include_str!("fixtures/auth/refresh_invalid_grant.json");
const API_401: &str = include_str!("fixtures/auth/api_unauthorized.json");

type Log = Arc<Mutex<Vec<String>>>;

fn t0() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000)
}

fn session(access: &str, refresh: &str, expires_at: SystemTime) -> Session {
    Session {
        access_token: access.into(),
        refresh_token: refresh.into(),
        expires_at,
        user_id: 1,
        country_code: "FI".into(),
    }
}

/// Inside the 60 s refresh window.
fn expiring() -> Session {
    session(
        "FAKE-ACCESS",
        "FAKE-REFRESH",
        t0() + Duration::from_secs(30),
    )
}

/// Valid for an hour.
fn fresh() -> Session {
    session(
        "FAKE-ACCESS",
        "FAKE-REFRESH",
        t0() + Duration::from_secs(3600),
    )
}

/// A [`MemoryStore`] that counts loads and logs saves.
#[derive(Default)]
struct RecordingStore {
    inner: MemoryStore,
    loads: AtomicUsize,
    log: Log,
}

impl RecordingStore {
    fn loads(&self) -> usize {
        self.loads.load(Ordering::SeqCst)
    }
}

impl SessionStore for RecordingStore {
    fn load(&self) -> Result<Option<Session>, StoreError> {
        self.loads.fetch_add(1, Ordering::SeqCst);
        self.inner.load()
    }

    fn save(&self, session: &Session) -> Result<(), StoreError> {
        self.log
            .lock()
            .unwrap()
            .push(format!("save {}", session.access_token));
        self.inner.save(session)
    }

    fn delete(&self) -> Result<(), StoreError> {
        self.inner.delete()
    }
}

/// Logs `label` to the shared log, then answers with `template`.
struct Logged {
    log: Log,
    label: &'static str,
    template: ResponseTemplate,
}

impl Respond for Logged {
    fn respond(&self, _: &Request) -> ResponseTemplate {
        self.log.lock().unwrap().push(self.label.into());
        self.template.clone()
    }
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

/// Saves `session` to `store` (another process logging in or refreshing),
/// then rejects the refresh token.
struct SwapStoreThenReject {
    store: Arc<RecordingStore>,
    session: Session,
}

impl Respond for SwapStoreThenReject {
    fn respond(&self, _: &Request) -> ResponseTemplate {
        self.store.inner.save(&self.session).unwrap();
        status_json(400, INVALID_GRANT)
    }
}

fn status_json(status: u16, body: &str) -> ResponseTemplate {
    ResponseTemplate::new(status).set_body_raw(body.to_owned(), "application/json")
}

fn ok_json() -> ResponseTemplate {
    status_json(200, r#"{"ok":true}"#)
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
fn form_has(fields: &[(&str, &str)]) -> impl Fn(&Request) -> bool + use<> {
    let fields: Vec<(String, String)> = fields
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    move |request: &Request| {
        let got = form(request);
        fields.iter().all(|field| got.contains(field))
    }
}

/// `POST /oauth2/token` refreshing `refresh_token`.
fn refresh_mock(refresh_token: &str) -> wiremock::MockBuilder {
    Mock::given(method("POST"))
        .and(path("/oauth2/token"))
        .and(basic_auth(CLIENT_ID, CLIENT_SECRET))
        .and(form_has(&[
            ("client_id", CLIENT_ID),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
        ]))
}

/// `GET /v1/thing` with `Authorization: Bearer <access_token>`.
fn api_mock(access_token: &str) -> wiremock::MockBuilder {
    Mock::given(method("GET"))
        .and(path(API_PATH))
        .and(header("authorization", format!("Bearer {access_token}")))
}

struct Setup {
    server: MockServer,
    clock: Arc<ManualClock>,
    store: Arc<RecordingStore>,
    auth: Arc<Authenticator>,
}

impl Setup {
    /// An authenticator holding `in_memory`; the store holds `stored`.
    async fn new(in_memory: Session, stored: Option<Session>) -> Self {
        let server = MockServer::start().await;
        let clock = Arc::new(ManualClock::new(t0()));
        let store = Arc::new(RecordingStore::default());
        if let Some(stored) = stored {
            store.inner.save(&stored).unwrap();
        }
        let config = AuthConfig::with_bases(
            format!("{}/oauth2", server.uri()),
            format!("{}/v1", server.uri()),
        );
        let auth = Authenticator::new(config, store.clone(), in_memory, clock.clone()).unwrap();
        Self {
            server,
            clock,
            store,
            auth: Arc::new(auth),
        }
    }

    async fn get(&self) -> Result<Value, AuthError> {
        self.auth.get_json::<Value>("/thing").await
    }

    /// `(method, path)` of every request the mock server got, in order.
    async fn requests(&self) -> Vec<(String, String)> {
        self.server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .map(|r| (r.method.to_string(), r.url.path().to_owned()))
            .collect()
    }
}

fn get_req() -> (String, String) {
    ("GET".into(), API_PATH.into())
}

fn refresh_req() -> (String, String) {
    ("POST".into(), "/oauth2/token".into())
}

#[tokio::test]
async fn ac5_bearer_header() {
    let s = Setup::new(fresh(), Some(fresh())).await;
    api_mock("FAKE-ACCESS")
        .respond_with(ok_json())
        .expect(1)
        .mount(&s.server)
        .await;

    let result = s.get().await;

    assert_eq!(result, Ok(serde_json::json!({"ok": true})));
    assert_eq!(s.requests().await, [get_req()]);
}

#[tokio::test]
async fn ac5_proactive_refresh() {
    let s = Setup::new(expiring(), Some(expiring())).await;
    refresh_mock("FAKE-REFRESH")
        .respond_with(status_json(200, REFRESHED))
        .expect(1)
        .mount(&s.server)
        .await;
    api_mock("FAKE-ACCESS-2")
        .respond_with(ok_json())
        .expect(1)
        .mount(&s.server)
        .await;

    let result = s.get().await;

    assert_eq!(result, Ok(serde_json::json!({"ok": true})));
    assert_eq!(s.requests().await, [refresh_req(), get_req()]);
}

#[tokio::test]
async fn ac5_retry_once_on_401() {
    let s = Setup::new(fresh(), Some(fresh())).await;
    api_mock("FAKE-ACCESS")
        .respond_with(status_json(401, API_401))
        .expect(1)
        .mount(&s.server)
        .await;
    refresh_mock("FAKE-REFRESH")
        .respond_with(status_json(200, REFRESHED))
        .expect(1)
        .mount(&s.server)
        .await;
    api_mock("FAKE-ACCESS-2")
        .respond_with(ok_json())
        .expect(1)
        .mount(&s.server)
        .await;

    let result = s.get().await;

    assert_eq!(result, Ok(serde_json::json!({"ok": true})));
    assert_eq!(s.requests().await, [get_req(), refresh_req(), get_req()]);
}

#[tokio::test]
async fn ac5_second_401_is_error() {
    let s = Setup::new(fresh(), Some(fresh())).await;
    Mock::given(method("GET"))
        .and(path(API_PATH))
        .respond_with(status_json(401, API_401))
        .expect(2)
        .mount(&s.server)
        .await;
    refresh_mock("FAKE-REFRESH")
        .respond_with(status_json(200, REFRESHED))
        .expect(1)
        .mount(&s.server)
        .await;

    let result = s.get().await;

    assert_eq!(result, Err(AuthError::Unauthorized));
    assert_eq!(s.requests().await, [get_req(), refresh_req(), get_req()]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ac5_single_flight() {
    let s = Setup::new(expiring(), Some(expiring())).await;
    refresh_mock("FAKE-REFRESH")
        .respond_with(status_json(200, REFRESHED))
        .expect(1)
        .mount(&s.server)
        .await;
    api_mock("FAKE-ACCESS-2")
        .respond_with(ok_json())
        .expect(8)
        .mount(&s.server)
        .await;

    let tasks: Vec<_> = (0..8)
        .map(|_| {
            let auth = s.auth.clone();
            tokio::spawn(async move { auth.get_json::<Value>("/thing").await })
        })
        .collect();
    for task in tasks {
        assert_eq!(task.await.unwrap(), Ok(serde_json::json!({"ok": true})));
    }

    s.server.verify().await;
}

#[tokio::test]
async fn ac6_persist_on_refresh() {
    let cases = [
        ("old refresh token kept", REFRESHED, "FAKE-REFRESH"),
        ("new refresh token replaces it", ROTATED, "FAKE-REFRESH-2"),
    ];
    for (name, refresh_body, expected_refresh) in cases {
        let s = Setup::new(fresh(), Some(fresh())).await;
        let log = s.store.log.clone();
        api_mock("FAKE-ACCESS")
            .respond_with(Logged {
                log: log.clone(),
                label: "GET FAKE-ACCESS",
                template: status_json(401, API_401),
            })
            .expect(1)
            .mount(&s.server)
            .await;
        refresh_mock("FAKE-REFRESH")
            .respond_with(status_json(200, refresh_body))
            .expect(1)
            .mount(&s.server)
            .await;
        api_mock("FAKE-ACCESS-2")
            .respond_with(Logged {
                log: log.clone(),
                label: "GET FAKE-ACCESS-2",
                template: ok_json(),
            })
            .expect(1)
            .mount(&s.server)
            .await;

        let result = s.get().await;

        assert!(result.is_ok(), "{name}: {result:?}");
        assert_eq!(
            *log.lock().unwrap(),
            ["GET FAKE-ACCESS", "save FAKE-ACCESS-2", "GET FAKE-ACCESS-2"],
            "{name}: saved before the retry"
        );
        let stored = s.store.inner.load().unwrap();
        let expected = session(
            "FAKE-ACCESS-2",
            expected_refresh,
            t0() + Duration::from_secs(14_400),
        );
        assert_eq!(stored, Some(expected), "{name}");
        s.server.verify().await;
    }
}

#[tokio::test]
async fn ac7_login_required_is_sticky() {
    let s = Setup::new(expiring(), Some(expiring())).await;
    refresh_mock("FAKE-REFRESH")
        .respond_with(status_json(400, INVALID_GRANT))
        .expect(1)
        .mount(&s.server)
        .await;
    Mock::given(method("GET"))
        .respond_with(ok_json())
        .expect(0)
        .mount(&s.server)
        .await;
    let status = s.auth.status();

    assert_eq!(s.get().await, Err(AuthError::LoginRequired));
    assert_eq!(*status.borrow(), AuthStatus::LoginRequired);
    let sent = s.requests().await;
    assert_eq!(sent, [refresh_req()]);

    assert_eq!(s.get().await, Err(AuthError::LoginRequired));
    assert_eq!(s.requests().await, sent, "no request in the lost state");
    assert_eq!(
        s.store.inner.load(),
        Ok(Some(expiring())),
        "stored session kept"
    );
    assert!(s.store.log.lock().unwrap().is_empty(), "no save");
}

#[tokio::test]
async fn ac7_transient_keeps_session() {
    let s = Setup::new(expiring(), Some(expiring())).await;
    refresh_mock("FAKE-REFRESH")
        .respond_with(Sequence {
            responses: vec![ResponseTemplate::new(503), status_json(200, REFRESHED)],
            next: AtomicUsize::new(0),
        })
        .expect(2)
        .mount(&s.server)
        .await;
    api_mock("FAKE-ACCESS-2")
        .respond_with(ok_json())
        .expect(1)
        .mount(&s.server)
        .await;
    let status = s.auth.status();

    let first = s.get().await;
    assert!(
        first.as_ref().is_err_and(AuthError::is_transient),
        "{first:?}"
    );
    assert_eq!(*status.borrow(), AuthStatus::Active);

    let second = s.get().await;
    assert_eq!(second, Ok(serde_json::json!({"ok": true})));
    assert_eq!(
        s.requests().await,
        [refresh_req(), refresh_req(), get_req()]
    );
}

#[tokio::test]
async fn ac8a_adopt_newer_stored() {
    let newer = session(
        "FAKE-ACCESS-NEW",
        "FAKE-REFRESH-NEW",
        t0() + Duration::from_secs(14_400),
    );
    let s = Setup::new(expiring(), Some(newer)).await;
    Mock::given(method("POST"))
        .respond_with(status_json(200, REFRESHED))
        .expect(0)
        .mount(&s.server)
        .await;
    api_mock("FAKE-ACCESS-NEW")
        .respond_with(ok_json())
        .expect(1)
        .mount(&s.server)
        .await;

    let result = s.get().await;

    assert_eq!(result, Ok(serde_json::json!({"ok": true})));
    assert_eq!(s.requests().await, [get_req()]);
}

#[tokio::test]
async fn ac8b_retry_with_rotated_token() {
    let s = Setup::new(expiring(), Some(expiring())).await;
    let other = session(
        "FAKE-ACCESS-OTHER",
        "FAKE-REFRESH-OTHER",
        t0() + Duration::from_secs(30),
    );
    refresh_mock("FAKE-REFRESH")
        .respond_with(SwapStoreThenReject {
            store: s.store.clone(),
            session: other,
        })
        .expect(1)
        .mount(&s.server)
        .await;
    refresh_mock("FAKE-REFRESH-OTHER")
        .respond_with(status_json(200, REFRESHED))
        .expect(1)
        .mount(&s.server)
        .await;
    api_mock("FAKE-ACCESS-2")
        .respond_with(ok_json())
        .expect(1)
        .mount(&s.server)
        .await;

    let result = s.get().await;

    assert_eq!(result, Ok(serde_json::json!({"ok": true})));
    assert_eq!(
        s.requests().await,
        [refresh_req(), refresh_req(), get_req()]
    );
    let stored = s.store.inner.load().unwrap().unwrap();
    assert_eq!(stored.refresh_token, "FAKE-REFRESH-OTHER");
    assert_eq!(stored.access_token, "FAKE-ACCESS-2");
}

#[tokio::test]
async fn ac9_recover_after_login() {
    let s = Setup::new(expiring(), Some(expiring())).await;
    refresh_mock("FAKE-REFRESH")
        .respond_with(status_json(400, INVALID_GRANT))
        .expect(1)
        .mount(&s.server)
        .await;
    api_mock("FAKE-ACCESS-NEW")
        .respond_with(ok_json())
        .expect(1)
        .mount(&s.server)
        .await;
    let status = s.auth.status();
    assert_eq!(s.get().await, Err(AuthError::LoginRequired));
    let sent = s.requests().await;

    // Another process logs in.
    let new = session(
        "FAKE-ACCESS-NEW",
        "FAKE-REFRESH-NEW",
        t0() + Duration::from_secs(14_400),
    );
    s.store.inner.save(&new).unwrap();

    s.clock.advance(Duration::from_secs(4));
    let loads = s.store.loads();
    assert_eq!(s.get().await, Err(AuthError::LoginRequired), "at +4 s");
    assert_eq!(s.store.loads(), loads, "no re-read within 5 s");
    assert_eq!(s.requests().await, sent, "no request within 5 s");

    s.clock.advance(Duration::from_secs(1));
    assert_eq!(
        s.get().await,
        Ok(serde_json::json!({"ok": true})),
        "at +5 s"
    );
    assert_eq!(*status.borrow(), AuthStatus::Active);
    s.server.verify().await;
}

/// Spec 0002 AC18: a `401` with `subStatus` 4005 ("Asset is not ready for
/// playback") is not about the token: no refresh, no retry, and the caller
/// gets the response as sent.
#[tokio::test]
async fn ac18_401_4005_is_not_auth() {
    let not_available = include_str!("fixtures/stream/error_not_available_4005.json");
    let s = Setup::new(fresh(), Some(fresh())).await;
    api_mock("FAKE-ACCESS")
        .respond_with(status_json(401, not_available))
        .mount(&s.server)
        .await;
    refresh_mock("FAKE-REFRESH")
        .respond_with(status_json(200, REFRESHED))
        .mount(&s.server)
        .await;
    api_mock("FAKE-ACCESS-2")
        .respond_with(ok_json())
        .mount(&s.server)
        .await;

    let result = s.auth.get("/thing", &[], None).await;

    assert_eq!(s.requests().await, [get_req()], "no refresh, no retry");
    let response = result.expect("the 401 response");
    assert_eq!(response.status().as_u16(), 401);
    assert_eq!(response.body(), not_available.as_bytes());
    assert_eq!(response.sub_status(), Some(4005));
    assert_eq!(*s.auth.status().borrow(), AuthStatus::Active);
}
