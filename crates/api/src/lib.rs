//! Tidal HTTP API client. See docs/specs/0001-architecture.md.
//!
//! Base URLs are always injected by the caller. The production URLs are named
//! constants in [`auth`] (spec 0002); authenticated requests go through
//! [`auth::Authenticator`].

pub mod auth;

use serde::de::DeserializeOwned;

/// Errors returned by [`Client`].
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    /// The request could not be sent or the response could not be read or decoded.
    #[error("transport error: {0}")]
    Transport(#[from] reqwest::Error),
    /// The server answered with a non-2xx status.
    #[error("unexpected HTTP status {0}")]
    Status(u16),
}

/// HTTP client bound to an injectable base URL.
#[derive(Debug, Clone)]
pub struct Client {
    base_url: String,
    http: reqwest::Client,
}

impl Client {
    /// Creates a client that resolves request paths against `base_url`.
    pub fn new(base_url: impl Into<String>) -> Result<Self, ApiError> {
        Ok(Self {
            base_url: base_url.into(),
            http: reqwest::Client::builder().build()?,
        })
    }

    /// Joins the base URL and `path` with exactly one slash between them.
    fn url(&self, path: &str) -> String {
        format!(
            "{}/{}",
            self.base_url.trim_end_matches('/'),
            path.trim_start_matches('/')
        )
    }

    /// Sends `GET {base_url}{path}` and decodes the JSON body as `T`.
    pub async fn get_json<T: DeserializeOwned>(&self, path: &str) -> Result<T, ApiError> {
        let response = self.http.get(self.url(path)).send().await?;
        let status = response.status();
        if !status.is_success() {
            return Err(ApiError::Status(status.as_u16()));
        }
        Ok(response.json().await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_has_exactly_one_slash() {
        for base in ["http://h:1", "http://h:1/"] {
            for path in ["/v1/echo", "v1/echo"] {
                let c = Client::new(base).unwrap();
                assert_eq!(c.url(path), "http://h:1/v1/echo", "{base} + {path}");
            }
        }
    }
}
