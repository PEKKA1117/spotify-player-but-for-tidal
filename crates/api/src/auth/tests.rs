use super::*;
use std::sync::Arc;
use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const T0: Duration = Duration::from_secs(1_800_000_000);

fn t0() -> SystemTime {
    SystemTime::UNIX_EPOCH + T0
}

fn fixture(name: &str) -> &'static str {
    match name {
        "token_pending" => include_str!("../../tests/fixtures/auth/token_pending.json"),
        "token_slow_down" => include_str!("../../tests/fixtures/auth/token_slow_down.json"),
        "token_expired" => include_str!("../../tests/fixtures/auth/token_expired.json"),
        "token_denied" => include_str!("../../tests/fixtures/auth/token_denied.json"),
        "token_granted" => include_str!("../../tests/fixtures/auth/token_granted.json"),
        "token_granted_no_user" => {
            include_str!("../../tests/fixtures/auth/token_granted_no_user.json")
        }
        "token_granted_empty_country" => {
            include_str!("../../tests/fixtures/auth/token_granted_empty_country.json")
        }
        "token_granted_no_user_id" => {
            include_str!("../../tests/fixtures/auth/token_granted_no_user_id.json")
        }
        "token_refreshed" => include_str!("../../tests/fixtures/auth/token_refreshed.json"),
        "token_refreshed_rotated" => {
            include_str!("../../tests/fixtures/auth/token_refreshed_rotated.json")
        }
        "token_refreshed_country_changed" => {
            include_str!("../../tests/fixtures/auth/token_refreshed_country_changed.json")
        }
        "token_refreshed_no_user" => {
            include_str!("../../tests/fixtures/auth/token_refreshed_no_user.json")
        }
        "refresh_invalid_grant" => {
            include_str!("../../tests/fixtures/auth/refresh_invalid_grant.json")
        }
        "refresh_invalid_client" => {
            include_str!("../../tests/fixtures/auth/refresh_invalid_client.json")
        }
        "refresh_unauthorized_client" => {
            include_str!("../../tests/fixtures/auth/refresh_unauthorized_client.json")
        }
        "echoing_error" => include_str!("../../tests/fixtures/auth/echoing_error.json"),
        "echoing_malformed_grant" => {
            include_str!("../../tests/fixtures/auth/echoing_malformed_grant.json")
        }
        other => panic!("unknown fixture {other}"),
    }
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

#[test]
fn ac2_poll_outcome() {
    let two = Duration::from_secs(2);
    let granted = TokenGrant::from_json(fixture("token_granted")).unwrap();
    let cases: [(&str, u16, &str, Result<PollOutcome, AuthError>); 8] = [
        (
            "pending keeps the interval",
            400,
            "token_pending",
            Ok(PollOutcome::Pending { interval: two }),
        ),
        (
            "slow_down adds 5 s",
            400,
            "token_slow_down",
            Ok(PollOutcome::Pending {
                interval: Duration::from_secs(7),
            }),
        ),
        (
            "expired_token",
            400,
            "token_expired",
            Err(AuthError::CodeExpired),
        ),
        ("access_denied", 400, "token_denied", Err(AuthError::Denied)),
        (
            "granted",
            200,
            "token_granted",
            Ok(PollOutcome::Granted(granted)),
        ),
        (
            "5xx is not yet",
            503,
            "token_pending",
            Ok(PollOutcome::Pending { interval: two }),
        ),
        (
            "other OAuth error",
            401,
            "refresh_invalid_client",
            Err(AuthError::Http {
                status: 401,
                code: Some("invalid_client".into()),
            }),
        ),
        (
            "malformed 200",
            200,
            "echoing_malformed_grant",
            Err(AuthError::Decode("token")),
        ),
    ];
    for (name, status, body, expected) in cases {
        assert_eq!(
            classify_poll(status, fixture(body), two),
            expected,
            "{name}"
        );
    }
}

