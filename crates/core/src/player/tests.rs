//! Spec 0004 AC1–AC11 and AC26: the player state machine, driven by hand-built
//! inputs. Every step goes through [`step`], which also checks AC11's
//! broadcast rule.

use std::collections::HashSet;
use std::time::Duration;

use super::*;
use crate::protocol::{Command as C, Event, InsertAt, PlaybackState as S, RepeatMode};

const LEN: Duration = Duration::from_secs(200);

fn secs(n: u64) -> Duration {
    Duration::from_secs(n)
}

fn track(id: u64) -> Track {
    Track {
        id: TrackId(id),
        title: format!("Track {id}"),
        artists: vec!["Artist".into()],
        album: Some("Album".into()),
        duration: Some(LEN),
        streamable: true,
    }
}

fn tracks(ids: &[u64]) -> Vec<Track> {
    ids.iter().copied().map(track).collect()
}

fn details() -> TrackDetails {
    TrackDetails {
        source: "FLAC 16-bit 44.1 kHz stereo".into(),
        output: "hw:1,0 (exclusive) S32_LE 44.1 kHz 2 ch".into(),
        bit_perfect: true,
        reason: None,
    }
}

fn failure(kind: FailureKind, message: &str) -> Failure {
    Failure {
        kind,
        message: message.into(),
    }
}

fn player() -> PlayerState {
    PlayerState::new(PlayerConfig::default(), 42)
}

/// Applies one input and checks AC11 on the result: an engine `Position`
/// broadcasts only `Event::Position`; any other input broadcasts exactly one
/// snapshot, last and equal to the new state, iff the state changed.
fn step(st: &mut PlayerState, input: PlayerInput) -> Vec<PlayerEffect> {
    let before = st.snapshot();
    let is_position = matches!(input, PlayerInput::Engine(EngineEvent::Position(_)));
    let fx = update(st, input.clone());
    let after = st.snapshot();
    let snapshots: Vec<usize> = fx
        .iter()
        .enumerate()
        .filter(|(_, e)| matches!(e, PlayerEffect::Broadcast(Event::Player(_))))
        .map(|(i, _)| i)
        .collect();
    if is_position {
        assert!(
            snapshots.is_empty(),
            "AC11: Position sent a snapshot: {fx:?}"
        );
    } else if before == after {
        assert!(
            snapshots.is_empty(),
            "AC11: no change but a snapshot for {input:?}: {fx:?}"
        );
        assert!(
            !fx.iter().any(|e| matches!(e, PlayerEffect::Broadcast(_))),
            "AC11: {input:?} broadcast without a change: {fx:?}"
        );
    } else {
        assert_eq!(
            snapshots,
            vec![fx.len() - 1],
            "AC11: {input:?} must end with exactly one snapshot: {fx:?}"
        );
        assert_eq!(
            fx.last(),
            Some(&PlayerEffect::Broadcast(Event::Player(after))),
            "AC11: the snapshot is the new state"
        );
    }
    fx
}

fn cmd(st: &mut PlayerState, c: C) -> Vec<PlayerEffect> {
    step(st, PlayerInput::Command(c))
}

fn engine(st: &mut PlayerState, e: EngineEvent) -> Vec<PlayerEffect> {
    step(st, PlayerInput::Engine(e))
}

fn position(st: &mut PlayerState, p: Duration) -> Vec<PlayerEffect> {
    engine(st, EngineEvent::Position(p))
}

fn resolved(st: &mut PlayerState, tag: u64) -> Vec<PlayerEffect> {
    step(
        st,
        PlayerInput::Resolved {
            tag,
            result: Ok(AudioQuality::Lossless),
        },
    )
}

fn resolve_failed(st: &mut PlayerState, tag: u64, f: Failure) -> Vec<PlayerEffect> {
    step(
        st,
        PlayerInput::Resolved {
            tag,
            result: Err(f),
        },
    )
}

fn started(st: &mut PlayerState, tag: u64) -> Vec<PlayerEffect> {
    engine(
        st,
        EngineEvent::Started {
            tag,
            details: details(),
        },
    )
}

/// The `Resolve { purpose }` in `fx`, as (entry, track, tag).
fn resolve_of(fx: &[PlayerEffect], purpose: Purpose) -> Option<(EntryId, TrackId, u64)> {
    fx.iter().find_map(|e| match e {
        PlayerEffect::Resolve {
            entry,
            track,
            tag,
            purpose: p,
        } if *p == purpose => Some((*entry, *track, *tag)),
        _ => None,
    })
}

/// The `Resolve { purpose }` that `fx` must contain.
fn expect_resolve(fx: &[PlayerEffect], purpose: Purpose) -> (EntryId, TrackId, u64) {
    let found = resolve_of(fx, purpose);
    assert!(
        found.is_some(),
        "expected a Resolve {{ {purpose:?} }} in {fx:?}"
    );
    found.unwrap_or((EntryId(0), TrackId(0), 0))
}

fn has(fx: &[PlayerEffect], pred: impl Fn(&PlayerEffect) -> bool) -> bool {
    fx.iter().any(pred)
}

fn any_resolve(fx: &[PlayerEffect]) -> bool {
    has(fx, |e| matches!(e, PlayerEffect::Resolve { .. }))
}

fn touches_engine(fx: &[PlayerEffect]) -> bool {
    has(fx, |e| {
        !matches!(
            e,
            PlayerEffect::Broadcast(_)
                | PlayerEffect::Resolve { .. }
                | PlayerEffect::FetchSuggestions { .. }
        )
    })
}

/// Completes the `Resolve { Play }` in `fx`: resolution, then `Started`.
/// Returns the tag. Panics (test failure) if there is none.
fn complete(st: &mut PlayerState, fx: &[PlayerEffect]) -> u64 {
    let (_, _, tag) = expect_resolve(fx, Purpose::Play);
    let fx = resolved(st, tag);
    assert!(
        fx.contains(&PlayerEffect::EnginePlay {
            tag,
            start_at: st.snapshot().position
        }),
        "resolution must play: {fx:?}"
    );
    started(st, tag);
    tag
}

/// A player playing `ids[start]`, with its tag.
fn playing(ids: &[u64], start: usize) -> (PlayerState, u64) {
    playing_with(player(), ids, start)
}

fn playing_with(mut st: PlayerState, ids: &[u64], start: usize) -> (PlayerState, u64) {
    let fx = cmd(
        &mut st,
        C::LoadQueue {
            tracks: tracks(ids),
            start,
        },
    );
    let tag = complete(&mut st, &fx);
    assert_eq!(st.snapshot().state, S::Playing);
    (st, tag)
}

fn current_track(st: &PlayerState) -> Option<u64> {
    let s = st.snapshot();
    let current = s.current?;
    s.queue
        .iter()
        .find(|e| e.id == current)
        .map(|e| e.track.id.0)
}

fn order(st: &PlayerState) -> Vec<u64> {
    st.snapshot().queue.iter().map(|e| e.track.id.0).collect()
}

fn entry_of(st: &PlayerState, track: u64) -> EntryId {
    st.snapshot()
        .queue
        .iter()
        .find(|e| e.track.id.0 == track)
        .map(|e| e.id)
        .unwrap_or_else(|| panic!("track {track} not queued"))
}

// --- AC1 ----------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
enum From {
    Stopped,
    Loading,
    Playing,
    Paused,
}

/// Queue 1, 2, 3 with track 2 current, in the given state.
fn in_state(from: From) -> PlayerState {
    let mut st = player();
    let fx = cmd(
        &mut st,
        C::LoadQueue {
            tracks: tracks(&[1, 2, 3]),
            start: 1,
        },
    );
    if matches!(from, From::Loading) {
        return st;
    }
    if matches!(from, From::Stopped) {
        let (_, _, tag) = expect_resolve(&fx, Purpose::Play);
        resolve_failed(
            &mut st,
            tag,
            failure(FailureKind::Transient, "Network error"),
        );
        assert_eq!(st.snapshot().state, S::Stopped);
        return st;
    }
    complete(&mut st, &fx);
    if matches!(from, From::Paused) {
        cmd(&mut st, C::TogglePause);
        assert_eq!(st.snapshot().state, S::Paused);
    }
    st
}

