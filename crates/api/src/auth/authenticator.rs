//! The per-process [`Authenticator`] (spec 0002, AC4–AC9).

use std::fmt;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use reqwest::{StatusCode, Url};
use serde::de::DeserializeOwned;
use tokio::sync::{Mutex, watch};

use crate::{ApiResponse, SUB_STATUS_NOT_AVAILABLE};

use super::{
    AuthConfig, AuthError, Clock, LOST_RECHECK, RefreshFailure, SCOPE, Session, SessionStore,
    TokenGrant, classify_refresh_failure, error_code, form_body, needs_refresh,
    session_from_refresh,
};

/// Whether the session is usable, for the player to broadcast
/// `Event::LoginRequired` / `Event::LoginRestored` on changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthStatus {
    /// Requests are sent (refreshing as needed).
    Active,
    /// The refresh token was rejected: requests fail with
    /// [`AuthError::LoginRequired`] until a new login is found in storage.
    LoginRequired,
}

/// In-memory state, behind one async mutex so refreshes are single flight.
struct State {
    session: Session,
    /// `Some(last storage re-read)` while the session is lost.
    lost_since_check: Option<SystemTime>,
}

/// Owns the in-memory session and sends authenticated requests: bearer
/// header, proactive and reactive refresh (single flight, persisted to the
/// store), cross-process pickup, and the "session lost" state.
pub struct Authenticator {
    http: reqwest::Client,
    config: AuthConfig,
    store: Arc<dyn SessionStore>,
    clock: Arc<dyn Clock>,
    state: Mutex<State>,
    status: watch::Sender<AuthStatus>,
}

impl fmt::Debug for Authenticator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Authenticator")
            .field("config", &self.config)
            .field("status", &*self.status.borrow())
            .finish_non_exhaustive()
    }
}

impl Authenticator {
    /// An authenticator for `session`, which `store` holds.
    pub fn new(
        config: AuthConfig,
        store: Arc<dyn SessionStore>,
        session: Session,
        clock: Arc<dyn Clock>,
    ) -> Result<Self, AuthError> {
        let http = reqwest::Client::builder()
            .build()
            .map_err(|e| AuthError::transport(&e))?;
        Ok(Self {
            http,
            config,
            store,
            clock,
            state: Mutex::new(State {
                session,
                lost_since_check: None,
            }),
            status: watch::Sender::new(AuthStatus::Active),
        })
    }

    /// An authenticator for the session in `store`, or `None` when not
    /// logged in.
    pub fn from_store(
        config: AuthConfig,
        store: Arc<dyn SessionStore>,
        clock: Arc<dyn Clock>,
    ) -> Result<Option<Self>, AuthError> {
        match store.load()? {
            Some(session) => Self::new(config, store, session, clock).map(Some),
            None => Ok(None),
        }
    }

    /// Follows [`AuthStatus`] changes.
    pub fn status(&self) -> watch::Receiver<AuthStatus> {
        self.status.subscribe()
    }

    /// The account's user ID and country code.
    pub async fn account(&self) -> (u64, String) {
        let state = self.state.lock().await;
        (state.session.user_id, state.session.country_code.clone())
    }

    /// `GET {api_base}{path}` with the bearer token, decoded as JSON.
    ///
    /// As [`Self::get`], then any non-2xx status is an
    /// [`AuthError::Http`].
    pub async fn get_json<T: DeserializeOwned>(&self, path: &str) -> Result<T, AuthError> {
        let response = self.get(path, &[], None).await?;
        if !response.status().is_success() {
            return Err(AuthError::Http {
                status: response.status().as_u16(),
                code: None,
            });
        }
        response.json().map_err(|_| AuthError::Decode("API"))
    }

