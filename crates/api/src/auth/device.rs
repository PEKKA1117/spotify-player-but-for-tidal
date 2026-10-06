//! The OAuth2 device flow (spec 0002, AC1–AC3).

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;

use super::{
    AuthConfig, AuthError, Clock, DEVICE_CODE_GRANT, PollOutcome, SCOPE, Session, SessionStore,
    Sleeper, SystemClock, classify_poll, form_body, session_from_login,
};

/// Tidal's answer to `POST /device_authorization`: what the user must open
/// and type, and how to poll. Its `Debug` redacts `device_code`, which can be
/// exchanged for tokens until it expires.
#[derive(Clone, PartialEq, Eq)]
pub struct DeviceCode {
    pub device_code: String,
    /// The code the user types on the Tidal page (5 characters).
    pub user_code: String,
    /// Without a scheme, e.g. `link.tidal.com`.
    pub verification_uri: String,
    /// The URL with the code, e.g. `link.tidal.com/ABCDE`, when sent.
    pub verification_uri_complete: Option<String>,
    /// How long the code is valid (300 s on the live API).
    pub expires_in: Duration,
    /// Minimum wait between polls.
    pub interval: Duration,
}

impl fmt::Debug for DeviceCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DeviceCode")
            .field("device_code", &"<redacted>")
            .field("user_code", &self.user_code)
            .field("verification_uri", &self.verification_uri)
            .field("verification_uri_complete", &self.verification_uri_complete)
            .field("expires_in", &self.expires_in)
            .field("interval", &self.interval)
            .finish()
    }
}

/// `device_authorization`'s camelCase body.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawDeviceCode {
    device_code: String,
    user_code: String,
    verification_uri: String,
    #[serde(default)]
    verification_uri_complete: Option<String>,
    expires_in: u64,
    interval: u64,
}

impl DeviceCode {
    /// Parses Tidal's camelCase `device_authorization` body.
    pub fn from_json(body: &str) -> Result<Self, AuthError> {
        let raw: RawDeviceCode =
            serde_json::from_str(body).map_err(|_| AuthError::Decode("device authorization"))?;
        Ok(Self {
            device_code: raw.device_code,
            user_code: raw.user_code,
            verification_uri: raw.verification_uri,
            verification_uri_complete: raw.verification_uri_complete,
            expires_in: Duration::from_secs(raw.expires_in),
            interval: Duration::from_secs(raw.interval),
        })
    }

    /// The link printed for the user: `https://` + `verificationUri`.
    pub fn login_link(&self) -> String {
        String::new() // STUB (red)
    }

    /// The link opened in a browser: `https://` + `verificationUriComplete`
    /// when sent, else [`login_link`](Self::login_link).
    pub fn browser_link(&self) -> String {
        String::new() // STUB (red)
    }
}

/// Runs the device flow: [`start_device_flow`](Self::start_device_flow),
/// show the code, then [`complete_login`](Self::complete_login).
pub struct DeviceFlow {
    http: reqwest::Client,
    config: AuthConfig,
    clock: Arc<dyn Clock>,
    sleeper: Arc<dyn Sleeper>,
}

impl fmt::Debug for DeviceFlow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DeviceFlow")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl DeviceFlow {
    /// A flow on the real clock.
    pub fn new(config: AuthConfig) -> Result<Self, AuthError> {
        Self::with_time(config, Arc::new(SystemClock), Arc::new(SystemClock))
    }

    /// A flow with an injected clock and sleep (tests).
    pub fn with_time(
        config: AuthConfig,
        clock: Arc<dyn Clock>,
        sleeper: Arc<dyn Sleeper>,
    ) -> Result<Self, AuthError> {
        let http = reqwest::Client::builder()
            .build()
            .map_err(|e| AuthError::transport(&e))?;
        Ok(Self {
            http,
            config,
            clock,
            sleeper,
        })
    }

    /// `POST {auth_base}/device_authorization` (AC1).
    pub async fn start_device_flow(&self) -> Result<DeviceCode, AuthError> {
        // STUB (red): sends nothing.
        Err(AuthError::Transport("not implemented".into()))
    }

    /// One `POST {auth_base}/token` for `code` (AC2). A transport error is
    /// "not yet" (the live API dropped one poll, see the spec's edge cases).
    pub async fn poll(
        &self,
        code: &DeviceCode,
        interval: Duration,
    ) -> Result<PollOutcome, AuthError> {
        let response = self
            .http
            .post(self.config.auth_url("token"))
            .basic_auth(&self.config.client_id, Some(&self.config.client_secret))
            .header("content-type", "application/x-www-form-urlencoded")
            .body(form_body(&[
                ("client_id", &self.config.client_id),
                ("grant_type", DEVICE_CODE_GRANT),
                ("device_code", &code.device_code),
                ("scope", SCOPE),
            ]))
            .send()
            .await;
        let response = match response {
            Ok(response) => response,
            Err(error) => {
                tracing::debug!(%error, "device-code poll got no response; polling on");
                return Ok(PollOutcome::Pending { interval });
            }
        };
        let status = response.status().as_u16();
        let body = match response.text().await {
            Ok(body) => body,
            Err(_) => return Ok(PollOutcome::Pending { interval }),
        };
        classify_poll(status, &body, interval)
    }

    /// Polls until the user approves, the code expires or is denied (AC2),
    /// and builds the session from the token response (AC3).
    pub async fn wait_for_session(&self, code: &DeviceCode) -> Result<Session, AuthError> {
        // STUB (red): one request, then an empty session with a default country.
        let _ = self.poll(code, code.interval).await;
        let _ = (&self.sleeper, session_from_login);
        Ok(Session {
            access_token: String::new(),
            refresh_token: String::new(),
            expires_at: self.clock.now(),
            user_id: 0,
            country_code: "US".into(),
        })
    }

    /// [`wait_for_session`](Self::wait_for_session), then saves the session
    /// to `store`. Nothing is stored when it fails.
    pub async fn complete_login(
        &self,
        code: &DeviceCode,
        store: &dyn SessionStore,
    ) -> Result<Session, AuthError> {
        let session = self.wait_for_session(code).await?;
        store.save(&session)?;
        Ok(session)
    }
}
