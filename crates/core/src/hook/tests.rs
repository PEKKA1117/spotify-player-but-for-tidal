use std::time::Duration;

use super::*;
use crate::protocol::{NowPlaying, PlaybackState, QueueEntry, RepeatMode};
use crate::{AlbumRef, ArtistRef, AudioQuality, EntryId, TrackId};

use PlaybackState::{Buffering, Loading, Paused, Playing, Stopped};

fn track(id: u64) -> Track {
    Track {
        id: TrackId(id),
        title: format!("Title {id}"),
        version: None,
        artists: vec![ArtistRef {
            id: 1,
            name: "Artist".into(),
        }],
        album: Some(AlbumRef {
            id: 1,
            title: "Album".into(),
            cover: None,
        }),
        duration: Some(Duration::from_millis(200_000)),
        streamable: true,
    }
}

fn now_playing() -> NowPlaying {
    NowPlaying {
        quality: AudioQuality::Lossless,
        source: "FLAC".into(),
        output: "hw:0".into(),
        bit_perfect: true,
        bit_perfect_reason: None,
        released: false,
    }
}

/// Entries 1 to 3 in the queue (entry `n` holds track `n`), `current`
/// being `n`.
fn snap(state: PlaybackState, current: Option<u64>) -> PlayerSnapshot {
    PlayerSnapshot {
        queue: (1..=3)
            .map(|n| QueueEntry {
                id: EntryId(n),
                track: track(n),
                suggested: false,
            })
            .collect(),
        current: current.map(EntryId),
        state,
        position: Duration::ZERO,
        shuffle: false,
        repeat: RepeatMode::Off,
        autoplay: false,
        volume: 100,
        muted: false,
        now_playing: matches!(state, Playing | Buffering | Paused).then(now_playing),
        message: None,
        device: "hw:0".into(),
    }
}

fn at(mut snapshot: PlayerSnapshot, ms: u64) -> PlayerSnapshot {
    snapshot.position = Duration::from_millis(ms);
    snapshot
}

fn lines(events: &[HookEvent]) -> Vec<String> {
    events.iter().map(|e| e.arguments().join(" ")).collect()
}

/// A tracker that has seen `track` start, or nothing.
fn tracker(started: Option<u64>) -> HookTracker {
    let mut tracker = HookTracker::default();
    if let Some(id) = started {
        tracker.update(Transition {
            before: &snap(Loading, Some(id)),
            after: &snap(Playing, Some(id)),
            started: true,
            ended: false,
            seek: false,
        });
    }
    tracker
}

/// One row: the track already started, the snapshots around the input,
/// its flags and the events expected.
struct Row {
    name: &'static str,
    started_before: Option<u64>,
    before: PlayerSnapshot,
    after: PlayerSnapshot,
    started: bool,
    ended: bool,
    seek: bool,
    want: &'static [&'static str],
}

impl Row {
    fn new(
        name: &'static str,
        started_before: Option<u64>,
        before: PlayerSnapshot,
        after: PlayerSnapshot,
        want: &'static [&'static str],
    ) -> Self {
        Self {
            name,
            started_before,
            before,
            after,
            started: false,
            ended: false,
            seek: false,
            want,
        }
    }

    fn started(mut self) -> Self {
        self.started = true;
        self
    }

    fn ended(mut self) -> Self {
        self.ended = true;
        self
    }

    fn seek(mut self) -> Self {
        self.seek = true;
        self
    }
}

fn check(rows: Vec<Row>) {
    for row in rows {
        let events = tracker(row.started_before).update(Transition {
            before: &row.before,
            after: &row.after,
            started: row.started,
            ended: row.ended,
            seek: row.seek,
        });
        assert_eq!(lines(&events), row.want, "{}", row.name);
    }
}

