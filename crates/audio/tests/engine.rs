//! Engine tests (spec 0003 AC14–AC23), driven with fixture sources, a fake
//! clock and the scripted `MemorySink` fake device. No test sleeps or opens
//! a real device; "nothing happens" checks are bounded waits on the event
//! channel.

mod common;

use std::time::Duration;

use common::*;
use tidal_player_audio::clock::{Clock, duration_to_frames, frames_to_duration};
use tidal_player_audio::testing::{FakeSource, Fetch};
use tidal_player_audio::{
    Command, EngineError, Event, MemoryDevices, SinkCall, SinkError, SinkScript, SourceError,
};

const QUIET: Duration = Duration::from_millis(150);

fn held(frames: u64) -> SinkScript {
    SinkScript {
        hold_after_frames: Some(frames),
        ..SinkScript::default()
    }
}

fn is_started(e: &Event) -> bool {
    matches!(e, Event::Started { .. })
}

/// The samples written after the last `Discard` (a seek or a new `Play`).
fn after_last_discard(rig: &Rig) -> Vec<i32> {
    let calls = rig.sinks.calls();
    let last = calls
        .iter()
        .rposition(|c| matches!(c, SinkCall::Discard { .. }))
        .expect("a Discard");
    let before: usize = calls[..last]
        .iter()
        .map(|c| match c {
            SinkCall::Write { frames, .. } => *frames,
            _ => 0,
        })
        .sum();
    rig.sinks.samples()[before * 2..].to_vec()
}

fn assert_samples(actual: &[i32], expected: &[i32], what: &str) {
    assert_eq!(actual.len(), expected.len(), "{what}: sample count");
    if let Some(i) = actual.iter().zip(expected).position(|(a, b)| a != b) {
        panic!("{what}: first difference at sample {i}");
    }
}

/// AC14: an underrun is recovered: every frame reaches the device exactly
/// once, and the underrun is counted.
#[test]
fn ac14_underrun_recovered() {
    let rig = rig(SinkScript {
        underrun_on_write: Some(2),
        ..SinkScript::default()
    });
    let (_, expected) = reference(flac16());
    rig.play(flac16());
    let events = rig.until_end();
    assert!(
        matches!(events.last(), Some(Event::TrackEnded { .. })),
        "{events:?}"
    );
    assert_samples(&rig.sinks.samples(), &expected, "after an underrun");
    assert_eq!(rig.engine.underruns(), 1);
}

/// AC14: a lost device fails the track with one `Error(Output(Lost))`, no
/// `TrackEnded`, and the device is closed.
#[test]
fn ac14_device_lost() {
    let rig = rig(SinkScript {
        lost_on_write: Some(2),
        ..SinkScript::default()
    });
    rig.play(flac16());
    let mut events = rig.until_end();
    events.extend(rig.stop());
    assert_eq!(
        count(&events, |e| matches!(
            e,
            Event::Error {
                error: EngineError::Output(SinkError::Lost(_)),
                ..
            }
        )),
        1,
        "{events:?}"
    );
    assert_eq!(count(&events, |e| matches!(e, Event::Error { .. })), 1);
    assert_eq!(count(&events, |e| matches!(e, Event::TrackEnded { .. })), 0);
    assert_eq!(rig.sinks.open_now(), 0, "device closed");
}

/// AC15: Play → Started → every frame once, in order → drain → TrackEnded
/// once → device closed.
#[test]
fn ac15_play_to_end() {
    let rig = rig(SinkScript::default());
    let (format, expected) = reference(flac16());
    rig.play(flac16());
    let mut events = rig.until_end();
    match events.first() {
        Some(Event::Started { source, output, .. }) => {
            assert_eq!(*source, format);
            assert_eq!(output.device, "test");
            assert_eq!(output.sample_rate, 44_100);
        }
        other => panic!("expected Started first, got {other:?} in {events:?}"),
    }
    events.extend(rig.stop());
    assert_eq!(count(&events, |e| matches!(e, Event::TrackEnded { .. })), 1);
    assert_eq!(count(&events, |e| matches!(e, Event::Error { .. })), 0);
    assert_samples(&rig.sinks.samples(), &expected, "played track");
    let calls = rig.sinks.calls();
    assert!(
        matches!(calls.first(), Some(SinkCall::Open { .. })),
        "{calls:?}"
    );
    let n = calls.len();
    assert!(
        matches!(
            calls[n - 2..],
            [SinkCall::Drain { .. }, SinkCall::Close { .. }]
        ),
        "drain then close at the end: {calls:?}"
    );
    assert_eq!(rig.sinks.open_now(), 0);
}

/// AC15: a second Play stops the first: no interleaving, one device open.
#[test]
fn ac15_play_replaces_play() {
    let rig = rig(held(4_608));
    let (_, a) = reference(flac16());
    let (_, b) = reference(mono16());
    rig.play(flac16());
    rig.until("Started", is_started);
    rig.play(mono16());
    rig.sinks.release();
    let mut events = rig.until_end();
    events.extend(rig.stop());
    assert!(
        matches!(events.first(), Some(Event::Started { .. })),
        "{events:?}"
    );
    assert_eq!(count(&events, |e| matches!(e, Event::TrackEnded { .. })), 1);

    let all = rig.sinks.samples();
    let tail = after_last_discard(&rig);
    let head = &all[..all.len() - tail.len()];
    assert!(!head.is_empty() && head.len() < a.len());
    assert_samples(head, &a[..head.len()], "first track's prefix");
    assert_samples(&tail, &b, "second track");
    assert_eq!(rig.sinks.max_open(), 1, "one device open at a time");
}

