//! The per-process [`Authenticator`] (spec 0002, AC4–AC9).

use std::fmt;
use std::sync::Arc;
use std::time::SystemTime;

use serde::de::DeserializeOwned;
use tokio::sync::{Mutex, watch};

use super::{AuthConfig, AuthError, Clock, Session, SessionStore};

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
    pub async fn get_json<T: DeserializeOwned>(&self, path: &str) -> Result<T, AuthError> {
        // STUB (red): no Authorization header, never refreshes.
        let _ = (&self.store, &self.clock);
        let response = self
            .http
            .get(self.config.api_url(path))
            .send()
            .await
            .map_err(|e| AuthError::transport(&e))?;
        let status = response.status();
        if !status.is_success() {
            return Err(AuthError::Http {
                status: status.as_u16(),
                code: None,
            });
        }
        response.json().await.map_err(|_| AuthError::Decode("API"))
    }
}
