//! Spec 0009 AC1–AC3: the remembered playback state. `PlayerState::saved`
//! captures it, `PlayerState::restore` starts a stopped player on it, and the
//! restored player behaves as one that reached the same point by commands.

use std::collections::{BTreeSet, HashMap};

use super::*;

fn ms(d: Duration) -> u64 {
    d.as_millis() as u64
}

fn entry(id: u64, track_id: u64, suggested: bool) -> QueueEntry {
    QueueEntry {
        id: EntryId(id),
        track: track(track_id),
        suggested,
    }
}

/// Entries `1..=n` for tracks `1..=n` (what `LoadQueue` of tracks 1..n gives
/// a fresh player), those in `suggested` marked so.
fn loaded(n: u64, suggested: &[u64]) -> Vec<QueueEntry> {
    (1..=n)
        .map(|id| entry(id, id, suggested.contains(&id)))
        .collect()
}

fn ids(raw: &[u64]) -> Vec<EntryId> {
    raw.iter().copied().map(EntryId).collect()
}

fn snapshot_order(st: &PlayerState) -> Vec<EntryId> {
    st.snapshot().queue.iter().map(|e| e.id).collect()
}

/// Plays `ids[start]`, moves to `at`, and has a transient failure stop it
/// there: a stopped player with a current entry at `at`.
fn stopped_at(st: PlayerState, ids: &[u64], start: usize, at: Duration) -> PlayerState {
    let (mut st2, tag) = playing_with(st, ids, start);
    if !at.is_zero() {
        position(&mut st2, at);
    }
    engine(
        &mut st2,
        EngineEvent::Error {
            tag,
            failure: failure(FailureKind::Transient, "Network error"),
        },
    );
    let s = st2.snapshot();
    assert_eq!(s.state, S::Stopped);
    assert_eq!(s.position, at);
    st2
}

fn autoplay_config() -> PlayerConfig {
    PlayerConfig {
        autoplay: true,
        ..PlayerConfig::default()
    }
}

/// AC1's states: (name, state, the `SavedPlayback` it must give).
fn ac1_rows() -> Vec<(&'static str, PlayerState, SavedPlayback)> {
    let mut rows = Vec::new();

    rows.push(("empty", player(), SavedPlayback::default()));

    let st = stopped_at(player(), &[1, 2, 3], 1, secs(83));
    rows.push((
        "stopped mid-track (with a message)",
        st,
        SavedPlayback {
            entries: loaded(3, &[]),
            play_order: ids(&[1, 2, 3]),
            current: Some(EntryId(2)),
            position_ms: 83_000,
            ..SavedPlayback::default()
        },
    ));

    let (mut st, _) = playing(&[1, 2, 3, 4, 5], 0);
    cmd(&mut st, C::ToggleShuffle);
    position(&mut st, secs(50));
    let order = snapshot_order(&st);
    assert_eq!(order.first(), Some(&EntryId(1)));
    rows.push((
        "playing with shuffle on",
        st,
        SavedPlayback {
            entries: loaded(5, &[]),
            play_order: order,
            current: Some(EntryId(1)),
            position_ms: 50_000,
            shuffle: true,
            ..SavedPlayback::default()
        },
    ));

    let (mut st, _) = playing(&[1, 2, 3], 0);
    cmd(&mut st, C::CycleRepeat);
    position(&mut st, secs(70));
    cmd(&mut st, C::TogglePause);
    assert_eq!(st.snapshot().state, S::Paused);
    rows.push((
        "paused mid-track, repeat queue",
        st,
        SavedPlayback {
            entries: loaded(3, &[]),
            play_order: ids(&[1, 2, 3]),
            current: Some(EntryId(1)),
            position_ms: 70_000,
            repeat: RepeatMode::Queue,
            ..SavedPlayback::default()
        },
    ));

    let mut st = stopped_at(player(), &[1, 2, 3], 1, secs(90));
    let fx = cmd(&mut st, C::TogglePause);
    expect_resolve(&fx, Purpose::Play);
    assert_eq!(st.snapshot().state, S::Loading);
    rows.push((
        "loading with a start position",
        st,
        SavedPlayback {
            entries: loaded(3, &[]),
            play_order: ids(&[1, 2, 3]),
            current: Some(EntryId(2)),
            position_ms: 90_000,
            ..SavedPlayback::default()
        },
    ));

    let (mut st, _) = playing(&[1, 2], 0);
    cmd(&mut st, C::SetVolume(40));
    cmd(&mut st, C::ToggleMute);
    rows.push((
        "muted at 40 %",
        st,
        SavedPlayback {
            entries: loaded(2, &[]),
            play_order: ids(&[1, 2]),
            current: Some(EntryId(1)),
            volume: 40,
            muted: true,
            ..SavedPlayback::default()
        },
    ));

    let (mut st, _) = playing_with(PlayerState::new(autoplay_config(), 1), &[1, 2, 3], 2);
    let fx = position(&mut st, LEN - secs(30));
    let ftag = fx
        .iter()
        .find_map(|e| match e {
            PlayerEffect::FetchSuggestions { tag, .. } => Some(*tag),
            _ => None,
        })
        .expect("autoplay asks for suggestions");
    step(
        &mut st,
        PlayerInput::Suggestions {
            tag: ftag,
            result: Ok(tracks(&[4, 5])),
        },
    );
    rows.push((
        "autoplay entries marked suggested (a preload pending)",
        st,
        SavedPlayback {
            entries: loaded(5, &[4, 5]),
            play_order: ids(&[1, 2, 3, 4, 5]),
            current: Some(EntryId(3)),
            position_ms: ms(LEN - secs(30)),
            autoplay: true,
            ..SavedPlayback::default()
        },
    ));

    rows
}