/// AC16: a source failure, a corrupt FLAC frame and an output error each
/// produce one `Error` and no `TrackEnded`; the next `Play` plays.
#[test]
fn ac16_errors_never_end_track() {
    let corrupt = || {
        let mut bytes = fixture("flac16_44.flac");
        let mid = bytes.len() / 2;
        for b in &mut bytes[mid..mid + 64] {
            *b ^= 0x5A;
        }
        FakeSource::file(bytes, "flac")
    };
    type Expect = fn(&EngineError) -> bool;
    let rows: [(&str, FakeSource, SinkScript, Expect); 3] = [
        (
            "source error",
            flac16().fail_after(12_000, SourceError::Network("connection reset".into())),
            SinkScript::default(),
            |e| matches!(e, EngineError::Source(SourceError::Network(_))),
        ),
        ("corrupt frame", corrupt(), SinkScript::default(), |e| {
            matches!(e, EngineError::Decode(_))
        }),
        (
            "output error",
            flac16(),
            SinkScript {
                lost_on_write: Some(3),
                ..SinkScript::default()
            },
            |e| matches!(e, EngineError::Output(SinkError::Lost(_))),
        ),
    ];
    let (_, expected) = reference(flac16());
    for (name, source, script, expect) in rows {
        let rig = rig(script);
        rig.play(source);
        let events = rig.until_end();
        match events.last() {
            Some(Event::Error { error: e, .. }) => assert!(expect(e), "{name}: wrong error {e:?}"),
            other => panic!("{name}: expected an Error, got {other:?} in {events:?}"),
        }
        assert_eq!(
            count(&events, |e| matches!(e, Event::TrackEnded { .. })),
            0,
            "{name}"
        );

        let before = rig.sinks.frames_written();
        rig.play(flac16());
        let events = rig.until_end();
        assert!(
            matches!(events.last(), Some(Event::TrackEnded { .. })),
            "{name}: next Play: {events:?}"
        );
        assert_eq!(
            count(&events, |e| matches!(e, Event::Error { .. })),
            0,
            "{name}"
        );
        assert_samples(
            &rig.sinks.samples()[before * 2..],
            &expected,
            &format!("{name}: next track"),
        );
        let after = rig.stop();
        assert_eq!(
            count(&after, |e| matches!(e, Event::TrackEnded { .. })),
            0,
            "{name}"
        );
    }
}

/// AC17: a same-format preload follows the track with no drain, no reopen
/// and no gap, and `Transitioned` is sent.
#[test]
fn ac17_gapless_same_format() {
    let rig = rig(SinkScript::default());
    let (_, a) = reference(flac16());
    let (b_format, b) = reference(mono16());
    rig.play(flac16());
    rig.send(Command::Preload {
        tag: 0,
        source: Box::new(mono16()),
    });
    let mut events = rig.until_end();
    events.extend(rig.stop());
    assert_samples(&rig.sinks.samples(), &[a, b].concat(), "concatenation");
    let calls = rig.sinks.calls();
    let opens = count_calls(&calls, |c| matches!(c, SinkCall::Open { .. }));
    let drains = count_calls(&calls, |c| matches!(c, SinkCall::Drain { .. }));
    assert_eq!(
        (opens, drains),
        (1, 1),
        "one open, one final drain: {calls:?}"
    );
    assert!(matches!(calls.last(), Some(SinkCall::Close { .. })));
    let transitioned: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            Event::Transitioned { source, .. } => Some(*source),
            _ => None,
        })
        .collect();
    assert_eq!(transitioned, [b_format]);
    assert_eq!(count(&events, is_started), 1);
    assert_eq!(count(&events, |e| matches!(e, Event::TrackEnded { .. })), 1);
}

fn count_calls(calls: &[SinkCall], pred: impl Fn(&SinkCall) -> bool) -> usize {
    calls.iter().filter(|c| pred(c)).count()
}

/// AC17: a preload in another format: drain, close, reopen, continue.
#[test]
fn ac17_reopen_on_format_change() {
    let rig = rig(SinkScript::default());
    let (a_format, a) = reference(flac16());
    let (c_format, c) = reference(flac24());
    rig.play(flac16());
    rig.send(Command::Preload {
        tag: 0,
        source: Box::new(flac24()),
    });
    let mut events = rig.until_end();
    events.extend(rig.stop());
    assert_samples(&rig.sinks.samples(), &[a, c].concat(), "both tracks");
    let shape: Vec<String> = rig
        .sinks
        .calls()
        .iter()
        .filter_map(|call| match call {
            SinkCall::Open { source, .. } if *source == a_format => Some("open A".to_owned()),
            SinkCall::Open { source, .. } if *source == c_format => Some("open C".to_owned()),
            SinkCall::Drain { .. } => Some("drain".to_owned()),
            SinkCall::Close { .. } => Some("close".to_owned()),
            _ => None,
        })
        .collect();
    assert_eq!(
        shape,
        ["open A", "drain", "close", "open C", "drain", "close"]
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::Transitioned { source, .. } if *source == c_format)),
        "{events:?}"
    );
}

/// AC18: pause/resume loses and repeats nothing; nothing is written while
/// paused; the position after Resume equals the one at Pause.
#[test]
fn ac18_pause_resume_loses_nothing() {
    let rig = rig(SinkScript {
        delay_frames: 1_000,
        ..held(9_216)
    });
    let (_, expected) = reference(flac16());
    rig.play(flac16());
    rig.until("Started", is_started);
    rig.send(Command::Pause);
    let events = rig.until("Paused", |e| matches!(e, Event::Paused));
    assert_eq!(count(&events, |e| matches!(e, Event::Error { .. })), 0);
    let at_pause = match rig.next() {
        Event::Position(p) => p,
        other => panic!("expected a Position after Paused, got {other:?}"),
    };
    let written = rig.sinks.frames_written();
    rig.sinks.release();
    let quiet = rig.quiet_for(QUIET);
    assert!(quiet.is_empty(), "events while paused: {quiet:?}");
    assert_eq!(
        rig.sinks.frames_written(),
        written,
        "nothing written while paused"
    );

    rig.send(Command::Resume);
    let events = rig.until("Resumed", |e| matches!(e, Event::Resumed));
    assert_eq!(events, [Event::Resumed]);
    assert_eq!(
        rig.next(),
        Event::Position(at_pause),
        "position after Resume"
    );
    let events = rig.until_end();
    assert!(
        matches!(events.last(), Some(Event::TrackEnded { .. })),
        "{events:?}"
    );
    assert_samples(&rig.sinks.samples(), &expected, "paused and resumed track");
    let pauses: Vec<bool> = rig
        .sinks
        .calls()
        .iter()
        .filter_map(|c| match c {
            SinkCall::Pause { paused, .. } => Some(*paused),
            _ => None,
        })
        .collect();
    assert_eq!(pauses, [true, false]);
}

/// AC19: seek (table: playing, paused, past end, segmented, single-file).
#[test]
fn ac19_seek() {
    seek_while_playing();
    seek_while_paused();
    seek_past_end();
    seek_segmented();
    seek_single_file();
}

/// The first Position after a Seek is the target.
fn next_position(rig: &Rig) -> Duration {
    let events = rig.until("Position", |e| matches!(e, Event::Position(_)));
    positions(&events)[0]
}