/// AC2: tracks starting, pausing, resuming and ending, in order.
#[test]
fn ac2_track_events() {
    check(vec![
        Row::new(
            "a track starts",
            None,
            snap(Loading, Some(1)),
            snap(Playing, Some(1)),
            &["Changed 1", "Playing 1 0"],
        )
        .started(),
        Row::new(
            "a restored track starts at its position",
            None,
            at(snap(Loading, Some(1)), 83_000),
            at(snap(Playing, Some(1)), 83_000),
            &["Changed 1", "Playing 1 83000"],
        )
        .started(),
        Row::new(
            "gapless",
            Some(1),
            at(snap(Playing, Some(1)), 199_000),
            snap(Playing, Some(2)),
            &["EndOfTrack 1", "Changed 2", "Playing 2 0"],
        )
        .started()
        .ended(),
        Row::new(
            "an end, the next track loads",
            Some(1),
            at(snap(Playing, Some(1)), 200_000),
            snap(Loading, Some(2)),
            &["EndOfTrack 1"],
        )
        .ended(),
        Row::new(
            "a repeat of the same track",
            Some(1),
            at(snap(Playing, Some(1)), 199_000),
            snap(Playing, Some(1)),
            &["EndOfTrack 1", "Changed 1", "Playing 1 0"],
        )
        .started()
        .ended(),
        Row::new(
            "pause",
            Some(1),
            at(snap(Playing, Some(1)), 5000),
            at(snap(Paused, Some(1)), 5000),
            &["Paused 1 5000"],
        ),
        Row::new(
            "pause while buffering",
            Some(1),
            at(snap(Buffering, Some(1)), 5000),
            at(snap(Paused, Some(1)), 5000),
            &["Paused 1 5000"],
        ),
        Row::new(
            "resume",
            Some(1),
            at(snap(Paused, Some(1)), 5000),
            at(snap(Playing, Some(1)), 5000),
            &["Playing 1 5000"],
        ),
    ]);
}

/// AC3: every way of stopping, once, with the track that last started.
#[test]
fn ac3_stopped() {
    let mut cleared = snap(Stopped, None);
    cleared.queue.clear();
    check(vec![
        Row::new(
            "stop while playing",
            Some(1),
            at(snap(Playing, Some(1)), 7000),
            at(snap(Stopped, Some(1)), 7000),
            &["Stopped 1 7000"],
        ),
        Row::new(
            "stop while paused",
            Some(1),
            at(snap(Paused, Some(1)), 7000),
            at(snap(Stopped, Some(1)), 7000),
            &["Stopped 1 7000"],
        ),
        Row::new(
            "stop while buffering",
            Some(1),
            at(snap(Buffering, Some(1)), 7000),
            at(snap(Stopped, Some(1)), 7000),
            &["Stopped 1 7000"],
        ),
        Row::new(
            "the queue ran out",
            Some(3),
            at(snap(Playing, Some(3)), 200_000),
            snap(Stopped, Some(3)),
            &["EndOfTrack 3", "Stopped 3 0"],
        )
        .ended(),
        Row::new(
            "the queue was cleared",
            Some(1),
            at(snap(Playing, Some(1)), 7000),
            cleared,
            &["Stopped 1 0"],
        ),
        Row::new(
            "a skip that found nothing to play",
            Some(1),
            snap(Loading, Some(2)),
            snap(Stopped, Some(2)),
            &["Stopped 1 0"],
        ),
        Row::new(
            "nothing had started",
            None,
            snap(Loading, Some(1)),
            snap(Stopped, Some(1)),
            &[],
        ),
    ]);

    // Once: a second stop has nothing left to say.
    let mut tracker = tracker(Some(1));
    let (playing, stopped) = (snap(Playing, Some(1)), snap(Stopped, Some(1)));
    let input = |before, after| Transition {
        before,
        after,
        started: false,
        ended: false,
        seek: false,
    };
    assert_eq!(
        lines(&tracker.update(input(&playing, &stopped))),
        ["Stopped 1 0"]
    );
    assert!(tracker.update(input(&stopped, &stopped)).is_empty());
}

/// AC14: shutting down stops a track that started, and only then.
#[test]
fn ac14_shutdown() {
    let rows = [
        (
            "playing",
            Some(1),
            at(snap(Playing, Some(1)), 7000),
            Some("Stopped 1 7000"),
        ),
        (
            "paused",
            Some(1),
            at(snap(Paused, Some(1)), 7000),
            Some("Stopped 1 7000"),
        ),
        ("already stopped", None, snap(Stopped, Some(1)), None),
    ];
    for (name, started, snapshot, want) in rows {
        let got = tracker(started).shutdown(&snapshot);
        let got = got.map(|e| e.arguments().join(" "));
        assert_eq!(got.as_deref(), want, "{name}");
    }
}

