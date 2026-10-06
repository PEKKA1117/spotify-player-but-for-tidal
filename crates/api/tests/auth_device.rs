//! Spec 0002, device flow: AC1, AC2. No real network, no real sleep.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, SystemTime};

use tidal_player_api::auth::{
    AuthConfig, AuthError, CLIENT_ID, CLIENT_SECRET, DEVICE_CODE_GRANT, DeviceCode, DeviceFlow,
    ManualClock, SCOPE,
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