fn seek_while_playing() {
    let rig = rig(held(4_608));
    let (_, full) = reference(flac16());
    let t = Duration::from_millis(500);
    rig.play(flac16());
    rig.until("Started", is_started);
    rig.send(Command::Seek(t));
    assert_eq!(next_position(&rig), t, "playing: position after seek");
    rig.sinks.release();
    let events = rig.until_end();
    assert!(
        matches!(events.last(), Some(Event::TrackEnded { .. })),
        "playing: {events:?}"
    );
    let first = duration_to_frames(t, 44_100) as usize;
    assert_samples(
        &after_last_discard(&rig),
        &full[first * 2..],
        "playing: after seek",
    );
}

fn seek_while_paused() {
    let rig = rig(held(4_608));
    let (_, full) = reference(flac16());
    let t = Duration::from_millis(250);
    rig.play(flac16());
    rig.until("Started", is_started);
    rig.send(Command::Pause);
    rig.until("Paused", |e| matches!(e, Event::Paused));
    assert!(
        matches!(rig.next(), Event::Position(_)),
        "paused: position at pause"
    );
    rig.send(Command::Seek(t));
    assert_eq!(
        rig.next(),
        Event::Position(t),
        "paused: position after seek"
    );
    rig.sinks.release();
    let quiet = rig.quiet_for(QUIET);
    assert!(quiet.is_empty(), "paused: events while paused: {quiet:?}");
    assert!(
        after_last_discard(&rig).is_empty(),
        "paused: written while paused"
    );
    rig.send(Command::Resume);
    let events = rig.until_end();
    assert!(
        matches!(events.last(), Some(Event::TrackEnded { .. })),
        "paused: {events:?}"
    );
    let first = duration_to_frames(t, 44_100) as usize;
    assert_samples(
        &after_last_discard(&rig),
        &full[first * 2..],
        "paused: after seek",
    );
}

fn seek_past_end() {
    let rig = rig(held(4_608));
    rig.play(flac16());
    rig.until("Started", is_started);
    rig.send(Command::Seek(Duration::from_secs(2)));
    let events = rig.until_end();
    assert!(
        matches!(events.last(), Some(Event::TrackEnded { .. })),
        "past end: {events:?}"
    );
    assert!(
        after_last_discard(&rig).is_empty(),
        "past end: nothing written"
    );
}

fn seek_segmented() {
    let rig = rig(held(4_096));
    let (_, full) = reference(flac24());
    let (timescale, spans) = dash24_spans();
    let t = Duration::from_millis(300);
    let source = dash24();
    let log = source.log();
    rig.play(source);
    rig.until("Started", is_started);
    rig.send(Command::Seek(t));
    assert_eq!(next_position(&rig), t, "segmented: position after seek");
    rig.sinks.release();
    let events = rig.until_end();
    assert!(
        matches!(events.last(), Some(Event::TrackEnded { .. })),
        "segmented: {events:?}"
    );
    let first = duration_to_frames(t, 96_000) as usize;
    assert_samples(
        &after_last_discard(&rig),
        &full[first * 2..],
        "segmented: after seek",
    );

    let entries = log.entries();
    let restart = entries
        .iter()
        .rposition(|f| matches!(f, Fetch::Restart { .. }))
        .unwrap_or_else(|| panic!("segmented: no restart in {entries:?}"));
    let target_ticks = t.as_nanos() * u128::from(timescale) / 1_000_000_000;
    for fetch in &entries[restart..] {
        if let Fetch::Segment(i) = fetch {
            assert!(
                u128::from(spans[*i].end()) > target_ticks,
                "segmented: fetched segment {i}, which ends before the target: {entries:?}"
            );
        }
    }
    assert!(entries[restart..].contains(&Fetch::Init));
}

/// The absolute byte offset of the last seek point at or before `frame`.
fn flac_seek_point(bytes: &[u8], frame: u64) -> u64 {
    let mut i = 4;
    let mut points = Vec::new();
    loop {
        let header = bytes[i];
        let len = u32::from_be_bytes([0, bytes[i + 1], bytes[i + 2], bytes[i + 3]]) as usize;
        if header & 0x7F == 3 {
            for p in bytes[i + 4..i + 4 + len].chunks_exact(18) {
                let sample = u64::from_be_bytes(p[..8].try_into().unwrap());
                let offset = u64::from_be_bytes(p[8..16].try_into().unwrap());
                points.push((sample, offset));
            }
        }
        i += 4 + len;
        if header & 0x80 != 0 {
            break;
        }
    }
    let first_frame = i as u64;
    let (_, offset) = points
        .into_iter()
        .rfind(|(sample, _)| *sample <= frame)
        .expect("a seek point");
    first_frame + offset
}

fn seek_single_file() {
    let rig = rig(held(4_608));
    let (_, full) = reference(flac16());
    let t = Duration::from_millis(700);
    let first = duration_to_frames(t, 44_100);
    let seek_point = flac_seek_point(&fixture("flac16_44.flac"), first);
    let source = flac16();
    let log = source.log();
    rig.play(source);
    rig.until("Started", is_started);
    // The worker decodes ahead; let it read the whole file so that every
    // later read is the seek's.
    assert!(
        log.wait_for(SOON, |f| *f == Fetch::End),
        "single-file: read to the end"
    );
    let mark = log.entries().len();
    rig.send(Command::Seek(t));
    assert_eq!(next_position(&rig), t, "single-file: position after seek");
    rig.sinks.release();
    let events = rig.until_end();
    assert!(
        matches!(events.last(), Some(Event::TrackEnded { .. })),
        "single-file: {events:?}"
    );
    assert_samples(
        &after_last_discard(&rig),
        &full[first as usize * 2..],
        "single-file: after seek",
    );
    let after = &log.entries()[mark..];
    assert!(
        after.iter().any(|f| matches!(f, Fetch::Seek { .. })),
        "single-file: no seek in {after:?}"
    );
    for fetch in after {
        if let Fetch::Read { offset, .. } = fetch {
            assert!(
                *offset >= seek_point,
                "single-file: read at {offset}, before the seek point {seek_point}: {after:?}"
            );
        }
    }
}

