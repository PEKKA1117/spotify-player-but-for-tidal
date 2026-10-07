//! The queue's two orders and the shuffle PRNG (spec 0004 "The queue",
//! "Shuffle and repeat").

use crate::protocol::QueueEntry;
use crate::track::EntryId;

/// A small seeded PRNG (splitmix64 seeding, xorshift64* output). Core has no
/// randomness source: the runtime gives the seed, tests fix it.
#[derive(Debug, Clone)]
pub(super) struct Rng(u64);

impl Rng {
    pub(super) fn new(seed: u64) -> Self {
        // splitmix64 spreads close seeds apart; xorshift needs a non-zero state.
        let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        Self(if z == 0 { 0x2545_F491_4F6C_DD1D } else { z })
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Fisher–Yates.
    pub(super) fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            // The modulo bias is irrelevant for queue lengths.
            let j = (self.next_u64() % (i as u64 + 1)) as usize;
            items.swap(i, j);
        }
    }
}

/// Entries in their original order, the play order, and the current entry.
#[derive(Debug, Clone, Default)]
pub(super) struct Queue {
    /// Original order (as loaded and edited).
    entries: Vec<QueueEntry>,
    /// Play order: the same IDs, shuffled when shuffle is on.
    order: Vec<EntryId>,
    pub(super) current: Option<EntryId>,
}

impl Queue {
    pub(super) fn len(&self) -> usize {
        self.entries.len()
    }

    pub(super) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub(super) fn get(&self, id: EntryId) -> Option<&QueueEntry> {
        self.entries.iter().find(|e| e.id == id)
    }

    pub(super) fn contains_track(&self, track: crate::track::TrackId) -> bool {
        self.entries.iter().any(|e| e.track.id == track)
    }

    /// The entries in play order.
    pub(super) fn play_order(&self) -> Vec<QueueEntry> {
        self.order
            .iter()
            .filter_map(|id| self.get(*id).cloned())
            .collect()
    }

    fn play_index(&self, id: EntryId) -> Option<usize> {
        self.order.iter().position(|e| *e == id)
    }

    /// The entry after `id` in play order; with `wrap`, the last is followed
    /// by the first (a queue of one by itself).
    pub(super) fn after(&self, id: EntryId, wrap: bool) -> Option<EntryId> {
        let i = self.play_index(id)?;
        match self.order.get(i + 1) {
            Some(next) => Some(*next),
            None if wrap => self.order.first().copied(),
            None => None,
        }
    }

    /// The entry before `id` in play order; with `wrap`, the first is preceded
    /// by the last.
    pub(super) fn before(&self, id: EntryId, wrap: bool) -> Option<EntryId> {
        let i = self.play_index(id)?;
        match i.checked_sub(1) {
            Some(prev) => Some(self.order[prev]),
            None if wrap => self.order.last().copied(),
            None => None,
        }
    }

    /// Replaces everything; the play order is `current` first and the rest
    /// shuffled when `rng` is given, else the original order.
    pub(super) fn replace(
        &mut self,
        entries: Vec<QueueEntry>,
        current: Option<EntryId>,
        rng: Option<&mut Rng>,
    ) {
        self.entries = entries;
        self.current = current;
        match rng {
            Some(rng) => self.shuffle(rng),
            None => self.unshuffle(),
        }
    }

    /// Play order: the current entry first, the others shuffled.
    pub(super) fn shuffle(&mut self, rng: &mut Rng) {
        let mut rest: Vec<EntryId> = self
            .entries
            .iter()
            .map(|e| e.id)
            .filter(|id| Some(*id) != self.current)
            .collect();
        rng.shuffle(&mut rest);
        self.order = self.current.into_iter().chain(rest).collect();
    }

    /// Play order: the original order.
    pub(super) fn unshuffle(&mut self) {
        self.order = self.entries.iter().map(|e| e.id).collect();
    }

    /// Inserts right after the current entry in both orders (at the end when
    /// there is no current entry).
    pub(super) fn insert_after_current(&mut self, new: Vec<QueueEntry>) {
        let Some(current) = self.current else {
            return self.append(new);
        };
        let ids: Vec<EntryId> = new.iter().map(|e| e.id).collect();
        let at = self
            .entries
            .iter()
            .position(|e| e.id == current)
            .map_or(self.entries.len(), |i| i + 1);
        self.entries.splice(at..at, new);
        let at = self.play_index(current).map_or(self.order.len(), |i| i + 1);
        self.order.splice(at..at, ids);
    }

    /// Appends to both orders.
    pub(super) fn append(&mut self, new: Vec<QueueEntry>) {
        self.order.extend(new.iter().map(|e| e.id));
        self.entries.extend(new);
    }

    /// Removes from both orders (the current entry is left to the caller).
    pub(super) fn remove(&mut self, id: EntryId) {
        self.entries.retain(|e| e.id != id);
        self.order.retain(|e| *e != id);
    }

    /// Keeps only the current entry (nothing when there is none).
    pub(super) fn retain_current(&mut self) {
        let current = self.current;
        self.entries.retain(|e| Some(e.id) == current);
        self.order.retain(|e| Some(*e) == current);
    }
}
