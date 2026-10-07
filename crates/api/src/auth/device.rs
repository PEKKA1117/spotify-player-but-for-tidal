//! The OAuth2 device flow (spec 0002, AC1–AC3).

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;

use super::{
    AuthConfig, AuthError, Clock, DEVICE_CODE_GRANT, LOSSY_WARNING, PollOutcome, RefreshClient,
    SCOPE, Session, SessionStore, Sleeper, SystemClock, TokenGrant, classify_poll, error_code,
    form_body, post_refresh, session_from_login, session_from_refresh,
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
        https(&self.verification_uri)
    }

    /// The link opened in a browser: `https://` + `verificationUriComplete`
    /// when sent, else [`login_link`](Self::login_link).
    pub fn browser_link(&self) -> String {
        match &self.verification_uri_complete {
            Some(complete) => https(complete),
            None => self.login_link(),
        }
    }
}

/// Prepends `https://` unless `uri` already has a scheme.
fn https(uri: &str) -> String {
    if uri.starts_with("https://") || uri.starts_with("http://") {
        uri.to_owned()
    } else {
        format!("https://{uri}")
    }
}

/// What [`DeviceFlow::complete_login`] stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Login {
    pub session: Session,
    /// The client `session`'s access token belongs to:
    /// [`RefreshClient::DeviceFlow`] when the post-login refresh under the
    /// PKCE client failed and the grant was stored as is (AC19).
    pub client: RefreshClient,
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
        let response = self
            .http
            .post(self.config.auth_url("device_authorization"))
            .header("content-type", "application/x-www-form-urlencoded")
            .body(form_body(&[
                ("client_id", &self.config.client_id),
                ("scope", SCOPE),
            ]))
            .send()
            .await
            .map_err(|e| AuthError::transport(&e))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| AuthError::transport(&e))?;
        if !status.is_success() {
            return Err(AuthError::Http {
                status: status.as_u16(),
                code: error_code(&body).map(Into::into),
            });
        }
        DeviceCode::from_json(&body)
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
        let deadline = self.clock.now() + code.expires_in;
        let mut interval = code.interval;
        loop {
            match self.poll(code, interval).await? {
                PollOutcome::Granted(grant) => {
                    return session_from_login(&grant, self.clock.now());
                }
                PollOutcome::Pending { interval: next } => interval = next,
            }
            if self.clock.now() >= deadline {
                return Err(AuthError::CodeExpired);
            }
            self.sleeper.sleep(interval).await;
        }
    }

    /// [`wait_for_session`](Self::wait_for_session), one refresh under the
    /// PKCE client (AC19), then saves the session to `store`. When that
    /// refresh fails for any reason, the grant's session is saved as is.
    /// Nothing is stored when the login fails.
    pub async fn complete_login(
        &self,
        code: &DeviceCode,
        store: &dyn SessionStore,
    ) -> Result<Login, AuthError> {
        let granted = self.wait_for_session(code).await?;
        let login = match self.refresh_under_pkce(&granted).await {
            Ok(session) => Login {
                session,
                client: RefreshClient::Pkce,
            },
            Err(error) => {
                tracing::warn!(%error, "{LOSSY_WARNING}");
                Login {
                    session: granted,
                    client: RefreshClient::DeviceFlow,
                }
            }
        };
        store.save(&login.session)?;
        Ok(login)
    }

    /// `granted` refreshed under the PKCE client, without the fallback: the
    /// grant already is the device-flow client's.
    async fn refresh_under_pkce(&self, granted: &Session) -> Result<Session, AuthError> {
        let (status, body) = post_refresh(
            &self.http,
            &self.config,
            RefreshClient::Pkce,
            &granted.refresh_token,
        )
        .await?;
        if !(200..300).contains(&status) {
            return Err(AuthError::Http {
                status,
                code: error_code(&body).map(Into::into),
            });
        }
        let grant = TokenGrant::from_json(&body)?;
        Ok(session_from_refresh(granted, &grant, self.clock.now()))
    }
}
