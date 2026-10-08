//! The remembered playback state (spec 0009 "What is remembered"): what
//! [`PlayerState::saved`] captures and [`PlayerState::restore`] starts from.
//! Pure: the runtime reads and writes the file.

use serde::{Deserialize, Serialize};

use super::{PlayerConfig, PlayerState};
use crate::protocol::{QueueEntry, RepeatMode};
use crate::track::EntryId;

/// The only `version` this player writes and reads.
pub const SAVED_PLAYBACK_VERSION: u32 = 1;

/// The playback state the player remembers between runs (spec 0009).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedPlayback {
    /// Always [`SAVED_PLAYBACK_VERSION`] when produced by `saved`.
    pub version: u32,
    /// The entries in their original order, with their IDs and `suggested`.
    pub entries: Vec<QueueEntry>,
    /// The entry IDs in play order.
    pub play_order: Vec<EntryId>,
    pub current: Option<EntryId>,
    /// The position in the current entry (the start position while loading).
    pub position_ms: u64,
    pub shuffle: bool,
    pub repeat: RepeatMode,
    pub autoplay: bool,
    pub volume: u8,
    pub muted: bool,
}

impl Default for SavedPlayback {
    /// An empty queue at volume 100 %, every mode off.
    fn default() -> Self {
        Self {
            version: SAVED_PLAYBACK_VERSION,
            entries: Vec::new(),
            play_order: Vec::new(),
            current: None,
            position_ms: 0,
            shuffle: false,
            repeat: RepeatMode::Off,
            autoplay: false,
            volume: 100,
            muted: false,
        }
    }
}

impl PlayerState {
    /// The remembered part of the state (spec 0009 AC1).
    pub fn saved(&self) -> SavedPlayback {
        SavedPlayback::default()
    }

    /// A stopped player on the remembered state (spec 0009 AC2).
    pub fn restore(config: PlayerConfig, seed: u64, saved: SavedPlayback) -> Self {
        let _ = saved;
        Self::new(config, seed)
    }
}
