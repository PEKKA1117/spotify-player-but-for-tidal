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
//! - A failed reacquire's message (spec 0005) is cleared by the engine's
//!   next `Resumed`; `released` is cleared by `Resumed` and by any track
//!   start or stop

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
    Started {
        tag: u64,
        details: TrackDetails,
    },
    Transitioned {
        tag: u64,
        details: TrackDetails,
    },
    Position(Duration),
    Buffering,
    Buffered,
    TrackEnded {
        tag: u64,
    },
    Paused,
    Resumed,
    Stopped,
    Error {
        tag: u64,
        failure: Failure,
    },
    /// Paused, the engine released the output (spec 0005).
    Released,
    /// Reopening the output on resume failed (spec 0005): the engine is
    /// still paused with the track; `failure` carries 0003's message.
    ResumeFailed {
        failure: Failure,
    },
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
    /// Paused, the engine released the output (spec 0005); cleared by its
    /// `Resumed` and whenever another track starts or the player stops.
    released: bool,
    /// The message shown is a failed reacquire's (cleared on `Resumed`).
    resume_failed: bool,
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
            released: false,
            resume_failed: false,
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
                    released: self.released,
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
    let mut fx = Vec::new();
    if let PlayerInput::Engine(EngineEvent::Position(position)) = input {
        state.on_position(position, &mut fx);
        return fx;
    }
    let before = state.snapshot();
    match input {
        PlayerInput::Command(command) => state.on_command(command, &mut fx),
        PlayerInput::Engine(event) => state.on_engine(event, &mut fx),
        PlayerInput::Resolved { tag, result } => state.on_resolved(tag, result, &mut fx),
        PlayerInput::Suggestions { tag, result } => state.on_suggestions(tag, result, &mut fx),
    }
    let after = state.snapshot();
    if after != before {
        fx.push(PlayerEffect::Broadcast(Event::Player(after)));
    }
    fx
}

type Fx = Vec<PlayerEffect>;

impl PlayerState {
    fn fresh_tag(&mut self) -> u64 {
        let tag = self.next_tag;
        self.next_tag += 1;
        tag
    }

    fn new_entries(&mut self, tracks: Vec<Track>, suggested: bool) -> Vec<QueueEntry> {
        tracks
            .into_iter()
            .map(|track| {
                let id = EntryId(self.next_entry_id);
                self.next_entry_id += 1;
                QueueEntry {
                    id,
                    track,
                    suggested,
                }
            })
            .collect()
    }

    /// The tag of the current track, once its load started.
    fn current_tag(&self) -> Option<u64> {
        match &self.phase {
            Phase::Stopped => None,
            Phase::Loading(load) => Some(load.tag),
            Phase::Playing { tag, .. } | Phase::Paused { tag } => Some(*tag),
        }
    }

    fn current_duration(&self) -> Option<Duration> {
        self.queue
            .current
            .and_then(|id| self.queue.get(id))
            .and_then(|e| e.track.duration)
    }

    /// The entry `Next` (and a track-only failure) moves to.
    fn skip_target(&self, entry: EntryId) -> Option<EntryId> {
        self.queue.after(entry, self.repeat != RepeatMode::Off)
    }

    /// The entry a natural end moves to (and that is preloaded).
    fn natural_target(&self, entry: EntryId) -> Option<EntryId> {
        match self.repeat {
            RepeatMode::Track => Some(entry),
            RepeatMode::Queue => self.queue.after(entry, true),
            RepeatMode::Off => self.queue.after(entry, false),
        }
    }

    fn not_available(&self, track: TrackId) -> Failure {
        let message = match &self.config.country {
            Some(country) => format!("Track {track} is not available in {country}"),
            None => format!("Track {track} is not available"),
        };
        Failure {
            kind: FailureKind::TrackOnly,
            message,
        }
    }

    // --- starting and stopping ---------------------------------------------