#[test]
fn ac1_saved_fields() {
    for (name, st, expected) in ac1_rows() {
        let saved = st.saved();
        assert_eq!(saved, expected, "AC1 {name}");
        assert_eq!(saved.version, 1, "AC1 {name}: version");

        // Not remembered: the message, now-playing details, preloads,
        // failures (and the rest of the transient state).
        let mut other = st.clone();
        other.message = Some("something else".into());
        other.now_playing = Some(Started {
            quality: AudioQuality::HiResLossless,
            details: details(),
        });
        other.preload = None;
        other.armed = !other.armed;
        other.failures = 3;
        other.released = !other.released;
        other.suggestions = Suggestions::Idle;
        assert_eq!(other.saved(), saved, "AC1 {name}: transient state");
    }
}

#[test]
fn ac1_saved_round_trip() {
    for (name, st, _) in ac1_rows() {
        let saved = st.saved();
        let json = serde_json::to_string(&saved).expect("serializes");
        let back: SavedPlayback = serde_json::from_str(&json).expect("deserializes");
        assert_eq!(back, saved, "AC1 {name}: JSON round-trip");
    }

    // The file's field names (spec 0009 "Where").
    let (_, st, _) = ac1_rows().pop().expect("rows");
    let value = serde_json::to_value(st.saved()).expect("serializes");
    let keys: BTreeSet<&str> = value
        .as_object()
        .expect("an object")
        .keys()
        .map(String::as_str)
        .collect();
    let expected: BTreeSet<&str> = [
        "version",
        "entries",
        "play_order",
        "current",
        "position_ms",
        "shuffle",
        "repeat",
        "autoplay",
        "volume",
        "muted",
    ]
    .into_iter()
    .collect();
    assert_eq!(keys, expected, "AC1: field names");
    assert_eq!(value["version"], serde_json::json!(1));
    assert_eq!(value["current"], serde_json::json!(3));
    assert_eq!(value["play_order"], serde_json::json!([1, 2, 3, 4, 5]));
    assert_eq!(value["position_ms"], serde_json::json!(ms(LEN - secs(30))));
    assert_eq!(value["repeat"], serde_json::json!("Off"));
    assert_eq!(value["entries"][3]["id"], serde_json::json!(4));
    assert_eq!(value["entries"][3]["suggested"], serde_json::json!(true));
}