/// AC4: a seek on a track that plays or is paused, `Previous` included.
#[test]
fn ac4_seeked() {
    check(vec![
        Row::new(
            "playing",
            Some(1),
            at(snap(Playing, Some(1)), 1000),
            at(snap(Playing, Some(1)), 9000),
            &["Seeked 1 9000"],
        )
        .seek(),
        Row::new(
            "paused",
            Some(1),
            at(snap(Paused, Some(1)), 1000),
            at(snap(Paused, Some(1)), 9000),
            &["Seeked 1 9000"],
        )
        .seek(),
        Row::new(
            "buffering",
            Some(1),
            at(snap(Buffering, Some(1)), 1000),
            at(snap(Buffering, Some(1)), 9000),
            &["Seeked 1 9000"],
        )
        .seek(),
        Row::new(
            "Previous restarts the track",
            Some(1),
            at(snap(Playing, Some(1)), 9000),
            snap(Playing, Some(1)),
            &["Seeked 1 0"],
        )
        .seek(),
        Row::new(
            "Previous moves to another track",
            Some(2),
            snap(Playing, Some(2)),
            snap(Loading, Some(1)),
            &[],
        )
        .seek(),
        Row::new(
            "stopped",
            None,
            at(snap(Stopped, Some(1)), 1000),
            at(snap(Stopped, Some(1)), 9000),
            &[],
        )
        .seek(),
        Row::new(
            "loading",
            Some(1),
            at(snap(Loading, Some(2)), 1000),
            at(snap(Loading, Some(2)), 9000),
            &[],
        )
        .seek(),
        Row::new(
            "the same position",
            Some(1),
            at(snap(Playing, Some(1)), 9000),
            at(snap(Playing, Some(1)), 9000),
            &[],
        )
        .seek(),
    ]);
}

/// AC5: the volume heard.
#[test]
fn ac5_volume() {
    let volume = |percent, muted| {
        let mut snapshot = snap(Playing, Some(1));
        snapshot.volume = percent;
        snapshot.muted = muted;
        snapshot
    };
    check(vec![
        Row::new(
            "down",
            Some(1),
            volume(100, false),
            volume(80, false),
            &["VolumeChanged 80"],
        ),
        Row::new(
            "mute",
            Some(1),
            volume(80, false),
            volume(80, true),
            &["VolumeChanged 0"],
        ),
        Row::new(
            "unmute",
            Some(1),
            volume(80, true),
            volume(80, false),
            &["VolumeChanged 80"],
        ),
        Row::new(
            "no change",
            Some(1),
            volume(80, false),
            volume(80, false),
            &[],
        ),
        Row::new(
            "muted, not heard",
            Some(1),
            volume(80, true),
            volume(60, true),
            &[],
        ),
    ]);
}

/// AC6: the selected device.
#[test]
fn ac6_device() {
    let device = |name: &str| {
        let mut snapshot = snap(Playing, Some(1));
        snapshot.device = name.to_owned();
        snapshot
    };
    check(vec![
        Row::new(
            "changed",
            Some(1),
            device("hw:0"),
            device("hw:1"),
            &["DeviceChanged hw:1"],
        ),
        Row::new("the same", Some(1), device("hw:1"), device("hw:1"), &[]),
    ]);
}

/// AC7: what is not an event.
#[test]
fn ac7_silence() {
    let released = |on| {
        let mut snapshot = snap(Paused, Some(1));
        snapshot.now_playing.as_mut().expect("paused").released = on;
        snapshot
    };
    let mut fewer = snap(Playing, Some(1));
    fewer.queue.pop();
    check(vec![
        Row::new(
            "loading",
            None,
            snap(Stopped, Some(1)),
            snap(Loading, Some(1)),
            &[],
        ),
        Row::new(
            "a skip",
            Some(1),
            snap(Playing, Some(1)),
            snap(Loading, Some(2)),
            &[],
        ),
        Row::new(
            "skipping on",
            Some(1),
            snap(Loading, Some(2)),
            snap(Loading, Some(3)),
            &[],
        ),
        Row::new(
            "a stall",
            Some(1),
            snap(Playing, Some(1)),
            snap(Buffering, Some(1)),
            &[],
        ),
        Row::new(
            "recovering",
            Some(1),
            snap(Buffering, Some(1)),
            snap(Playing, Some(1)),
            &[],
        ),
        Row::new(
            "the output released",
            Some(1),
            released(false),
            released(true),
            &[],
        ),
        Row::new(
            "pausing a load",
            None,
            snap(Loading, Some(1)),
            snap(Paused, Some(1)),
            &[],
        ),
        Row::new("a queue edit", Some(1), snap(Playing, Some(1)), fewer, &[]),
        Row::new(
            "the position moving on",
            Some(1),
            at(snap(Playing, Some(1)), 1000),
            at(snap(Playing, Some(1)), 1250),
            &[],
        ),
        Row::new(
            "a restored session",
            None,
            at(snap(Stopped, Some(1)), 83_000),
            at(snap(Stopped, Some(1)), 83_000),
            &[],
        ),
        Row::new(
            "a track starting under a pause",
            None,
            snap(Loading, Some(1)),
            snap(Paused, Some(1)),
            &["Changed 1"],
        )
        .started(),
    ]);
}

type Variables = &'static [(&'static str, &'static str)];

