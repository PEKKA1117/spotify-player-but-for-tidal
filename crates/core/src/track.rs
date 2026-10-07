//! Tracks and queue entries (spec 0004 "The queue", "Filling the queue").

use std::fmt;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// A Tidal track ID (the number in `https://tidal.com/browse/track/<id>`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TrackId(pub u64);

impl fmt::Display for TrackId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// A track as the queue knows it: metadata from the API, never a stream URL.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Track {
    pub id: TrackId,
    pub title: String,
    /// Tidal's `version` (`Instrumental`, `Live`, ...), when it has one;
    /// not part of `title` (spec 0006 "Alternate versions").
    pub version: Option<String>,
    /// The artists, in the API's order (spec 0006: IDs for *Go to artist*).
    pub artists: Vec<ArtistRef>,
    /// The album, when the track has one (spec 0006: its ID for *Go to
    /// album*).
    pub album: Option<AlbumRef>,
    /// `None` when the metadata has no duration.
    pub duration: Option<Duration>,
    /// `allowStreaming && streamReady`: a track that is not streamable is
    /// queued and shown, but skipped without a stream request.
    pub streamable: bool,
}

impl Track {
    /// The artist names joined with `, `, as every view shows them.
    pub fn artist_names(&self) -> String {
        self.artists
            .iter()
            .map(|a| a.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// The album title, when the track has an album.
    pub fn album_title(&self) -> Option<&str> {
        self.album.as_ref().map(|a| a.title.as_str())
    }
}

/// An artist as a track or album names it: enough to show it and open its
/// page (spec 0006 "Types").
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ArtistRef {
    /// Tidal's artist ID.
    pub id: u64,
    pub name: String,
}

/// An album as a track names it (spec 0006 "Types").
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AlbumRef {
    /// Tidal's album ID, as in [`crate::Item::Album`].
    pub id: u64,
    pub title: String,
}

/// Identifies one queue entry for the life of the player, so the same track
/// can be queued twice and still be addressed unambiguously. Never reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct EntryId(pub u64);
