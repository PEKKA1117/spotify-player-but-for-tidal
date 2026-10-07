//! Tidal authentication. See docs/specs/0002-auth.md.
//!
//! - [`Session`] and the [`SessionStore`] seam the binary implements
//!   (keyring, encrypted file)
//! - the OAuth2 device flow: [`DeviceFlow`] (AC1–AC3)
//! - [`Authenticator`]: owns the in-memory session, sends authenticated
//!   requests, refreshes (single flight, persisted) and recovers (AC4–AC9)
//! - the pure classifiers the above are built on ([`classify_poll`],
//!   [`classify_refresh_failure`], [`needs_refresh`])
//!
//! Tokens never appear in `Debug` output or error messages (AC15).

mod authenticator;
mod device;

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

pub use authenticator::{AuthStatus, Authenticator};
pub use device::{DeviceCode, DeviceFlow, Login};

/// Production base URL of Tidal's OAuth2 endpoints.
pub const AUTH_BASE: &str = "https://auth.tidal.com/v1/oauth2";
/// Production base URL of the Tidal v1 API.
pub const API_BASE: &str = "https://api.tidal.com/v1";
/// Public client ID of the official Tidal app (spec 0002, decision 2).
pub const CLIENT_ID: &str = "fX2JxdmntZWK0ixT";
/// Public client secret of the official Tidal app (spec 0002, decision 2).
pub const CLIENT_SECRET: &str = "1Nn9AfDAjxrgJFJbKNWLeAyKGVGmINuXPPLHVXAvxAg=";
/// Public client ID of the official Android app's PKCE login, used for every
/// refresh: tokens of [`CLIENT_ID`] get AAC for 16-bit tracks (AC19). The
/// `client_id_pkce` that `tidalapi` ships (spec 0002, decision 2).
pub const PKCE_CLIENT_ID: &str = "6BDSRdpK9hqEBTgU";
/// Public client secret that goes with [`PKCE_CLIENT_ID`] (`tidalapi`'s
/// `client_secret_pkce`).
pub const PKCE_CLIENT_SECRET: &str = "xeuPmY7nbpZ9IIbLAcQ93shka1VNheUAqN6IcszjTG8=";
/// OAuth2 scopes requested at login.
pub const SCOPE: &str = "r_usr w_usr w_sub";

/// Grant type of a device-code poll (RFC 8628).
pub const DEVICE_CODE_GRANT: &str = "urn:ietf:params:oauth:grant-type:device_code";
/// A token that expires within this margin is refreshed before use (AC4).
pub const REFRESH_MARGIN: Duration = Duration::from_secs(60);
/// Added to the poll interval on `slow_down` (AC2).
pub const SLOW_DOWN_STEP: Duration = Duration::from_secs(5);
/// In the "session lost" state, storage is re-read at most this often (AC9).
pub const LOST_RECHECK: Duration = Duration::from_secs(5);

/// A logged-in Tidal session, as stored between runs.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    pub access_token: String,
    pub refresh_token: String,
    /// Absolute expiry of `access_token`, computed at receipt from `expires_in`.
    pub expires_at: SystemTime,
    pub user_id: u64,
    pub country_code: String,
}

/// Redacts both tokens (AC15).
impl fmt::Debug for Session {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Session")
            .field("access_token", &"<redacted>")
            .field("refresh_token", &"<redacted>")
            .field("expires_at", &self.expires_at)
            .field("user_id", &self.user_id)
            .field("country_code", &self.country_code)
            .finish()
    }
}

/// Errors from a [`SessionStore`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StoreError {
    /// The backend cannot be used at all here (no Secret Service, no
    /// passphrase source, ...); a fallback store may take over.
    #[error("session store unavailable: {0}")]
    Unavailable(String),
    /// The encrypted session file exists but the passphrase does not open it.
    #[error("wrong passphrase for the session file")]
    WrongPassphrase,
    /// Any other failure (I/O, encoding); never contains a token.
    #[error("session store error: {0}")]
    Other(String),
}

