//! Wire protocol between a daemon and its clients. Transport-agnostic: these
//! types only describe messages, they never send them.
//!
//! Playback messages: spec 0004 "Player and protocol".

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::item::Item;
use crate::library::{LibraryRequest, LibraryResponse};
use crate::log::LogBatch;
use crate::quality::AudioQuality;
use crate::track::{EntryId, Track};

/// A request a client sends to the daemon.
///
/// Modes are toggled rather than set, so two clients acting on a stale view
/// cannot fight (spec 0004).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Command {
    /// Ask the daemon to shut down.
    Shutdown,
    /// Replace the queue with `tracks` and play `tracks[start]` from the start
    /// (`start` past the end is clamped to the last track).
    LoadQueue {
        tracks: Vec<Track>,
        start: usize,
    },
    /// Add `tracks` after the current entry or at the end of the queue.
    AddToQueue {
        tracks: Vec<Track>,
        at: InsertAt,
    },
    /// Remove one entry.
    RemoveFromQueue(EntryId),
    /// Remove every entry except the current one.
    ClearQueue,
    /// Make an entry current and play it from the start.
    PlayEntry(EntryId),
    /// Play/pause.
    TogglePause,
    /// Start or resume; nothing when already playing (spec 0010).
    Play,
    /// Pause; nothing when already paused (spec 0010).
    Pause,
    /// Stop the engine, keep the current entry at `0:00` (spec 0010).
    Stop,
    Next,
    Previous,
    /// Seek relative to the current position, in signed milliseconds.
    SeekBy(i64),
    /// Seek to a position in the current track.
    SeekTo(Duration),
    /// Seek to `position` in `entry`, only while it is the current entry
    /// and the position is within its duration (spec 0010).
    SetPosition {
        entry: EntryId,
        position: Duration,
    },
    ToggleShuffle,
    /// Set shuffle on or off (spec 0010).
    SetShuffle(bool),
    /// Set the repeat mode (spec 0010).
    SetRepeat(RepeatMode),
    /// `off` → `queue` → `track` → `off`.
    CycleRepeat,
    ToggleAutoplay,
    /// Change the volume by a signed number of percentage points (clamped).
    ChangeVolume(i8),
    /// Set the volume in percent (values above 100 are clamped).
    SetVolume(u8),
    ToggleMute,
    /// Expand `items` in the player (spec 0005 "Opening items in the
    /// player"), in item order, then load them (`at: None`: replace the
    /// queue and play the first) or add them at `at`.
    Open {
        items: Vec<Item>,
        at: Option<InsertAt>,
    },
    /// Make `name` (an ALSA PCM name) the selected output device (spec
    /// 0014); an empty name is refused.
    SetDevice(String),
}

/// What a client sends over the socket (spec 0005 "Messages").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClientMessage {
    /// Answered with a [`ServerMessage::Welcome`], then every event.
    Subscribe,
    /// Apply `command`; answered with a [`ServerMessage::Reply`] carrying
    /// the same `id`, after the events the command caused.
    Request { id: u64, command: Command },
    /// Ask the player about the library (spec 0006 "Talking to the
    /// player"); answered to this client alone with one
    /// [`ServerMessage::LibraryReply`] carrying the same `id`.
    Library { id: u64, request: LibraryRequest },
    /// Ask the player for its output devices (spec 0014); answered to this
    /// client alone with one [`ServerMessage::DevicesReply`] carrying the
    /// same `id`.
    Devices { id: u64 },
    /// Ask the player for its log lines after `after` (all of them for
    /// `None`; spec 0027); answered to this client alone with one
    /// [`ServerMessage::LogsReply`] carrying the same `id`.
    Logs { id: u64, after: Option<u64> },
}