// --- AC2 ----------------------------------------------------------------------

/// Entries 3, 5, 7, 9 (tracks 30, 50, 70, 90; 9 suggested), played 7, 3, 9, 5,
/// 7 current at 1:23, shuffle on, repeat queue, autoplay on, 80 % muted.
fn rich() -> SavedPlayback {
    SavedPlayback {
        version: 1,
        entries: vec![
            entry(3, 30, false),
            entry(5, 50, false),
            entry(7, 70, false),
            entry(9, 90, true),
        ],
        play_order: ids(&[7, 3, 9, 5]),
        current: Some(EntryId(7)),
        position_ms: 83_000,
        shuffle: true,
        repeat: RepeatMode::Queue,
        autoplay: true,
        volume: 80,
        muted: true,
    }
}

#[derive(Debug)]
struct Restored {
    /// Entry IDs in play order.
    order: Vec<u64>,
    current: Option<u64>,
    position: Duration,
    shuffle: bool,
    volume: u8,
    /// The ID the next `AddToQueue` gets.
    next_id: u64,
}

#[test]
fn ac2_restore() {
    let as_saved = || Restored {
        order: vec![7, 3, 9, 5],
        current: Some(7),
        position: secs(83),
        shuffle: true,
        volume: 80,
        next_id: 10,
    };
    let original = |current, position| Restored {
        order: vec![3, 5, 7, 9],
        current,
        position,
        shuffle: false,
        volume: 80,
        next_id: 10,
    };
    let no_duration = {
        let mut s = rich();
        s.entries[2].track.duration = None;
        s.position_ms = 500_000;
        s
    };
    let rows: Vec<(&str, PlayerConfig, SavedPlayback, Restored)> = vec![
        ("as saved", PlayerConfig::default(), rich(), as_saved()),
        (
            "the saved autoplay, not the config's",
            autoplay_config(),
            SavedPlayback {
                autoplay: false,
                ..rich()
            },
            Restored {
                order: vec![7, 3, 9, 5],
                current: Some(7),
                position: secs(83),
                shuffle: true,
                volume: 80,
                next_id: 10,
            },
        ),
        (
            "position at the duration → 0:00",
            PlayerConfig::default(),
            SavedPlayback {
                position_ms: ms(LEN),
                ..rich()
            },
            Restored {
                position: Duration::ZERO,
                order: vec![7, 3, 9, 5],
                current: Some(7),
                shuffle: true,
                volume: 80,
                next_id: 10,
            },
        ),
        (
            "position past the duration → 0:00",
            PlayerConfig::default(),
            SavedPlayback {
                position_ms: ms(LEN) + 50_000,
                ..rich()
            },
            Restored {
                position: Duration::ZERO,
                order: vec![7, 3, 9, 5],
                current: Some(7),
                shuffle: true,
                volume: 80,
                next_id: 10,
            },
        ),
        (
            "no known duration: position kept",
            PlayerConfig::default(),
            no_duration,
            Restored {
                position: secs(500),
                order: vec![7, 3, 9, 5],
                current: Some(7),
                shuffle: true,
                volume: 80,
                next_id: 10,
            },
        ),
        (
            "unknown current → none, 0:00",
            PlayerConfig::default(),
            SavedPlayback {
                current: Some(EntryId(42)),
                ..rich()
            },
            Restored {
                current: None,
                position: Duration::ZERO,
                order: vec![7, 3, 9, 5],
                shuffle: true,
                volume: 80,
                next_id: 10,
            },
        ),
        (
            "no current → 0:00",
            PlayerConfig::default(),
            SavedPlayback {
                current: None,
                ..rich()
            },
            Restored {
                current: None,
                position: Duration::ZERO,
                order: vec![7, 3, 9, 5],
                shuffle: true,
                volume: 80,
                next_id: 10,
            },
        ),
        (
            "play order missing an entry → original, shuffle off",
            PlayerConfig::default(),
            SavedPlayback {
                play_order: ids(&[7, 3, 9]),
                ..rich()
            },
            original(Some(7), secs(83)),
        ),
        (
            "play order with an unknown ID → original, shuffle off",
            PlayerConfig::default(),
            SavedPlayback {
                play_order: ids(&[7, 3, 9, 5, 11]),
                ..rich()
            },
            original(Some(7), secs(83)),
        ),
        (
            "play order with a repeated ID → original, shuffle off",
            PlayerConfig::default(),
            SavedPlayback {
                play_order: ids(&[7, 3, 9, 9]),
                ..rich()
            },
            original(Some(7), secs(83)),
        ),
        (
            "volume 150 → 100",
            PlayerConfig::default(),
            SavedPlayback {
                volume: 150,
                ..rich()
            },
            Restored {
                volume: 100,
                order: vec![7, 3, 9, 5],
                current: Some(7),
                position: secs(83),
                shuffle: true,
                next_id: 10,
            },
        ),
        (
            "a repeated entry ID keeps the first",
            PlayerConfig::default(),
            SavedPlayback {
                entries: vec![
                    entry(3, 30, false),
                    entry(5, 50, false),
                    entry(5, 55, false),
                    entry(7, 70, false),
                    entry(9, 90, true),
                ],
                ..rich()
            },
            as_saved(),
        ),
    ];
    for (name, config, saved, want) in rows {
        let mut st = PlayerState::restore(config, 42, saved.clone());
        let s = st.snapshot();
        let want_queue: Vec<QueueEntry> = want
            .order
            .iter()
            .map(|id| {
                saved
                    .entries
                    .iter()
                    .find(|e| e.id == EntryId(*id))
                    .cloned()
                    .unwrap_or_else(|| panic!("{name}: entry {id}"))
            })
            .collect();
        assert_eq!(s.queue, want_queue, "AC2 {name}: queue in play order");
        assert_eq!(s.current, want.current.map(EntryId), "AC2 {name}: current");
        assert_eq!(s.position, want.position, "AC2 {name}: position");
        assert_eq!(s.state, S::Stopped, "AC2 {name}: state");
        assert_eq!(s.shuffle, want.shuffle, "AC2 {name}: shuffle");
        assert_eq!(s.repeat, saved.repeat, "AC2 {name}: repeat");
        assert_eq!(s.autoplay, saved.autoplay, "AC2 {name}: autoplay");
        assert_eq!(s.volume, want.volume, "AC2 {name}: volume");
        assert_eq!(s.muted, saved.muted, "AC2 {name}: muted");
        assert_eq!(s.now_playing, None, "AC2 {name}: now playing");
        assert_eq!(s.message, None, "AC2 {name}: message");

        cmd(
            &mut st,
            C::AddToQueue {
                tracks: tracks(&[99]),
                at: InsertAt::End,
            },
        );
        assert_eq!(
            entry_of(&st, 99),
            EntryId(want.next_id),
            "AC2 {name}: the next entry ID"
        );
    }

    // An empty saved state is a new player.
    let restored = PlayerState::restore(PlayerConfig::default(), 42, SavedPlayback::default());
    let new = PlayerState::new(PlayerConfig::default(), 42);
    assert_eq!(
        format!("{restored:?}"),
        format!("{new:?}"),
        "AC2: empty restore equals new"
    );
}

