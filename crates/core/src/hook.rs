//! The player event hook (spec 0025), its pure part: which events an input
//! caused, what the command gets for each, and the line of waiting events.
//! The process side is the binary's `hook` module.

use std::collections::VecDeque;
use std::time::Duration;

use crate::Track;
use crate::protocol::PlayerSnapshot;

#[cfg(test)]
mod tests;

/// The most events waiting for the command.
pub const MAX_WAITING: usize = 64;

/// One event, with what the command is told about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookEvent {
    Changed(Track),
    Playing(Track, Duration),
    Paused(Track, Duration),
    Stopped(Track, Duration),
    EndOfTrack(Track),
    Seeked(Track, Duration),
    VolumeChanged(u8),
    DeviceChanged(String),
}

/// What one input did.
#[derive(Debug, Clone, Copy)]
pub struct Transition<'a> {
    pub before: &'a PlayerSnapshot,
    pub after: &'a PlayerSnapshot,
    /// A track started (its first frame, or gaplessly).
    pub started: bool,
    /// A track played to its end.
    pub ended: bool,
    /// The input can move the position by hand.
    pub seek: bool,
}

/// Turns inputs into events, remembering the track that last started.
#[derive(Debug, Clone, Default)]
pub struct HookTracker {}

impl HookTracker {
    /// The events of one input, in the order they run.
    pub fn update(&mut self, _input: Transition<'_>) -> Vec<HookEvent> {
        Vec::new()
    }

    /// The event of shutting down in `snapshot`'s state.
    pub fn shutdown(&mut self, _snapshot: &PlayerSnapshot) -> Option<HookEvent> {
        None
    }
}

impl HookEvent {
    /// What follows the configured `args`: the event's name and arguments.
    pub fn arguments(&self) -> Vec<String> {
        Vec::new()
    }

    /// The variables added to the command's environment.
    pub fn environment(&self) -> Vec<(&'static str, String)> {
        Vec::new()
    }
}

/// The events waiting for the command, oldest first.
#[derive(Debug, Clone, Default)]
pub struct HookQueue {
    waiting: VecDeque<HookEvent>,
}

impl HookQueue {
    /// Adds `event`; `true` when the oldest had to go.
    pub fn push(&mut self, _event: HookEvent) -> bool {
        false
    }

    /// The next event to run.
    pub fn pop(&mut self) -> Option<HookEvent> {
        self.waiting.pop_front()
    }

    /// Forgets what is waiting.
    pub fn clear(&mut self) {
        self.waiting.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.waiting.is_empty()
    }
}
