//! Tidal HTTP API client. See docs/specs/0001-architecture.md.
//!
//! Base URLs are always injected by the caller. The production URLs are named
//! constants in [`auth`] (spec 0002); authenticated requests go through
//! [`auth::Authenticator`].

pub mod auth;
pub mod library;
pub mod metadata;
pub mod stream;

use std::fmt;

use reqwest::StatusCode;
use reqwest::header::HeaderMap;
use serde::de::DeserializeOwned;

/// Tidal's `subStatus` for "Asset is not ready for playback": a `401` that
/// says the track (or quality) is not available to the account, not that
/// the token is bad (spec 0002 AC18).
pub const SUB_STATUS_NOT_AVAILABLE: u64 = 4005;

/// A response from [`auth::Authenticator::get`] and its siblings: the status,
/// the headers and the whole body, as the server sent them.
#[derive(Clone, PartialEq, Eq)]
pub struct ApiResponse {
    status: StatusCode,
    headers: HeaderMap,
    body: Vec<u8>,
}

impl ApiResponse {
    pub(crate) fn new(status: StatusCode, headers: HeaderMap, body: Vec<u8>) -> Self {
        Self {
            status,
            headers,
            body,
        }
    }

    /// A response header's value (`ETag`, ...), when present and text.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name)?.to_str().ok()
    }

    /// The HTTP status.
    pub fn status(&self) -> StatusCode {
        self.status
    }

    /// The raw body.
    pub fn body(&self) -> &[u8] {
        &self.body
    }

    /// The body decoded as JSON.
    pub fn json<T: DeserializeOwned>(&self) -> Result<T, serde_json::Error> {
        serde_json::from_slice(&self.body)
    }

    /// Tidal's `subStatus` from an error body, when it has one.
    pub fn sub_status(&self) -> Option<u64> {
        #[derive(serde::Deserialize)]
        struct ErrorBody {
            #[serde(rename = "subStatus")]
            sub_status: Option<u64>,
        }
        self.json::<ErrorBody>().ok()?.sub_status
    }
}

/// Shows the status and body length only: a body may echo a token.
impl fmt::Debug for ApiResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ApiResponse")
            .field("status", &self.status)
            .field("body_len", &self.body.len())
            .finish()
    }
}