#[test]
fn ac2_restore_saved_identity() {
    for (name, st, _) in ac1_rows() {
        let saved = st.saved();
        let restored = PlayerState::restore(PlayerConfig::default(), 7, saved.clone());
        assert_eq!(restored.saved(), saved, "AC2 {name}: restore ∘ saved");
        let (a, b) = (restored.snapshot(), st.snapshot());
        assert_eq!(a.queue, b.queue, "AC2 {name}: queue");
        assert_eq!(a.current, b.current, "AC2 {name}: current");
        assert_eq!(a.position, b.position, "AC2 {name}: position");
        assert_eq!(a.state, S::Stopped, "AC2 {name}: stopped");
    }
}

// --- AC3 ----------------------------------------------------------------------

#[test]
fn ac3_play_restored() {
    let saved = SavedPlayback {
        entries: loaded(3, &[]),
        play_order: ids(&[1, 2, 3]),
        current: Some(EntryId(2)),
        position_ms: 83_000,
        ..SavedPlayback::default()
    };
    let mut st = PlayerState::restore(PlayerConfig::default(), 42, saved);
    let fx = cmd(&mut st, C::TogglePause);
    let (entry, track, tag) = expect_resolve(&fx, Purpose::Play);
    assert_eq!((entry, track), (EntryId(2), TrackId(2)), "AC3: resolves 2");
    assert_eq!(st.snapshot().state, S::Loading);
    let fx2 = resolved(&mut st, tag);
    assert!(
        fx2.contains(&PlayerEffect::EnginePlay {
            tag,
            start_at: secs(83)
        }),
        "AC3: plays from the restored position: {fx2:?}"
    );
    let fx3 = started(&mut st, tag);
    for fx in [&fx, &fx2, &fx3] {
        assert!(
            !has(fx, |e| matches!(e, PlayerEffect::EngineSeek(_))),
            "AC3: no seek: {fx:?}"
        );
    }
    let s = st.snapshot();
    assert_eq!(s.state, S::Playing);
    assert_eq!(s.position, secs(83));
}

