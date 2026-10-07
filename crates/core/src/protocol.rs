//! Wire protocol between a daemon and its clients. Transport-agnostic: these
//! types only describe messages, they never send them.
//!
//! Playback messages: spec 0004 "Player and protocol".

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::item::Item;
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
    Next,
    Previous,
    /// Seek relative to the current position, in signed milliseconds.
    SeekBy(i64),
    /// Seek to a position in the current track.
    SeekTo(Duration),
    ToggleShuffle,
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
}

/// What a client sends over the socket (spec 0005 "Messages").
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientMessage {
    /// Answered with a [`ServerMessage::Welcome`], then every event.
    Subscribe,
    /// Apply `command`; answered with a [`ServerMessage::Reply`] carrying
    /// the same `id`, after the events the command caused.
    Request { id: u64, command: Command },
}

/// What the player sends over the socket (spec 0005 "Messages").
#[derive(Debug, Clone, PartialEq, Eq)]
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
}

// Red stubs (spec 0005 Test plan AC1): replaced by derives.
impl Serialize for ClientMessage {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_unit()
    }
}
impl<'de> Deserialize<'de> for ClientMessage {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let _ = d;
        Err(serde::de::Error::custom("stub"))
    }
}
impl Serialize for ServerMessage {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_unit()
    }
}
impl<'de> Deserialize<'de> for ServerMessage {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let _ = d;
        Err(serde::de::Error::custom("stub"))
    }
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::track::TrackId;
    use serde::de::DeserializeOwned;

    fn round_trip<T>(v: &T)
    where
        T: Serialize + DeserializeOwned + PartialEq + Clone + std::fmt::Debug,
    {
        let json = serde_json::to_string(v).expect("serialize");
        assert_eq!(
            serde_json::from_str::<T>(&json).ok(),
            Some(v.clone()),
            "{v:?} did not survive JSON {json}"
        );
    }

    fn track(id: u64) -> Track {
        Track {
            id: TrackId(id),
            title: format!("Title {id}"),
            artists: vec!["A".into(), "B".into()],
            album: Some("Album".into()),
            duration: Some(Duration::from_secs(212)),
            streamable: true,
        }
    }

    /// Every variant of `Command`; extend when a variant is added.
    fn all_commands() -> Vec<Command> {
        vec![
            Command::Shutdown,
            Command::LoadQueue {
                tracks: vec![track(1), track(2)],
                start: 1,
            },
            Command::AddToQueue {
                tracks: vec![track(3)],
                at: InsertAt::Next,
            },
            Command::AddToQueue {
                tracks: vec![],
                at: InsertAt::End,
            },
            Command::RemoveFromQueue(EntryId(4)),
            Command::ClearQueue,
            Command::PlayEntry(EntryId(5)),
            Command::TogglePause,
            Command::Next,
            Command::Previous,
            Command::SeekBy(-5000),
            Command::SeekTo(Duration::from_millis(83_250)),
            Command::ToggleShuffle,
            Command::CycleRepeat,
            Command::ToggleAutoplay,
            Command::ChangeVolume(-25),
            Command::SetVolume(80),
            Command::ToggleMute,
            Command::Open {
                items: vec![
                    Item::Track(TrackId(1)),
                    Item::Album(2),
                    Item::Playlist("36ea71a8-445e-41a4-82ab-6628c581535d".into()),
                ],
                at: None,
            },
            Command::Open {
                items: vec![Item::Track(TrackId(3))],
                at: Some(InsertAt::End),
            },
            Command::Open {
                items: vec![],
                at: Some(InsertAt::Next),
            },
        ]
    }

    /// Every variant of `ClientMessage` and `ServerMessage` (spec 0005
    /// AC1); extend when a variant is added.
    fn all_messages() -> (Vec<ClientMessage>, Vec<ServerMessage>) {
        let mut client = vec![ClientMessage::Subscribe];
        client.extend(all_commands().into_iter().enumerate().map(|(id, command)| {
            ClientMessage::Request {
                id: id as u64 * 1_000_003,
                command,
            }
        }));
        let snapshot = match &all_events()[3] {
            Event::Player(snapshot) => snapshot.clone(),
            other => panic!("expected a snapshot, got {other:?}"),
        };
        let mut server = vec![
            ServerMessage::Welcome {
                snapshot: snapshot.clone(),
                login_required: false,
            },
            ServerMessage::Welcome {
                snapshot,
                login_required: true,
            },
            ServerMessage::Reply {
                id: 0,
                result: Ok(()),
            },
            ServerMessage::Reply {
                id: u64::MAX,
                result: Err("Album 1 was not found".into()),
            },
        ];
        server.extend(all_events().into_iter().map(ServerMessage::Event));
        (client, server)
    }

    /// Every variant of `Event`; extend when a variant is added.
    fn all_events() -> Vec<Event> {
        let snapshot = PlayerSnapshot {
            queue: vec![
                QueueEntry {
                    id: EntryId(1),
                    track: track(1),
                    suggested: false,
                },
                QueueEntry {
                    id: EntryId(2),
                    track: Track {
                        album: None,
                        duration: None,
                        streamable: false,
                        ..track(2)
                    },
                    suggested: true,
                },
            ],
            current: Some(EntryId(1)),
            state: PlaybackState::Playing,
            position: Duration::from_millis(83_000),
            shuffle: true,
            repeat: RepeatMode::Queue,
            autoplay: true,
            volume: 80,
            muted: false,
            now_playing: Some(NowPlaying {
                quality: AudioQuality::Lossless,
                source: "FLAC 16-bit 44.1 kHz stereo".into(),
                output: "hw:1,0 (exclusive) S32_LE 44.1 kHz 2 ch".into(),
                bit_perfect: false,
                bit_perfect_reason: Some("volume below 100%".into()),
            }),
            message: Some("Track 2 was not found".into()),
        };
        let mut events = vec![
            Event::ShuttingDown,
            Event::LoginRequired,
            Event::LoginRestored,
            Event::Player(snapshot.clone()),
            Event::Player(PlayerSnapshot {
                queue: vec![],
                current: None,
                now_playing: None,
                message: None,
                repeat: RepeatMode::Track,
                ..snapshot.clone()
            }),
            Event::Position {
                entry: EntryId(7),
                position: Duration::from_millis(250),
            },
        ];
        let states = [
            PlaybackState::Stopped,
            PlaybackState::Loading,
            PlaybackState::Buffering,
            PlaybackState::Paused,
        ];
        events.extend(states.into_iter().map(|state| {
            Event::Player(PlayerSnapshot {
                state,
                repeat: RepeatMode::Off,
                ..snapshot.clone()
            })
        }));
        events
    }

    #[test]
    fn ac11_round_trip() {
        // 0001 AC11, extended by 0004 AC12.
        all_commands().iter().for_each(round_trip);
        all_events().iter().for_each(round_trip);
        // 0005 AC1: the socket messages.
        let (client, server) = all_messages();
        client.iter().for_each(round_trip);
        server.iter().for_each(round_trip);
    }
}