/// What the player sends over the socket (spec 0005 "Messages").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServerMessage {
    /// The answer to `Subscribe`: the state after every input handled so
    /// far; every later event follows.
    Welcome {
        snapshot: PlayerSnapshot,
        login_required: bool,
    },
    Event(Event),
    /// The answer to a `Request`: applied, or why not.
    Reply {
        id: u64,
        result: Result<(), String>,
    },
    /// The answer to a `Library` request, sent to the asking client only
    /// (not an [`Event`]): the response, or why not.
    LibraryReply {
        id: u64,
        result: Result<LibraryResponse, String>,
    },
    /// The answer to a `Devices` request, sent to the asking client only:
    /// the player's devices right now, or why not (spec 0014).
    DevicesReply {
        id: u64,
        result: Result<Vec<DeviceEntry>, String>,
    },
    /// The answer to a `Logs` request, sent to the asking client only
    /// (spec 0027): the lines, or why not.
    LogsReply {
        id: u64,
        result: Result<LogBatch, String>,
    },
}

/// One output device of the player's machine (spec 0014 "Listing
/// devices"): its ALSA PCM name and what it is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceEntry {
    /// The PCM name, as `--device` takes it (`default`, `hw:1,0`).
    pub name: String,
    /// The card's description (`E30 II: USB Audio`).
    pub description: String,
}

/// Where `AddToQueue` inserts its tracks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InsertAt {
    /// Right after the current entry.
    Next,
    /// At the end of the queue.
    End,
}

/// A notification the daemon sends to its clients.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Event {
    /// The daemon is shutting down.
    ShuttingDown,
    /// The session has expired; login is required.
    LoginRequired,
    /// The session has been restored after expiry.
    LoginRestored,
    /// The whole player state, sent after every input that changed it.
    /// Clients replace their copy with it and never reorder its queue.
    Player(PlayerSnapshot),
    /// The position of the playing entry, forwarded from the engine.
    Position { entry: EntryId, position: Duration },
}

/// The player's state as clients see it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerSnapshot {
    /// The queue in play order (shuffled when shuffle is on).
    pub queue: Vec<QueueEntry>,
    /// The current entry, if any.
    pub current: Option<EntryId>,
    pub state: PlaybackState,
    /// The position in the current entry (the start position while loading).
    pub position: Duration,
    pub shuffle: bool,
    pub repeat: RepeatMode,
    pub autoplay: bool,
    /// 0–100 %.
    pub volume: u8,
    pub muted: bool,
    /// Details of the track the engine is playing; `None` until it started.
    pub now_playing: Option<NowPlaying>,
    /// A failure or autoplay message, kept until the next track starts or
    /// another message replaces it.
    pub message: Option<String>,
    /// The selected output device (spec 0014): the configured one at
    /// start, then the last `SetDevice` (or the fallback's device).
    pub device: String,
}

/// One queue entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueEntry {
    pub id: EntryId,
    pub track: Track,
    /// Added by autoplay.
    pub suggested: bool,
}

/// Playback state (spec 0004 "Playback state").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlaybackState {
    Stopped,
    /// The stream is being resolved and opened; nothing plays yet.
    Loading,
    Playing,
    /// Playing, but the engine's fetch buffer ran dry.
    Buffering,
    /// Paused, including a load held by a pause (it starts on the next toggle).
    Paused,
}

/// Repeat mode (spec 0004 "Shuffle and repeat").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum RepeatMode {
    #[default]
    Off,
    Queue,
    Track,
}

impl RepeatMode {
    /// The next mode in the `off` → `queue` → `track` → `off` cycle.
    pub fn cycled(self) -> Self {
        match self {
            Self::Off => Self::Queue,
            Self::Queue => Self::Track,
            Self::Track => Self::Off,
        }
    }
}

/// What is playing and how (spec 0003 "Track" and "Output" lines).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NowPlaying {
    /// The quality Tidal granted for this stream.
    pub quality: AudioQuality,
    /// The source description (0003's "Track" line, after the quality).
    pub source: String,
    /// The output description (0003's "Output" line, without the verdict).
    pub output: String,
    /// The engine's `bit_perfect`, cleared below 100 % volume or when muted.
    pub bit_perfect: bool,
    /// Why it is not bit-perfect (`muted`, `volume below 100%`, or the
    /// engine's reason); `None` when bit-perfect.
    pub bit_perfect_reason: Option<String>,
    /// Paused, the engine released the output for other applications
    /// (spec 0005); resuming reopens it.
    pub released: bool,
}

#[cfg(test)]
mod tests;
