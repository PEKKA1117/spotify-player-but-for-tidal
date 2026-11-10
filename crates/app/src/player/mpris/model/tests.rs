use super::*;
use tidal_player_core::player::{PlayerConfig, PlayerInput, PlayerState, update};
use tidal_player_core::protocol::NowPlaying;
use tidal_player_core::{AlbumRef, ArtistRef, AudioQuality, Item, Track, TrackId};

const COVER: &str = "2e4a5d2d-9a0d-4c3a-a0ba-42b0bd16a6ec";
const S: fn(u64) -> Duration = Duration::from_secs;

fn track(id: u64, duration: Option<u64>) -> Track {
    Track {
        id: TrackId(id),
        title: format!("Title {id}"),
        version: None,
        artists: vec![ArtistRef {
            id: 1,
            name: "A".into(),
        }],
        album: Some(AlbumRef {
            id: 2,
            title: "Album".into(),
            cover: Some(COVER.into()),
        }),
        duration: duration.map(Duration::from_secs),
        streamable: true,
    }
}

fn entry(id: u64, track: Track) -> QueueEntry {
    QueueEntry {
        id: EntryId(id),
        track,
        suggested: false,
    }
}

/// Entries 1, 2, 3 (tracks 11, 12, 13, 200 s), entry `current` current.
fn snap(current: Option<u64>, state: PlaybackState) -> PlayerSnapshot {
    PlayerSnapshot {
        queue: (1..=3)
            .map(|i| entry(i, track(10 + i, Some(200))))
            .collect(),
        current: current.map(EntryId),
        state,
        position: S(10),
        shuffle: false,
        repeat: RepeatMode::Off,
        autoplay: false,
        volume: 100,
        muted: false,
        now_playing: None,
        message: None,
        device: "default".into(),
    }
}

fn empty() -> PlayerSnapshot {
    PlayerSnapshot {
        queue: vec![],
        position: Duration::ZERO,
        ..snap(None, PlaybackState::Stopped)
    }
}

fn now_playing(released: bool) -> NowPlaying {
    NowPlaying {
        quality: AudioQuality::Lossless,
        source: "FLAC".into(),
        output: "hw:0,0".into(),
        bit_perfect: true,
        bit_perfect_reason: None,
        released,
    }
}

fn file_art(cover: &str) -> Option<String> {
    Some(format!("file:///cache/covers/{cover}.jpg"))
}

fn no_art(_: &str) -> Option<String> {
    None
}

fn get<'a>(p: &'a Properties, key: &str) -> &'a Value {
    p.get(key).unwrap_or_else(|| panic!("no {key} in {p:?}"))
}