    /// Makes `entry` current and starts loading it from `start_at`.
    fn start(&mut self, entry: EntryId, start_at: Duration, fx: &mut Fx) {
        self.queue.current = Some(entry);
        self.armed = false;
        self.now_playing = None;
        self.released = false;
        self.resume_failed = false;
        self.position = start_at;
        if self.engine_busy {
            fx.push(PlayerEffect::EngineStop);
            self.engine_busy = false;
        }
        let preload = self.preload.take();
        if let Some(p) = preload.filter(|p| p.entry == entry && start_at.is_zero()) {
            match p.stage {
                PreloadStage::Resolving => {
                    self.phase = Phase::Loading(Load {
                        tag: p.tag,
                        stage: Stage::Resolving,
                        held: false,
                        start_at,
                    });
                    return;
                }
                PreloadStage::Failed(failure) => {
                    self.phase = Phase::Stopped;
                    return self.fail(entry, failure, fx);
                }
                // The engine has dropped it; resolve afresh.
                PreloadStage::Sent(_) => {}
            }
        }
        let Some(track) = self.queue.get(entry).map(|e| e.track.clone()) else {
            return;
        };
        if !track.streamable {
            self.phase = Phase::Stopped;
            let failure = self.not_available(track.id);
            return self.fail(entry, failure, fx);
        }
        let tag = self.fresh_tag();
        fx.push(PlayerEffect::Resolve {
            entry,
            track: track.id,
            tag,
            purpose: Purpose::Play,
        });
        self.phase = Phase::Loading(Load {
            tag,
            stage: Stage::Resolving,
            held: false,
            start_at,
        });
    }

    /// Stops on `entry` (or with nothing current) at `position`.
    fn stop_on(&mut self, entry: Option<EntryId>, position: Duration, fx: &mut Fx) {
        if self.engine_busy {
            fx.push(PlayerEffect::EngineStop);
            self.engine_busy = false;
        }
        self.queue.current = entry;
        self.phase = Phase::Stopped;
        self.position = position;
        self.preload = None;
        self.armed = false;
        self.now_playing = None;
        self.released = false;
        self.resume_failed = false;
    }

    /// Handles a failure of `entry` per the "Failures" table.
    fn fail(&mut self, entry: EntryId, failure: Failure, fx: &mut Fx) {
        self.message = Some(failure.message);
        match failure.kind {
            FailureKind::TrackOnly => {
                self.failures += 1;
                let limit = MAX_FAILURE_RUN.min(self.queue.len()).max(1);
                if self.failures >= limit {
                    self.stop_on(Some(entry), Duration::ZERO, fx);
                    // A run of one keeps the failure's own message (0004 Bugs).
                    if self.failures > 1 {
                        self.message = Some(format!(
                            "Stopped: {} tracks in a row could not be played",
                            self.failures
                        ));
                    }
                } else {
                    match self.skip_target(entry) {
                        Some(next) => self.start(next, Duration::ZERO, fx),
                        None => self.stop_on(Some(entry), Duration::ZERO, fx),
                    }
                }
            }
            FailureKind::Transient | FailureKind::Output | FailureKind::Session => {
                self.stop_on(Some(entry), self.position, fx);
            }
        }
    }

    /// The current track ended naturally.
    fn natural_end(&mut self, entry: EntryId, fx: &mut Fx) {
        match self.natural_target(entry) {
            Some(next) => self.start(next, Duration::ZERO, fx),
            None => {
                let pending = std::mem::replace(&mut self.suggestions, Suggestions::Idle);
                self.stop_on(Some(entry), Duration::ZERO, fx);
                self.suggestions = match pending {
                    Suggestions::Pending { tag, seed, .. } => Suggestions::Pending {
                        tag,
                        seed,
                        ended: true,
                    },
                    other => other,
                };
            }
        }
    }

    /// Restarts the current entry at 0:00 (`Previous` near the start).
    fn restart(&mut self, fx: &mut Fx) {
        self.seek_to(Duration::ZERO, fx);
        if matches!(self.phase, Phase::Stopped) {
            self.position = Duration::ZERO;
        }
    }