#[derive(Debug, Clone)]
enum Step {
    Cmd(C),
    /// Resolve the last `Resolve { Play }`.
    Resolve,
    /// The engine starts the last resolved track.
    Start,
    Pos(Duration),
}

/// Runs `steps`, returning each step's effects with tags renamed in order of
/// first appearance (the two players' tag counters differ).
fn run(st: &mut PlayerState, steps: &[Step]) -> Vec<Vec<PlayerEffect>> {
    let mut names: HashMap<u64, u64> = HashMap::new();
    let mut last_play = 0;
    let mut out = Vec::new();
    for s in steps {
        let fx = match s {
            Step::Cmd(c) => cmd(st, c.clone()),
            Step::Resolve => resolved(st, last_play),
            Step::Start => started(st, last_play),
            Step::Pos(p) => position(st, *p),
        };
        if let Some((_, _, tag)) = resolve_of(&fx, Purpose::Play) {
            last_play = tag;
        }
        let mut rename = |tag: u64| {
            let n = names.len() as u64 + 1;
            *names.entry(tag).or_insert(n)
        };
        let fx = fx
            .into_iter()
            .map(|e| match e {
                PlayerEffect::Resolve {
                    entry,
                    track,
                    tag,
                    purpose,
                } => PlayerEffect::Resolve {
                    entry,
                    track,
                    tag: rename(tag),
                    purpose,
                },
                PlayerEffect::EnginePlay { tag, start_at } => PlayerEffect::EnginePlay {
                    tag: rename(tag),
                    start_at,
                },
                PlayerEffect::EnginePreload { tag } => {
                    PlayerEffect::EnginePreload { tag: rename(tag) }
                }
                PlayerEffect::FetchSuggestions { seed, tag } => PlayerEffect::FetchSuggestions {
                    seed,
                    tag: rename(tag),
                },
                other => other,
            })
            .collect();
        out.push(fx);
    }
    out
}

