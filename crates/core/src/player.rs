//! The player state machine (spec 0004 "Player and protocol"): a pure
//! `update(&mut PlayerState, PlayerInput) -> Vec<PlayerEffect>`.
//!
//! The runtime feeds it client commands, engine events, stream resolutions and
//! autoplay suggestions (each mapped into the core types below, with the tag
//! of the track they are about) and executes the effects it returns, in order.
//! Core never sees a URL: the runtime keeps the resolved stream for a tag and
//! hands it to the engine on `EnginePlay`/`EnginePreload` with that tag.
//!
//! Behaviour not fixed by the spec, chosen here:
//!
//! - Starting a track while the engine has one (playing, paused or opening)
//!   first sends `EngineStop`, so nothing plays while the next one loads
//! - An engine `Position` is accepted only while `Playing`/`Paused`; one that
//!   arrives while loading belongs to the replaced track and is dropped
//! - `TogglePause` on a stopped player plays the current entry from the
//!   position it stopped at: `0:00` after a stop at the end, the position
//!   reached after a transient/output/session failure (the retry)
//! - The consecutive-failure count is reset by a successful start and by any
//!   command that starts a track (load, next, previous, play entry, play)
//! - A pause while loading is shown as `Paused`; the stream starts on the next
//!   toggle. If `EnginePlay` was already sent, the pause is `EnginePause`
//! - Replacing a preload that the engine already holds sends
//!   `EngineCancelPreload` before resolving the new one, so the old entry can
//!   never play
//! - Removing the current entry while playing with nothing after it stops
//!   with no current entry; `Previous` wraps only with repeat `queue`
//! - If the last track ends while a suggestions request is pending, the
//!   player stops on it and, when the suggestions arrive, plays the first one

mod queue;
#[cfg(test)]
mod tests;

use std::time::Duration;

use crate::protocol::{
    Command, Event, InsertAt, NowPlaying, PlaybackState, PlayerSnapshot, QueueEntry, RepeatMode,
};
use crate::quality::AudioQuality;
use crate::track::{EntryId, Track, TrackId};

use queue::{Queue, Rng};

/// Time left in a track at which the next entry is preloaded (and autoplay
/// asks for suggestions).
pub const PRELOAD_BEFORE_END: Duration = Duration::from_secs(30);

/// Consecutive track-only failures after which the player stops.
pub const MAX_FAILURE_RUN: usize = 5;

/// Player settings (spec 0004 "Settings"). Volume and seek steps are not
/// here: clients send them with `ChangeVolume` and `SeekBy`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerConfig {
    /// `Previous` restarts the track when the position is more than this;
    /// zero means previous always goes back.
    pub previous_restart: Duration,
    /// Autoplay at start.
    pub autoplay: bool,
    /// The session's country, for the "not available in <country>" message
    /// of a track that is not streamable.
    pub country: Option<String>,
}

impl Default for PlayerConfig {
    fn default() -> Self {
        Self {
            previous_restart: Duration::from_secs(3),
            autoplay: false,
            country: None,
        }
    }
}

/// How the player reacts to a failure (spec 0004 "Failures"). The runtime
/// classifies resolver and engine errors into these.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureKind {
    /// This track only (`NotFound`, `NotAvailable`, `PreviewOnly`,
    /// `Unsupported`, `Decode`): move on.
    TrackOnly,
    /// Network after retries, `429`, `5xx`: stop, never skip.
    Transient,
    /// The output (`Busy`, `NotFound`, `Lost`): stop, never skip.
    Output,
    /// `LoginRequired`: stop.
    Session,
}

/// A classified failure and the message to show (0003's wording).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub kind: FailureKind,
    pub message: String,
}

/// What the engine reports about a started track, as the runtime formats it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackDetails {
    /// The source description (0003's "Track" line after the quality).
    pub source: String,
    /// The output description (0003's "Output" line without the verdict).
    pub output: String,
    /// The engine's `bit_perfect`.
    pub bit_perfect: bool,
    /// The engine's reason when not bit-perfect.
    pub reason: Option<String>,
}