/// Where the session lives between runs. Implementations block briefly
/// (keyring call, small file), so they are synchronous.
pub trait SessionStore: Send + Sync {
    /// The stored session, or `None` when not logged in (missing, unreadable
    /// or unknown-version data).
    fn load(&self) -> Result<Option<Session>, StoreError>;
    /// Replaces the stored session.
    fn save(&self, session: &Session) -> Result<(), StoreError>;
    /// Removes the stored session; succeeds when there is none.
    fn delete(&self) -> Result<(), StoreError>;
}

/// In-memory [`SessionStore`], for tests and as a building block.
#[derive(Debug, Default)]
pub struct MemoryStore {
    session: Mutex<Option<Session>>,
}

impl MemoryStore {
    /// A store that already holds `session`.
    pub fn with_session(session: Session) -> Self {
        Self {
            session: Mutex::new(Some(session)),
        }
    }
}

impl SessionStore for MemoryStore {
    fn load(&self) -> Result<Option<Session>, StoreError> {
        Ok(self.session.lock().expect("poisoned").clone())
    }

    fn save(&self, session: &Session) -> Result<(), StoreError> {
        *self.session.lock().expect("poisoned") = Some(session.clone());
        Ok(())
    }

    fn delete(&self) -> Result<(), StoreError> {
        *self.session.lock().expect("poisoned") = None;
        Ok(())
    }
}

/// Errors from the device flow and the [`Authenticator`]. No variant ever
/// carries a token or a server body (AC15).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AuthError {
    /// No HTTP response (connection, timeout, TLS). Transient.
    #[error("network error talking to Tidal: {0}")]
    Transport(String),
    /// An HTTP status the caller does not handle. `code` is the OAuth2
    /// `error` code, kept only when it is one of the known codes. A `5xx`
    /// or `429` from a refresh is transient.
    #[error("unexpected HTTP status {status}{}", code.as_deref().map(|c| format!(" ({c})")).unwrap_or_default())]
    Http { status: u16, code: Option<String> },
    /// A `2xx` body that could not be decoded; names what was being read.
    #[error("malformed {0} response from Tidal")]
    Decode(&'static str),
    /// The device code expired before the user approved it (AC2).
    #[error("Login code expired, run \"tidal-player login\" again")]
    CodeExpired,
    /// The user denied the login on the Tidal page (AC2).
    #[error("Login was denied")]
    Denied,
    /// The token response lacks the user ID or country (AC3); names which.
    #[error("Tidal's login response has no {0}")]
    SessionInfo(&'static str),
    /// The session is lost (refresh token rejected): run `tidal-player login`.
    #[error("Session expired: run \"tidal-player login\"")]
    LoginRequired,
    /// A request was rejected with `401` again right after a refresh (AC5).
    #[error("Tidal rejected the access token (HTTP 401) after a refresh")]
    Unauthorized,
    /// The session store failed.
    #[error(transparent)]
    Store(#[from] StoreError),
}

impl AuthError {
    /// Whether a later attempt may succeed without a new login: network
    /// errors, `5xx` and `429`.
    pub fn is_transient(&self) -> bool {
        match self {
            Self::Transport(_) => true,
            Self::Http { status, .. } => *status == 429 || *status >= 500,
            _ => false,
        }
    }

    /// Maps a transport-level failure; the message never contains a token
    /// (tokens travel in headers and form bodies, never in URLs).
    pub(crate) fn transport(error: &reqwest::Error) -> Self {
        Self::Transport(error.to_string())
    }
}

/// Source of the current time, injected so tests never wait.
pub trait Clock: Send + Sync {
    fn now(&self) -> SystemTime;
}

/// A boxed `Send` future, as returned by [`Sleeper::sleep`].
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Waits between device-flow polls, injected so tests never sleep.
pub trait Sleeper: Send + Sync {
    fn sleep(&self, duration: Duration) -> BoxFuture<'_, ()>;
}

/// The real clock and `tokio` sleep.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> SystemTime {
        SystemTime::now()
    }
}

