use super::*;
use crate::library::{
    AlbumKind, AlbumSummary, BrowseMore, BrowseRef, BrowseSection, CreditedTrack, FavoriteKind,
    ListItems, ListPage, ListRef, MixSummary, MixedRow, PageData, PageLink, PageRequest,
    PlaylistSummary, RadioSeed, RoleCategory, TopHit,
};
use crate::lyrics::{LyricLine, Lyrics};
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
        description: None,
        public: false,
    }
}

fn list<T>(items: Vec<T>, offset: u32, total: u32) -> ListPage<T> {
    ListPage {
        items,
        offset,
        total,
        hidden: 0,
        positions: Vec::new(),
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
        // 0024 AC1.
        ListRef::BrowseData("pages/data/cdde61b6?albumId=10581228".into()),
        ListRef::BrowseViewAll("home/pages/DAILY_MIXES/view-all".into()),
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
        // 0011 AC1.
        PageRequest::Mixes,
        PageRequest::Mix("016214541b7a008d226272cf9d0964".into()),
        PageRequest::TrackRadio(145060431),
        PageRequest::ArtistRadio(35361),
        // 0021 AC1.
        PageRequest::Lyrics(62300893),
        // 0024 AC1.
        PageRequest::Browse(BrowseRef::Root),
        PageRequest::Browse(BrowseRef::Home),
        PageRequest::Browse(BrowseRef::Page("pages/genre_hip_hop".into())),
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
            indices: vec![3],
            etag: "\"1759831234567\"".into(),
        },
        LibraryRequest::RemoveFromPlaylist {
            uuid: UUID.into(),
            indices: vec![1, 4, 7],
            etag: "\"1759831234567\"".into(),
        },
        LibraryRequest::CreatePlaylist {
            title: "Late night".into(),
            description: "For the road".into(),
        },
        LibraryRequest::MovePlaylistItem {
            uuid: UUID.into(),
            from: 3,
            to: 1,
            etag: "\"1759831234567\"".into(),
        },
        LibraryRequest::EditPlaylist {
            uuid: UUID.into(),
            title: Some("Late night".into()),
            description: None,
        },
        LibraryRequest::SetPlaylistPublic {
            uuid: UUID.into(),
            public: true,
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
            // 0024 AC1: a browse page, with every kind of section.
            PageData::Browse {
                title: "Hip-Hop".into(),
                sections: vec![
                    BrowseSection {
                        title: "New Tracks".into(),
                        rows: ListItems::Tracks(list(vec![track(8)], 0, 240)),
                        more: BrowseMore::Data("pages/data/6eebf48e".into()),
                    },
                    BrowseSection {
                        title: "Genres".into(),
                        rows: ListItems::Links(list(
                            vec![
                                PageLink {
                                    title: "Home".into(),
                                    target: BrowseRef::Home,
                                },
                                PageLink {
                                    title: "Pop".into(),
                                    target: BrowseRef::Page("pages/genre_pop".into()),
                                },
                            ],
                            0,
                            2,
                        )),
                        more: BrowseMore::None,
                    },
                    BrowseSection {
                        title: "Recently played".into(),
                        rows: ListItems::Mixed(list(
                            vec![
                                MixedRow::Track(track(8)),
                                MixedRow::Album(album(6, AlbumKind::Album)),
                                MixedRow::Playlist(playlist(false)),
                                MixedRow::Artist(artist(11, "Pierce The Veil")),
                                MixedRow::Mix(MixSummary {
                                    id: "016214541b7a008d226272cf9d0964".into(),
                                    title: "My Daily Discovery".into(),
                                    subtitle: None,
                                }),
                            ],
                            0,
                            5,
                        )),
                        more: BrowseMore::ViewAll("home/pages/CONTINUE_LISTEN_TO/view-all".into()),
                    },
                ],
            },
            // 0011 AC1: the mixes, a mix and both radios.
            PageData::Mixes {
                mixes: list(
                    vec![
                        MixSummary {
                            id: "016214541b7a008d226272cf9d0964".into(),
                            title: "My Daily Discovery".into(),
                            subtitle: Some("Songs by new and familiar artists".into()),
                        },
                        MixSummary {
                            id: "00209e91224287130d9eb3ea64016b".into(),
                            title: "My Mix 1".into(),
                            subtitle: None,
                        },
                    ],
                    0,
                    2,
                ),
            },
            PageData::Mix {
                mix: MixSummary {
                    id: "016214541b7a008d226272cf9d0964".into(),
                    title: "My Daily Discovery".into(),
                    subtitle: None,
                },
                tracks: list(vec![track(9)], 0, 1),
            },
            PageData::Radio {
                seed: RadioSeed::Track(track(9)),
                tracks: list(vec![track(9), track(10)], 0, 2),
            },
            PageData::Radio {
                seed: RadioSeed::Artist(artist(12, "The Wonders")),
                tracks: list(vec![], 0, 0),
            },
            // 0021 AC1: lyrics, synced and plain, and none.
            PageData::Lyrics {
                track: 62300893,
                lyrics: Some(Lyrics {
                    synced: vec![
                        LyricLine {
                            at: Duration::from_millis(6_320),
                            text: "We hung the lanterns".into(),
                        },
                        LyricLine {
                            at: Duration::from_millis(115_150),
                            text: String::new(),
                        },
                    ],
                    plain: "We hung the lanterns\n\nOn the wire".into(),
                    right_to_left: true,
                }),
            },
            PageData::Lyrics {
                track: 1558491,
                lyrics: Some(Lyrics {
                    synced: vec![],
                    plain: "Only words".into(),
                    right_to_left: false,
                }),
            },
            PageData::Lyrics {
                track: 1403577,
                lyrics: None,
            },
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
                positions: Vec::new(),
                ..list(vec![credited], 50, 548)
            }),
            // 0024 AC1.
            ListItems::Mixes(list(vec![], 0, 0)),
            ListItems::Links(list(vec![], 0, 0)),
            ListItems::Mixed(list(vec![MixedRow::Track(track(6))], 50, 240)),
        ]
        .into_iter()
        .map(LibraryResponse::Items),
    );
    responses.extend([
        LibraryResponse::Favorite(true),
        LibraryResponse::Favorite(false),
        LibraryResponse::Created(playlist(true)),
        LibraryResponse::Moved {
            etag: Some("\"1\"".into()),
        },
        LibraryResponse::Moved { etag: None },
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
        Command::Play,
        Command::Pause,
        Command::Stop,
        Command::SetShuffle(true),
        Command::SetRepeat(RepeatMode::Track),
        Command::SetPosition {
            entry: EntryId(7),
            position: Duration::from_millis(1500),
        },
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
        // 0014 AC1.
        Command::SetDevice("hw:1,0".into()),
        Command::SetDevice(String::new()),
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
    // 0014 AC1: the device list.
    client.push(ClientMessage::Devices { id: 7 });
    server.extend(device_replies());
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
        device: "hw:1,0".into(),
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

/// Every shape of `DevicesReply` (spec 0014 AC1).
fn device_replies() -> Vec<ServerMessage> {
    vec![
        ServerMessage::DevicesReply {
            id: 7,
            result: Ok(vec![
                DeviceEntry {
                    name: "default".into(),
                    description: "shared, through the system mixer".into(),
                },
                DeviceEntry {
                    name: "hw:1,0".into(),
                    description: "E30 II: USB Audio".into(),
                },
            ]),
        },
        ServerMessage::DevicesReply {
            id: 8,
            result: Ok(vec![]),
        },
        ServerMessage::DevicesReply {
            id: u64::MAX,
            result: Err("cannot read /proc/asound/cards: permission denied".into()),
        },
    ]
}

/// 0014 AC1: `SetDevice`, `Devices`, `DevicesReply` and the snapshot's
/// `device` round-trip through the codec.
#[test]
fn ac1_device_messages_round_trip() {
    for name in ["hw:1,0", "default", "plughw:0,0", ""] {
        round_trip(&Command::SetDevice(name.into()));
        round_trip(&ClientMessage::Request {
            id: 3,
            command: Command::SetDevice(name.into()),
        });
    }
    round_trip(&ClientMessage::Devices { id: 0 });
    round_trip(&ClientMessage::Devices { id: u64::MAX });
    device_replies().iter().for_each(round_trip);
    let snapshot = match &all_events()[3] {
        Event::Player(snapshot) => snapshot.clone(),
        other => panic!("expected a snapshot, got {other:?}"),
    };
    for device in ["hw:1,0", "default", "a custom PCM"] {
        let snapshot = PlayerSnapshot {
            device: device.into(),
            ..snapshot.clone()
        };
        round_trip(&Event::Player(snapshot.clone()));
        round_trip(&ServerMessage::Welcome {
            snapshot,
            login_required: false,
        });
    }
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