/// AC3: every property of the `Player` table per snapshot, the `Can*`
/// rules included; the constants in every row.
#[test]
fn ac3_player_properties() {
    struct Row {
        name: &'static str,
        snapshot: PlayerSnapshot,
        status: &'static str,
        loop_status: &'static str,
        shuffle: bool,
        volume: f64,
        position: i64,
        can: [bool; 5],
    }
    let with = |f: &dyn Fn(&mut PlayerSnapshot), base: PlayerSnapshot| {
        let mut s = base;
        f(&mut s);
        s
    };
    let pos = 10_000_000;
    // can: [next, previous, play, pause, seek]
    let rows = [
        Row {
            name: "empty",
            snapshot: empty(),
            status: "Stopped",
            loop_status: "None",
            shuffle: false,
            volume: 1.0,
            position: 0,
            can: [false; 5],
        },
        Row {
            name: "stopped with a current entry",
            snapshot: snap(Some(1), PlaybackState::Stopped),
            status: "Stopped",
            loop_status: "None",
            shuffle: false,
            volume: 1.0,
            position: pos,
            can: [true; 5],
        },
        Row {
            name: "loading",
            snapshot: snap(Some(1), PlaybackState::Loading),
            status: "Playing",
            loop_status: "None",
            shuffle: false,
            volume: 1.0,
            position: pos,
            can: [true; 5],
        },
        Row {
            // The player shows a load held by a pause as `Paused`.
            name: "held load",
            snapshot: snap(Some(1), PlaybackState::Paused),
            status: "Paused",
            loop_status: "None",
            shuffle: false,
            volume: 1.0,
            position: pos,
            can: [true; 5],
        },
        Row {
            name: "playing the last entry, shuffle, repeat queue",
            snapshot: with(
                &|s| {
                    s.shuffle = true;
                    s.repeat = RepeatMode::Queue;
                    s.now_playing = Some(now_playing(false));
                },
                snap(Some(3), PlaybackState::Playing),
            ),
            status: "Playing",
            loop_status: "Playlist",
            shuffle: true,
            volume: 1.0,
            position: pos,
            can: [true; 5],
        },
        Row {
            name: "buffering",
            snapshot: snap(Some(2), PlaybackState::Buffering),
            status: "Playing",
            loop_status: "None",
            shuffle: false,
            volume: 1.0,
            position: pos,
            can: [true; 5],
        },
        Row {
            name: "paused",
            snapshot: with(
                &|s| s.now_playing = Some(now_playing(false)),
                snap(Some(2), PlaybackState::Paused),
            ),
            status: "Paused",
            loop_status: "None",
            shuffle: false,
            volume: 1.0,
            position: pos,
            can: [true; 5],
        },
        Row {
            name: "paused and released",
            snapshot: with(
                &|s| s.now_playing = Some(now_playing(true)),
                snap(Some(2), PlaybackState::Paused),
            ),
            status: "Paused",
            loop_status: "None",
            shuffle: false,
            volume: 1.0,
            position: pos,
            can: [true; 5],
        },
        Row {
            name: "muted at 40 %",
            snapshot: with(
                &|s| {
                    s.volume = 40;
                    s.muted = true;
                },
                snap(Some(1), PlaybackState::Playing),
            ),
            status: "Playing",
            loop_status: "None",
            shuffle: false,
            volume: 0.0,
            position: pos,
            can: [true; 5],
        },
        Row {
            name: "40 %",
            snapshot: with(&|s| s.volume = 40, snap(Some(1), PlaybackState::Playing)),
            status: "Playing",
            loop_status: "None",
            shuffle: false,
            volume: 0.4,
            position: pos,
            can: [true; 5],
        },
        Row {
            name: "last entry, repeat off",
            snapshot: snap(Some(3), PlaybackState::Playing),
            status: "Playing",
            loop_status: "None",
            shuffle: false,
            volume: 1.0,
            position: pos,
            can: [false, true, true, true, true],
        },
        Row {
            name: "last entry, repeat track",
            snapshot: with(
                &|s| s.repeat = RepeatMode::Track,
                snap(Some(3), PlaybackState::Playing),
            ),
            status: "Playing",
            loop_status: "Track",
            shuffle: false,
            volume: 1.0,
            position: pos,
            can: [true; 5],
        },
        Row {
            name: "unknown duration",
            snapshot: with(
                &|s| s.queue[0].track.duration = None,
                snap(Some(1), PlaybackState::Playing),
            ),
            status: "Playing",
            loop_status: "None",
            shuffle: false,
            volume: 1.0,
            position: pos,
            can: [true, true, true, true, false],
        },
    ];
    for row in rows {
        let p = player_properties(&row.snapshot, S(10), &file_art);
        let name = row.name;
        let can = [
            "CanGoNext",
            "CanGoPrevious",
            "CanPlay",
            "CanPause",
            "CanSeek",
        ]
        .map(|k| get(&p, k).clone());
        let want: Vec<(&str, Value)> = vec![
            ("PlaybackStatus", Value::Str(row.status.into())),
            ("LoopStatus", Value::Str(row.loop_status.into())),
            ("Shuffle", Value::Bool(row.shuffle)),
            ("Volume", Value::Double(row.volume)),
            ("Position", Value::Int64(row.position)),
            ("Rate", Value::Double(1.0)),
            ("MinimumRate", Value::Double(1.0)),
            ("MaximumRate", Value::Double(1.0)),
            ("CanControl", Value::Bool(true)),
            (
                "Metadata",
                Value::Metadata(metadata(&row.snapshot, &file_art)),
            ),
        ];
        for (key, value) in want {
            assert_eq!(get(&p, key), &value, "{name}: {key}");
        }
        assert_eq!(can, row.can.map(Value::Bool), "{name}: Can*");
        assert_eq!(p.len(), 15, "{name}: {:?}", p.keys());
    }
}