/// AC20: Position = (frames written − device delay) / rate, at most 250 ms
/// apart, never backwards.
#[test]
fn ac20_position_tracks_delay() {
    const DELAY: u64 = 1_000;
    let rig = rig(SinkScript {
        delay_frames: DELAY,
        ..SinkScript::default()
    });
    rig.play(flac16());
    let events = rig.until_end();
    let positions = positions(&events);
    assert!(positions.len() >= 3, "{positions:?}");

    // Every position is (a prefix of the written frames − the delay) / rate.
    let mut written = 0u64;
    let mut heard = vec![Duration::ZERO];
    for call in rig.sinks.calls() {
        if let SinkCall::Write { frames, .. } = call {
            written += frames as u64;
            heard.push(frames_to_duration(written.saturating_sub(DELAY), 44_100));
        }
    }
    for p in &positions {
        assert!(heard.contains(p), "{p:?} is not (written − delay) / rate");
    }
    for pair in positions.windows(2) {
        assert!(pair[1] >= pair[0], "backwards: {pair:?}");
        assert!(
            pair[1] - pair[0] <= Duration::from_millis(250),
            "too far apart: {pair:?}"
        );
    }
    let end = frames_to_duration(44_100 - DELAY, 44_100);
    let last = *positions.last().unwrap();
    assert!(
        end - last <= Duration::from_millis(250),
        "last {last:?}, end {end:?}"
    );
    assert!(
        rig.clock.now() >= Duration::from_millis(999),
        "the fake device paced the clock"
    );
}

/// AC21: SetDevice closes the old device before opening the new one, and
/// continues from the position heard on the old one.
#[test]
fn ac21_set_device_moves_playback() {
    const DELAY: u64 = 1_500;
    let devices = MemoryDevices::new().with_script(
        "a",
        SinkScript {
            delay_frames: DELAY,
            ..held(9_216)
        },
    );
    let rig = rig_with(devices, SinkScript::default(), "a");
    let (_, full) = reference(flac16());
    rig.play(flac16());
    rig.until("Started", is_started);
    rig.send(Command::SetDevice("b".into()));
    let mut events = rig.until_end();
    events.extend(rig.stop());
    assert!(
        matches!(events.iter().rev().nth(1), Some(Event::TrackEnded { .. })),
        "{events:?}"
    );

    let calls = rig.sinks.calls();
    let close_a = calls
        .iter()
        .position(|c| *c == SinkCall::Close { device: "a".into() })
        .expect("a closed");
    let open_b = calls
        .iter()
        .position(|c| matches!(c, SinkCall::Open { device, .. } if device == "b"))
        .expect("b opened");
    assert!(close_a < open_b, "close a before opening b: {calls:?}");
    assert_eq!(rig.sinks.max_open(), 1);

    let on_a = rig.sinks.samples_of("a").len() as u64 / 2;
    let heard = on_a - DELAY;
    assert_samples(
        &rig.sinks.samples_of("b"),
        &full[heard as usize * 2..],
        "b continues where a was heard",
    );
}

/// AC22: Stop and Shutdown return within 1 s with the source blocked in a
/// read that never returns; the device is closed; the thread is joined.
#[test]
fn ac22_stop_with_blocked_source() {
    const LIMIT: Duration = Duration::from_secs(1);
    for shutdown in [false, true] {
        let rig = rig(SinkScript::default());
        let (source, _gate) = flac16().block_after(12_000);
        rig.play(source);
        rig.until("Started", is_started);
        rig.send(if shutdown {
            Command::Shutdown
        } else {
            Command::Stop
        });
        let mut seen = Vec::new();
        loop {
            match rig.engine.events().recv_timeout(LIMIT) {
                Ok(Event::Stopped) => break,
                Ok(other) => seen.push(other),
                Err(_) => panic!(
                    "Stopped did not arrive within {LIMIT:?} (shutdown: {shutdown}); events: {seen:?}"
                ),
            }
        }
        assert!(
            matches!(rig.sinks.calls().last(), Some(SinkCall::Close { .. })),
            "device closed: {:?}",
            rig.sinks.calls()
        );
        assert_eq!(rig.sinks.open_now(), 0);
        if !shutdown {
            rig.send(Command::Shutdown);
        }
        let (done, joined) = std::sync::mpsc::channel();
        let engine = rig.engine;
        std::thread::spawn(move || {
            engine.join();
            let _ = done.send(());
        });
        assert!(
            joined.recv_timeout(LIMIT).is_ok(),
            "the engine thread was not joined within {LIMIT:?} (shutdown: {shutdown})"
        );
    }
}

/// AC23: a stalling source gives Buffering, then no writes and a frozen
/// position, then Buffered and normal playback; no underrun.
#[test]
fn ac23_buffering() {
    let rig = rig(SinkScript {
        delay_frames: 500,
        ..SinkScript::default()
    });
    let (_, expected) = reference(flac16());
    let (source, gate) = flac16().stall_after(12_000);
    rig.play(source);
    let before = rig.until("Buffering", |e| matches!(e, Event::Buffering));
    assert!(
        matches!(before.first(), Some(Event::Started { .. })),
        "{before:?}"
    );
    let written = rig.sinks.frames_written();
    assert!(
        written > 0 && written < 44_100,
        "{written} frames before the stall"
    );
    let quiet = rig.quiet_for(QUIET);
    assert!(quiet.is_empty(), "events while buffering: {quiet:?}");
    assert_eq!(
        rig.sinks.frames_written(),
        written,
        "no writes while buffering"
    );

    gate.open();
    let after = rig.until_end();
    let buffered = after
        .iter()
        .position(|e| *e == Event::Buffered)
        .unwrap_or_else(|| panic!("no Buffered in {after:?}"));
    assert!(
        positions(&after[..buffered]).is_empty(),
        "position moved: {after:?}"
    );
    assert!(
        matches!(after.last(), Some(Event::TrackEnded { .. })),
        "{after:?}"
    );
    assert_samples(&rig.sinks.samples(), &expected, "every frame exactly once");
    assert_eq!(rig.engine.underruns(), 0);
    let last_before = positions(&before).last().copied();
    let first_after = positions(&after).first().copied();
    if let (Some(a), Some(b)) = (last_before, first_after) {
        assert!(b >= a, "position went backwards across the stall");
    }
}

/// The tag an event is about, if it is track-scoped.
fn tag_of(event: &Event) -> Option<u64> {
    match event {
        Event::Started { tag, .. }
        | Event::Transitioned { tag, .. }
        | Event::TrackEnded { tag }
        | Event::Error { tag, .. } => Some(*tag),
        _ => None,
    }
}

/// The track-scoped events as `(kind, tag)`.
fn tagged(events: &[Event]) -> Vec<(&'static str, u64)> {
    events
        .iter()
        .filter_map(|e| {
            let kind = match e {
                Event::Started { .. } => "Started",
                Event::Transitioned { .. } => "Transitioned",
                Event::TrackEnded { .. } => "TrackEnded",
                Event::Error { .. } => "Error",
                _ => return None,
            };
            Some((kind, tag_of(e)?))
        })
        .collect()
}

