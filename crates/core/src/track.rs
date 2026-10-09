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
    /// Tidal's cover image ID (a UUID), `None` when the album has none
    /// (spec 0010 "Cover art"). Missing in files written before it existed.
    #[serde(default)]
    pub cover: Option<String>,
}

/// The URL of a cover image of `size` (`640x640`, ...): the UUID's dashes
/// become slashes (spec 0010 "Cover art").
pub fn cover_url(_cover: &str, _size: &str) -> String {
    String::new()
}

/// Identifies one queue entry for the life of the player, so the same track
/// can be queued twice and still be addressed unambiguously. Never reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct EntryId(pub u64);

#[cfg(test)]
mod tests {
    use super::*;

    /// Spec 0010 AC18: dashes become slashes; the size is used as given.
    #[test]
    fn ac18_cover_url() {
        let uuid = "2e4a5d2d-9a0d-4c3a-a0ba-42b0bd16a6ec";
        let rows = [
            (
                uuid,
                "640x640",
                "https://resources.tidal.com/images/2e4a5d2d/9a0d/4c3a/a0ba/42b0bd16a6ec/640x640.jpg",
            ),
            (
                uuid,
                "320x320",
                "https://resources.tidal.com/images/2e4a5d2d/9a0d/4c3a/a0ba/42b0bd16a6ec/320x320.jpg",
            ),
            (
                "no-dashes-0",
                "80x80",
                "https://resources.tidal.com/images/no/dashes/0/80x80.jpg",
            ),
            (
                "plain",
                "1280x1280",
                "https://resources.tidal.com/images/plain/1280x1280.jpg",
            ),
        ];
        for (cover, size, want) in rows {
            assert_eq!(cover_url(cover, size), want, "{cover} {size}");
        }
    }
}