    /// `GET {api_base}{path}?{query}` with the bearer token; the response
    /// is returned whatever its status, with its body read.
    ///
    /// Refreshes first when the token expires within 60 s, and once more on
    /// a `401` before retrying once (AC5); a second `401` is
    /// [`AuthError::Unauthorized`]. A `401` with `subStatus`
    /// [`SUB_STATUS_NOT_AVAILABLE`] is not about the token: it is returned
    /// as is, with no refresh and no retry (AC18). Fails fast with
    /// [`AuthError::LoginRequired`] while the session is lost (AC7, AC9).
    /// `timeout` bounds each HTTP request (none by default).
    pub async fn get(
        &self,
        path: &str,
        query: &[(&str, &str)],
        timeout: Option<Duration>,
    ) -> Result<ApiResponse, AuthError> {
        let url = self.api_url_with_query(path, query)?;
        let token = self.valid_token().await?;
        let mut response = self.send_get(&url, &token, timeout).await?;
        if is_token_rejection(&response) {
            let token = self.token_after_401(&token).await?;
            response = self.send_get(&url, &token, timeout).await?;
            if is_token_rejection(&response) {
                return Err(AuthError::Unauthorized);
            }
        }
        Ok(response)
    }

    fn api_url_with_query(&self, path: &str, query: &[(&str, &str)]) -> Result<Url, AuthError> {
        let mut url = Url::parse(&self.config.api_url(path))
            .map_err(|e| AuthError::Transport(format!("invalid API URL: {e}")))?;
        if !query.is_empty() {
            url.query_pairs_mut().extend_pairs(query);
        }
        Ok(url)
    }

    async fn send_get(
        &self,
        url: &Url,
        token: &str,
        timeout: Option<Duration>,
    ) -> Result<ApiResponse, AuthError> {
        let mut request = self.http.get(url.clone()).bearer_auth(token);
        if let Some(timeout) = timeout {
            request = request.timeout(timeout);
        }
        let response = request.send().await.map_err(|e| AuthError::transport(&e))?;
        let status = response.status();
        let body = response
            .bytes()
            .await
            .map_err(|e| AuthError::transport(&e))?
            .to_vec();
        Ok(ApiResponse::new(status, body))
    }

    /// The access token to send, refreshed first if it is about to expire.
    async fn valid_token(&self) -> Result<String, AuthError> {
        let mut state = self.state.lock().await;
        self.leave_lost_state(&mut state)?;
        if needs_refresh(&state.session, self.clock.now()) {
            self.refresh(&mut state, Reason::Expiring).await?;
        }
        Ok(state.session.access_token.clone())
    }

    /// The access token to retry with after `rejected` got a `401`.
    async fn token_after_401(&self, rejected: &str) -> Result<String, AuthError> {
        let mut state = self.state.lock().await;
        self.leave_lost_state(&mut state)?;
        // Another request refreshed while this one was in flight.
        if state.session.access_token == rejected {
            self.refresh(&mut state, Reason::Rejected).await?;
        }
        Ok(state.session.access_token.clone())
    }

    /// In the lost state, re-reads storage at most every [`LOST_RECHECK`]
    /// and leaves the state when it holds a different refresh token (AC9).
    fn leave_lost_state(&self, state: &mut State) -> Result<(), AuthError> {
        let Some(last_check) = state.lost_since_check else {
            return Ok(());
        };
        let now = self.clock.now();
        let due = now
            .duration_since(last_check)
            .is_ok_and(|elapsed| elapsed >= LOST_RECHECK);
        if !due {
            return Err(AuthError::LoginRequired);
        }
        state.lost_since_check = Some(now);
        match self.load_stored() {
            Some(stored) if stored.refresh_token != state.session.refresh_token => {
                tracing::info!("found a new login in the session store");
                state.session = stored;
                state.lost_since_check = None;
                self.set_status(AuthStatus::Active);
                Ok(())
            }
            _ => Err(AuthError::LoginRequired),
        }
    }

