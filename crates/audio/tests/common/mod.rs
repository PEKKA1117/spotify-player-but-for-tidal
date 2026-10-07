//! Shared helpers for the decode and engine tests (spec 0003 test plan).
#![allow(dead_code)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tidal_player_audio::clock::FakeClock;
use tidal_player_audio::decode::decode_all;
use tidal_player_audio::testing::FakeSource;
use tidal_player_audio::{
    Command, Engine, EngineConfig, Event, MemoryDevices, MemorySinkHandle, SegmentSpan, SinkScript,
    SourceFormat,
};

/// Generous bound for anything the engine should do "soon".
pub const SOON: Duration = Duration::from_secs(10);

pub fn fixture(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// 16-bit 44.1 kHz stereo FLAC, 1 s, single file with a dense seek table.
pub fn flac16() -> FakeSource {
    FakeSource::file(fixture("flac16_44.flac"), "flac")
}

/// 16-bit 44.1 kHz mono FLAC, 0.5 s.
pub fn mono16() -> FakeSource {
    FakeSource::file(fixture("mono16_44.flac"), "flac")
}

/// 24-bit 96 kHz stereo FLAC, 0.6 s, single file (the reference for `dash24`).
pub fn flac24() -> FakeSource {
    FakeSource::file(fixture("flac24_96.flac"), "flac")
}

/// The segments listed in `flac24_96_dash-segments.txt`: (timescale, spans).
pub fn dash24_spans() -> (u32, Vec<SegmentSpan>) {
    let text = String::from_utf8(fixture("flac24_96_dash-segments.txt")).unwrap();
    let mut lines = text.lines();
    let timescale = lines
        .next()
        .and_then(|l| l.strip_prefix("timescale "))
        .unwrap()
        .parse()
        .unwrap();
    let spans = lines
        .map(|l| {
            let v: Vec<u64> = l.split(' ').map(|x| x.parse().unwrap()).collect();
            SegmentSpan {
                start: v[1],
                duration: v[2],
            }
        })
        .collect();
    (timescale, spans)
}

/// The 24/96 FLAC in fragmented MP4: init segment + media segments.
pub fn dash24() -> FakeSource {
    let (timescale, spans) = dash24_spans();
    let segments = spans
        .into_iter()
        .enumerate()
        .map(|(i, span)| (fixture(&format!("flac24_96_dash-{}.m4s", i + 1)), span))
        .collect();
    FakeSource::segmented(fixture("flac24_96_dash-init.mp4"), segments, timescale)
}

/// The reference decode of a source: format and interleaved stereo samples.
pub fn reference(source: FakeSource) -> (SourceFormat, Vec<i32>) {
    decode_all(Box::new(source)).expect("reference decode")
}

/// Little-endian signed PCM of `bytes_per_sample` bytes, left-justified to i32.
pub fn pcm_left_justified(bytes: &[u8], bytes_per_sample: usize) -> Vec<i32> {
    bytes
        .chunks_exact(bytes_per_sample)
        .map(|b| {
            let mut word = [0u8; 4];
            word[4 - bytes_per_sample..].copy_from_slice(b);
            i32::from_le_bytes(word)
        })
        .collect()
}

/// An engine on fake devices and a fake clock.
pub struct Rig {
    pub engine: Engine,
    pub sinks: MemorySinkHandle,
    pub clock: FakeClock,
}

/// Every device uses `script` (its clock is replaced by the rig's).
pub fn rig(script: SinkScript) -> Rig {
    rig_with(MemoryDevices::new(), script, "test")
}

/// `devices` (their scripts as given) with `default` for other devices; the
/// engine starts on `device`.
pub fn rig_with(devices: MemoryDevices, default: SinkScript, device: &str) -> Rig {
    let clock = FakeClock::new();
    let devices = devices.with_default_script(SinkScript {
        clock: Some(clock.clone()),
        ..default
    });
    let sinks = devices.handle();
    let config = EngineConfig::new(device).with_clock(Arc::new(clock.clone()));
    let engine = Engine::spawn(Box::new(devices), config);
    Rig {
        engine,
        sinks,
        clock,
    }
}

impl Rig {
    pub fn send(&self, command: Command) {
        self.engine.send(command).expect("engine running");
    }

    pub fn play(&self, source: FakeSource) {
        self.send(Command::Play {
            source: Box::new(source),
            start_at: Duration::ZERO,
        });
    }

    /// The next event; panics after `SOON`.
    pub fn next(&self) -> Event {
        self.engine
            .events()
            .recv_timeout(SOON)
            .unwrap_or_else(|_| panic!("no event within {SOON:?}"))
    }

    /// Events up to and including the first one matching `pred`.
    pub fn until(&self, what: &str, pred: impl Fn(&Event) -> bool) -> Vec<Event> {
        let mut seen = Vec::new();
        loop {
            match self.engine.events().recv_timeout(SOON) {
                Ok(event) => {
                    let done = pred(&event);
                    seen.push(event);
                    if done {
                        return seen;
                    }
                }
                Err(_) => panic!("timed out waiting for {what}; events so far: {seen:?}"),
            }
        }
    }

    /// Events until the track ends or fails (inclusive).
    pub fn until_end(&self) -> Vec<Event> {
        self.until("TrackEnded or Error", |e| {
            matches!(e, Event::TrackEnded | Event::Error(_))
        })
    }

    /// Events that arrive within `wait` (a bounded wait, for "nothing
    /// happens" checks).
    pub fn quiet_for(&self, wait: Duration) -> Vec<Event> {
        let mut seen = Vec::new();
        while let Ok(event) = self.engine.events().recv_timeout(wait) {
            seen.push(event);
        }
        seen
    }

    /// `Stop`, then the events up to `Stopped`: proves nothing else was
    /// pending (commands and events are ordered).
    pub fn stop(&self) -> Vec<Event> {
        self.send(Command::Stop);
        self.until("Stopped", |e| matches!(e, Event::Stopped))
    }
}

pub fn count(events: &[Event], pred: impl Fn(&Event) -> bool) -> usize {
    events.iter().filter(|e| pred(e)).count()
}

pub fn positions(events: &[Event]) -> Vec<Duration> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::Position(p) => Some(*p),
            _ => None,
        })
        .collect()
}