#[test]
fn ac1_load_and_skip_paths() {
    #[derive(Debug, Clone, Copy)]
    enum Path {
        Load,
        Next,
        Previous,
        PlayEntry,
        AutoAdvance,
    }
    let froms = [From::Stopped, From::Loading, From::Playing, From::Paused];
    // (path, track expected to play)
    let paths = [
        (Path::Load, 5),
        (Path::Next, 3),
        (Path::Previous, 1),
        (Path::PlayEntry, 3),
        (Path::AutoAdvance, 3),
    ];
    for (path, expected) in paths {
        for from in froms {
            if matches!(path, Path::AutoAdvance) && !matches!(from, From::Playing) {
                continue; // a natural end only happens while playing
            }
            let mut st = in_state(from);
            let seen_tags: Vec<u64> = (0..st.next_tag).collect();
            let fx = match path {
                Path::Load => cmd(
                    &mut st,
                    C::LoadQueue {
                        tracks: tracks(&[4, 5, 6]),
                        start: 1,
                    },
                ),
                Path::Next => cmd(&mut st, C::Next),
                Path::Previous => cmd(&mut st, C::Previous),
                Path::PlayEntry => {
                    let id = entry_of(&st, 3);
                    cmd(&mut st, C::PlayEntry(id))
                }
                Path::AutoAdvance => {
                    let tag = st.current_tag().unwrap();
                    engine(&mut st, EngineEvent::TrackEnded { tag })
                }
            };
            let ctx = format!("{path:?} from {from:?}");
            let (entry, track_id, tag) = resolve_of(&fx, Purpose::Play)
                .unwrap_or_else(|| panic!("{ctx}: no Resolve {{ Play }} in {fx:?}"));
            assert_eq!(track_id, TrackId(expected), "{ctx}");
            assert!(!seen_tags.contains(&tag), "{ctx}: tag {tag} is not fresh");
            assert_eq!(st.snapshot().current, Some(entry), "{ctx}");
            assert_eq!(st.snapshot().state, S::Loading, "{ctx}");
            assert_eq!(st.snapshot().position, Duration::ZERO, "{ctx}");
            // Whatever played before is stopped, never waited for.
            if matches!(from, From::Playing | From::Paused) {
                assert_eq!(fx[0], PlayerEffect::EngineStop, "{ctx}: {fx:?}");
            }
            let fx = resolved(&mut st, tag);
            assert_eq!(
                fx,
                vec![PlayerEffect::EnginePlay {
                    tag,
                    start_at: Duration::ZERO
                }],
                "{ctx}"
            );
            started(&mut st, tag);
            assert_eq!(st.snapshot().state, S::Playing, "{ctx}");
            assert_eq!(current_track(&st), Some(expected), "{ctx}");
        }
    }
}