/// AC4: the metadata of the current entry; `NoTrack` alone without one.
#[test]
fn ac4_metadata() {
    let s = |v: &str| Value::Str(v.into());
    let full = {
        let mut t = track(77, Some(212));
        t.version = Some("Live".into());
        t.artists.push(ArtistRef {
            id: 3,
            name: "B".into(),
        });
        let mut snapshot = snap(Some(5), PlaybackState::Playing);
        snapshot.queue = vec![entry(5, t)];
        snapshot
    };
    let mut no_album = full.clone();
    no_album.queue[0].track.album = None;
    let mut no_cover = full.clone();
    no_cover.queue[0].track.album.as_mut().unwrap().cover = None;
    let mut no_duration = full.clone();
    no_duration.queue[0].track.duration = None;
    let mut twice = full.clone();
    twice.queue.push(entry(6, twice.queue[0].track.clone()));
    twice.current = Some(EntryId(6));

    let base = |id: u64| -> Vec<(&'static str, Value)> {
        vec![
            (
                "mpris:trackid",
                Value::ObjectPath(format!("/tidal_player/entry/{id}")),
            ),
            ("mpris:length", Value::Int64(212_000_000)),
            ("xesam:title", s("Title 77 (Live)")),
            ("xesam:artist", Value::StrList(vec!["A".into(), "B".into()])),
            ("xesam:album", s("Album")),
            ("xesam:url", s("https://tidal.com/browse/track/77")),
            (
                "mpris:artUrl",
                s(&format!("file:///cache/covers/{COVER}.jpg")),
            ),
        ]
    };
    let without = |keys: &[&str], id: u64| {
        base(id)
            .into_iter()
            .filter(|(k, _)| !keys.contains(k))
            .collect::<Vec<_>>()
    };
    type Row = (&'static str, PlayerSnapshot, Vec<(&'static str, Value)>);
    let rows: Vec<Row> = vec![
        (
            "no current entry",
            snap(None, PlaybackState::Stopped),
            vec![("mpris:trackid", Value::ObjectPath(NO_TRACK.into()))],
        ),
        (
            "empty",
            empty(),
            vec![("mpris:trackid", Value::ObjectPath(NO_TRACK.into()))],
        ),
        ("version, artists, album, cover", full.clone(), base(5)),
        (
            "no album",
            no_album,
            without(&["xesam:album", "mpris:artUrl"], 5),
        ),
        ("no cover", no_cover, without(&["mpris:artUrl"], 5)),
        (
            "unknown duration",
            no_duration,
            without(&["mpris:length"], 5),
        ),
        ("the same track queued twice", twice, base(6)),
    ];
    for (name, snapshot, want) in rows {
        let want: Metadata = want.into_iter().map(|(k, v)| (k.to_owned(), v)).collect();
        assert_eq!(metadata(&snapshot, &file_art), want, "{name}");
    }
    // No art yet (the cover is downloading): no `mpris:artUrl`.
    let got = metadata(&full, &no_art);
    assert!(!got.contains_key("mpris:artUrl"), "{got:?}");
    assert_eq!(got.len(), 6);
}

/// The player's snapshot after `commands` on a fresh player with
/// three tracks loaded and playing.
fn played(commands: &[Command]) -> (PlayerSnapshot, PlayerSnapshot) {
    let mut state = PlayerState::new(PlayerConfig::default(), 7);
    update(
        &mut state,
        PlayerInput::Command(Command::LoadQueue {
            tracks: (1..=3).map(|i| track(i, Some(200))).collect(),
            start: 0,
        }),
    );
    let before = state.snapshot();
    for command in commands {
        update(&mut state, PlayerInput::Command(command.clone()));
    }
    (before, state.snapshot())
}

