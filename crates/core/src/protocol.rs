//! Wire protocol between a daemon and its clients. Transport-agnostic: these
//! types only describe messages, they never send them.
//!
//! Playback messages: spec 0004 "Player and protocol".

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::item::Item;
use crate::library::{LibraryRequest, LibraryResponse};
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
    /// Paused, the engine released the output for other applications
    /// (spec 0005); resuming reopens it.
    pub released: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::{
        AlbumKind, AlbumSummary, CreditedTrack, FavoriteKind, ListItems, ListPage, ListRef,
        PageData, PageRequest, PlaylistSummary, RoleCategory, TopHit,
    };
    use crate::track::{AlbumRef, ArtistRef, TrackId};
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
            version: None,
            artists: vec![artist(10, "A"), artist(11, "B")],
            album: Some(AlbumRef {
                id: 20,
                title: "Album".into(),
                cover: Some("2e4a5d2d-9a0d-4c3a-a0ba-42b0bd16a6ec".into()),
            }),
            duration: Some(Duration::from_secs(212)),
            streamable: true,
        }
    }

    fn artist(id: u64, name: &str) -> ArtistRef {
        ArtistRef {
            id,
            name: name.into(),
        }
    }

    const UUID: &str = "36ea71a8-445e-41a4-82ab-6628c581535d";

    fn album(id: u64, kind: AlbumKind) -> AlbumSummary {
        AlbumSummary {
            id,
            title: format!("Album {id}"),
            artists: vec![artist(10, "A")],
            year: Some(2012),
            kind,
            tracks: Some(17),
            duration: Some(Duration::from_secs(3735)),
        }
    }

    fn playlist(own: bool) -> PlaylistSummary {
        PlaylistSummary {
            uuid: UUID.into(),
            title: "Running".into(),
            tracks: Some(42),
            duration: Some(Duration::from_secs(9667)),
            own,
        }
    }

    fn list<T>(items: Vec<T>, offset: u32, total: u32) -> ListPage<T> {
        ListPage {
            items,
            offset,
            total,
            hidden: 0,
        }
    }

    /// Every `ListRef` variant (spec 0006 AC1).
    fn all_list_refs() -> Vec<ListRef> {
        vec![
            ListRef::FavoriteTracks,
            ListRef::Playlists,
            ListRef::FavoriteAlbums,
            ListRef::FavoriteArtists,
            ListRef::AlbumTracks(1),
            ListRef::PlaylistTracks(UUID.into()),
            ListRef::TopTracks(2),
            ListRef::ArtistAlbums(3),
            ListRef::ArtistAppearsOn(4),
            ListRef::Credits(5),
            ListRef::SearchTracks("pierce the veil".into()),
            ListRef::SearchAlbums("AC/DC".into()),
            ListRef::SearchArtists("Sigur Rós".into()),
            ListRef::SearchPlaylists("米津玄師".into()),
        ]
    }

    /// Every variant of `LibraryRequest`, `PageRequest`, `ListRef` and
    /// `FavoriteKind` (spec 0006 AC1); extend when a variant is added.
    fn all_library_requests() -> Vec<LibraryRequest> {
        let mut requests: Vec<LibraryRequest> = [
            PageRequest::Library,
            PageRequest::FavoriteTracks,
            PageRequest::Album(1),
            PageRequest::Playlist(UUID.into()),
            PageRequest::Artist(2),
            PageRequest::Search("pierce the veil".into()),
        ]
        .into_iter()
        .map(LibraryRequest::Page)
        .collect();
        requests.extend(
            all_list_refs()
                .into_iter()
                .map(|list| LibraryRequest::More {
                    list,
                    offset: 100,
                    limit: 50,
                }),
        );
        let kinds = [
            (FavoriteKind::Track, "1"),
            (FavoriteKind::Album, "2"),
            (FavoriteKind::Artist, "3"),
            (FavoriteKind::Playlist, UUID),
        ];
        for (kind, id) in kinds {
            requests.push(LibraryRequest::IsFavorite(kind, id.into()));
            requests.push(LibraryRequest::AddFavorite(kind, id.into()));
            requests.push(LibraryRequest::RemoveFavorite(kind, id.into()));
        }
        requests.extend([
            LibraryRequest::AddToPlaylist {
                uuid: UUID.into(),
                tracks: vec![TrackId(1), TrackId(2)],
                allow_duplicates: false,
            },
            LibraryRequest::AddToPlaylist {
                uuid: UUID.into(),
                tracks: vec![],
                allow_duplicates: true,
            },
            LibraryRequest::RemoveFromPlaylist {
                uuid: UUID.into(),
                index: 3,
                etag: "\"1759831234567\"".into(),
            },
            LibraryRequest::CreatePlaylist {
                title: "Late night".into(),
            },
            LibraryRequest::DeletePlaylist { uuid: UUID.into() },
        ]);
        requests
    }

    /// Every variant of `LibraryResponse`, `PageData`, `ListItems`,
    /// `AlbumKind` and `RoleCategory`, the summaries, `CreditedTrack` and
    /// `ListPage` (spec 0006 AC1); extend when a variant is added.
    fn all_library_responses() -> Vec<LibraryResponse> {
        let credited = CreditedTrack {
            track: Track {
                version: Some("Acoustic".into()),
                ..track(7)
            },
            roles: vec![
                RoleCategory::Performer,
                RoleCategory::Songwriter,
                RoleCategory::Producer,
                RoleCategory::Engineer,
            ],
        };
        let pages = vec![
            PageData::Library {
                playlists: list(vec![playlist(true), playlist(false)], 0, 22),
                albums: list(vec![album(1, AlbumKind::Album)], 0, 14),
                artists: list(vec![artist(10, "A")], 0, 196),
            },
            PageData::Library {
                playlists: list(vec![], 0, 0),
                albums: list(vec![], 0, 0),
                artists: list(vec![], 0, 0),
            },
            PageData::FavoriteTracks {
                tracks: list(vec![track(1), track(2)], 0, 362),
            },
            PageData::Album {
                album: AlbumSummary {
                    year: None,
                    tracks: None,
                    duration: None,
                    ..album(2, AlbumKind::Ep)
                },
                tracks: list(vec![track(3)], 0, 1),
            },
            PageData::Playlist {
                playlist: PlaylistSummary {
                    tracks: None,
                    duration: None,
                    ..playlist(true)
                },
                etag: Some("\"1759831234567\"".into()),
                tracks: list(vec![track(4)], 0, 39),
            },
            PageData::Playlist {
                playlist: playlist(false),
                etag: None,
                tracks: list(vec![], 0, 0),
            },
            PageData::Artist {
                artist: artist(10, "A"),
                top_tracks: list(vec![track(5)], 0, 91),
                albums: list(vec![album(3, AlbumKind::Single)], 0, 78),
                appears_on: list(vec![album(4, AlbumKind::Album)], 0, 30),
            },
        ];
        // 0007 AC1: a search page with each kind of top hit, and none.
        let search = |top_hit: Option<Box<TopHit>>| PageData::Search {
            top_hit,
            tracks: list(vec![track(8)], 0, 223),
            albums: list(vec![album(6, AlbumKind::Album)], 0, 55),
            artists: list(vec![artist(11, "Pierce The Veil")], 0, 7),
            playlists: list(vec![playlist(false)], 0, 3),
        };
        let pages: Vec<PageData> = pages
            .into_iter()
            .chain([
                search(Some(Box::new(TopHit::Track(track(8))))),
                search(Some(Box::new(TopHit::Album(album(6, AlbumKind::Album))))),
                search(Some(Box::new(TopHit::Artist(artist(
                    11,
                    "Pierce The Veil",
                ))))),
                search(Some(Box::new(TopHit::Playlist(playlist(false))))),
                search(None),
            ])
            .collect();
        let mut responses: Vec<LibraryResponse> =
            pages.into_iter().map(LibraryResponse::Page).collect();
        responses.extend(
            [
                ListItems::Tracks(list(vec![track(6)], 100, 362)),
                ListItems::Albums(list(vec![album(5, AlbumKind::Album)], 50, 78)),
                ListItems::Playlists(list(vec![playlist(true)], 50, 51)),
                ListItems::Artists(list(vec![], 1000, 196)),
                ListItems::Credits(ListPage {
                    hidden: 37,
                    ..list(vec![credited], 50, 548)
                }),
            ]
            .into_iter()
            .map(LibraryResponse::Items),
        );
        responses.extend([
            LibraryResponse::Favorite(true),
            LibraryResponse::Favorite(false),
            LibraryResponse::Created(playlist(true)),
            LibraryResponse::Done,
        ]);
        responses
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
        // 0006 AC1: library requests.
        client.extend(
            all_library_requests()
                .into_iter()
                .enumerate()
                .map(|(id, request)| ClientMessage::Library {
                    id: id as u64 * 1_000_003,
                    request,
                }),
        );
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
        // 0006 AC1: library replies.
        server.extend(
            all_library_responses()
                .into_iter()
                .enumerate()
                .map(|(id, response)| ServerMessage::LibraryReply {
                    id: id as u64,
                    result: Ok(response),
                }),
        );
        server.push(ServerMessage::LibraryReply {
            id: u64::MAX,
            result: Err("Artist 1 was not found".into()),
        });
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
                released: false,
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
        // 0005 AC1: the device released while paused.
        events.push(Event::Player(PlayerSnapshot {
            state: PlaybackState::Paused,
            now_playing: snapshot.now_playing.clone().map(|np| NowPlaying {
                released: true,
                ..np
            }),
            ..snapshot.clone()
        }));
        events
    }

    #[test]
    fn ac11_round_trip() {
        // 0001 AC11, extended by 0004 AC12.
        all_commands().iter().for_each(round_trip);
        all_events().iter().for_each(round_trip);
        // 0006 AC1: the library's requests and responses, standalone.
        all_library_requests().iter().for_each(round_trip);
        all_library_responses().iter().for_each(round_trip);
        // 0005 AC1: the socket messages (0006 AC1: with the library's).
        let (client, server) = all_messages();
        client.iter().for_each(round_trip);
        server.iter().for_each(round_trip);
    }
}
