//! Tidal authentication. See docs/specs/0002-auth.md.
//!
//! This module starts as the interface shared by the spec's slices: the
//! [`Session`] that gets stored, and the [`SessionStore`] seam the binary
//! implements (keyring, encrypted file). The device flow, refresh and the
//! `Authenticator` are added by the auth slice.

use std::sync::Mutex;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

/// A logged-in Tidal session, as stored between runs.
// The derived `Debug` prints the tokens; AC15 replaces it with a redacting one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    pub access_token: String,
    pub refresh_token: String,
    /// Absolute expiry of `access_token`, computed at receipt from `expires_in`.
    pub expires_at: SystemTime,
    pub user_id: u64,
    pub country_code: String,
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn memory_store_round_trip() {
        let session = Session {
            access_token: "FAKE-ACCESS".into(),
            refresh_token: "FAKE-REFRESH".into(),
            expires_at: SystemTime::UNIX_EPOCH + Duration::from_secs(14_400),
            user_id: 1,
            country_code: "FI".into(),
        };
        let store = MemoryStore::default();
        assert_eq!(store.load(), Ok(None));
        store.save(&session).unwrap();
        assert_eq!(store.load(), Ok(Some(session)));
        store.delete().unwrap();
        store.delete().unwrap();
        assert_eq!(store.load(), Ok(None));
    }
}