/// 0004 AC13: track-scoped events carry the tag of their track: a plain
/// play, a gapless transition, a `Play` that replaces a playing track, and a
/// preloaded track that fails after the transition.
#[test]
fn ac13_events_carry_tags() {
    // Play, then a gapless preload.
    let rig = self::rig(SinkScript::default());
    rig.play_tagged(7, flac16());
    rig.preload(8, mono16());
    let events = rig.until_end();
    assert_eq!(
        tagged(&events),
        [("Started", 7), ("Transitioned", 8), ("TrackEnded", 8)],
        "{events:?}"
    );

    // A Play replaces a playing track: nothing of the first track follows.
    let rig = self::rig(held(4_608));
    rig.play_tagged(1, flac16());
    let first = rig.until("Started", is_started);
    assert_eq!(tagged(&first), [("Started", 1)]);
    rig.play_tagged(2, mono16());
    rig.sinks.release();
    let events = rig.until_end();
    assert_eq!(
        tagged(&events),
        [("Started", 2), ("TrackEnded", 2)],
        "{events:?}"
    );

    // A preloaded track fails after the transition: the error is its own.
    let rig = self::rig(SinkScript::default());
    rig.play_tagged(3, flac16());
    rig.preload(
        4,
        flac16().fail_after(12_000, SourceError::Network("connection reset".into())),
    );
    let events = rig.until_end();
    let kinds = tagged(&events);
    assert_eq!(kinds.first(), Some(&("Started", 3)), "{events:?}");
    assert_eq!(kinds.last(), Some(&("Error", 4)), "{events:?}");
    assert!(
        kinds[1..kinds.len() - 1]
            .iter()
            .all(|k| *k == ("Transitioned", 4)),
        "{events:?}"
    );
    assert_eq!(count(&events, |e| matches!(e, Event::TrackEnded { .. })), 0);
}

/// 0004 AC14: `CancelPreload` closes the cancelled source; the current
/// track drains and ends with its own tag; no frame of the cancelled track
/// reaches the sink. With nothing preloaded it does nothing.
#[test]
fn ac14_cancel_preload() {
    let rig = self::rig(held(4_608));
    let (_, a) = reference(flac16());
    let cancelled = mono16();
    let closed = cancelled.closed_watch();
    rig.play_tagged(1, flac16());
    rig.until("Started", is_started);
    rig.preload(2, cancelled);
    rig.send(Command::CancelPreload);
    rig.sinks.release();
    let events = rig.until_end();
    assert_eq!(
        events.last(),
        Some(&Event::TrackEnded { tag: 1 }),
        "{events:?}"
    );
    assert_eq!(
        tagged(&events),
        [("TrackEnded", 1)],
        "no Transitioned: {events:?}"
    );
    assert!(closed.wait(SOON), "the cancelled source was not closed");
    let calls = rig.sinks.calls();
    assert!(
        matches!(
            calls[calls.len() - 1],
            SinkCall::Drain { .. } | SinkCall::Close { .. }
        ),
        "{calls:?}"
    );
    assert_samples(&rig.sinks.samples(), &a, "only the current track");
    let after = rig.stop();
    assert_eq!(count(&after, |e| matches!(e, Event::TrackEnded { .. })), 0);

    // Nothing preloaded: nothing happens, the track plays on.
    let rig = self::rig(SinkScript::default());
    rig.play_tagged(5, flac16());
    rig.send(Command::CancelPreload);
    let events = rig.until_end();
    assert_eq!(tagged(&events), [("Started", 5), ("TrackEnded", 5)]);
    assert_samples(&rig.sinks.samples(), &a, "cancel with nothing preloaded");

    // Cancel, then preload again: the new preload is used.
    let rig = self::rig(held(4_608));
    let (_, b) = reference(mono16());
    rig.play_tagged(1, flac16());
    rig.until("Started", is_started);
    rig.preload(2, flac16());
    rig.send(Command::CancelPreload);
    rig.preload(3, mono16());
    rig.sinks.release();
    let events = rig.until_end();
    assert_eq!(
        tagged(&events),
        [("Transitioned", 3), ("TrackEnded", 3)],
        "{events:?}"
    );
    assert_samples(&rig.sinks.samples(), &[a, b].concat(), "re-preloaded");
}

/// The expected output for gain `gain`: untouched at 1.0, else
/// `round(s x gain)` in `f64`.
fn scaled(samples: &[i32], gain: f32) -> Vec<i32> {
    if gain == 1.0 {
        return samples.to_vec();
    }
    samples
        .iter()
        .map(|&s| (f64::from(s) * f64::from(gain)).round() as i32)
        .collect()
}

/// `actual` is `input` unscaled up to some frame `k`, then scaled by `gain`,
/// with `k` at most one write slice (50 ms) after frame `from` (the engine
/// may be inside a write when the command arrives).
fn assert_gain_change(actual: &[i32], input: &[i32], gain: f32, from: usize, what: &str) {
    assert_eq!(actual.len(), input.len(), "{what}: sample count");
    let slice = duration_to_frames(Duration::from_millis(50), 44_100) as usize;
    let boundary = (from..=from + slice).find(|&k| {
        actual[..k * 2] == input[..k * 2] && actual[k * 2..] == scaled(&input[k * 2..], gain)[..]
    });
    assert!(
        boundary.is_some(),
        "{what}: no unscaled-then-scaled boundary within {slice} frames after frame {from}"
    );
}