    fn seek_to(&mut self, target: Duration, fx: &mut Fx) {
        match &mut self.phase {
            Phase::Stopped => {}
            Phase::Loading(load) => {
                load.start_at = target;
                if matches!(load.stage, Stage::Opening(_)) {
                    fx.push(PlayerEffect::EngineSeek(target));
                }
                self.position = target;
            }
            Phase::Playing { .. } | Phase::Paused { .. } => {
                if target != self.position {
                    fx.push(PlayerEffect::EngineSeek(target));
                    self.position = target;
                }
            }
        }
    }

    // --- preload and autoplay ------------------------------------------------

    /// Brings the preload in line with the next entry once the current track
    /// reached its preload point; asks for suggestions at the end of the queue.
    fn reconcile_preload(&mut self, fx: &mut Fx) {
        if !self.armed {
            return;
        }
        let Some(current) = self.queue.current else {
            return;
        };
        let desired = self.natural_target(current);
        if let Some(p) = &self.preload {
            if Some(p.entry) == desired {
                return;
            }
            if matches!(p.stage, PreloadStage::Sent(_)) {
                fx.push(PlayerEffect::EngineCancelPreload);
            }
            self.preload = None;
        }
        match desired {
            Some(next) => {
                let Some(track) = self.queue.get(next).map(|e| e.track.clone()) else {
                    return;
                };
                let tag = self.fresh_tag();
                let stage = if track.streamable {
                    fx.push(PlayerEffect::Resolve {
                        entry: next,
                        track: track.id,
                        tag,
                        purpose: Purpose::Preload,
                    });
                    PreloadStage::Resolving
                } else {
                    PreloadStage::Failed(self.not_available(track.id))
                };
                self.preload = Some(Preload {
                    entry: next,
                    tag,
                    stage,
                });
            }
            None => self.request_suggestions(current, fx),
        }
    }

    fn request_suggestions(&mut self, current: EntryId, fx: &mut Fx) {
        if !self.autoplay || self.repeat != RepeatMode::Off {
            return;
        }
        let asked = match self.suggestions {
            Suggestions::Idle => false,
            Suggestions::Pending { .. } => true,
            Suggestions::Done { seed } => seed == current,
        };
        let Some(seed) = self.queue.get(current).map(|e| e.track.id) else {
            return;
        };
        if asked {
            return;
        }
        let tag = self.fresh_tag();
        fx.push(PlayerEffect::FetchSuggestions { seed, tag });
        self.suggestions = Suggestions::Pending {
            tag,
            seed: current,
            ended: false,
        };
    }

    // --- inputs ----------------------------------------------------------------

    fn on_position(&mut self, position: Duration, fx: &mut Fx) {
        if !matches!(self.phase, Phase::Playing { .. } | Phase::Paused { .. }) {
            return;
        }
        let Some(entry) = self.queue.current else {
            return;
        };
        if !self.armed
            && self
                .current_duration()
                .is_some_and(|d| d.saturating_sub(position) <= PRELOAD_BEFORE_END)
        {
            self.armed = true;
            self.reconcile_preload(fx);
        }
        if position != self.position {
            self.position = position;
            fx.push(PlayerEffect::Broadcast(Event::Position { entry, position }));
        }
    }