/// AC3, login half: the full device-code poll against a mock server that
/// would count any `/v1/sessions` call.
#[tokio::test]
async fn ac3_session_from_token_response() {
    type Account = Result<(u64, &'static str), AuthError>;
    let cases: [(&str, Account); 4] = [
        ("token_granted", Ok((1, "FI"))),
        (
            "token_granted_no_user",
            Err(AuthError::SessionInfo("country code")),
        ),
        (
            "token_granted_empty_country",
            Err(AuthError::SessionInfo("country code")),
        ),
        (
            "token_granted_no_user_id",
            Err(AuthError::SessionInfo("user ID")),
        ),
    ];
    for (body, expected) in cases {
        let server = MockServer::start().await;
        // The poll only: the post-login refresh (AC19) gets a 404, which
        // keeps the grant's session as is.
        Mock::given(method("POST"))
            .and(path("/oauth2/token"))
            .and(body_string_contains("device_code=FAKE-DEVICE-CODE"))
            .respond_with(
                ResponseTemplate::new(200).set_body_raw(fixture(body), "application/json"),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(path("/v1/sessions"))
            .respond_with(ResponseTemplate::new(500))
            .expect(0)
            .mount(&server)
            .await;
        let clock = Arc::new(ManualClock::new(t0()));
        let config = AuthConfig::with_bases(
            format!("{}/oauth2", server.uri()),
            format!("{}/v1", server.uri()),
        );
        let flow = DeviceFlow::with_time(config, clock.clone(), clock).unwrap();
        let code = DeviceCode {
            device_code: "FAKE-DEVICE-CODE".into(),
            user_code: "ABCDE".into(),
            verification_uri: "link.tidal.com".into(),
            verification_uri_complete: None,
            expires_in: Duration::from_secs(300),
            interval: Duration::from_secs(2),
        };
        let store = MemoryStore::default();
        let result = flow
            .complete_login(&code, &store)
            .await
            .map(|login| login.session);
        match expected {
            Ok((user_id, country)) => {
                let session = result.unwrap_or_else(|e| panic!("{body}: {e}"));
                assert_eq!(
                    (session.user_id, session.country_code.as_str()),
                    (user_id, country),
                    "{body}"
                );
                assert_eq!(session.expires_at, t0() + Duration::from_secs(14_400));
                assert_eq!(store.load(), Ok(Some(session)), "{body}: stored");
            }
            Err(error) => {
                assert_eq!(result, Err(error), "{body}");
                assert_eq!(store.load(), Ok(None), "{body}: nothing stored");
            }
        }
        server.verify().await;
    }

    // Refresh half: a changed country is picked up, a missing one keeps
    // the previous values.
    let previous = session("FAKE-ACCESS", "FAKE-REFRESH", t0());
    let refresh_cases = [
        ("token_refreshed_country_changed", 1, "SE"),
        ("token_refreshed_no_user", 1, "FI"),
        ("token_refreshed", 1, "FI"),
    ];
    for (body, user_id, country) in refresh_cases {
        let grant = TokenGrant::from_json(fixture(body)).unwrap();
        let refreshed = session_from_refresh(&previous, &grant, t0());
        assert_eq!(
            (
                refreshed.access_token.as_str(),
                refreshed.user_id,
                refreshed.country_code.as_str()
            ),
            ("FAKE-ACCESS-2", user_id, country),
            "{body}"
        );
    }
}

#[test]
fn ac4_needs_refresh() {
    let cases = [
        ("far future", Duration::from_secs(14_400), 0, false),
        ("exactly 60 s", Duration::from_secs(60), 0, false),
        ("59 s", Duration::from_secs(59), 0, true),
        ("expires now", Duration::ZERO, 0, true),
        ("expired", Duration::ZERO, 10, true),
    ];
    for (name, expires_in, elapsed, expected) in cases {
        let s = session("FAKE-ACCESS", "FAKE-REFRESH", t0() + expires_in);
        let now = t0() + Duration::from_secs(elapsed);
        assert_eq!(needs_refresh(&s, now), expected, "{name}");
    }
}

#[test]
fn ac6_refresh_token_kept_or_replaced() {
    let previous = session("FAKE-ACCESS", "FAKE-REFRESH", t0());
    let cases = [
        ("token_refreshed", "FAKE-REFRESH"),
        ("token_refreshed_rotated", "FAKE-REFRESH-2"),
    ];
    for (body, refresh) in cases {
        let grant = TokenGrant::from_json(fixture(body)).unwrap();
        let refreshed = session_from_refresh(&previous, &grant, t0());
        assert_eq!(refreshed.refresh_token, refresh, "{body}");
        assert_eq!(refreshed.access_token, "FAKE-ACCESS-2", "{body}");
        assert_eq!(refreshed.expires_at, t0() + Duration::from_secs(14_400));
    }
}

#[test]
fn ac7_classify_refresh_failure() {
    use RefreshFailure::{SessionLost, Transient};
    let cases = [
        (400, "refresh_invalid_grant", SessionLost),
        (401, "refresh_invalid_grant", SessionLost),
        (400, "refresh_invalid_client", SessionLost),
        (401, "refresh_invalid_client", SessionLost),
        (400, "refresh_unauthorized_client", SessionLost),
        (401, "refresh_unauthorized_client", SessionLost),
        (401, "", SessionLost),
        (500, "", Transient),
        (502, "refresh_invalid_grant", Transient),
        (503, "", Transient),
        (429, "", Transient),
        // Not in the spec's table: kept usable rather than logged out.
        (400, "token_pending", Transient),
        (403, "", Transient),
    ];
    for (status, body, expected) in cases {
        let body = if body.is_empty() { "" } else { fixture(body) };
        assert_eq!(
            classify_refresh_failure(status, body),
            expected,
            "{status} {body}"
        );
    }
}

/// AC19: the warning is true whatever made the PKCE refresh fail.
#[test]
fn ac19_warning_names_no_cause() {
    assert!(LOSSY_WARNING.contains("AAC"), "{LOSSY_WARNING}");
    for cause in ["did not accept", "rejected", "refused"] {
        assert!(!LOSSY_WARNING.contains(cause), "{LOSSY_WARNING}");
    }
}

#[test]
fn ac15_no_tokens_in_debug_or_errors() {
    let s = session("FAKE-ACCESS", "FAKE-REFRESH", t0());
    let debug = format!("{s:?}");
    assert!(!debug.contains("FAKE-ACCESS"), "{debug}");
    assert!(!debug.contains("FAKE-REFRESH"), "{debug}");
    assert!(debug.contains("<redacted>"), "{debug}");

    let grant = TokenGrant::from_json(fixture("token_granted")).unwrap();
    let debug = format!("{grant:?} {:?}", PollOutcome::Granted(grant.clone()));
    assert!(!debug.contains("FAKE-ACCESS"), "{debug}");
    assert!(!debug.contains("FAKE-REFRESH"), "{debug}");

    // Errors built from server bodies that echo the tokens.
    let two = Duration::from_secs(2);
    let echoed = [
        classify_poll(400, fixture("echoing_error"), two).unwrap_err(),
        classify_poll(200, fixture("echoing_malformed_grant"), two).unwrap_err(),
        TokenGrant::from_json(fixture("echoing_malformed_grant")).unwrap_err(),
    ];
    let mut errors = vec![
        AuthError::Transport("connection refused".into()),
        AuthError::Http {
            status: 500,
            code: None,
        },
        AuthError::Decode("token"),
        AuthError::CodeExpired,
        AuthError::Denied,
        AuthError::SessionInfo("country code"),
        AuthError::LoginRequired,
        AuthError::Unauthorized,
        AuthError::Store(StoreError::WrongPassphrase),
    ];
    errors.extend(echoed);
    for error in &errors {
        // Fails to compile when a variant is added, so it gets a case.
        match error {
            AuthError::Transport(_)
            | AuthError::Http { .. }
            | AuthError::Decode(_)
            | AuthError::CodeExpired
            | AuthError::Denied
            | AuthError::SessionInfo(_)
            | AuthError::LoginRequired
            | AuthError::Unauthorized
            | AuthError::Store(_) => {}
        }
        for text in [error.to_string(), format!("{error:?}")] {
            assert!(!text.contains("FAKE-ACCESS"), "{text}");
            assert!(!text.contains("FAKE-REFRESH"), "{text}");
        }
    }
}

#[test]
fn memory_store_round_trip() {
    let session = session(
        "FAKE-ACCESS",
        "FAKE-REFRESH",
        SystemTime::UNIX_EPOCH + Duration::from_secs(14_400),
    );
    let store = MemoryStore::default();
    assert_eq!(store.load(), Ok(None));
    store.save(&session).unwrap();
    assert_eq!(store.load(), Ok(Some(session)));
    store.delete().unwrap();
    store.delete().unwrap();
    assert_eq!(store.load(), Ok(None));
}

#[test]
fn form_body_encodes() {
    assert_eq!(
        form_body(&[("scope", SCOPE), ("grant_type", DEVICE_CODE_GRANT)]),
        "scope=r_usr+w_usr+w_sub&grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Adevice_code"
    );
}