impl Sleeper for SystemClock {
    fn sleep(&self, duration: Duration) -> BoxFuture<'_, ()> {
        Box::pin(tokio::time::sleep(duration))
    }
}

/// A manually driven clock for tests: `sleep` returns at once and advances
/// the clock by the requested duration, which it also records.
#[derive(Debug)]
pub struct ManualClock {
    now: Mutex<SystemTime>,
    sleeps: Mutex<Vec<Duration>>,
}

impl ManualClock {
    /// A clock that reads `start` until advanced.
    pub fn new(start: SystemTime) -> Self {
        Self {
            now: Mutex::new(start),
            sleeps: Mutex::new(Vec::new()),
        }
    }

    /// Moves the clock forward by `by`.
    pub fn advance(&self, by: Duration) {
        *self.now.lock().expect("poisoned") += by;
    }

    /// Every duration passed to [`Sleeper::sleep`] so far, in order.
    pub fn sleeps(&self) -> Vec<Duration> {
        self.sleeps.lock().expect("poisoned").clone()
    }
}

impl Clock for ManualClock {
    fn now(&self) -> SystemTime {
        *self.now.lock().expect("poisoned")
    }
}

impl Sleeper for ManualClock {
    fn sleep(&self, duration: Duration) -> BoxFuture<'_, ()> {
        self.sleeps.lock().expect("poisoned").push(duration);
        self.advance(duration);
        Box::pin(std::future::ready(()))
    }
}

/// Endpoints and client credentials, injected so tests use a mock server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthConfig {
    /// Base of the OAuth2 endpoints (`/device_authorization`, `/token`).
    pub auth_base: String,
    /// Base of the API that [`Authenticator::get_json`] paths resolve against.
    pub api_base: String,
    /// The device-flow (login) client: [`CLIENT_ID`] in production.
    pub client_id: String,
    pub client_secret: String,
    /// The client refreshes use: [`PKCE_CLIENT_ID`] in production (AC19).
    pub pkce_client_id: String,
    pub pkce_client_secret: String,
}

impl AuthConfig {
    /// Tidal's production endpoints and the official app's credentials.
    pub fn production() -> Self {
        Self::with_bases(AUTH_BASE, API_BASE)
    }

    /// The production credentials against other base URLs (tests).
    pub fn with_bases(auth_base: impl Into<String>, api_base: impl Into<String>) -> Self {
        Self {
            auth_base: auth_base.into(),
            api_base: api_base.into(),
            client_id: CLIENT_ID.into(),
            client_secret: CLIENT_SECRET.into(),
            pkce_client_id: PKCE_CLIENT_ID.into(),
            pkce_client_secret: PKCE_CLIENT_SECRET.into(),
        }
    }

    pub(crate) fn auth_url(&self, endpoint: &str) -> String {
        join(&self.auth_base, endpoint)
    }

    pub(crate) fn api_url(&self, path: &str) -> String {
        join(&self.api_base, path)
    }
}

/// Which client a session's access token was refreshed under (AC19).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshClient {
    /// The PKCE client: 16-bit tracks stream as FLAC.
    Pkce,
    /// The device-flow client the login used, after Tidal rejected the PKCE
    /// client: 16-bit tracks stream as AAC.
    DeviceFlow,
}

/// Joins a base URL and a path with exactly one slash between them.
fn join(base: &str, path: &str) -> String {
    format!(
        "{}/{}",
        base.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}

/// Encodes `pairs` as an `application/x-www-form-urlencoded` body.
pub(crate) fn form_body(pairs: &[(&str, &str)]) -> String {
    let mut url = reqwest::Url::parse("http://form.invalid/").expect("static URL");
    url.query_pairs_mut().extend_pairs(pairs);
    url.query().unwrap_or_default().to_owned()
}

/// A `200` from `POST /token`: a device-code grant or a refresh. Its `Debug`
/// redacts the tokens.
#[derive(Clone, PartialEq, Eq, Deserialize)]
pub struct TokenGrant {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    expires_in: u64,
    #[serde(default)]
    user_id: Option<u64>,
    #[serde(default)]
    user: Option<GrantUser>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct GrantUser {
    #[serde(rename = "countryCode", default)]
    country_code: Option<String>,
}

impl fmt::Debug for TokenGrant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TokenGrant")
            .field("access_token", &"<redacted>")
            .field(
                "refresh_token",
                &self.refresh_token.as_ref().map(|_| "<redacted>"),
            )
            .field("expires_in", &self.expires_in)
            .field("user_id", &self.user_id)
            .field("user", &self.user)
            .finish()
    }
}

