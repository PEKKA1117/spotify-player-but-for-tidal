//! The remembered playback state (spec 0009 "What is remembered"): what
//! [`PlayerState::saved`] captures and [`PlayerState::restore`] starts from.
//! Pure: the runtime reads and writes the file.

use std::collections::HashSet;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::{PlayerConfig, PlayerState, Queue};
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
    /// The remembered part of the state (spec 0009 AC1): the queue with its
    /// IDs and both orders, the current entry, the position (the start
    /// position while loading), the modes, volume and mute.
    pub fn saved(&self) -> SavedPlayback {
        SavedPlayback {
            version: SAVED_PLAYBACK_VERSION,
            entries: self.queue.entries().to_vec(),
            play_order: self.queue.order().to_vec(),
            current: self.queue.current,
            position_ms: u64::try_from(self.position.as_millis()).unwrap_or(u64::MAX),
            shuffle: self.shuffle,
            repeat: self.repeat,
            autoplay: self.autoplay,
            volume: self.volume,
            muted: self.muted,
        }
    }

    /// A stopped player on the remembered state (spec 0009 AC2, "Starting
    /// from the remembered state"); `config.autoplay` is ignored for the
    /// saved `autoplay`. Repairs, silently: a repeated entry ID keeps its
    /// first entry; a play order that is not a permutation of the entries
    /// becomes the original order with shuffle off; a current ID not in the
    /// queue becomes none; no current entry, or a position at or past the
    /// current entry's known duration, becomes `0:00`; volume is clamped to
    /// 100. New entries get IDs above the highest restored one.
    pub fn restore(config: PlayerConfig, seed: u64, saved: SavedPlayback) -> Self {
        let mut seen = HashSet::new();
        let entries: Vec<QueueEntry> = saved
            .entries
            .into_iter()
            .filter(|e| seen.insert(e.id))
            .collect();

        let mut shuffle = saved.shuffle;
        let permutation = saved.play_order.len() == seen.len()
            && saved.play_order.iter().collect::<HashSet<_>>().len() == seen.len()
            && saved.play_order.iter().all(|id| seen.contains(id));
        let order = if permutation {
            saved.play_order
        } else {
            shuffle = false;
            entries.iter().map(|e| e.id).collect()
        };

        let current = saved
            .current
            .and_then(|id| entries.iter().find(|e| e.id == id));
        let position = Duration::from_millis(saved.position_ms);
        let position = match current.map(|e| e.track.duration) {
            Some(Some(duration)) if position >= duration => Duration::ZERO,
            Some(_) => position,
            None => Duration::ZERO,
        };
        let current = current.map(|e| e.id);
        let next_entry_id = entries
            .iter()
            .map(|e| e.id.0)
            .max()
            .map_or(1, |max| max.saturating_add(1));

        let mut state = Self::new(config, seed);
        state.queue = Queue::from_parts(entries, order, current);
        state.next_entry_id = next_entry_id;
        state.position = position;
        state.shuffle = shuffle;
        state.repeat = saved.repeat;
        state.autoplay = saved.autoplay;
        state.volume = saved.volume.min(100);
        state.muted = saved.muted;
        state
    }
}