/// 0004 AC15: `SetGain`.
#[test]
fn ac15_gain() {
    // Table: whole tracks at a fixed gain, on 16- and 24-bit sources.
    type Source = fn() -> FakeSource;
    let sources: [(&str, Source); 2] = [("16-bit", flac16), ("24-bit", flac24)];
    let gains = [1.0f32, 0.5, 0.0, 0.3];
    for (name, source) in sources {
        let (_, baseline) = reference(source());
        // The engine without any SetGain is the bit-exact reference.
        let plain = rig(SinkScript::default());
        plain.play(source());
        plain.until_end();
        assert_samples(&plain.sinks.samples(), &baseline, name);
        for gain in gains {
            let rig = self::rig(SinkScript::default());
            rig.send(Command::SetGain(gain));
            rig.play(source());
            let events = rig.until_end();
            assert!(
                matches!(events.last(), Some(Event::TrackEnded { .. })),
                "{name} {gain}: {events:?}"
            );
            assert_samples(
                &rig.sinks.samples(),
                &scaled(&baseline, gain),
                &format!("{name} at gain {gain}"),
            );
        }
    }

    const HELD: usize = 4_608;
    let (_, a) = reference(flac16());
    let (_, b) = reference(mono16());

    // Change mid-track: frames written after the command are scaled, the
    // ones before are not (here the device holds exactly at the boundary).
    let rig = self::rig(held(HELD as u64));
    rig.play(flac16());
    rig.wait_frames(HELD);
    rig.send(Command::SetGain(0.5));
    rig.sinks.release();
    rig.until_end();
    assert_gain_change(
        &rig.sinks.samples(),
        &a,
        0.5,
        HELD,
        "gain changed mid-track",
    );

    // Across a gapless transition (set during the first track).
    let rig = self::rig(held(HELD as u64));
    rig.play(flac16());
    rig.wait_frames(HELD);
    rig.send(Command::SetGain(0.5));
    rig.preload(1, mono16());
    rig.sinks.release();
    let events = rig.until_end();
    assert_eq!(
        count(&events, |e| matches!(e, Event::Transitioned { .. })),
        1
    );
    assert_gain_change(
        &rig.sinks.samples(),
        &[a.clone(), b.clone()].concat(),
        0.5,
        HELD,
        "gain across a transition",
    );

    // Across a new Play.
    let rig = self::rig(held(HELD as u64));
    rig.play(flac16());
    rig.wait_frames(HELD);
    rig.send(Command::SetGain(0.5));
    rig.play(mono16());
    rig.sinks.release();
    rig.until_end();
    assert_samples(
        &after_last_discard(&rig),
        &scaled(&b, 0.5),
        "gain across a new Play",
    );

    // Across SetDevice: the replayed frames are scaled once, not twice.
    const DELAY: u64 = 1_500;
    let devices = MemoryDevices::new().with_script(
        "a",
        SinkScript {
            delay_frames: DELAY,
            ..held(9_216)
        },
    );
    let rig = rig_with(devices, SinkScript::default(), "a");
    rig.send(Command::SetGain(0.5));
    rig.play(flac16());
    rig.wait_frames(9_216);
    rig.send(Command::SetDevice("b".into()));
    rig.sinks.release();
    rig.until_end();
    let on_a = rig.sinks.samples_of("a");
    assert_samples(&on_a, &scaled(&a[..on_a.len()], 0.5), "device a");
    let heard = on_a.len() / 2 - DELAY as usize;
    assert_samples(
        &rig.sinks.samples_of("b"),
        &scaled(&a[heard * 2..], 0.5),
        "device b continues at the same gain",
    );
}

// --- spec 0005: releasing the device while paused ---------------------------

/// The fake device's delay in the 0005 tests: frames written but not heard.
const DEVICE_DELAY: u64 = 1_000;
/// The fake device holds after this many frames, so the engine is paused
/// at a known point.
const HOLD: u64 = 9_216;
/// The default release delay.
const RELEASE_DELAY: Duration = Duration::from_secs(10);

type Fixture = fn() -> FakeSource;

/// The 16- and 24-bit fixtures.
fn fixtures() -> [(&'static str, Fixture); 2] {
    [("16-bit", flac16), ("24-bit", flac24)]
}

/// A reserved fake device with a delay, held at [`HOLD`] frames.
fn reserved_device() -> SinkScript {
    SinkScript {
        delay_frames: DEVICE_DELAY,
        reserved: true,
        ..held(HOLD)
    }
}

/// An engine on [`reserved_device`]s (every device, `script` changes the
/// default) releasing after `delay`.
fn releasing(script: SinkScript, delay: Option<Duration>) -> Rig {
    rig_full(MemoryDevices::new(), script, "test", |config| {
        config.with_release_paused(delay)
    })
}

/// What the listener hears in an unpaused run of `source` on the same fake
/// device.
fn unpaused(source: Fixture) -> Vec<i32> {
    let rig = releasing(reserved_device(), None);
    rig.play(source());
    rig.until("Started", is_started);
    rig.sinks.release();
    let events = rig.until_end();
    assert!(
        matches!(events.last(), Some(Event::TrackEnded { .. })),
        "unpaused run: {events:?}"
    );
    rig.sinks.heard()
}

/// Opens, closes, reservations and releases logged so far.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Counts {
    opens: usize,
    closes: usize,
    reserves: usize,
    releases: usize,
}

fn counts(calls: &[SinkCall]) -> Counts {
    Counts {
        opens: count_calls(calls, |c| matches!(c, SinkCall::Open { .. })),
        closes: count_calls(calls, |c| matches!(c, SinkCall::Close { .. })),
        reserves: count_calls(calls, |c| matches!(c, SinkCall::Reserve { .. })),
        releases: count_calls(calls, |c| matches!(c, SinkCall::Release { .. })),
    }
}

fn counts_of(opens: usize, closes: usize) -> Counts {
    Counts {
        opens,
        closes,
        reserves: opens,
        releases: closes,
    }
}

/// Plays `source` until the device holds, then pauses; the position at
/// the pause.
fn play_and_pause(rig: &Rig, source: FakeSource) -> Duration {
    rig.play(source);
    rig.until("Started", is_started);
    rig.wait_frames(HOLD as usize);
    rig.send(Command::Pause);
    rig.until("Paused", |e| matches!(e, Event::Paused));
    match rig.next() {
        Event::Position(p) => p,
        other => panic!("expected a Position after Paused, got {other:?}"),
    }
}

/// Waits out the release delay on the fake clock: nothing is released
/// before it, `Released` comes once it passed (`None`: never, even an hour
/// later). Returns whether the device was released.
fn wait_for_release(rig: &Rig, delay: Option<Duration>, what: &str) -> bool {
    let not_released = |rig: &Rig, when: &str| {
        let quiet = rig.quiet_for(QUIET);
        assert!(
            !quiet.contains(&Event::Released),
            "{what}: released {when}: {quiet:?}"
        );
    };
    match delay {
        None => {
            not_released(rig, "at once");
            rig.clock.advance(Duration::from_secs(3_600));
            not_released(rig, "with the delay set to never");
            false
        }
        Some(delay) => {
            if !delay.is_zero() {
                not_released(rig, "at once");
                rig.clock.advance(delay - Duration::from_millis(1));
                not_released(rig, "before the delay");
                rig.clock.advance(Duration::from_millis(1));
            }
            let events = rig.until("Released", |e| *e == Event::Released);
            assert_eq!(events, [Event::Released], "{what}");
            true
        }
    }
}