/// AC5: exactly the changed properties, `Position` never.
#[test]
fn ac5_changes() {
    let props = |s: &PlayerSnapshot, pos: u64| player_properties(s, S(pos), &file_art);
    let base = snap(Some(1), PlaybackState::Playing);
    let with = |f: &dyn Fn(&mut PlayerSnapshot)| {
        let mut s = base.clone();
        f(&mut s);
        s
    };
    let keys = |p: Properties| p.into_iter().collect::<Vec<_>>();
    type Row = (
        &'static str,
        PlayerSnapshot,
        u64,
        Vec<(&'static str, Value)>,
    );
    let rows: Vec<Row> = vec![
        ("no change", base.clone(), 10, vec![]),
        ("position alone", with(&|s| s.position = S(50)), 50, vec![]),
        (
            "volume",
            with(&|s| s.volume = 50),
            10,
            vec![("Volume", Value::Double(0.5))],
        ),
        (
            "volume and mute together",
            with(&|s| {
                s.volume = 50;
                s.muted = true;
            }),
            10,
            vec![("Volume", Value::Double(0.0))],
        ),
        (
            "pause",
            with(&|s| s.state = PlaybackState::Paused),
            10,
            vec![("PlaybackStatus", Value::Str("Paused".into()))],
        ),
        (
            "repeat",
            with(&|s| s.repeat = RepeatMode::Track),
            10,
            vec![("LoopStatus", Value::Str("Track".into()))],
        ),
    ];
    for (name, new, pos, want) in rows {
        assert_eq!(
            keys(changes(&props(&base, 10), &props(&new, pos))),
            want,
            "{name}"
        );
    }
    // A track change: `Metadata` and the `Can*` that changed (the last
    // entry has nothing after it).
    let last = with(&|s| s.current = Some(EntryId(3)));
    let got = changes(&props(&base, 10), &props(&last, 0));
    assert_eq!(
        got.keys().copied().collect::<Vec<_>>(),
        vec!["CanGoNext", "Metadata"],
        "track change"
    );
    assert_eq!(got["CanGoNext"], Value::Bool(false));
    assert_eq!(got["Metadata"], Value::Metadata(metadata(&last, &file_art)));

    // Shuffle, from the player's own commands: the toggle, the setter
    // and a client's toggle (the same command) alike.
    for (name, commands) in [
        ("ToggleShuffle", vec![Command::ToggleShuffle]),
        ("SetShuffle", vec![Command::SetShuffle(true)]),
        (
            "a client's toggle after a no-op setter",
            vec![Command::SetShuffle(false), Command::ToggleShuffle],
        ),
    ] {
        let (before, after) = played(&commands);
        let got = changes(&props(&before, 0), &props(&after, 0));
        assert_eq!(keys(got), vec![("Shuffle", Value::Bool(true))], "{name}");
    }
}

/// AC6: when `Seeked` fires.
#[test]
fn ac6_seeked() {
    let e = |id: u64, secs: u64| Some((EntryId(id), S(secs)));
    let ms = Duration::from_millis;
    let rows: [(&str, At, At, Duration, Option<Duration>); 9] = [
        ("forward by elapsed", e(1, 10), e(1, 11), S(1), None),
        (
            "forward within the second",
            e(1, 10),
            Some((EntryId(1), ms(11_900))),
            S(1),
            None,
        ),
        ("backward", e(1, 10), e(1, 5), S(1), Some(S(5))),
        ("back to 0:00", e(1, 10), e(1, 0), S(0), Some(S(0))),
        (
            "forward by elapsed + 2 s",
            e(1, 10),
            e(1, 13),
            S(1),
            Some(S(13)),
        ),
        ("another entry at 0", e(1, 10), e(2, 0), S(0), None),
        (
            "another entry at a restored position",
            e(1, 10),
            e(2, 83),
            S(0),
            Some(S(83)),
        ),
        ("no current entry", e(1, 10), None, S(0), None),
        ("a first entry at 0", None, e(1, 0), S(0), None),
    ];
    for (name, old, new, elapsed, want) in rows {
        assert_eq!(seeked(old, new, elapsed), want, "{name}");
    }
}

/// AC7: `Position` moves on with the clock while playing only, and
/// stops at the duration.
#[test]
fn ac7_position() {
    let t0 = Instant::now();
    type Row = (
        &'static str,
        Option<u64>,
        u64,
        PlaybackState,
        Option<u64>,
        u64,
        u64,
    );
    let rows: [Row; 8] = [
        // name, entry, reported, state, duration, seconds later, want
        (
            "playing",
            Some(1),
            10,
            PlaybackState::Playing,
            Some(200),
            3,
            13,
        ),
        (
            "paused",
            Some(1),
            10,
            PlaybackState::Paused,
            Some(200),
            3,
            10,
        ),
        (
            "buffering",
            Some(1),
            10,
            PlaybackState::Buffering,
            Some(200),
            3,
            10,
        ),
        (
            "loading",
            Some(1),
            10,
            PlaybackState::Loading,
            Some(200),
            3,
            10,
        ),
        (
            "stopped",
            Some(1),
            10,
            PlaybackState::Stopped,
            Some(200),
            3,
            10,
        ),
        (
            "capped at the duration",
            Some(1),
            199,
            PlaybackState::Playing,
            Some(200),
            5,
            200,
        ),
        (
            "unknown duration",
            Some(1),
            199,
            PlaybackState::Playing,
            None,
            5,
            204,
        ),
        (
            "no current entry",
            None,
            10,
            PlaybackState::Stopped,
            None,
            3,
            0,
        ),
    ];
    for (name, entry, reported, state, duration, later, want) in rows {
        let mut clock = PositionClock::new(t0);
        clock.report(entry.map(EntryId), S(reported), state, duration.map(S), t0);
        assert_eq!(clock.position(t0 + S(later)), S(want), "{name}");
    }
    // A new report restarts the extrapolation.
    let mut clock = PositionClock::new(t0);
    clock.report(
        Some(EntryId(1)),
        S(10),
        PlaybackState::Playing,
        Some(S(200)),
        t0,
    );
    clock.report(
        Some(EntryId(1)),
        S(40),
        PlaybackState::Playing,
        Some(S(200)),
        t0 + S(5),
    );
    assert_eq!(clock.position(t0 + S(7)), S(42));
    assert_eq!(clock.elapsed(t0 + S(7)), S(2));
}

/// AC8: each call's command, or its error.
#[test]
fn ac8_command_for() {
    let invalid = |m: &str| Err(CallError::InvalidArgs(m.into()));
    let rows: Vec<(Call, Result<Option<Command>, CallError>)> = vec![
        (Call::PlayPause, Ok(Some(Command::TogglePause))),
        (Call::Play, Ok(Some(Command::Play))),
        (Call::Pause, Ok(Some(Command::Pause))),
        (Call::Stop, Ok(Some(Command::Stop))),
        (Call::Next, Ok(Some(Command::Next))),
        (Call::Previous, Ok(Some(Command::Previous))),
        (Call::Seek(5_000_000), Ok(Some(Command::SeekBy(5000)))),
        (Call::Seek(-1500), Ok(Some(Command::SeekBy(-1)))),
        (Call::Seek(1999), Ok(Some(Command::SeekBy(1)))),
        (
            Call::SetPosition {
                trackid: "/tidal_player/entry/7".into(),
                position: 30_000_000,
            },
            Ok(Some(Command::SetPosition {
                entry: EntryId(7),
                position: S(30),
            })),
        ),
        (
            Call::SetPosition {
                trackid: NO_TRACK.into(),
                position: 0,
            },
            Ok(None),
        ),
        (
            Call::SetPosition {
                trackid: "/org/mpris/MediaPlayer2/Track/7".into(),
                position: 0,
            },
            Ok(None),
        ),
        (
            Call::SetPosition {
                trackid: "/tidal_player/entry/seven".into(),
                position: 0,
            },
            Ok(None),
        ),
        (
            Call::SetPosition {
                trackid: "/tidal_player/entry/7".into(),
                position: -1,
            },
            Ok(None),
        ),
        (
            Call::SetPosition {
                trackid: "not a path".into(),
                position: 0,
            },
            invalid("Not an object path: not a path"),
        ),
        (
            Call::OpenUri("https://tidal.com/browse/album/123".into()),
            Ok(Some(Command::Open {
                items: vec![Item::Album(123)],
                at: None,
            })),
        ),
        (
            Call::OpenUri("tidal://track/5".into()),
            Ok(Some(Command::Open {
                items: vec![Item::Track(TrackId(5))],
                at: None,
            })),
        ),
        (
            Call::OpenUri("https://tidal.com/browse/artist/1".into()),
            invalid("Not a Tidal track, album or playlist: https://tidal.com/browse/artist/1"),
        ),
        (
            Call::OpenUri("garbage".into()),
            invalid("Not a Tidal track, album or playlist: garbage"),
        ),
        (Call::SetShuffle(true), Ok(Some(Command::SetShuffle(true)))),
        (
            Call::SetShuffle(false),
            Ok(Some(Command::SetShuffle(false))),
        ),
        (
            Call::SetLoopStatus("None".into()),
            Ok(Some(Command::SetRepeat(RepeatMode::Off))),
        ),
        (
            Call::SetLoopStatus("Playlist".into()),
            Ok(Some(Command::SetRepeat(RepeatMode::Queue))),
        ),
        (
            Call::SetLoopStatus("Track".into()),
            Ok(Some(Command::SetRepeat(RepeatMode::Track))),
        ),
        (
            Call::SetLoopStatus("Shuffle".into()),
            invalid("LoopStatus must be None, Track or Playlist, got \"Shuffle\""),
        ),
        (Call::SetVolume(0.5), Ok(Some(Command::SetVolume(50)))),
        (Call::SetVolume(-0.5), Ok(Some(Command::SetVolume(0)))),
        (Call::SetVolume(1.7), Ok(Some(Command::SetVolume(100)))),
        (Call::SetVolume(0.333), Ok(Some(Command::SetVolume(33)))),
        (Call::SetVolume(f64::NAN), Ok(Some(Command::SetVolume(0)))),
        (Call::SetRate(2.0), Ok(None)),
    ];
    for (call, want) in rows {
        assert_eq!(command_for(&call), want, "{call:?}");
    }
}

/// Our track IDs round-trip; others are not ours.
#[test]
fn trackid_round_trip() {
    assert_eq!(entry_of(&trackid(EntryId(42))), Some(EntryId(42)));
    assert_eq!(entry_of(NO_TRACK), None);
    assert_eq!(entry_of("/tidal_player/entry/"), None);
}