    /// Refreshes the session in place (holding the state lock, so concurrent
    /// requests share it): adopts a newer stored session instead when there
    /// is one (AC8a), persists the result (AC6), and on "session lost" tries
    /// a different stored refresh token once (AC8b) before giving up (AC7).
    async fn refresh(&self, state: &mut State, reason: Reason) -> Result<(), AuthError> {
        if let Some(stored) = self.load_stored()
            && stored.expires_at > state.session.expires_at
        {
            tracing::info!("adopting a newer session from the session store");
            state.session = stored;
            let usable = match reason {
                Reason::Expiring => !needs_refresh(&state.session, self.clock.now()),
                Reason::Rejected => true,
            };
            if usable {
                return Ok(());
            }
        }
        match self.refresh_once(state).await {
            Err(Failure::Lost) => {}
            other => return other.map_err(Failure::into_error),
        }
        if let Some(stored) = self.load_stored()
            && stored.refresh_token != state.session.refresh_token
        {
            tracing::info!("refresh token rejected; retrying with the stored one");
            state.session = stored;
            match self.refresh_once(state).await {
                Err(Failure::Lost) => {}
                other => return other.map_err(Failure::into_error),
            }
        }
        tracing::warn!("refresh token rejected: run \"tidal-player login\"");
        state.lost_since_check = Some(self.clock.now());
        self.set_status(AuthStatus::LoginRequired);
        Err(AuthError::LoginRequired)
    }

    /// One `grant_type=refresh_token` call; on success the new session is
    /// saved, then installed.
    async fn refresh_once(&self, state: &mut State) -> Result<(), Failure> {
        let response = self
            .http
            .post(self.config.auth_url("token"))
            .basic_auth(&self.config.client_id, Some(&self.config.client_secret))
            .header("content-type", "application/x-www-form-urlencoded")
            .body(form_body(&[
                ("client_id", &self.config.client_id),
                ("grant_type", "refresh_token"),
                ("refresh_token", &state.session.refresh_token),
                ("scope", SCOPE),
            ]))
            .send()
            .await
            .map_err(|e| Failure::Transient(AuthError::transport(&e)))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| Failure::Transient(AuthError::transport(&e)))?;
        if !status.is_success() {
            let status = status.as_u16();
            return match classify_refresh_failure(status, &body) {
                RefreshFailure::SessionLost => Err(Failure::Lost),
                RefreshFailure::Transient => Err(Failure::Transient(AuthError::Http {
                    status,
                    code: error_code(&body).map(Into::into),
                })),
            };
        }
        let grant = TokenGrant::from_json(&body).map_err(Failure::Transient)?;
        let session = session_from_refresh(&state.session, &grant, self.clock.now());
        if let Err(error) = self.store.save(&session) {
            tracing::warn!(%error, "could not save the refreshed session");
        }
        state.session = session;
        Ok(())
    }

    fn load_stored(&self) -> Option<Session> {
        self.store.load().unwrap_or_else(|error| {
            tracing::warn!(%error, "could not read the session store");
            None
        })
    }

    fn set_status(&self, status: AuthStatus) {
        self.status.send_if_modified(|current| {
            let changed = *current != status;
            *current = status;
            changed
        });
    }
}

/// Whether `response` rejects the access token: a `401`, unless its
/// `subStatus` is [`SUB_STATUS_NOT_AVAILABLE`].
fn is_token_rejection(response: &ApiResponse) -> bool {
    response.status() == StatusCode::UNAUTHORIZED
        && response.sub_status() != Some(SUB_STATUS_NOT_AVAILABLE)
}

/// Why a refresh is attempted.
#[derive(Clone, Copy)]
enum Reason {
    /// The token expires within the refresh margin.
    Expiring,
    /// The token got a `401`.
    Rejected,
}

/// A failed refresh call.
enum Failure {
    /// The refresh token is rejected.
    Lost,
    /// Anything else; the session stays usable.
    Transient(AuthError),
}

impl Failure {
    fn into_error(self) -> AuthError {
        match self {
            Self::Lost => AuthError::LoginRequired,
            Self::Transient(error) => error,
        }
    }
}