impl TokenGrant {
    /// Parses a `200` body of `POST /token`.
    pub fn from_json(body: &str) -> Result<Self, AuthError> {
        serde_json::from_str(body).map_err(|_| AuthError::Decode("token"))
    }

    fn expires_at(&self, now: SystemTime) -> SystemTime {
        now + Duration::from_secs(self.expires_in)
    }

    /// The account's user ID and non-empty country, or which one is missing.
    fn account(&self) -> Result<(u64, String), AuthError> {
        let user_id = self.user_id.ok_or(AuthError::SessionInfo("user ID"))?;
        let country = self
            .country_code()
            .ok_or(AuthError::SessionInfo("country code"))?;
        Ok((user_id, country))
    }

    /// `user.countryCode`, when present and not empty.
    fn country_code(&self) -> Option<String> {
        self.user
            .as_ref()?
            .country_code
            .clone()
            .filter(|country| !country.is_empty())
    }
}

/// Builds the session of a fresh login (AC3). Fails with
/// [`AuthError::SessionInfo`] when the user ID or country is missing.
pub fn session_from_login(grant: &TokenGrant, now: SystemTime) -> Result<Session, AuthError> {
    let (user_id, country_code) = grant.account()?;
    Ok(Session {
        access_token: grant.access_token.clone(),
        refresh_token: grant.refresh_token.clone().unwrap_or_default(),
        expires_at: grant.expires_at(now),
        user_id,
        country_code,
    })
}

/// Builds the session after a refresh of `previous` (AC3, AC6): a missing
/// `refresh_token` keeps the old one, missing account data keeps the old
/// values (with a warning).
pub fn session_from_refresh(previous: &Session, grant: &TokenGrant, now: SystemTime) -> Session {
    let (user_id, country_code) = match grant.account() {
        Ok(account) => account,
        Err(error) => {
            tracing::warn!(%error, "refresh response lacks account data; keeping the previous values");
            (previous.user_id, previous.country_code.clone())
        }
    };
    Session {
        access_token: grant.access_token.clone(),
        refresh_token: grant
            .refresh_token
            .clone()
            .unwrap_or_else(|| previous.refresh_token.clone()),
        expires_at: grant.expires_at(now),
        user_id,
        country_code,
    }
}

/// What one device-code poll says to do next (AC2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PollOutcome {
    /// Not approved yet: poll again after `interval`.
    Pending { interval: Duration },
    /// Approved: the tokens.
    Granted(TokenGrant),
}

/// The OAuth2 error codes Tidal's `/token` is known or assumed to send. Only
/// these are ever copied into an error, so a body echoing a token cannot leak
/// it (AC15).
const KNOWN_ERROR_CODES: [&str; 10] = [
    "authorization_pending",
    "slow_down",
    "expired_token",
    "access_denied",
    "invalid_grant",
    "invalid_client",
    "invalid_request",
    "invalid_scope",
    "unauthorized_client",
    "unsupported_grant_type",
];

/// The OAuth2 `error` code of a `/token` error body, if it is a known one.
pub(crate) fn error_code(body: &str) -> Option<&'static str> {
    #[derive(Deserialize)]
    struct ErrorBody {
        error: Option<String>,
    }
    let error = serde_json::from_str::<ErrorBody>(body).ok()?.error?;
    KNOWN_ERROR_CODES.into_iter().find(|known| *known == error)
}