    fn on_command(&mut self, command: Command, fx: &mut Fx) {
        match command {
            Command::Shutdown => {}
            Command::LoadQueue { tracks, start } => self.load(tracks, start, fx),
            Command::AddToQueue { tracks, at } => self.add(tracks, at, fx),
            Command::RemoveFromQueue(id) => self.remove(id, fx),
            Command::ClearQueue => {
                self.queue.retain_current();
                self.reconcile_preload(fx);
            }
            Command::PlayEntry(id) => {
                if self.queue.get(id).is_some() {
                    self.failures = 0;
                    self.start(id, Duration::ZERO, fx);
                }
            }
            Command::TogglePause => self.toggle_pause(fx),
            Command::Next => {
                let Some(current) = self.queue.current else {
                    return;
                };
                self.failures = 0;
                match self.skip_target(current) {
                    Some(next) => self.start(next, Duration::ZERO, fx),
                    None => self.stop_on(Some(current), Duration::ZERO, fx),
                }
            }
            Command::Previous => self.previous(fx),
            Command::SeekBy(ms) => {
                let now = i128::try_from(self.position.as_millis()).unwrap_or(i128::MAX);
                let target = u64::try_from((now + i128::from(ms)).max(0)).unwrap_or(u64::MAX);
                self.seek_to(Duration::from_millis(target), fx);
            }
            Command::SeekTo(target) => self.seek_to(target, fx),
            Command::ToggleShuffle => {
                self.shuffle = !self.shuffle;
                if self.shuffle {
                    self.queue.shuffle(&mut self.rng);
                } else {
                    self.queue.unshuffle();
                }
                self.reconcile_preload(fx);
            }
            Command::CycleRepeat => {
                self.repeat = self.repeat.cycled();
                self.reconcile_preload(fx);
            }
            Command::ToggleAutoplay => {
                self.autoplay = !self.autoplay;
                if !self.autoplay {
                    self.suggestions = Suggestions::Idle;
                }
                self.reconcile_preload(fx);
            }
            Command::ChangeVolume(delta) => {
                let volume = (i16::from(self.volume) + i16::from(delta)).clamp(0, 100);
                self.set_volume(volume as u8, fx);
            }
            Command::SetVolume(volume) => self.set_volume(volume.min(100), fx),
            Command::ToggleMute => {
                self.muted = !self.muted;
                fx.push(PlayerEffect::EngineSetGain(self.gain()));
            }
        }
    }

    fn set_volume(&mut self, volume: u8, fx: &mut Fx) {
        if volume == self.volume && !self.muted {
            return;
        }
        self.volume = volume;
        self.muted = false;
        fx.push(PlayerEffect::EngineSetGain(self.gain()));
    }

    fn load(&mut self, tracks: Vec<Track>, start: usize, fx: &mut Fx) {
        if tracks.is_empty() {
            if !self.queue.is_empty() || !matches!(self.phase, Phase::Stopped) {
                self.queue.replace(Vec::new(), None, None);
                self.suggestions = Suggestions::Idle;
                self.stop_on(None, Duration::ZERO, fx);
            }
            return;
        }
        let entries = self.new_entries(tracks, false);
        let current = entries[start.min(entries.len() - 1)].id;
        let rng = self.shuffle.then_some(&mut self.rng);
        self.queue.replace(entries, Some(current), rng);
        self.failures = 0;
        self.suggestions = Suggestions::Idle;
        self.start(current, Duration::ZERO, fx);
    }

    fn add(&mut self, tracks: Vec<Track>, at: InsertAt, fx: &mut Fx) {
        if tracks.is_empty() {
            return;
        }
        let entries = self.new_entries(tracks, false);
        let first = entries[0].id;
        match (self.queue.current, at) {
            (None, _) | (Some(_), InsertAt::End) => self.queue.append(entries),
            (Some(_), InsertAt::Next) => self.queue.insert_after_current(entries),
        }
        if self.queue.current.is_none() {
            // Nothing starts: the client sends `PlayEntry` if it wants that.
            self.queue.current = Some(first);
            self.position = Duration::ZERO;
        }
        self.reconcile_preload(fx);
    }

    fn remove(&mut self, id: EntryId, fx: &mut Fx) {
        if self.queue.get(id).is_none() {
            return;
        }
        if matches!(self.suggestions, Suggestions::Pending { seed, .. } if seed == id) {
            self.suggestions = Suggestions::Idle;
        }
        if self.queue.current != Some(id) {
            self.queue.remove(id);
            return self.reconcile_preload(fx);
        }
        if matches!(self.phase, Phase::Stopped) {
            let next = self.queue.after(id, false);
            self.queue.remove(id);
            self.queue.current = next;
            self.position = Duration::ZERO;
            return;
        }
        let next = self.skip_target(id).filter(|next| *next != id);
        self.queue.remove(id);
        match next {
            Some(next) => self.start(next, Duration::ZERO, fx),
            None => self.stop_on(None, Duration::ZERO, fx),
        }
    }

