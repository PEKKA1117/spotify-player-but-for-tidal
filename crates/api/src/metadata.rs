//! Track metadata and suggestions (spec 0004 "Filling the queue",
//! "Autoplay"): single tracks, album and playlist contents, track radio.

use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use serde::de::DeserializeOwned;

use tidal_player_core::{AlbumRef, ArtistRef, Item, Track, TrackId};

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

/// How long one metadata request may take before it fails with
/// [`AuthError::Transport`].
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// Tidal's `subStatus` for an unknown track, album, playlist or radio seed
/// (on a `404`).
const SUB_STATUS_NOT_FOUND: u64 = 2001;

/// The largest page the playlist endpoint accepts (`limit=1000` is a `400`).
const PAGE_SIZE: u64 = 100;

/// Fetches metadata through the [`Authenticator`].
#[derive(Debug, Clone)]
pub struct MetadataClient {
    auth: Arc<Authenticator>,
}

impl MetadataClient {
    pub fn new(auth: Arc<Authenticator>) -> Self {
        Self { auth }
    }

    /// `GET /tracks/{id}` (AC17).
    pub async fn get_track(&self, id: TrackId) -> Result<Track, MetadataError> {
        let path = format!("tracks/{id}");
        let dto: TrackDto = self
            .fetch(&path, &[], "track")
            .await?
            .ok_or(MetadataError::NotFound(Item::Track(id)))?;
        Ok(dto.into())
    }

    /// Every track of an album, in album order (AC17).
    pub async fn get_album_tracks(&self, album: u64) -> Result<Vec<Track>, MetadataError> {
        let path = format!("albums/{album}/tracks");
        let pages: Vec<TrackDto> = self
            .walk(&path, "album", || {
                MetadataError::NotFound(Item::Album(album))
            })
            .await?;
        Ok(pages.into_iter().map(Track::from).collect())
    }

    /// Every track of a playlist, in playlist order. Items that are not
    /// tracks (videos) are left out (AC17).
    pub async fn get_playlist_tracks(&self, uuid: &str) -> Result<Vec<Track>, MetadataError> {
        let path = format!("playlists/{uuid}/items");
        let entries: Vec<PlaylistEntry> = self
            .walk(&path, "playlist", || {
                MetadataError::NotFound(Item::Playlist(uuid.to_owned()))
            })
            .await?;
        let mut tracks = Vec::new();
        for entry in entries.into_iter().filter(|e| e.kind == "track") {
            let dto = serde_json::from_value::<TrackDto>(entry.item)
                .map_err(|_| MetadataError::Malformed("playlist"))?;
            tracks.push(dto.into());
        }
        Ok(tracks)
    }

    /// Tidal's suggestions for `seed`, in order, the seed included (the
    /// radio starts with it): one request. An unknown seed has none (AC27).
    pub async fn get_suggestions(&self, seed: TrackId) -> Result<Vec<Track>, MetadataError> {
        let path = format!("tracks/{seed}/radio");
        let limit = PAGE_SIZE.to_string();
        let page: Option<Page<TrackDto>> = self
            .fetch(&path, &[("limit", limit.as_str())], "suggestions")
            .await?;
        Ok(page
            .map(|p| p.items.into_iter().map(Track::from).collect())
            .unwrap_or_default())
    }

    /// Every item of a paged endpoint: pages of [`PAGE_SIZE`], `offset`
    /// advanced by the number of items received, until `offset` reaches the
    /// total or a page comes back empty.
    async fn walk<T: DeserializeOwned>(
        &self,
        path: &str,
        what: &'static str,
        not_found: impl Fn() -> MetadataError,
    ) -> Result<Vec<T>, MetadataError> {
        let limit = PAGE_SIZE.to_string();
        let mut all = Vec::new();
        let mut offset: u64 = 0;
        loop {
            let offset_text = offset.to_string();
            let query = [("limit", limit.as_str()), ("offset", offset_text.as_str())];
            let page: Page<T> = self
                .fetch(path, &query, what)
                .await?
                .ok_or_else(&not_found)?;
            let received = page.items.len() as u64;
            all.extend(page.items);
            offset += received;
            if received == 0 || offset >= page.total {
                return Ok(all);
            }
        }
    }

    /// One `GET`, with `countryCode` from the session. `Ok(None)` is the
    /// `404`/`2001` of an unknown ID; any other failure is an error.
    async fn fetch<T: DeserializeOwned>(
        &self,
        path: &str,
        extra: &[(&str, &str)],
        what: &'static str,
    ) -> Result<Option<T>, MetadataError> {
        let (_, country) = self.auth.account().await;
        let mut query = vec![("countryCode", country.as_str())];
        query.extend_from_slice(extra);
        let response = self.auth.get(path, &query, Some(REQUEST_TIMEOUT)).await?;
        let status = response.status();
        if status.is_success() {
            return response
                .json()
                .map(Some)
                .map_err(|_| MetadataError::Malformed(what));
        }
        if status.as_u16() == 404 && response.sub_status() == Some(SUB_STATUS_NOT_FOUND) {
            return Ok(None);
        }
        Err(AuthError::Http {
            status: status.as_u16(),
            code: None,
        }
        .into())
    }
}

#[derive(Deserialize)]
struct Page<T> {
    #[serde(rename = "totalNumberOfItems")]
    total: u64,
    items: Vec<T>,
}

#[derive(Deserialize)]
struct PlaylistEntry {
    #[serde(rename = "type")]
    kind: String,
    item: serde_json::Value,
}

#[derive(Deserialize)]
pub(crate) struct ArtistDto {
    pub(crate) id: u64,
    pub(crate) name: String,
}

#[derive(Deserialize)]
struct AlbumDto {
    id: u64,
    title: String,
}

impl From<ArtistDto> for ArtistRef {
    fn from(dto: ArtistDto) -> Self {
        ArtistRef {
            id: dto.id,
            name: dto.name,
        }
    }
}

/// The keys of a track response this player reads; the rest is ignored.
#[derive(Deserialize)]
pub(crate) struct TrackDto {
    id: u64,
    title: String,
    /// `Instrumental`, `Live`, ...; `null` for most tracks.
    version: Option<String>,
    /// Whole seconds.
    duration: Option<u64>,
    #[serde(default, rename = "allowStreaming")]
    allow_streaming: bool,
    #[serde(default, rename = "streamReady")]
    stream_ready: bool,
    #[serde(default)]
    artists: Vec<ArtistDto>,
    artist: Option<ArtistDto>,
    album: Option<AlbumDto>,
    /// `STEREO`, `DOLBY_ATMOS`: read by the artist's *All tracks* (spec
    /// 0006 AC6).
    #[serde(default, rename = "audioModes")]
    pub(crate) audio_modes: Vec<String>,
}

impl From<TrackDto> for Track {
    fn from(dto: TrackDto) -> Self {
        let artists: Vec<ArtistRef> = if dto.artists.is_empty() {
            dto.artist.into_iter().map(ArtistRef::from).collect()
        } else {
            dto.artists.into_iter().map(ArtistRef::from).collect()
        };
        Track {
            id: TrackId(dto.id),
            title: dto.title,
            version: dto.version,
            artists,
            album: dto.album.map(|a| AlbumRef {
                id: a.id,
                title: a.title,
            }),
            duration: dto.duration.map(Duration::from_secs),
            streamable: dto.allow_streaming && dto.stream_ready,
        }
    }
}