/// An engine event, mapped by the runtime. Track-scoped events carry the tag
/// given with the `EnginePlay`/`EnginePreload` of their track.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineEvent {
    Started { tag: u64, details: TrackDetails },
    Transitioned { tag: u64, details: TrackDetails },
    Position(Duration),
    Buffering,
    Buffered,
    TrackEnded { tag: u64 },
    Paused,
    Resumed,
    Stopped,
    Error { tag: u64, failure: Failure },
}

/// An input to the player.
#[derive(Debug, Clone, PartialEq)]
pub enum PlayerInput {
    /// A client command.
    Command(Command),
    /// An engine event.
    Engine(EngineEvent),
    /// The result of a `Resolve` effect: the granted quality, or a failure.
    Resolved {
        tag: u64,
        result: Result<AudioQuality, Failure>,
    },
    /// The result of a `FetchSuggestions` effect: tracks, or an error message.
    Suggestions {
        tag: u64,
        result: Result<Vec<Track>, String>,
    },
}

/// Why a stream is resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    Play,
    Preload,
}

/// A side effect the runtime executes, in order.
#[derive(Debug, Clone, PartialEq)]
pub enum PlayerEffect {
    /// Resolve the stream of `track` (entry `entry`) and answer with
    /// `PlayerInput::Resolved { tag, .. }`; keep the stream for `tag`.
    Resolve {
        entry: EntryId,
        track: TrackId,
        tag: u64,
        purpose: Purpose,
    },
    /// Engine `Play` of the stream resolved for `tag`.
    EnginePlay {
        tag: u64,
        start_at: Duration,
    },
    /// Engine `Preload` of the stream resolved for `tag`.
    EnginePreload {
        tag: u64,
    },
    EngineCancelPreload,
    EnginePause,
    EngineResume,
    EngineSeek(Duration),
    EngineStop,
    /// Software gain, `0.0..=1.0`.
    EngineSetGain(f32),
    /// Fetch autoplay suggestions seeded by `seed`; answer with
    /// `PlayerInput::Suggestions { tag, .. }`.
    FetchSuggestions {
        seed: TrackId,
        tag: u64,
    },
    /// Send to every client.
    Broadcast(Event),
}

/// The player's whole state. Read it through [`PlayerState::snapshot`].
#[derive(Debug, Clone)]
pub struct PlayerState {
    config: PlayerConfig,
    rng: Rng,
    queue: Queue,
    next_entry_id: u64,
    next_tag: u64,
    phase: Phase,
    /// Position in the current entry (the start position while loading).
    position: Duration,
    shuffle: bool,
    repeat: RepeatMode,
    autoplay: bool,
    volume: u8,
    muted: bool,
    now_playing: Option<Started>,
    message: Option<String>,
    preload: Option<Preload>,
    /// The current track has reached its preload point.
    armed: bool,
    failures: usize,
    suggestions: Suggestions,
    /// The engine holds a track (sent `Play`, no `TrackEnded`/`Error`/`Stop`).
    engine_busy: bool,
}

#[derive(Debug, Clone, PartialEq)]
enum Phase {
    Stopped,
    Loading(Load),
    Playing { tag: u64, buffering: bool },
    Paused { tag: u64 },
}