// --- AC2 ----------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
enum Action {
    Next,
    End,
    /// Previous at this position (s) with this restart threshold (s).
    Previous(u64, u64),
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Outcome {
    /// The entry at this play-order index starts playing.
    Plays(usize),
    /// Stopped on the entry at this play-order index, at 0:00.
    Stops(usize),
    /// The current entry restarts at 0:00 (a seek, no new track).
    Restarts,
}

#[test]
fn ac2_next_previous_end() {
    use Action::*;
    use Outcome::*;
    use RepeatMode::{Off, Queue, Track};
    // (repeat, current play-order index of 3, action, outcome)
    let rows = [
        (Off, 0, Next, Plays(1)),
        (Off, 1, Next, Plays(2)),
        (Off, 2, Next, Stops(2)),
        (Queue, 0, Next, Plays(1)),
        (Queue, 2, Next, Plays(0)),
        (Track, 1, Next, Plays(2)),
        (Track, 2, Next, Plays(0)),
        (Off, 0, End, Plays(1)),
        (Off, 1, End, Plays(2)),
        (Off, 2, End, Stops(2)),
        (Queue, 1, End, Plays(2)),
        (Queue, 2, End, Plays(0)),
        (Track, 0, End, Plays(0)),
        (Track, 1, End, Plays(1)),
        (Track, 2, End, Plays(2)),
        // previous at or below the threshold goes back
        (Off, 0, Previous(2, 3), Restarts),
        (Off, 1, Previous(2, 3), Plays(0)),
        (Off, 2, Previous(2, 3), Plays(1)),
        (Off, 1, Previous(3, 3), Plays(0)),
        (Queue, 0, Previous(2, 3), Plays(2)),
        (Queue, 1, Previous(2, 3), Plays(0)),
        (Track, 0, Previous(2, 3), Restarts),
        (Track, 2, Previous(2, 3), Plays(1)),
        // above the threshold it restarts
        (Off, 0, Previous(4, 3), Restarts),
        (Off, 1, Previous(4, 3), Restarts),
        (Queue, 0, Previous(4, 3), Restarts),
        (Track, 2, Previous(4, 3), Restarts),
        // threshold 0: previous always goes back
        (Off, 1, Previous(4, 0), Plays(0)),
        (Off, 2, Previous(100, 0), Plays(1)),
        (Off, 0, Previous(4, 0), Restarts),
        (Queue, 0, Previous(4, 0), Plays(2)),
    ];
    for shuffle in [false, true] {
        for (repeat, at, action, outcome) in rows {
            let ctx = format!("{repeat:?} at {at} {action:?} shuffle={shuffle}");
            let threshold = match action {
                Previous(_, t) => t,
                _ => 3,
            };
            let config = PlayerConfig {
                previous_restart: secs(threshold),
                ..PlayerConfig::default()
            };
            let mut st = PlayerState::new(config, 7);
            if shuffle {
                cmd(&mut st, C::ToggleShuffle);
            }
            while st.snapshot().repeat != repeat {
                cmd(&mut st, C::CycleRepeat);
            }
            let fx = cmd(
                &mut st,
                C::LoadQueue {
                    tracks: tracks(&[1, 2, 3]),
                    start: 0,
                },
            );
            complete(&mut st, &fx);
            let play_order = order(&st);
            let id = entry_of(&st, play_order[at]);
            let fx = cmd(&mut st, C::PlayEntry(id));
            let tag = complete(&mut st, &fx);
            let fx = match action {
                Next => cmd(&mut st, C::Next),
                End => engine(&mut st, EngineEvent::TrackEnded { tag }),
                Previous(pos, _) => {
                    position(&mut st, secs(pos));
                    cmd(&mut st, C::Previous)
                }
            };
            assert_eq!(order(&st), play_order, "{ctx}: the play order is kept");
            match outcome {
                Plays(j) => {
                    let (_, track_id, _) = resolve_of(&fx, Purpose::Play)
                        .unwrap_or_else(|| panic!("{ctx}: nothing plays: {fx:?}"));
                    assert_eq!(track_id.0, play_order[j], "{ctx}");
                    assert_eq!(current_track(&st), Some(play_order[j]), "{ctx}");
                    assert_eq!(st.snapshot().state, S::Loading, "{ctx}");
                }
                Stops(j) => {
                    assert!(!any_resolve(&fx), "{ctx}: {fx:?}");
                    let s = st.snapshot();
                    assert_eq!(current_track(&st), Some(play_order[j]), "{ctx}");
                    assert_eq!(s.state, S::Stopped, "{ctx}");
                    assert_eq!(s.position, Duration::ZERO, "{ctx}");
                    // TogglePause then plays it from the start.
                    let fx = cmd(&mut st, C::TogglePause);
                    let (_, track_id, tag) = resolve_of(&fx, Purpose::Play)
                        .unwrap_or_else(|| panic!("{ctx}: play after stop: {fx:?}"));
                    assert_eq!(track_id.0, play_order[j], "{ctx}");
                    assert_eq!(
                        resolved(&mut st, tag),
                        vec![PlayerEffect::EnginePlay {
                            tag,
                            start_at: Duration::ZERO
                        }],
                        "{ctx}"
                    );
                }
                Restarts => {
                    assert!(!any_resolve(&fx), "{ctx}: {fx:?}");
                    assert!(
                        fx.contains(&PlayerEffect::EngineSeek(Duration::ZERO)),
                        "{ctx}: {fx:?}"
                    );
                    assert_eq!(current_track(&st), Some(play_order[at]), "{ctx}");
                    assert_eq!(st.snapshot().position, Duration::ZERO, "{ctx}");
                    assert_eq!(st.snapshot().state, S::Playing, "{ctx}");
                }
            }
        }
    }
}

// --- AC3 ----------------------------------------------------------------------

#[test]
fn ac3_shuffle() {
    let ids: Vec<u64> = (1..=10).collect();
    let shuffled = |seed: u64| {
        let (mut st, _) = playing_with(PlayerState::new(PlayerConfig::default(), seed), &ids, 3);
        let fx = cmd(&mut st, C::ToggleShuffle);
        assert!(!touches_engine(&fx), "seed {seed}: {fx:?}");
        assert!(!any_resolve(&fx), "seed {seed}: {fx:?}");
        let s = st.snapshot();
        assert!(s.shuffle);
        assert_eq!(s.state, S::Playing, "the current track keeps playing");
        assert_eq!(current_track(&st), Some(4));
        assert_eq!(fx, vec![PlayerEffect::Broadcast(Event::Player(s))]);
        let o = order(&st);
        (st, o)
    };

    let (mut st, a) = shuffled(1);
    assert_eq!(a[0], 4, "the current entry comes first");
    let mut sorted = a.clone();
    sorted.sort_unstable();
    assert_eq!(sorted, ids, "every entry exactly once");
    assert_ne!(a, ids, "the order is shuffled");
    assert_eq!(shuffled(1).1, a, "same seed, same order");
    assert_ne!(shuffled(2).1, a, "another seed, another order");

    // Off: the original order, current unchanged, engine untouched.
    let fx = cmd(&mut st, C::ToggleShuffle);
    assert!(!touches_engine(&fx), "{fx:?}");
    assert_eq!(order(&st), ids);
    assert_eq!(current_track(&st), Some(4));
    assert_eq!(st.snapshot().state, S::Playing);

    // Load with shuffle on starts at the chosen entry, first in play order.
    let mut st = PlayerState::new(PlayerConfig::default(), 1);
    cmd(&mut st, C::ToggleShuffle);
    let fx = cmd(
        &mut st,
        C::LoadQueue {
            tracks: tracks(&ids),
            start: 5,
        },
    );
    assert_eq!(
        resolve_of(&fx, Purpose::Play).map(|r| r.1),
        Some(TrackId(6))
    );
    let o = order(&st);
    assert_eq!(o[0], 6);
    assert_ne!(o, ids);
    let mut sorted = o.clone();
    sorted.sort_unstable();
    assert_eq!(sorted, ids);
}

// --- AC4 ----------------------------------------------------------------------

#[test]
fn ac4_repeat_cycle() {
    let mut st = player();
    assert_eq!(st.snapshot().repeat, RepeatMode::Off);
    for expected in [
        RepeatMode::Queue,
        RepeatMode::Track,
        RepeatMode::Off,
        RepeatMode::Queue,
    ] {
        let fx = cmd(&mut st, C::CycleRepeat);
        match fx.last() {
            Some(PlayerEffect::Broadcast(Event::Player(s))) => assert_eq!(s.repeat, expected),
            other => panic!("expected a snapshot with {expected:?}, got {other:?}"),
        }
    }
}

// --- AC5 ----------------------------------------------------------------------

/// Plays `ids[at]` and brings it to its preload point with a sent preload.
/// Returns the preload's tag and next track.
fn preloaded(ids: &[u64], at: usize) -> (PlayerState, u64, u64) {
    let (mut st, _) = playing(ids, at);
    let fx = position(&mut st, LEN - secs(30));
    let (_, next, tag) = resolve_of(&fx, Purpose::Preload).expect("preload");
    assert_eq!(
        resolved(&mut st, tag),
        vec![PlayerEffect::EnginePreload { tag }]
    );
    (st, tag, next.0)
}

#[test]
fn ac5_preload() {
    // 31 s left: nothing; 30 s left: Resolve { Preload } for the next entry, once.
    let (mut st, tag) = playing(&[1, 2, 3], 0);
    let fx = position(&mut st, LEN - secs(31));
    assert!(!any_resolve(&fx), "{fx:?}");
    let fx = position(&mut st, LEN - secs(30));
    let (entry, next, ptag) = resolve_of(&fx, Purpose::Preload).expect("preload at 30 s left");
    assert_eq!(next, TrackId(2));
    assert_ne!(ptag, tag);
    let fx = position(&mut st, LEN - secs(29));
    assert!(!any_resolve(&fx), "only once: {fx:?}");
    assert_eq!(
        resolved(&mut st, ptag),
        vec![PlayerEffect::EnginePreload { tag: ptag }]
    );
    // Transitioned makes it current without any EnginePlay.
    let fx = engine(
        &mut st,
        EngineEvent::Transitioned {
            tag: ptag,
            details: details(),
        },
    );
    assert!(
        !has(&fx, |e| matches!(e, PlayerEffect::EnginePlay { .. })),
        "{fx:?}"
    );
    assert_eq!(st.snapshot().current, Some(entry));
    assert_eq!(st.snapshot().state, S::Playing);
    assert_eq!(st.snapshot().position, Duration::ZERO);
    assert!(st.snapshot().now_playing.is_some());
    // The transitioned track preloads the one after it in turn.
    let fx = position(&mut st, LEN - secs(10));
    assert_eq!(
        resolve_of(&fx, Purpose::Preload).map(|r| r.1),
        Some(TrackId(3))
    );

    // Unknown duration: the preload starts at Started.
    let mut st = player();
    let mut first = track(1);
    first.duration = None;
    let fx = cmd(
        &mut st,
        C::LoadQueue {
            tracks: vec![first, track(2)],
            start: 0,
        },
    );
    let (_, _, tag) = expect_resolve(&fx, Purpose::Play);
    resolved(&mut st, tag);
    let fx = started(&mut st, tag);
    assert_eq!(
        resolve_of(&fx, Purpose::Preload).map(|r| r.1),
        Some(TrackId(2))
    );

    // No next entry: no preload.
    let (mut st, _) = playing(&[1, 2], 1);
    let fx = position(&mut st, LEN - secs(10));
    assert!(!any_resolve(&fx), "{fx:?}");
    assert!(!touches_engine(&fx), "{fx:?}");

    // The next entry changed by an edit or a mode: cancel, and resolve the new
    // one; or cancel only when there is none any more.
    type Edit = fn(&mut PlayerState) -> Vec<PlayerEffect>;
    let remove_next: Edit = |st| {
        let id = entry_of(st, 2);
        cmd(st, C::RemoveFromQueue(id))
    };
    let add_next: Edit = |st| {
        cmd(
            st,
            C::AddToQueue {
                tracks: tracks(&[9]),
                at: InsertAt::Next,
            },
        )
    };
    let clear: Edit = |st| cmd(st, C::ClearQueue);
    let repeat: Edit = |st| cmd(st, C::CycleRepeat);
    let shuffle: Edit = |st| cmd(st, C::ToggleShuffle);
    let add_end: Edit = |st| {
        cmd(
            st,
            C::AddToQueue {
                tracks: tracks(&[9]),
                at: InsertAt::End,
            },
        )
    };
    let remove_other: Edit = |st| {
        let id = entry_of(st, 4);
        cmd(st, C::RemoveFromQueue(id))
    };
    let ten: &[u64] = &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
    // (name, queue, current index, repeat cycles before playing, edit, the new
    // next track: Some(Some(t)) resolve t (0: whatever follows the current
    // entry now); Some(None) cancel only; None unchanged)
    let rows: [(&str, &[u64], usize, usize, Edit, Option<Option<u64>>); 11] = [
        ("remove next", &[1, 2, 3], 0, 0, remove_next, Some(Some(3))),
        (
            "remove the only next",
            &[1, 2],
            0,
            0,
            remove_next,
            Some(None),
        ),
        ("add next", &[1, 2, 3], 0, 0, add_next, Some(Some(9))),
        ("clear", &[1, 2, 3], 0, 0, clear, Some(None)),
        (
            "repeat off→queue at the end",
            &[1, 2, 3],
            2,
            0,
            repeat,
            Some(Some(1)),
        ),
        (
            "repeat queue→track",
            &[1, 2, 3],
            0,
            1,
            repeat,
            Some(Some(1)),
        ),
        (
            "repeat track→off at the end",
            &[1, 2, 3],
            2,
            2,
            repeat,
            Some(None),
        ),
        ("shuffle", ten, 0, 0, shuffle, Some(Some(0))),
        ("add at the end", &[1, 2, 3, 4], 0, 0, add_end, None),
        ("remove another", &[1, 2, 3, 4], 0, 0, remove_other, None),
        ("repeat off→queue mid-queue", &[1, 2, 3], 0, 0, repeat, None),
    ];
    for (name, ids, at, cycles, edit, expected) in rows {
        let mut st = player();
        for _ in 0..cycles {
            cmd(&mut st, C::CycleRepeat);
        }
        let (mut st, _) = playing_with(st, ids, at);
        let fx = position(&mut st, LEN - secs(30));
        if let Some((_, _, tag)) = resolve_of(&fx, Purpose::Preload) {
            assert_eq!(
                resolved(&mut st, tag),
                vec![PlayerEffect::EnginePreload { tag }]
            );
        }
        let sent = st
            .preload
            .as_ref()
            .is_some_and(|p| matches!(p.stage, PreloadStage::Sent(_)));
        let fx = edit(&mut st);
        let cancel = fx.contains(&PlayerEffect::EngineCancelPreload);
        match expected {
            None => {
                assert!(!any_resolve(&fx), "{name}: {fx:?}");
                assert!(!cancel, "{name}: {fx:?}");
                assert!(!touches_engine(&fx), "{name}: {fx:?}");
            }
            Some(None) => {
                assert!(sent, "{name}: set-up");
                assert!(cancel, "{name}: {fx:?}");
                assert!(!any_resolve(&fx), "{name}: {fx:?}");
            }
            Some(Some(t)) => {
                let t = if t == 0 { order(&st)[1] } else { t };
                let (_, next, tag) = resolve_of(&fx, Purpose::Preload)
                    .unwrap_or_else(|| panic!("{name}: no new preload: {fx:?}"));
                assert_eq!(next.0, t, "{name}");
                assert_eq!(cancel, sent, "{name}: cancel a sent preload: {fx:?}");
                if sent {
                    let c = fx
                        .iter()
                        .position(|e| *e == PlayerEffect::EngineCancelPreload);
                    let r = fx
                        .iter()
                        .position(|e| matches!(e, PlayerEffect::Resolve { .. }));
                    assert!(c < r, "{name}: cancel before the new resolve: {fx:?}");
                }
                assert_eq!(
                    resolved(&mut st, tag),
                    vec![PlayerEffect::EnginePreload { tag }],
                    "{name}"
                );
            }
        }
    }

    // A preload whose next entry was removed never plays: its Transitioned is
    // stale, and the track end plays the new next entry.
    let (mut st, ptag, _) = preloaded(&[1, 2, 3], 0);
    let id = entry_of(&st, 2);
    cmd(&mut st, C::RemoveFromQueue(id));
    let fx = engine(
        &mut st,
        EngineEvent::Transitioned {
            tag: ptag,
            details: details(),
        },
    );
    assert!(fx.is_empty(), "{fx:?}");
    assert_eq!(current_track(&st), Some(1));
}

// --- AC6 ----------------------------------------------------------------------

#[test]
fn ac6_stale_inputs_ignored() {
    // Playing track 1 (tag T) with a pending preload of track 2 (tag P).
    let setup = || {
        let (mut st, tag) = playing(&[1, 2, 3], 0);
        let fx = position(&mut st, LEN - secs(20));
        let (_, _, ptag) = expect_resolve(&fx, Purpose::Preload);
        (st, tag, ptag)
    };
    let (_, tag, ptag) = setup();
    let stale_tags = [tag - 1, ptag + 1, 999];
    let inputs = |stale: u64| {
        vec![
            PlayerInput::Resolved {
                tag: stale,
                result: Ok(AudioQuality::HiResLossless),
            },
            PlayerInput::Resolved {
                tag: stale,
                result: Err(failure(FailureKind::TrackOnly, "x")),
            },
            PlayerInput::Engine(EngineEvent::Started {
                tag: stale,
                details: details(),
            }),
            PlayerInput::Engine(EngineEvent::Transitioned {
                tag: stale,
                details: details(),
            }),
            PlayerInput::Engine(EngineEvent::TrackEnded { tag: stale }),
            PlayerInput::Engine(EngineEvent::Error {
                tag: stale,
                failure: failure(FailureKind::Transient, "x"),
            }),
            PlayerInput::Suggestions {
                tag: stale,
                result: Ok(tracks(&[7])),
            },
        ]
    };
    for stale in stale_tags {
        for input in inputs(stale) {
            let (mut st, _, _) = setup();
            let before = st.snapshot();
            let fx = step(&mut st, input.clone());
            assert!(fx.is_empty(), "{input:?}: {fx:?}");
            assert_eq!(st.snapshot(), before, "{input:?}");
        }
    }

    // tidalt bug 1: Next, then the replaced track's TrackEnded: one advance.
    let (mut st, old) = playing(&[1, 2, 3], 0);
    let fx = cmd(&mut st, C::Next);
    let new = complete(&mut st, &fx);
    let fx = engine(&mut st, EngineEvent::TrackEnded { tag: old });
    assert!(fx.is_empty(), "{fx:?}");
    assert_eq!(current_track(&st), Some(2));
    // The new track's end advances once.
    let fx = engine(&mut st, EngineEvent::TrackEnded { tag: new });
    assert_eq!(
        resolve_of(&fx, Purpose::Play).map(|r| r.1),
        Some(TrackId(3))
    );

    // A Next mash while loading: earlier resolutions are dropped.
    let (mut st, _) = playing(&[1, 2, 3, 4], 0);
    let a = expect_resolve(&cmd(&mut st, C::Next), Purpose::Play).2;
    let b = expect_resolve(&cmd(&mut st, C::Next), Purpose::Play).2;
    assert!(resolved(&mut st, a).is_empty(), "stale resolution");
    assert_eq!(
        resolved(&mut st, b),
        vec![PlayerEffect::EnginePlay {
            tag: b,
            start_at: Duration::ZERO
        }]
    );
    assert_eq!(current_track(&st), Some(3));
}

// --- AC7 ----------------------------------------------------------------------

#[test]
fn ac7_failure_policy() {
    #[derive(Debug, Clone, Copy)]
    enum Source {
        Resolve,
        Engine,
    }
    use FailureKind::*;
    // (kind, source, skips)
    let rows = [
        (TrackOnly, Source::Resolve, true),
        (TrackOnly, Source::Engine, true),
        (Transient, Source::Resolve, false),
        (Transient, Source::Engine, false),
        (Output, Source::Resolve, false),
        (Output, Source::Engine, false),
        (Session, Source::Resolve, false),
        (Session, Source::Engine, false),
    ];
    for repeat_track in [false, true] {
        for (kind, source, skips) in rows {
            let ctx = format!("{kind:?} from {source:?}, repeat track {repeat_track}");
            let mut st = player();
            if repeat_track {
                cmd(&mut st, C::CycleRepeat);
                cmd(&mut st, C::CycleRepeat);
            }
            let message = format!("{kind:?} failure");
            let fx = cmd(
                &mut st,
                C::LoadQueue {
                    tracks: tracks(&[1, 2, 3]),
                    start: 0,
                },
            );
            let (_, _, tag) = expect_resolve(&fx, Purpose::Play);
            let fx = match source {
                Source::Resolve => resolve_failed(&mut st, tag, failure(kind, &message)),
                Source::Engine => {
                    resolved(&mut st, tag);
                    started(&mut st, tag);
                    position(&mut st, secs(50));
                    engine(
                        &mut st,
                        EngineEvent::Error {
                            tag,
                            failure: failure(kind, &message),
                        },
                    )
                }
            };
            let s = st.snapshot();
            assert_eq!(s.message.as_deref(), Some(message.as_str()), "{ctx}");
            assert!(
                !has(&fx, |e| matches!(e, PlayerEffect::EnginePlay { .. })),
                "{ctx}"
            );
            if skips {
                // repeat track moves on too
                assert_eq!(
                    resolve_of(&fx, Purpose::Play).map(|r| r.1),
                    Some(TrackId(2)),
                    "{ctx}: {fx:?}"
                );
                assert_eq!(s.state, S::Loading, "{ctx}");
            } else {
                assert!(!any_resolve(&fx), "{ctx}: never skips: {fx:?}");
                assert_eq!(s.state, S::Stopped, "{ctx}");
                assert_eq!(current_track(&st), Some(1), "{ctx}");
                let reached = match source {
                    Source::Resolve => Duration::ZERO,
                    Source::Engine => secs(50),
                };
                assert_eq!(s.position, reached, "{ctx}");
                // Play/pause retries the same entry from there.
                let fx = cmd(&mut st, C::TogglePause);
                let (_, t, tag) = resolve_of(&fx, Purpose::Play).expect("retry");
                assert_eq!(t, TrackId(1), "{ctx}");
                assert_eq!(
                    resolved(&mut st, tag),
                    vec![PlayerEffect::EnginePlay {
                        tag,
                        start_at: reached
                    }],
                    "{ctx}"
                );
                started(&mut st, tag);
                assert_eq!(st.snapshot().message, None, "{ctx}: cleared at the start");
            }
        }
    }

    // Not streamable: a track-only failure with no Resolve for it.
    let mut st = PlayerState::new(
        PlayerConfig {
            country: Some("DE".into()),
            ..PlayerConfig::default()
        },
        1,
    );
    let mut ts = tracks(&[1, 2, 3]);
    ts[1].streamable = false;
    let fx = cmd(
        &mut st,
        C::LoadQueue {
            tracks: ts,
            start: 0,
        },
    );
    complete(&mut st, &fx);
    let fx = cmd(&mut st, C::Next);
    let resolves: Vec<_> = fx
        .iter()
        .filter_map(|e| match e {
            PlayerEffect::Resolve { track, .. } => Some(track.0),
            _ => None,
        })
        .collect();
    assert_eq!(resolves, vec![3], "{fx:?}");
    assert_eq!(
        st.snapshot().message.as_deref(),
        Some("Track 2 is not available in DE")
    );

    // An Error is never handled as TrackEnded, nor the other way round.
    // Repeat track: TrackEnded replays the entry, a track-only Error moves on;
    // a transient Error stops where TrackEnded would advance.
    let repeat_track = || {
        let mut st = player();
        cmd(&mut st, C::CycleRepeat);
        cmd(&mut st, C::CycleRepeat);
        playing_with(st, &[1, 2, 3], 0)
    };
    let (mut st, tag) = repeat_track();
    let fx = engine(&mut st, EngineEvent::TrackEnded { tag });
    assert_eq!(
        resolve_of(&fx, Purpose::Play).map(|r| r.1),
        Some(TrackId(1))
    );
    assert_eq!(st.snapshot().message, None);
    let (mut st, tag) = repeat_track();
    let fx = engine(
        &mut st,
        EngineEvent::Error {
            tag,
            failure: failure(FailureKind::TrackOnly, "Decode error"),
        },
    );
    assert_eq!(
        resolve_of(&fx, Purpose::Play).map(|r| r.1),
        Some(TrackId(2))
    );
    let (mut st, tag) = playing(&[1, 2, 3], 0);
    let fx = engine(
        &mut st,
        EngineEvent::Error {
            tag,
            failure: failure(FailureKind::Transient, "Network error"),
        },
    );
    assert!(!any_resolve(&fx));
    assert_eq!(st.snapshot().state, S::Stopped);
}

#[test]
fn ac7_failure_run_stops() {
    let not_found = |n: u64| failure(FailureKind::TrackOnly, &format!("Track {n} was not found"));
    // (queue length, repeat cycles, failures before the stop)
    let rows = [(20, 0, 5), (3, 0, 3), (3, 1, 3), (20, 2, 5), (1, 1, 1)];
    for (len, cycles, limit) in rows {
        let ctx = format!("{len} entries, {cycles} repeat cycles");
        let mut st = player();
        for _ in 0..cycles {
            cmd(&mut st, C::CycleRepeat);
        }
        let ids: Vec<u64> = (1..=len).collect();
        let mut fx = cmd(
            &mut st,
            C::LoadQueue {
                tracks: tracks(&ids),
                start: 0,
            },
        );
        let mut failed = Vec::new();
        while let Some((_, t, tag)) = resolve_of(&fx, Purpose::Play) {
            assert!(failed.len() < 50, "{ctx}: endless skip loop");
            failed.push(t.0);
            fx = resolve_failed(&mut st, tag, not_found(t.0));
        }
        assert_eq!(failed.len(), limit, "{ctx}: {failed:?}");
        let s = st.snapshot();
        assert_eq!(s.state, S::Stopped, "{ctx}");
        assert_eq!(
            current_track(&st),
            failed.last().copied(),
            "{ctx}: on the failing entry"
        );
        assert_eq!(
            s.message,
            Some(format!(
                "Stopped: {limit} tracks in a row could not be played"
            )),
            "{ctx}"
        );
    }

    // A Started resets the count: 4 failures, a success, 4 more: still going.
    let ids: Vec<u64> = (1..=20).collect();
    let mut st = player();
    let mut fx = cmd(
        &mut st,
        C::LoadQueue {
            tracks: tracks(&ids),
            start: 0,
        },
    );
    for round in 0..2 {
        for _ in 0..4 {
            let (_, t, tag) = resolve_of(&fx, Purpose::Play).expect("keeps skipping");
            fx = resolve_failed(&mut st, tag, not_found(t.0));
        }
        if round == 0 {
            let tag = complete(&mut st, &fx);
            fx = engine(&mut st, EngineEvent::TrackEnded { tag });
        }
    }
    assert_eq!(
        st.snapshot().state,
        S::Loading,
        "the run was reset by Started"
    );
    assert_eq!(current_track(&st), Some(10));
}

// --- AC8 ----------------------------------------------------------------------

#[test]
fn ac8_pause_and_seek() {
    use PlayerEffect::*;
    // Playing ⇄ Paused.
    let (mut st, _) = playing(&[1, 2], 0);
    let fx = cmd(&mut st, C::TogglePause);
    assert_eq!(fx[..1], [EnginePause]);
    assert_eq!(st.snapshot().state, S::Paused);
    let fx = cmd(&mut st, C::TogglePause);
    assert_eq!(fx[..1], [EngineResume]);
    assert_eq!(st.snapshot().state, S::Playing);

    // Toggled while loading: no EnginePlay until toggled again.
    let mut st = player();
    let fx = cmd(
        &mut st,
        C::LoadQueue {
            tracks: tracks(&[1]),
            start: 0,
        },
    );
    let (_, _, tag) = expect_resolve(&fx, Purpose::Play);
    let fx = cmd(&mut st, C::TogglePause);
    assert!(!touches_engine(&fx), "{fx:?}");
    assert_eq!(st.snapshot().state, S::Paused);
    let fx = resolved(&mut st, tag);
    assert!(fx.is_empty(), "held: {fx:?}");
    let fx = cmd(&mut st, C::TogglePause);
    assert_eq!(
        fx[..1],
        [EnginePlay {
            tag,
            start_at: Duration::ZERO
        }]
    );
    started(&mut st, tag);
    assert_eq!(st.snapshot().state, S::Playing);

    // Toggled twice while loading: plays as soon as resolved.
    let mut st = player();
    let fx = cmd(
        &mut st,
        C::LoadQueue {
            tracks: tracks(&[1]),
            start: 0,
        },
    );
    let (_, _, tag) = expect_resolve(&fx, Purpose::Play);
    cmd(&mut st, C::TogglePause);
    cmd(&mut st, C::TogglePause);
    assert_eq!(
        resolved(&mut st, tag),
        vec![EnginePlay {
            tag,
            start_at: Duration::ZERO
        }]
    );

    // Seeks while playing or paused: (start position s, command, seek target).
    let rows = [
        (3, C::SeekBy(-5000), Some(Duration::ZERO)),
        (3, C::SeekBy(5000), Some(secs(8))),
        (10, C::SeekBy(-2500), Some(Duration::from_millis(7500))),
        (10, C::SeekTo(secs(42)), Some(secs(42))),
        (0, C::SeekBy(-5000), None), // already at 0:00
    ];
    for paused in [false, true] {
        for (at, c, target) in rows.clone() {
            let ctx = format!("{c:?} at {at}s paused={paused}");
            let (mut st, _) = playing(&[1, 2], 0);
            position(&mut st, secs(at));
            if paused {
                cmd(&mut st, C::TogglePause);
            }
            let fx = cmd(&mut st, c);
            match target {
                Some(t) => {
                    assert_eq!(fx[..1], [EngineSeek(t)], "{ctx}");
                    assert_eq!(st.snapshot().position, t, "{ctx}");
                }
                None => assert!(fx.is_empty(), "{ctx}: {fx:?}"),
            }
            let state = if paused { S::Paused } else { S::Playing };
            assert_eq!(st.snapshot().state, state, "{ctx}");
        }
    }

    // Seeks while loading change the coming start_at.
    let mut st = player();
    let fx = cmd(
        &mut st,
        C::LoadQueue {
            tracks: tracks(&[1]),
            start: 0,
        },
    );
    let (_, _, tag) = expect_resolve(&fx, Purpose::Play);
    let fx = cmd(&mut st, C::SeekBy(5000));
    assert!(!touches_engine(&fx), "{fx:?}");
    cmd(&mut st, C::SeekTo(secs(20)));
    cmd(&mut st, C::SeekBy(-30_000));
    cmd(&mut st, C::SeekBy(12_000));
    assert_eq!(st.snapshot().position, secs(12));
    assert_eq!(
        resolved(&mut st, tag),
        vec![EnginePlay {
            tag,
            start_at: secs(12)
        }]
    );

    // Stopped: seeks emit nothing.
    let mut st = in_state(From::Stopped);
    for c in [C::SeekBy(5000), C::SeekBy(-5000), C::SeekTo(secs(9))] {
        assert!(cmd(&mut st, c.clone()).is_empty(), "{c:?}");
    }
    let mut st = player();
    assert!(cmd(&mut st, C::SeekBy(5000)).is_empty());

    // A seek past the end lets the engine end the track; repeat track replays it.
    let mut st = player();
    cmd(&mut st, C::CycleRepeat);
    cmd(&mut st, C::CycleRepeat);
    let (mut st, tag) = playing_with(st, &[1, 2], 0);
    let fx = cmd(&mut st, C::SeekTo(LEN + secs(5)));
    assert_eq!(fx[..1], [EngineSeek(LEN + secs(5))]);
    let fx = engine(&mut st, EngineEvent::TrackEnded { tag });
    assert_eq!(
        resolve_of(&fx, Purpose::Play).map(|r| r.1),
        Some(TrackId(1))
    );
}

// --- AC9 ----------------------------------------------------------------------

fn gain(volume: u8) -> f32 {
    (f64::from(volume) / 100.0).powi(3) as f32
}

#[test]
fn ac9_volume() {
    let st = player();
    assert_eq!((st.snapshot().volume, st.snapshot().muted), (100, false));

    // (start volume, command, new volume, gain effect)
    let rows = [
        (100, C::ChangeVolume(-1), 99, Some(gain(99))),
        (100, C::ChangeVolume(-5), 95, Some(gain(95))),
        (100, C::ChangeVolume(-25), 75, Some(gain(75))),
        (100, C::ChangeVolume(1), 100, None),
        (100, C::ChangeVolume(5), 100, None),
        (100, C::ChangeVolume(25), 100, None),
        (98, C::ChangeVolume(5), 100, Some(1.0)),
        (90, C::ChangeVolume(25), 100, Some(1.0)),
        (3, C::ChangeVolume(-5), 0, Some(0.0)),
        (10, C::ChangeVolume(-25), 0, Some(0.0)),
        (1, C::ChangeVolume(-1), 0, Some(0.0)),
        (0, C::ChangeVolume(-5), 0, None),
        (0, C::ChangeVolume(1), 1, Some(gain(1))),
        (100, C::SetVolume(50), 50, Some(0.125)),
        (50, C::SetVolume(100), 100, Some(1.0)),
        (40, C::SetVolume(250), 100, Some(1.0)),
        (40, C::SetVolume(40), 40, None),
    ];
    for (from, c, volume, effect) in rows {
        let ctx = format!("{c:?} from {from}");
        let mut st = player();
        cmd(&mut st, C::SetVolume(from));
        let fx = cmd(&mut st, c);
        assert_eq!(st.snapshot().volume, volume, "{ctx}");
        match effect {
            Some(g) => assert_eq!(fx[..1], [PlayerEffect::EngineSetGain(g)], "{ctx}"),
            None => assert!(fx.is_empty(), "{ctx}: {fx:?}"),
        }
    }

    // Mute twice restores the previous gain; a volume change unmutes.
    let mut st = player();
    cmd(&mut st, C::SetVolume(80));
    let fx = cmd(&mut st, C::ToggleMute);
    assert_eq!(fx[..1], [PlayerEffect::EngineSetGain(0.0)]);
    assert!(st.snapshot().muted);
    assert_eq!(st.snapshot().volume, 80, "mute keeps the volume");
    let fx = cmd(&mut st, C::ToggleMute);
    assert_eq!(fx[..1], [PlayerEffect::EngineSetGain(gain(80))]);
    assert!(!st.snapshot().muted);
    cmd(&mut st, C::ToggleMute);
    let fx = cmd(&mut st, C::ChangeVolume(5));
    assert_eq!(fx[..1], [PlayerEffect::EngineSetGain(gain(85))]);
    assert!(!st.snapshot().muted);
    cmd(&mut st, C::ToggleMute);
    let fx = cmd(&mut st, C::SetVolume(85));
    assert_eq!(fx[..1], [PlayerEffect::EngineSetGain(gain(85))], "unmutes");
    assert!(!st.snapshot().muted);

    // bit_perfect: (engine bit_perfect, engine reason, volume, muted) → snapshot.
    let lossy = Some("lossy source");
    let rows = [
        (true, None, 100, false, true, None),
        (true, None, 99, false, false, Some("volume below 100%")),
        (true, None, 100, true, false, Some("muted")),
        (true, None, 50, true, false, Some("muted")),
        (false, lossy, 100, false, false, lossy),
        (false, lossy, 80, false, false, Some("volume below 100%")),
    ];
    for (engine_bp, reason, volume, muted, bp, expected_reason) in rows {
        let ctx = format!("engine {engine_bp} {reason:?}, volume {volume}, muted {muted}");
        let mut st = player();
        let fx = cmd(
            &mut st,
            C::LoadQueue {
                tracks: tracks(&[1]),
                start: 0,
            },
        );
        let (_, _, tag) = expect_resolve(&fx, Purpose::Play);
        resolved(&mut st, tag);
        engine(
            &mut st,
            EngineEvent::Started {
                tag,
                details: TrackDetails {
                    bit_perfect: engine_bp,
                    reason: reason.map(String::from),
                    ..details()
                },
            },
        );
        cmd(&mut st, C::SetVolume(volume));
        if muted {
            cmd(&mut st, C::ToggleMute);
        }
        let np = st.snapshot().now_playing.expect("now playing");
        assert_eq!(np.bit_perfect, bp, "{ctx}");
        assert_eq!(np.bit_perfect_reason.as_deref(), expected_reason, "{ctx}");
        assert_eq!(np.quality, AudioQuality::Lossless, "{ctx}");
    }
}

// --- AC10 ---------------------------------------------------------------------

#[test]
fn ac10_queue_edits() {
    type Op = fn(&PlayerState) -> C;
    type Order = fn(&[u64], u64) -> Vec<u64>;
    /// The track before/after the current one in play order.
    fn neighbour(st: &PlayerState, offset: isize) -> Option<u64> {
        let o = order(st);
        let i = o.iter().position(|t| Some(*t) == current_track(st))?;
        o.get(i.checked_add_signed(offset)?).copied()
    }
    fn remove(o: &[u64], t: u64) -> Vec<u64> {
        o.iter().copied().filter(|x| *x != t).collect()
    }
    fn insert_after(o: &[u64], cur: u64, new: &[u64]) -> Vec<u64> {
        let i = o.iter().position(|t| *t == cur).unwrap() + 1;
        let mut o = o.to_vec();
        o.splice(i..i, new.iter().copied());
        o
    }
    #[derive(Debug, Clone, Copy, PartialEq)]
    enum Then {
        /// The current entry keeps playing, untouched.
        Keeps,
        /// This track starts loading.
        Starts(Where),
        /// Stopped with no current entry.
        StopsEmpty,
    }
    #[derive(Debug, Clone, Copy, PartialEq)]
    enum Where {
        /// The entry that followed the current one in play order.
        NextOfCurrent,
        Track(u64),
    }
    let add_next: Op = |_| C::AddToQueue {
        tracks: tracks(&[6, 7]),
        at: InsertAt::Next,
    };
    let add_end: Op = |_| C::AddToQueue {
        tracks: tracks(&[6, 7]),
        at: InsertAt::End,
    };
    let remove_before: Op = |st| C::RemoveFromQueue(entry_of(st, neighbour(st, -1).unwrap()));
    let remove_after: Op = |st| C::RemoveFromQueue(entry_of(st, neighbour(st, 1).unwrap()));
    let remove_current: Op = |st| C::RemoveFromQueue(st.snapshot().current.unwrap());
    let clear: Op = |_| C::ClearQueue;
    let play_5: Op = |st| C::PlayEntry(entry_of(st, 5));
    // Applied to both orders: (order, current track) → new order.
    let o_add_next: Order = |o, c| insert_after(o, c, &[6, 7]);
    let o_add_end: Order = |o, _| [o, &[6, 7]].concat();
    let o_clear: Order = |_, c| vec![c];
    let o_same: Order = |o, _| o.to_vec();
    // (name, current play-order index of 5, op, removed track (relative
    // neighbour: -1 before, 0 current, 1 after), order transform, then)
    let rows: [(&str, usize, Op, Option<isize>, Order, Then); 13] = [
        (
            "add next, first",
            0,
            add_next,
            None,
            o_add_next,
            Then::Keeps,
        ),
        (
            "add next, middle",
            2,
            add_next,
            None,
            o_add_next,
            Then::Keeps,
        ),
        ("add next, last", 4, add_next, None, o_add_next, Then::Keeps),
        ("add end, middle", 2, add_end, None, o_add_end, Then::Keeps),
        ("add end, last", 4, add_end, None, o_add_end, Then::Keeps),
        (
            "remove before",
            2,
            remove_before,
            Some(-1),
            o_same,
            Then::Keeps,
        ),
        (
            "remove after",
            2,
            remove_after,
            Some(1),
            o_same,
            Then::Keeps,
        ),
        (
            "remove after, first",
            0,
            remove_after,
            Some(1),
            o_same,
            Then::Keeps,
        ),
        (
            "remove current, middle",
            2,
            remove_current,
            Some(0),
            o_same,
            Then::Starts(Where::NextOfCurrent),
        ),
        (
            "remove current, first",
            0,
            remove_current,
            Some(0),
            o_same,
            Then::Starts(Where::NextOfCurrent),
        ),
        (
            "remove current, last",
            4,
            remove_current,
            Some(0),
            o_same,
            Then::StopsEmpty,
        ),
        ("clear", 2, clear, None, o_clear, Then::Keeps),
        (
            "play entry",
            0,
            play_5,
            None,
            o_same,
            Then::Starts(Where::Track(5)),
        ),
    ];
    for shuffle in [false, true] {
        for (name, at, op, removed, transform, then) in rows {
            let ctx = format!("{name}, shuffle={shuffle}");
            let mut st = PlayerState::new(PlayerConfig::default(), 3);
            if shuffle {
                cmd(&mut st, C::ToggleShuffle);
            }
            let fx = cmd(
                &mut st,
                C::LoadQueue {
                    tracks: tracks(&[1, 2, 3, 4, 5]),
                    start: 0,
                },
            );
            complete(&mut st, &fx);
            let target = order(&st)[at];
            let c = C::PlayEntry(entry_of(&st, target));
            let fx = cmd(&mut st, c);
            complete(&mut st, &fx);
            let play_before = order(&st);
            let current = current_track(&st).unwrap();
            let next_of_current = neighbour(&st, 1);
            let removed = removed.map(|r| neighbour(&st, r).unwrap());
            let ids_before: HashSet<EntryId> = st.snapshot().queue.iter().map(|e| e.id).collect();

            let c = op(&st);

            let fx = cmd(&mut st, c);

            let expect = |o: &[u64]| {
                let o = transform(o, current);
                match removed {
                    Some(t) => remove(&o, t),
                    None => o,
                }
            };
            assert_eq!(order(&st), expect(&play_before), "{ctx}: play order");
            // New entries get new IDs.
            for e in st.snapshot().queue {
                if !ids_before.contains(&e.id) {
                    assert!([6, 7].contains(&e.track.id.0), "{ctx}");
                    assert!(e.id > *ids_before.iter().max().unwrap(), "{ctx}: fresh ID");
                    assert!(!e.suggested, "{ctx}");
                }
            }
            match then {
                Then::Keeps => {
                    assert!(!touches_engine(&fx), "{ctx}: {fx:?}");
                    assert!(!any_resolve(&fx), "{ctx}: {fx:?}");
                    assert_eq!(current_track(&st), Some(current), "{ctx}");
                    assert_eq!(st.snapshot().state, S::Playing, "{ctx}");
                }
                Then::Starts(w) => {
                    let t = match w {
                        Where::NextOfCurrent => next_of_current.unwrap(),
                        Where::Track(t) => t,
                    };
                    assert_eq!(fx[0], PlayerEffect::EngineStop, "{ctx}: {fx:?}");
                    assert_eq!(
                        resolve_of(&fx, Purpose::Play).map(|r| r.1.0),
                        Some(t),
                        "{ctx}"
                    );
                    assert_eq!(current_track(&st), Some(t), "{ctx}");
                    assert_eq!(st.snapshot().state, S::Loading, "{ctx}");
                }
                Then::StopsEmpty => {
                    assert_eq!(fx[0], PlayerEffect::EngineStop, "{ctx}: {fx:?}");
                    assert!(!any_resolve(&fx), "{ctx}");
                    assert_eq!(st.snapshot().current, None, "{ctx}");
                    assert_eq!(st.snapshot().state, S::Stopped, "{ctx}");
                }
            }
            // The original order took the same edit.
            if shuffle {
                cmd(&mut st, C::ToggleShuffle);
                assert_eq!(
                    order(&st),
                    expect(&[1, 2, 3, 4, 5]),
                    "{ctx}: original order"
                );
            }
        }
    }

    // Removing the current entry while stopped: the next one becomes current,
    // nothing plays; the last one: none.
    let mut st = in_state(From::Stopped); // 1, 2*, 3
    let c = C::RemoveFromQueue(entry_of(&st, 2));
    let fx = cmd(&mut st, c);
    assert!(!touches_engine(&fx) && !any_resolve(&fx), "{fx:?}");
    assert_eq!(current_track(&st), Some(3));
    assert_eq!(st.snapshot().state, S::Stopped);
    let c = C::RemoveFromQueue(entry_of(&st, 3));
    let fx = cmd(&mut st, c);
    assert!(!touches_engine(&fx) && !any_resolve(&fx), "{fx:?}");
    assert_eq!(st.snapshot().current, None);
    assert_eq!(order(&st), vec![1]);

    // Editing an empty queue starts nothing.
    let mut st = player();
    assert!(cmd(&mut st, C::ClearQueue).is_empty());
    assert!(cmd(&mut st, C::RemoveFromQueue(EntryId(1))).is_empty());
    assert!(cmd(&mut st, C::PlayEntry(EntryId(1))).is_empty());
    for at in [InsertAt::Next, InsertAt::End] {
        let mut st = player();
        let fx = cmd(
            &mut st,
            C::AddToQueue {
                tracks: tracks(&[1, 2]),
                at,
            },
        );
        assert!(!touches_engine(&fx) && !any_resolve(&fx), "{at:?}: {fx:?}");
        assert_eq!(order(&st), vec![1, 2], "{at:?}");
        assert_eq!(current_track(&st), Some(1), "{at:?}: the first is current");
        assert_eq!(st.snapshot().state, S::Stopped, "{at:?}");
    }

    // The same track twice; IDs are never reused.
    let (mut st, _) = playing(&[1, 1], 0);
    let s = st.snapshot();
    assert_eq!(order(&st), vec![1, 1]);
    assert_ne!(s.queue[0].id, s.queue[1].id);
    let mut seen: HashSet<EntryId> = s.queue.iter().map(|e| e.id).collect();
    cmd(&mut st, C::RemoveFromQueue(s.queue[1].id));
    cmd(
        &mut st,
        C::AddToQueue {
            tracks: tracks(&[1]),
            at: InsertAt::End,
        },
    );
    cmd(
        &mut st,
        C::LoadQueue {
            tracks: tracks(&[1, 1]),
            start: 0,
        },
    );
    for e in st.snapshot().queue {
        assert!(seen.insert(e.id), "ID {:?} reused", e.id);
    }
}

// --- AC11 ---------------------------------------------------------------------

#[test]
fn ac11_one_snapshot_per_change() {
    // `step` checks the rule on every input of every test in this file; here
    // the cases that must emit nothing at all, and Position's broadcast.
    let mut st = player();
    for c in [
        C::TogglePause,
        C::Next,
        C::Previous,
        C::SeekBy(5000),
        C::SeekTo(secs(1)),
        C::ClearQueue,
        C::PlayEntry(EntryId(9)),
        C::RemoveFromQueue(EntryId(9)),
        C::ChangeVolume(5),
        C::SetVolume(100),
        C::Shutdown,
        C::AddToQueue {
            tracks: vec![],
            at: InsertAt::End,
        },
        C::LoadQueue {
            tracks: vec![],
            start: 0,
        },
    ] {
        assert!(
            cmd(&mut st, c.clone()).is_empty(),
            "{c:?} on an empty player"
        );
    }
    for e in [
        EngineEvent::Position(secs(1)),
        EngineEvent::Buffering,
        EngineEvent::Buffered,
        EngineEvent::Paused,
        EngineEvent::Resumed,
        EngineEvent::Stopped,
        EngineEvent::TrackEnded { tag: 1 },
    ] {
        assert!(
            engine(&mut st, e.clone()).is_empty(),
            "{e:?} on an empty player"
        );
    }

    // A mode change on an empty player still broadcasts.
    let fx = cmd(&mut st, C::ToggleShuffle);
    assert_eq!(
        fx,
        vec![PlayerEffect::Broadcast(Event::Player(st.snapshot()))]
    );

    // Position: only Event::Position; the same position again: nothing.
    let (mut st, _) = playing(&[1, 2], 0);
    let entry = st.snapshot().current.unwrap();
    let fx = position(&mut st, Duration::from_millis(250));
    assert_eq!(
        fx,
        vec![PlayerEffect::Broadcast(Event::Position {
            entry,
            position: Duration::from_millis(250)
        })]
    );
    assert!(position(&mut st, Duration::from_millis(250)).is_empty());
    // At the preload point, the preload comes first and Position is still the
    // only broadcast.
    let fx = position(&mut st, LEN - secs(30));
    assert!(matches!(
        fx[0],
        PlayerEffect::Resolve {
            purpose: Purpose::Preload,
            ..
        }
    ));
    assert_eq!(
        fx[1..],
        [PlayerEffect::Broadcast(Event::Position {
            entry,
            position: LEN - secs(30)
        })]
    );
    // Buffering is a change: one snapshot; engine confirmations are not.
    let fx = engine(&mut st, EngineEvent::Buffering);
    assert_eq!(st.snapshot().state, S::Buffering);
    assert_eq!(fx.len(), 1);
    assert!(engine(&mut st, EngineEvent::Resumed).is_empty());
    let fx = engine(&mut st, EngineEvent::Buffered);
    assert_eq!(st.snapshot().state, S::Playing);
    assert_eq!(fx.len(), 1);
    // A Position while loading belongs to the replaced track.
    cmd(&mut st, C::Next);
    assert!(position(&mut st, secs(3)).is_empty());
}

// --- AC26 ---------------------------------------------------------------------

#[test]
fn ac26_autoplay() {
    #[derive(Debug, Clone)]
    enum Reply {
        Ok(Vec<u64>),
        Err(&'static str),
        Stale,
        AfterToggleOff,
    }
    #[derive(Debug, Clone, PartialEq)]
    enum Expect {
        /// These tracks appended as suggested; the first preloaded.
        Appends(Vec<u64>),
        /// Nothing appended; this message; stops at the end.
        NoSuggestions(&'static str),
        /// Nothing changes.
        Ignored,
    }
    // Queue 1, 2, 3 playing 3 (last); suggestions reply → expectation.
    let rows = [
        (
            Reply::Ok(vec![3, 4, 5, 1, 4, 6]),
            Expect::Appends(vec![4, 5, 6]),
        ),
        (
            Reply::Ok(vec![]),
            Expect::NoSuggestions("Autoplay: no suggestions (no new tracks)"),
        ),
        (
            Reply::Ok(vec![3, 1, 2]),
            Expect::NoSuggestions("Autoplay: no suggestions (no new tracks)"),
        ),
        (
            Reply::Err("Network error"),
            Expect::NoSuggestions("Autoplay: no suggestions (Network error)"),
        ),
        (Reply::Stale, Expect::Ignored),
        (Reply::AfterToggleOff, Expect::Ignored),
    ];
    let autoplay_on = || {
        PlayerState::new(
            PlayerConfig {
                autoplay: true,
                ..PlayerConfig::default()
            },
            1,
        )
    };
    for (reply, expect) in rows {
        let ctx = format!("{reply:?}");
        let (mut st, tag) = playing_with(autoplay_on(), &[1, 2, 3], 2);
        assert!(st.snapshot().autoplay);
        let fx = position(&mut st, LEN - secs(40));
        assert!(fx.len() == 1, "{ctx}: not yet: {fx:?}");
        let fx = position(&mut st, LEN - secs(30));
        let ftag = match fx[..] {
            [PlayerEffect::FetchSuggestions { seed, tag }, _] => {
                assert_eq!(seed, TrackId(3), "{ctx}");
                tag
            }
            _ => panic!("{ctx}: expected FetchSuggestions: {fx:?}"),
        };
        let fx = position(&mut st, LEN - secs(20));
        assert!(
            !has(&fx, |e| matches!(e, PlayerEffect::FetchSuggestions { .. })),
            "{ctx}: once per seed"
        );
        let (reply_tag, result) = match &reply {
            Reply::Ok(ids) => (ftag, Ok(tracks(ids))),
            Reply::Err(m) => (ftag, Err(m.to_string())),
            Reply::Stale => (ftag + 100, Ok(tracks(&[4]))),
            Reply::AfterToggleOff => {
                cmd(&mut st, C::ToggleAutoplay);
                (ftag, Ok(tracks(&[4])))
            }
        };
        let before = st.snapshot();
        let fx = step(
            &mut st,
            PlayerInput::Suggestions {
                tag: reply_tag,
                result,
            },
        );
        match &expect {
            Expect::Appends(added) => {
                let s = st.snapshot();
                assert_eq!(order(&st), [&[1, 2, 3][..], added].concat(), "{ctx}");
                let marks: Vec<bool> = s.queue.iter().map(|e| e.suggested).collect();
                assert_eq!(
                    marks,
                    [vec![false; 3], vec![true; added.len()]].concat(),
                    "{ctx}"
                );
                assert_eq!(
                    resolve_of(&fx, Purpose::Preload).map(|r| r.1.0),
                    Some(added[0]),
                    "{ctx}: the first suggestion is preloaded"
                );
            }
            Expect::NoSuggestions(message) => {
                assert!(!any_resolve(&fx), "{ctx}");
                assert_eq!(order(&st), vec![1, 2, 3], "{ctx}");
                assert_eq!(st.snapshot().message.as_deref(), Some(*message), "{ctx}");
                // Never retried; the queue stops at its end.
                let fx = position(&mut st, LEN - secs(5));
                assert!(
                    !has(&fx, |e| matches!(e, PlayerEffect::FetchSuggestions { .. })),
                    "{ctx}"
                );
                let fx = engine(&mut st, EngineEvent::TrackEnded { tag });
                assert!(!any_resolve(&fx), "{ctx}");
                assert_eq!(st.snapshot().state, S::Stopped, "{ctx}");
                assert_eq!(current_track(&st), Some(3), "{ctx}");
            }
            Expect::Ignored => {
                assert!(fx.is_empty(), "{ctx}: {fx:?}");
                assert_eq!(st.snapshot(), before, "{ctx}");
            }
        }
        if matches!(reply, Reply::AfterToggleOff) {
            // On again at the preload point: asks again.
            let fx = cmd(&mut st, C::ToggleAutoplay);
            assert!(
                has(
                    &fx,
                    |e| matches!(e, PlayerEffect::FetchSuggestions { seed: TrackId(3), tag } if *tag != ftag)
                ),
                "{ctx}: asks again: {fx:?}"
            );
        }
    }

    // Suggested entries transition gaplessly and are ordinary entries after.
    let (mut st, _) = playing_with(autoplay_on(), &[1], 0);
    let fx = position(&mut st, LEN - secs(30));
    let ftag = match fx[0] {
        PlayerEffect::FetchSuggestions { tag, .. } => tag,
        _ => panic!("{fx:?}"),
    };
    let fx = step(
        &mut st,
        PlayerInput::Suggestions {
            tag: ftag,
            result: Ok(tracks(&[1, 8, 9])),
        },
    );
    let (_, _, ptag) = expect_resolve(&fx, Purpose::Preload);
    resolved(&mut st, ptag);
    engine(
        &mut st,
        EngineEvent::Transitioned {
            tag: ptag,
            details: details(),
        },
    );
    assert_eq!(current_track(&st), Some(8));
    let c = C::RemoveFromQueue(entry_of(&st, 9));
    cmd(&mut st, c);
    assert_eq!(order(&st), vec![1, 8]);

    // The last track ends before the suggestions arrive: stop, then play the
    // first suggestion when they come.
    let (mut st, tag) = playing_with(autoplay_on(), &[1], 0);
    let fx = position(&mut st, LEN - secs(30));
    let ftag = match fx[0] {
        PlayerEffect::FetchSuggestions { tag, .. } => tag,
        _ => panic!("{fx:?}"),
    };
    engine(&mut st, EngineEvent::TrackEnded { tag });
    assert_eq!(st.snapshot().state, S::Stopped);
    let fx = step(
        &mut st,
        PlayerInput::Suggestions {
            tag: ftag,
            result: Ok(tracks(&[8])),
        },
    );
    assert_eq!(
        resolve_of(&fx, Purpose::Play).map(|r| r.1),
        Some(TrackId(8))
    );

    // Unknown duration: asks at Started.
    let mut st = autoplay_on();
    let mut t = track(1);
    t.duration = None;
    let fx = cmd(
        &mut st,
        C::LoadQueue {
            tracks: vec![t],
            start: 0,
        },
    );
    let (_, _, tag) = expect_resolve(&fx, Purpose::Play);
    resolved(&mut st, tag);
    let fx = started(&mut st, tag);
    assert!(has(&fx, |e| matches!(
        e,
        PlayerEffect::FetchSuggestions {
            seed: TrackId(1),
            ..
        }
    )));

    // Never with repeat queue/track or autoplay off; nor mid-queue.
    // (autoplay, repeat cycles, playing index of 3)
    for (autoplay, cycles, at) in [(true, 1, 2), (true, 2, 2), (false, 0, 2), (true, 0, 1)] {
        let ctx = format!("autoplay {autoplay}, {cycles} repeat cycles, at {at}");
        let mut st = PlayerState::new(
            PlayerConfig {
                autoplay,
                ..PlayerConfig::default()
            },
            1,
        );
        for _ in 0..cycles {
            cmd(&mut st, C::CycleRepeat);
        }
        let (mut st, tag) = playing_with(st, &[1, 2, 3], at);
        let mut all = Vec::new();
        for p in [LEN - secs(30), LEN - secs(1)] {
            all.extend(position(&mut st, p));
        }
        all.extend(engine(&mut st, EngineEvent::TrackEnded { tag }));
        assert!(
            !has(&all, |e| matches!(e, PlayerEffect::FetchSuggestions { .. })),
            "{ctx}: {all:?}"
        );
    }
}