/// AC8: the command's arguments and environment for each event.
#[test]
fn ac8_arguments_and_environment() {
    let ms = Duration::from_millis;
    let full = {
        let mut track = track(7);
        track.version = Some("Live".into());
        track.artists.push(ArtistRef {
            id: 2,
            name: "Other".into(),
        });
        track
    };
    let bare = {
        let mut track = track(8);
        track.album = None;
        track.duration = None;
        track
    };
    let full_env: Variables = &[
        ("TIDAL_PLAYER_TRACK_TITLE", "Title 7 (Live)"),
        ("TIDAL_PLAYER_TRACK_ARTISTS", "Artist, Other"),
        ("TIDAL_PLAYER_TRACK_ALBUM", "Album"),
        ("TIDAL_PLAYER_TRACK_DURATION_MS", "200000"),
    ];
    let bare_env: Variables = &[
        ("TIDAL_PLAYER_TRACK_TITLE", "Title 8"),
        ("TIDAL_PLAYER_TRACK_ARTISTS", "Artist"),
    ];
    let rows: Vec<(HookEvent, &[&str], Variables)> = vec![
        (
            HookEvent::Changed(full.clone()),
            &["Changed", "7"],
            full_env,
        ),
        (
            HookEvent::Playing(full.clone(), ms(1500)),
            &["Playing", "7", "1500"],
            full_env,
        ),
        (
            HookEvent::Paused(full.clone(), ms(1500)),
            &["Paused", "7", "1500"],
            full_env,
        ),
        (
            HookEvent::Stopped(full.clone(), ms(1500)),
            &["Stopped", "7", "1500"],
            full_env,
        ),
        (
            HookEvent::EndOfTrack(full.clone()),
            &["EndOfTrack", "7"],
            full_env,
        ),
        (
            HookEvent::Seeked(full, ms(1500)),
            &["Seeked", "7", "1500"],
            full_env,
        ),
        (HookEvent::Changed(bare), &["Changed", "8"], bare_env),
        (HookEvent::VolumeChanged(40), &["VolumeChanged", "40"], &[]),
        (
            HookEvent::DeviceChanged("hw:1".into()),
            &["DeviceChanged", "hw:1"],
            &[],
        ),
    ];
    for (event, arguments, environment) in rows {
        assert_eq!(event.arguments(), arguments, "{event:?}");
        let got = event.environment();
        let got: Vec<(&str, &str)> = got.iter().map(|(k, v)| (*k, v.as_str())).collect();
        assert_eq!(got, environment, "{event:?}");
    }
}

/// AC9: the line of waiting events.
#[test]
fn ac9_queue() {
    let ms = Duration::from_millis;
    let drain = |mut queue: HookQueue| {
        let mut got = Vec::new();
        while let Some(event) = queue.pop() {
            got.push(event.arguments().join(" "));
        }
        got
    };
    let seeked = |id, at| HookEvent::Seeked(track(id), ms(at));
    let changed = |id| HookEvent::Changed(track(id));
    let volume = HookEvent::VolumeChanged;
    let rows: Vec<(&str, Vec<HookEvent>, &[&str])> = vec![
        (
            "in order",
            vec![changed(1), changed(2)],
            &["Changed 1", "Changed 2"],
        ),
        (
            "the last seek is replaced",
            vec![changed(1), seeked(1, 10), seeked(1, 20)],
            &["Changed 1", "Seeked 1 20"],
        ),
        (
            "the last volume is replaced",
            vec![volume(90), volume(80)],
            &["VolumeChanged 80"],
        ),
        (
            "not across another event",
            vec![seeked(1, 10), changed(2), seeked(2, 20)],
            &["Seeked 1 10", "Changed 2", "Seeked 2 20"],
        ),
        (
            "not across the other kind",
            vec![volume(90), seeked(1, 10), volume(80)],
            &["VolumeChanged 90", "Seeked 1 10", "VolumeChanged 80"],
        ),
        (
            "others are kept",
            vec![changed(1), changed(1)],
            &["Changed 1", "Changed 1"],
        ),
    ];
    for (name, events, want) in rows {
        let mut queue = HookQueue::default();
        for event in events {
            assert!(!queue.push(event), "{name}");
        }
        assert_eq!(drain(queue), want, "{name}");
    }

    // The limit: one more drops the oldest, and says so.
    let mut queue = HookQueue::default();
    for id in 0..MAX_WAITING as u64 {
        assert!(!queue.push(changed(id)));
    }
    assert!(queue.push(changed(1000)));
    let got = drain(queue);
    assert_eq!(got.len(), MAX_WAITING);
    assert_eq!(got.first().map(String::as_str), Some("Changed 1"));
    assert_eq!(got.last().map(String::as_str), Some("Changed 1000"));
}
