//! The library: lists, pages and writes (spec 0006). STUB for the red step.

use std::sync::Arc;

use tidal_player_core::Item;
use tidal_player_core::library::{LibraryRequest, LibraryResponse};

use crate::auth::{AuthError, Authenticator};

/// What was not found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Subject {
    Item(Item),
    Artist(u64),
}

impl std::fmt::Display for Subject {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Item(item) => write!(f, "{item}"),
            Self::Artist(id) => write!(f, "Artist {id}"),
        }
    }
}

/// Why a library request failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LibraryError {
    #[error("{0} was not found")]
    NotFound(Subject),
    #[error("malformed {0} response from Tidal")]
    Malformed(&'static str),
    #[error("The playlist changed: nothing was changed, try again")]
    PlaylistChanged,
    #[error("Not a valid ID: {0}")]
    InvalidId(String),
    #[error("{}", auth_message(.0))]
    Auth(AuthError),
}

fn auth_message(error: &AuthError) -> String {
    error.to_string()
}

impl LibraryError {
    /// The underlying [`AuthError`], for `LoginRequired` and transient checks.
    pub fn auth(&self) -> Option<&AuthError> {
        match self {
            Self::Auth(error) => Some(error),
            _ => None,
        }
    }
}

impl From<AuthError> for LibraryError {
    fn from(error: AuthError) -> Self {
        Self::Auth(error)
    }
}

/// Reads and writes the library through the [`Authenticator`].
#[derive(Debug, Clone)]
pub struct LibraryClient {
    #[allow(dead_code)]
    auth: Arc<Authenticator>,
}

impl LibraryClient {
    pub fn new(auth: Arc<Authenticator>) -> Self {
        Self { auth }
    }

    /// Answers one request (stub: does nothing).
    pub async fn request(
        &self,
        _request: LibraryRequest,
        _page_size: u32,
        _hidden_words: &[String],
    ) -> Result<LibraryResponse, LibraryError> {
        Ok(LibraryResponse::Done)
    }
}