/// The samples written after the `n`-th `Open` (1-based).
fn after_open(rig: &Rig, n: usize) -> Vec<i32> {
    let calls = rig.sinks.calls();
    let open = calls
        .iter()
        .enumerate()
        .filter(|(_, c)| matches!(c, SinkCall::Open { .. }))
        .nth(n - 1)
        .map(|(i, _)| i)
        .unwrap_or_else(|| panic!("no open #{n}: {calls:?}"));
    let before: usize = calls[..open]
        .iter()
        .map(|c| match c {
            SinkCall::Write { frames, .. } => *frames,
            _ => 0,
        })
        .sum();
    rig.sinks.samples()[before * 2..].to_vec()
}

/// 0005 AC19: release while paused (table: 16/24-bit; delay 0, 10 s and
/// never; resume, seek, play, stop and device change while released).
#[test]
fn ac19_release_while_paused() {
    #[derive(Debug, Clone, Copy)]
    enum Then {
        Resume,
        Seek,
        Play,
        Stop,
        SetDevice,
    }
    let delays = [Some(Duration::ZERO), Some(RELEASE_DELAY)];
    let thens = [
        Then::Resume,
        Then::Seek,
        Then::Play,
        Then::Stop,
        Then::SetDevice,
    ];
    for (fixture, source) in fixtures() {
        let (format, full) = reference(source());
        let rate = format.sample_rate;
        let expected = unpaused(source);
        assert_samples(&expected, &full, &format!("{fixture}: unpaused run"));

        // Never: paused for an hour, nothing is released (0003's pause).
        let what = format!("{fixture}, never");
        let rig = releasing(reserved_device(), None);
        let at_pause = play_and_pause(&rig, source());
        assert!(!wait_for_release(&rig, None, &what));
        assert_eq!(counts(&rig.sinks.calls()), counts_of(1, 0), "{what}");
        rig.sinks.release();
        rig.send(Command::Resume);
        assert_eq!(rig.next(), Event::Resumed, "{what}");
        assert_eq!(rig.next(), Event::Position(at_pause), "{what}");
        rig.until_end();
        assert_samples(&rig.sinks.heard(), &expected, &what);
        rig.stop(); // the device closes after TrackEnded
        assert_eq!(counts(&rig.sinks.calls()), counts_of(1, 1), "{what}");

        for delay in delays {
            for then in thens {
                let what = format!("{fixture}, delay {delay:?}, {then:?}");
                let rig = releasing(reserved_device(), delay);
                let at_pause = play_and_pause(&rig, source());
                assert!(wait_for_release(&rig, delay, &what));
                let calls = rig.sinks.calls();
                assert_eq!(counts(&calls), counts_of(1, 1), "{what}: {calls:?}");
                assert!(
                    matches!(
                        calls[calls.len() - 2..],
                        [SinkCall::Close { .. }, SinkCall::Release { .. }]
                    ),
                    "{what}: closed, then released: {calls:?}"
                );
                assert_eq!(rig.sinks.open_now(), 0, "{what}");
                let on_test = rig.sinks.samples_of("test").len() as u64 / 2;
                let first_unheard = (on_test - DEVICE_DELAY) as usize;
                assert_eq!(
                    at_pause,
                    frames_to_duration(first_unheard as u64, rate),
                    "{what}: position at the pause"
                );
                rig.sinks.release();
                match then {
                    Then::Resume => {
                        rig.send(Command::Resume);
                        assert_eq!(rig.next(), Event::Resumed, "{what}");
                        assert_eq!(rig.next(), Event::Position(at_pause), "{what}");
                        let events = rig.until_end();
                        assert!(
                            matches!(events.last(), Some(Event::TrackEnded { .. })),
                            "{what}: {events:?}"
                        );
                        assert_samples(
                            &after_open(&rig, 2),
                            &full[first_unheard * 2..],
                            &format!("{what}: from the first unheard frame"),
                        );
                        assert_samples(&rig.sinks.heard(), &expected, &what);
                        rig.stop(); // the device closes after TrackEnded
                        assert_eq!(counts(&rig.sinks.calls()), counts_of(2, 2), "{what}");
                    }
                    Then::Seek => {
                        let t = Duration::from_millis(300);
                        rig.send(Command::Seek(t));
                        assert_eq!(rig.next(), Event::Position(t), "{what}");
                        let quiet = rig.quiet_for(QUIET);
                        assert!(quiet.is_empty(), "{what}: {quiet:?}");
                        assert_eq!(counts(&rig.sinks.calls()), counts_of(1, 1), "{what}");
                        rig.send(Command::Resume);
                        assert_eq!(rig.next(), Event::Resumed, "{what}");
                        assert_eq!(rig.next(), Event::Position(t), "{what}");
                        rig.until_end();
                        let first = duration_to_frames(t, rate) as usize;
                        assert_samples(
                            &after_open(&rig, 2),
                            &full[first * 2..],
                            &format!("{what}: from the seek target"),
                        );
                        rig.stop(); // the device closes after TrackEnded
                        assert_eq!(counts(&rig.sinks.calls()), counts_of(2, 2), "{what}");
                    }
                    Then::Play => {
                        let (_, mono) = reference(mono16());
                        rig.play(mono16());
                        let events = rig.until_end();
                        assert!(
                            matches!(events.last(), Some(Event::TrackEnded { .. })),
                            "{what}: {events:?}"
                        );
                        assert_samples(&after_open(&rig, 2), &mono, &what);
                        rig.stop(); // the device closes after TrackEnded
                        let calls = rig.sinks.calls();
                        assert_eq!(counts(&calls), counts_of(2, 2), "{what}: {calls:?}");
                        assert_eq!(rig.sinks.max_open(), 1, "{what}");
                    }
                    Then::Stop => {
                        let events = rig.stop();
                        assert_eq!(events, [Event::Stopped], "{what}");
                        assert_eq!(counts(&rig.sinks.calls()), counts_of(1, 1), "{what}");
                        assert_eq!(rig.sinks.open_now(), 0, "{what}");
                    }
                    Then::SetDevice => {
                        rig.send(Command::SetDevice("b".into()));
                        let quiet = rig.quiet_for(QUIET);
                        assert!(quiet.is_empty(), "{what}: {quiet:?}");
                        assert_eq!(counts(&rig.sinks.calls()), counts_of(1, 1), "{what}");
                        rig.send(Command::Resume);
                        assert_eq!(rig.next(), Event::Resumed, "{what}");
                        rig.until_end();
                        assert_samples(
                            &rig.sinks.samples_of("b"),
                            &full[first_unheard * 2..],
                            &format!("{what}: b continues from the first unheard frame"),
                        );
                        rig.stop(); // the device closes after TrackEnded
                        assert_eq!(counts(&rig.sinks.calls()), counts_of(2, 2), "{what}");
                        assert_eq!(rig.sinks.max_open(), 1, "{what}");
                    }
                }
            }
        }
    }
}