/// (name, commands before the load, start index, stop position, steps)
type Ac3Row = (&'static str, Vec<C>, usize, Duration, Vec<Step>);

#[test]
fn ac3_restored_equals_reached() {
    use Step::{Cmd, Pos, Resolve, Start};
    let rows: Vec<Ac3Row> = vec![
        (
            "play, pause",
            vec![],
            1,
            secs(83),
            vec![
                Cmd(C::TogglePause),
                Resolve,
                Start,
                Pos(secs(84)),
                Cmd(C::TogglePause),
            ],
        ),
        (
            "next",
            vec![],
            1,
            secs(83),
            vec![Cmd(C::Next), Resolve, Start],
        ),
        (
            "previous past the restart threshold restarts",
            vec![],
            1,
            secs(83),
            vec![Cmd(C::Previous), Cmd(C::TogglePause), Resolve, Start],
        ),
        (
            "previous under the restart threshold goes back",
            vec![],
            1,
            secs(2),
            vec![Cmd(C::Previous), Resolve, Start],
        ),
        (
            "previous at the first entry, repeat queue, wraps",
            vec![C::CycleRepeat],
            0,
            secs(1),
            vec![Cmd(C::Previous), Resolve, Start],
        ),
        (
            "shuffle on, then next",
            vec![],
            1,
            secs(83),
            vec![
                Cmd(C::ToggleShuffle),
                Cmd(C::Next),
                Resolve,
                Start,
                Cmd(C::Next),
            ],
        ),
        (
            "shuffled, repeat queue: next, shuffle off, previous",
            vec![C::ToggleShuffle, C::CycleRepeat],
            2,
            secs(50),
            vec![
                Cmd(C::Next),
                Resolve,
                Start,
                Cmd(C::ToggleShuffle),
                Cmd(C::Previous),
                Resolve,
                Start,
            ],
        ),
        (
            "volume and mute, then play",
            vec![C::SetVolume(60), C::ToggleMute],
            0,
            secs(30),
            vec![
                Cmd(C::ChangeVolume(10)),
                Cmd(C::ToggleMute),
                Cmd(C::TogglePause),
                Resolve,
                Start,
            ],
        ),
    ];
    for (name, setup, start, at, steps) in rows {
        let mut reached = player();
        for c in setup {
            cmd(&mut reached, c);
        }
        let mut reached = stopped_at(reached, &[1, 2, 3, 4], start, at);
        // The failure's message is not remembered.
        reached.message = None;
        let mut restored = PlayerState::restore(PlayerConfig::default(), 42, reached.saved());
        assert_eq!(
            restored.snapshot(),
            reached.snapshot(),
            "AC3 {name}: the same point"
        );
        let a = run(&mut restored, &steps);
        let b = run(&mut reached, &steps);
        for (i, (fa, fb)) in a.iter().zip(&b).enumerate() {
            assert_eq!(fa, fb, "AC3 {name}: effects of step {i} ({:?})", steps[i]);
        }
        assert_eq!(restored.snapshot(), reached.snapshot(), "AC3 {name}: end");
    }
}

/// Spec 0014 AC6: the selected device is not remembered. After a
/// `SetDevice`, the saved state is the same as without it, and a player
/// restored from it starts on its configured device.
#[test]
fn ac6_device_not_remembered() {
    let configured = |device: &str| PlayerConfig {
        device: device.into(),
        ..PlayerConfig::default()
    };
    for (start, chosen) in [("default", "hw:1,0"), ("hw:0,0", "default")] {
        let untouched = stopped_at(
            PlayerState::new(configured(start), 42),
            &[1, 2, 3],
            1,
            secs(42),
        );
        let mut switched = untouched.clone();
        cmd(&mut switched, C::SetDevice(chosen.into()));
        assert_eq!(switched.snapshot().device, chosen, "{start}: switched");
        assert_eq!(
            switched.saved(),
            untouched.saved(),
            "{start}: no device saved"
        );
        for config_device in [start, "hw:2,0"] {
            let restored = PlayerState::restore(configured(config_device), 42, switched.saved());
            assert_eq!(
                restored.snapshot().device,
                config_device,
                "{start}: restored on the configured device"
            );
        }
    }
}