#[derive(Debug, Clone, PartialEq)]
struct Load {
    tag: u64,
    stage: Stage,
    /// Paused while loading: do not start (or keep paused) until toggled.
    held: bool,
    start_at: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Stage {
    /// `Resolve` sent.
    Resolving,
    /// Resolved while held; `EnginePlay` not sent yet.
    Ready(AudioQuality),
    /// `EnginePlay` sent, waiting for `Started`.
    Opening(AudioQuality),
}

#[derive(Debug, Clone)]
struct Preload {
    entry: EntryId,
    tag: u64,
    stage: PreloadStage,
}

#[derive(Debug, Clone)]
enum PreloadStage {
    Resolving,
    /// `EnginePreload` sent.
    Sent(AudioQuality),
    /// Reported only when the entry would have started.
    Failed(Failure),
}

#[derive(Debug, Clone)]
struct Started {
    quality: AudioQuality,
    details: TrackDetails,
}

#[derive(Debug, Clone, PartialEq)]
enum Suggestions {
    Idle,
    Pending {
        tag: u64,
        seed: EntryId,
        /// The seed entry ended (the player stopped) before the result came.
        ended: bool,
    },
    /// Asked for this seed entry already (at most one request per seed).
    Done {
        seed: EntryId,
    },
}

impl PlayerState {
    /// A stopped player with an empty queue, volume 100 % and the shuffle
    /// PRNG seeded with `seed`.
    pub fn new(config: PlayerConfig, seed: u64) -> Self {
        Self {
            autoplay: config.autoplay,
            config,
            rng: Rng::new(seed),
            queue: Queue::default(),
            next_entry_id: 1,
            next_tag: 1,
            phase: Phase::Stopped,
            position: Duration::ZERO,
            shuffle: false,
            repeat: RepeatMode::Off,
            volume: 100,
            muted: false,
            now_playing: None,
            message: None,
            preload: None,
            armed: false,
            failures: 0,
            suggestions: Suggestions::Idle,
            engine_busy: false,
        }
    }

    /// The state as clients see it.
    pub fn snapshot(&self) -> PlayerSnapshot {
        PlayerSnapshot {
            queue: self.queue.play_order(),
            current: self.queue.current,
            state: match &self.phase {
                Phase::Stopped => PlaybackState::Stopped,
                Phase::Loading(load) if load.held => PlaybackState::Paused,
                Phase::Loading(_) => PlaybackState::Loading,
                Phase::Playing {
                    buffering: true, ..
                } => PlaybackState::Buffering,
                Phase::Playing { .. } => PlaybackState::Playing,
                Phase::Paused { .. } => PlaybackState::Paused,
            },
            position: self.position,
            shuffle: self.shuffle,
            repeat: self.repeat,
            autoplay: self.autoplay,
            volume: self.volume,
            muted: self.muted,
            now_playing: self.now_playing.as_ref().map(|s| {
                let reason = if self.muted {
                    Some("muted".to_owned())
                } else if self.volume < 100 {
                    Some("volume below 100%".to_owned())
                } else if s.details.bit_perfect {
                    None
                } else {
                    s.details.reason.clone()
                };
                NowPlaying {
                    quality: s.quality,
                    source: s.details.source.clone(),
                    output: s.details.output.clone(),
                    bit_perfect: s.details.bit_perfect && !self.muted && self.volume == 100,
                    bit_perfect_reason: reason,
                }
            }),
            message: self.message.clone(),
        }
    }

    /// The software gain for the current volume: `(volume / 100)³`, 0 muted.
    pub fn gain(&self) -> f32 {
        if self.muted {
            0.0
        } else {
            (f64::from(self.volume) / 100.0).powi(3) as f32
        }
    }
}

/// Applies `input` and returns the effects the runtime must execute, in order.
///
/// An input that changes the snapshot ends with exactly one
/// `Broadcast(Event::Player(..))`; an engine `Position` broadcasts only
/// `Event::Position`; an input that changes nothing returns nothing.
pub fn update(state: &mut PlayerState, input: PlayerInput) -> Vec<PlayerEffect> {
    // Stub (red): no effects, no change.
    let _ = (state, input);
    Vec::new()
}

impl PlayerState {
    /// The tag of the current track, once its load started.
    fn current_tag(&self) -> Option<u64> {
        match &self.phase {
            Phase::Stopped => None,
            Phase::Loading(load) => Some(load.tag),
            Phase::Playing { tag, .. } | Phase::Paused { tag } => Some(*tag),
        }
    }
}