    fn previous(&mut self, fx: &mut Fx) {
        let Some(current) = self.queue.current else {
            return;
        };
        let threshold = self.config.previous_restart;
        if !threshold.is_zero() && self.position > threshold {
            return self.restart(fx);
        }
        match self.queue.before(current, self.repeat == RepeatMode::Queue) {
            Some(previous) => {
                self.failures = 0;
                self.start(previous, Duration::ZERO, fx);
            }
            None => self.restart(fx),
        }
    }

    fn toggle_pause(&mut self, fx: &mut Fx) {
        match &mut self.phase {
            Phase::Playing { tag, .. } => {
                fx.push(PlayerEffect::EnginePause);
                self.phase = Phase::Paused { tag: *tag };
            }
            Phase::Paused { tag } => {
                fx.push(PlayerEffect::EngineResume);
                self.phase = Phase::Playing {
                    tag: *tag,
                    buffering: false,
                };
            }
            Phase::Loading(load) => match load.stage {
                Stage::Resolving => load.held = !load.held,
                Stage::Ready(quality) => {
                    fx.push(PlayerEffect::EnginePlay {
                        tag: load.tag,
                        start_at: load.start_at,
                    });
                    load.stage = Stage::Opening(quality);
                    load.held = false;
                    self.engine_busy = true;
                }
                Stage::Opening(_) => {
                    fx.push(if load.held {
                        PlayerEffect::EngineResume
                    } else {
                        PlayerEffect::EnginePause
                    });
                    load.held = !load.held;
                }
            },
            Phase::Stopped => {
                if let Some(current) = self.queue.current {
                    self.failures = 0;
                    self.start(current, self.position, fx);
                }
            }
        }
    }

    fn on_engine(&mut self, event: EngineEvent, fx: &mut Fx) {
        match event {
            EngineEvent::Started { tag, details } => {
                let Phase::Loading(load) = &self.phase else {
                    return;
                };
                let Stage::Opening(quality) = load.stage else {
                    return;
                };
                if load.tag != tag {
                    return;
                }
                let (held, start_at) = (load.held, load.start_at);
                self.phase = if held {
                    Phase::Paused { tag }
                } else {
                    Phase::Playing {
                        tag,
                        buffering: false,
                    }
                };
                self.position = start_at;
                self.track_started(quality, details, fx);
            }
            EngineEvent::Transitioned { tag, details } => {
                if !matches!(self.phase, Phase::Playing { .. } | Phase::Paused { .. }) {
                    return;
                }
                let Some(p) = self.preload.take_if(|p| p.tag == tag) else {
                    return;
                };
                let PreloadStage::Sent(quality) = p.stage else {
                    self.preload = Some(p);
                    return;
                };
                self.queue.current = Some(p.entry);
                self.phase = Phase::Playing {
                    tag,
                    buffering: false,
                };
                self.position = Duration::ZERO;
                self.track_started(quality, details, fx);
            }
            EngineEvent::Position(_) => {}
            EngineEvent::Buffering | EngineEvent::Buffered => {
                if let Phase::Playing { buffering, .. } = &mut self.phase {
                    *buffering = matches!(event, EngineEvent::Buffering);
                }
            }
            EngineEvent::TrackEnded { tag } => {
                let playing = matches!(self.phase, Phase::Playing { .. } | Phase::Paused { .. });
                if !playing || self.current_tag() != Some(tag) {
                    return;
                }
                self.engine_busy = false;
                if let Some(current) = self.queue.current {
                    self.natural_end(current, fx);
                }
            }
            EngineEvent::Error { tag, failure } => {
                let engine_has_it = match &self.phase {
                    Phase::Loading(load) => matches!(load.stage, Stage::Opening(_)),
                    Phase::Playing { .. } | Phase::Paused { .. } => true,
                    Phase::Stopped => false,
                };
                if engine_has_it && self.current_tag() == Some(tag) {
                    self.engine_busy = false;
                    self.preload = None;
                    if let Some(current) = self.queue.current {
                        self.fail(current, failure, fx);
                    }
                } else if let Some(p) = self.preload.as_mut().filter(|p| p.tag == tag) {
                    // The engine dropped the preload; reported at its start.
                    p.stage = PreloadStage::Failed(failure);
                }
            }
            EngineEvent::Released => {
                if matches!(self.phase, Phase::Playing { .. } | Phase::Paused { .. }) {
                    self.released = true;
                }
            }
            EngineEvent::Resumed => {
                self.released = false;
                if std::mem::take(&mut self.resume_failed) {
                    // The failed reacquire's message is over.
                    self.message = None;
                }
            }
            EngineEvent::ResumeFailed { failure } => {
                // Not a track failure (spec 0005): still paused on the same
                // track and position, never skipped; the next toggle sends
                // `EngineResume` again.
                if let Phase::Playing { tag, .. } | Phase::Paused { tag } = self.phase {
                    self.phase = Phase::Paused { tag };
                    self.message = Some(failure.message);
                    self.resume_failed = true;
                }
            }
            EngineEvent::Paused | EngineEvent::Stopped => {}
        }
    }

