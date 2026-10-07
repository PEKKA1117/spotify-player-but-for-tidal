//! Track metadata and suggestions (spec 0004 "Filling the queue",
//! "Autoplay"): single tracks, album and playlist contents, track radio.

use std::sync::Arc;

use tidal_player_core::{Item, Track, TrackId};

use crate::auth::{AuthError, Authenticator};

/// Why metadata could not be fetched.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MetadataError {
    /// `404` with `subStatus` `2001`: no such track, album or playlist.
    #[error("{0} was not found")]
    NotFound(Item),
    /// A `2xx` body that could not be read; never quotes the body.
    #[error("malformed {0} response from Tidal")]
    Malformed(&'static str),
    /// From the [`Authenticator`], unchanged: `LoginRequired`, transport
    /// errors, unexpected statuses (`5xx`, `429`, ...).
    #[error(transparent)]
    Auth(#[from] AuthError),
}

/// Fetches metadata through the [`Authenticator`].
#[derive(Debug, Clone)]
pub struct MetadataClient {
    #[allow(dead_code)]
    auth: Arc<Authenticator>,
}

impl MetadataClient {
    pub fn new(auth: Arc<Authenticator>) -> Self {
        Self { auth }
    }

    /// `GET /tracks/{id}` (AC17).
    pub async fn get_track(&self, id: TrackId) -> Result<Track, MetadataError> {
        Err(MetadataError::NotFound(Item::Track(id)))
    }

    /// Every track of an album, in album order (AC17).
    pub async fn get_album_tracks(&self, album: u64) -> Result<Vec<Track>, MetadataError> {
        Err(MetadataError::NotFound(Item::Album(album)))
    }

    /// Every track of a playlist, in playlist order, videos left out (AC17).
    pub async fn get_playlist_tracks(&self, uuid: &str) -> Result<Vec<Track>, MetadataError> {
        Err(MetadataError::NotFound(Item::Playlist(uuid.to_owned())))
    }

    /// Tidal's suggestions for `seed`, the seed included (AC27).
    pub async fn get_suggestions(&self, _seed: TrackId) -> Result<Vec<Track>, MetadataError> {
        Ok(Vec::new())
    }
}
