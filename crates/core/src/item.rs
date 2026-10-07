//! Items the queue is filled from: a track ID or a Tidal link (spec 0004
//! "Filling the queue").

use serde::{Deserialize, Serialize};

use crate::track::TrackId;

/// What a command-line argument or a pasted link names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Item {
    Track(TrackId),
    Album(u64),
    /// A playlist UUID, as it appears in the link.
    Playlist(String),
}

/// Not a Tidal track, album or playlist.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("Not a Tidal track, album or playlist: {0}")]
pub struct ItemError(pub String);