/// Asks the engine to release the device; its answer and the sink calls
/// logged when it answered. Fails unless it answers within 400 ms.
fn ask_release(rig: &Rig) -> (bool, Vec<SinkCall>) {
    let (tx, rx) = std::sync::mpsc::channel();
    let sinks = rig.sinks.clone();
    assert!(
        rig.engine.release_requests().ask(move |answer| {
            let _ = tx.send((answer, sinks.calls()));
        }),
        "the engine is running"
    );
    rx.recv_timeout(tidal_player_audio::ANSWER_WITHIN)
        .expect("answered within 400 ms")
}

/// 0005 AC20: `RequestRelease` (table over engine states): `true` within
/// 400 ms while paused (released or not, any delay), with the PCM closed
/// before the answer and the reservation released after it; `false` while
/// loading, playing or buffering, which keeps the device.
#[test]
fn ac20_request_release() {
    // Refused: loading, playing, buffering.
    let rig = releasing(reserved_device(), Some(Duration::ZERO));
    let (source, gate) = flac16().block_after(0);
    rig.play(source);
    let quiet = rig.quiet_for(QUIET);
    assert!(quiet.is_empty(), "loading: {quiet:?}");
    let (answer, _) = ask_release(&rig);
    assert!(!answer, "loading");
    gate.open();
    rig.stop();

    let (_, full) = reference(flac16());
    let rig = releasing(reserved_device(), Some(Duration::ZERO));
    rig.play(flac16());
    rig.until("Started", is_started);
    let (answer, _) = ask_release(&rig);
    assert!(!answer, "playing");
    assert!(
        !rig.engine
            .release_requests()
            .request(tidal_player_audio::ANSWER_WITHIN),
        "playing, blocking request"
    );
    rig.sinks.release();
    let events = rig.until_end();
    assert!(!events.contains(&Event::Released), "playing: {events:?}");
    assert_samples(&rig.sinks.samples(), &full, "playing: kept the device");
    rig.stop(); // the device closes after TrackEnded
    assert_eq!(counts(&rig.sinks.calls()), counts_of(1, 1), "playing");

    let rig = releasing(
        SinkScript {
            delay_frames: 500,
            reserved: true,
            ..SinkScript::default()
        },
        Some(Duration::ZERO),
    );
    let (source, gate) = flac16().stall_after(12_000);
    rig.play(source);
    rig.until("Buffering", |e| matches!(e, Event::Buffering));
    let (answer, _) = ask_release(&rig);
    assert!(!answer, "buffering");
    gate.open();
    let events = rig.until_end();
    assert!(!events.contains(&Event::Released), "buffering: {events:?}");
    assert_samples(&rig.sinks.samples(), &full, "buffering: kept the device");

    // Answered: paused, before the delay (10 s, never) and after it (0).
    let rows = [
        ("paused, 10 s delay", Some(RELEASE_DELAY), false),
        ("paused, never", None, false),
        ("paused, released", Some(Duration::ZERO), true),
    ];
    let expected = unpaused(flac16);
    for (what, delay, already) in rows {
        let rig = releasing(reserved_device(), delay);
        let at_pause = play_and_pause(&rig, flac16());
        if already {
            assert!(wait_for_release(&rig, delay, what));
        }
        let (answer, at_answer) = ask_release(&rig);
        assert!(answer, "{what}");
        assert_eq!(
            counts(&at_answer),
            Counts {
                opens: 1,
                closes: 1,
                reserves: 1,
                releases: usize::from(already),
            },
            "{what}: the PCM is closed, the name not yet released, when answering: {at_answer:?}"
        );
        if !already {
            let events = rig.until("Released", |e| *e == Event::Released);
            assert_eq!(events, [Event::Released], "{what}");
        }
        assert_eq!(
            counts(&rig.sinks.calls()),
            counts_of(1, 1),
            "{what}: released after the answer"
        );
        // Asked again: still true, nothing closed or released twice.
        let (answer, _) = ask_release(&rig);
        assert!(answer, "{what}: asked again");
        assert_eq!(counts(&rig.sinks.calls()), counts_of(1, 1), "{what}");

        rig.sinks.release();
        rig.send(Command::Resume);
        assert_eq!(rig.next(), Event::Resumed, "{what}");
        assert_eq!(rig.next(), Event::Position(at_pause), "{what}");
        rig.until_end();
        assert_samples(&rig.sinks.heard(), &expected, what);
    }
}

/// 0005 AC21: a busy, missing or lost device on resume gives
/// `ResumeFailed(error)`, leaves nothing open, and a later `Resume` tries
/// again (table).
#[test]
fn ac21_resume_failure() {
    let errors = [
        SinkError::Busy {
            device: "test".into(),
            holder: Some("PipeWire".into()),
        },
        SinkError::NotFound("test".into()),
        SinkError::Lost("test".into()),
    ];
    let expected = unpaused(flac16);
    for error in errors {
        let what = format!("{error:?}");
        let rig = releasing(
            SinkScript {
                open_errors: vec![(2, error.clone())],
                ..reserved_device()
            },
            Some(Duration::ZERO),
        );
        let at_pause = play_and_pause(&rig, flac16());
        assert!(wait_for_release(&rig, Some(Duration::ZERO), &what));
        rig.sinks.release();
        let written = rig.sinks.frames_written();

        rig.send(Command::Resume);
        assert_eq!(rig.next(), Event::ResumeFailed(error.clone()), "{what}");
        let quiet = rig.quiet_for(QUIET);
        assert!(quiet.is_empty(), "{what}: still paused: {quiet:?}");
        assert_eq!(
            rig.sinks.frames_written(),
            written,
            "{what}: nothing written"
        );
        assert_eq!(rig.sinks.open_now(), 0, "{what}: nothing open");
        assert_eq!(counts(&rig.sinks.calls()), counts_of(1, 1), "{what}");

        rig.send(Command::Resume);
        assert_eq!(rig.next(), Event::Resumed, "{what}: tried again");
        assert_eq!(rig.next(), Event::Position(at_pause), "{what}");
        let events = rig.until_end();
        assert!(
            matches!(events.last(), Some(Event::TrackEnded { .. })),
            "{what}: {events:?}"
        );
        assert_samples(&rig.sinks.heard(), &expected, &what);
        rig.stop(); // the device closes after TrackEnded
        assert_eq!(counts(&rig.sinks.calls()), counts_of(2, 2), "{what}");
    }
}