    /// A track started (or the engine moved into the preload).
    fn track_started(&mut self, quality: AudioQuality, details: TrackDetails, fx: &mut Fx) {
        self.now_playing = Some(Started { quality, details });
        self.message = None;
        self.failures = 0;
        // Unknown duration: the preload point is the start.
        self.armed = self.current_duration().is_none();
        self.reconcile_preload(fx);
    }

    fn on_resolved(&mut self, tag: u64, result: Result<AudioQuality, Failure>, fx: &mut Fx) {
        if let Phase::Loading(load) = &mut self.phase
            && load.tag == tag
            && load.stage == Stage::Resolving
        {
            match result {
                Ok(quality) if load.held => load.stage = Stage::Ready(quality),
                Ok(quality) => {
                    fx.push(PlayerEffect::EnginePlay {
                        tag,
                        start_at: load.start_at,
                    });
                    load.stage = Stage::Opening(quality);
                    self.engine_busy = true;
                }
                Err(failure) => {
                    self.phase = Phase::Stopped;
                    if let Some(current) = self.queue.current {
                        self.fail(current, failure, fx);
                    }
                }
            }
            return;
        }
        if let Some(p) = self.preload.as_mut()
            && p.tag == tag
            && matches!(p.stage, PreloadStage::Resolving)
        {
            p.stage = match result {
                Ok(quality) => {
                    fx.push(PlayerEffect::EnginePreload { tag });
                    PreloadStage::Sent(quality)
                }
                Err(failure) => PreloadStage::Failed(failure),
            };
        }
    }

    fn on_suggestions(&mut self, tag: u64, result: Result<Vec<Track>, String>, fx: &mut Fx) {
        let Suggestions::Pending {
            tag: pending,
            seed,
            ended,
        } = self.suggestions
        else {
            return;
        };
        if pending != tag {
            return;
        }
        self.suggestions = Suggestions::Done { seed };
        let mut fresh: Vec<Track> = Vec::new();
        let reason = match result {
            Ok(tracks) => {
                for track in tracks {
                    if !self.queue.contains_track(track.id)
                        && !fresh.iter().any(|t| t.id == track.id)
                    {
                        fresh.push(track);
                    }
                }
                "no new tracks".to_owned()
            }
            Err(message) => message,
        };
        if fresh.is_empty() {
            self.message = Some(format!("Autoplay: no suggestions ({reason})"));
            return;
        }
        let entries = self.new_entries(fresh, true);
        let first = entries[0].id;
        self.queue.append(entries);
        if ended {
            self.start(first, Duration::ZERO, fx);
        } else {
            self.reconcile_preload(fx);
        }
    }
}