/// Maps one `/token` response to a device-code poll to what happens next
/// (AC2). `interval` is the current poll interval. `5xx` and `429` count as
/// "not yet", like transport errors.
pub fn classify_poll(
    status: u16,
    body: &str,
    interval: Duration,
) -> Result<PollOutcome, AuthError> {
    if (200..300).contains(&status) {
        return TokenGrant::from_json(body).map(PollOutcome::Granted);
    }
    if status == 429 || status >= 500 {
        return Ok(PollOutcome::Pending { interval });
    }
    match error_code(body) {
        Some("authorization_pending") => Ok(PollOutcome::Pending { interval }),
        Some("slow_down") => Ok(PollOutcome::Pending {
            interval: interval + SLOW_DOWN_STEP,
        }),
        Some("expired_token") => Err(AuthError::CodeExpired),
        Some("access_denied") => Err(AuthError::Denied),
        code => Err(AuthError::Http {
            status,
            code: code.map(Into::into),
        }),
    }
}

/// How a failed refresh is handled (spec 0002, "When refresh fails").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshFailure {
    /// The refresh token is rejected: a new login is needed.
    SessionLost,
    /// May succeed later; the session stays usable.
    Transient,
}

/// Classifies a non-`2xx` refresh response (AC7). Network errors are
/// [`RefreshFailure::Transient`] too, without reaching this function.
pub fn classify_refresh_failure(status: u16, body: &str) -> RefreshFailure {
    match (status, error_code(body)) {
        (400 | 401, Some("invalid_grant" | "invalid_client")) => RefreshFailure::SessionLost,
        (401, _) if body.trim().is_empty() => RefreshFailure::SessionLost,
        // `5xx`, `429` and anything the spec's table does not list: keep the
        // session, the next request tries again.
        _ => RefreshFailure::Transient,
    }
}

/// Whether `session`'s access token expires within [`REFRESH_MARGIN`] of
/// `now`, or already has (AC4).
pub fn needs_refresh(session: &Session, now: SystemTime) -> bool {
    match session.expires_at.duration_since(now) {
        Ok(left) => left < REFRESH_MARGIN,
        Err(_) => true,
    }
}

#[cfg(test)]
mod tests {
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
            "token_pending" => include_str!("../tests/fixtures/auth/token_pending.json"),
            "token_slow_down" => include_str!("../tests/fixtures/auth/token_slow_down.json"),
            "token_expired" => include_str!("../tests/fixtures/auth/token_expired.json"),
            "token_denied" => include_str!("../tests/fixtures/auth/token_denied.json"),
            "token_granted" => include_str!("../tests/fixtures/auth/token_granted.json"),
            "token_granted_no_user" => {
                include_str!("../tests/fixtures/auth/token_granted_no_user.json")
            }
            "token_granted_empty_country" => {
                include_str!("../tests/fixtures/auth/token_granted_empty_country.json")
            }
            "token_granted_no_user_id" => {
                include_str!("../tests/fixtures/auth/token_granted_no_user_id.json")
            }
            "token_refreshed" => include_str!("../tests/fixtures/auth/token_refreshed.json"),
            "token_refreshed_rotated" => {
                include_str!("../tests/fixtures/auth/token_refreshed_rotated.json")
            }
            "token_refreshed_country_changed" => {
                include_str!("../tests/fixtures/auth/token_refreshed_country_changed.json")
            }
            "token_refreshed_no_user" => {
                include_str!("../tests/fixtures/auth/token_refreshed_no_user.json")
            }
            "refresh_invalid_grant" => {
                include_str!("../tests/fixtures/auth/refresh_invalid_grant.json")
            }
            "refresh_invalid_client" => {
                include_str!("../tests/fixtures/auth/refresh_invalid_client.json")
            }
            "echoing_error" => include_str!("../tests/fixtures/auth/echoing_error.json"),
            "echoing_malformed_grant" => {
                include_str!("../tests/fixtures/auth/echoing_malformed_grant.json")
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
}
