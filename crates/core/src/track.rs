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
    /// Artist names, in the API's order.
    pub artists: Vec<String>,
    /// The album title, when the track has one.
    pub album: Option<String>,
    /// `None` when the metadata has no duration.
    pub duration: Option<Duration>,
    /// `allowStreaming && streamReady`: a track that is not streamable is
    /// queued and shown, but skipped without a stream request.
    pub streamable: bool,
}

/// Identifies one queue entry for the life of the player, so the same track
/// can be queued twice and still be addressed unambiguously. Never reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct EntryId(pub u64);
