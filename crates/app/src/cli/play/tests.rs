use super::*;
use crate::settings::resolve_play_config;
use tidal_player_api::stream::Unsupported;

const FLAC_24_96: SourceFormat = SourceFormat {
    codec: Codec::Flac,
    sample_rate: 96_000,
    channels: 2,
    bits_per_sample: Some(24),
};
const FLAC_16_44: SourceFormat = SourceFormat {
    codec: Codec::Flac,
    sample_rate: 44_100,
    channels: 2,
    bits_per_sample: Some(16),
};
const FLAC_MONO: SourceFormat = SourceFormat {
    codec: Codec::Flac,
    sample_rate: 48_000,
    channels: 1,
    bits_per_sample: Some(16),
};
const AAC: SourceFormat = SourceFormat {
    codec: Codec::AacLc,
    sample_rate: 44_100,
    channels: 2,
    bits_per_sample: None,
};

fn output(
    device: &str,
    kind: OutputKind,
    format: SampleFormat,
    rate: u32,
    reason: Option<&str>,
) -> OutputInfo {
    OutputInfo {
        requested: device.replace("plughw", "hw"),
        device: device.into(),
        kind,
        sample_format: format,
        sample_rate: rate,
        channels: 2,
        bit_perfect: reason.is_none(),
        not_bit_perfect_reason: reason.map(Into::into),
    }
}

#[test]
fn ac26_status_lines() {
    let tracks = [
        (
            AudioQuality::HiResLossless,
            FLAC_24_96,
            "Track 77640617: HI_RES_LOSSLESS, FLAC 24-bit 96 kHz stereo",
        ),
        (
            AudioQuality::Lossless,
            FLAC_16_44,
            "Track 77640617: LOSSLESS, FLAC 16-bit 44.1 kHz stereo",
        ),
        (
            AudioQuality::Lossless,
            FLAC_MONO,
            "Track 77640617: LOSSLESS, FLAC 16-bit 48 kHz mono",
        ),
        // Asked for hi-res, granted less: the line shows the grant.
        (
            AudioQuality::High,
            AAC,
            "Track 77640617: HIGH, AAC 44.1 kHz stereo",
        ),
    ];
    for (granted, source, want) in tracks {
        assert_eq!(track_line(77_640_617, granted, &source), want);
    }

    let outputs = [
        (
            output(
                "hw:1,0",
                OutputKind::Exclusive,
                SampleFormat::S32Le,
                96_000,
                None,
            ),
            "Output: hw:1,0 (exclusive) S32_LE 96 kHz 2 ch, bit-perfect",
        ),
        (
            output(
                "hw:1,0",
                OutputKind::Exclusive,
                SampleFormat::S24_3Le,
                44_100,
                None,
            ),
            "Output: hw:1,0 (exclusive) S24_3LE 44.1 kHz 2 ch, bit-perfect",
        ),
        (
            output(
                "plughw:1,0",
                OutputKind::Fallback,
                SampleFormat::S24Le,
                96_000,
                Some("resampled (plughw fallback: device refused S24_3LE/S24_LE/S32_LE at 96 kHz)"),
            ),
            "Output: plughw:1,0 (fallback) S24_LE 96 kHz 2 ch, resampled (plughw fallback: \
                 device refused S24_3LE/S24_LE/S32_LE at 96 kHz)",
        ),
        (
            output(
                "default",
                OutputKind::Shared,
                SampleFormat::S16Le,
                48_000,
                Some("shared (system mixer)"),
            ),
            "Output: default (shared) S16_LE 48 kHz 2 ch, shared (system mixer)",
        ),
        (
            output(
                "hw:1,0",
                OutputKind::Exclusive,
                SampleFormat::S32Le,
                44_100,
                Some("lossy source"),
            ),
            "Output: hw:1,0 (exclusive) S32_LE 44.1 kHz 2 ch, lossy source",
        ),
    ];
    for (info, want) in outputs {
        assert_eq!(output_line(&info), want);
    }

    let progress = [
        (
            Duration::from_secs(83),
            Some(Duration::from_secs(296)),
            "  1:23 / 4:56",
        ),
        (
            Duration::ZERO,
            Some(Duration::from_millis(291_008)),
            "  0:00 / 4:51",
        ),
        (Duration::from_millis(5_999), None, "  0:05 / ?:??"),
        (
            Duration::from_secs(3_725),
            Some(Duration::from_secs(4_000)),
            "  1:02:05 / 1:06:40",
        ),
    ];
    for (position, duration, want) in progress {
        assert_eq!(progress_line(position, duration), want);
    }
}

#[test]
fn ac26_error_messages() {
    let stream = [
        (
            StreamError::PreviewOnly,
            "Track 123 is only available as a preview for this account",
        ),
        (
            StreamError::NotAvailable,
            "Track 123 is not available in NO",
        ),
        (
            StreamError::NotFound,
            "Track 123 was not found, or cannot be streamed in NO",
        ),
        (
            StreamError::Unsupported(Unsupported::Codec("mp4a.40.5".into())),
            "Track 123 is not playable: codec mp4a.40.5 is not supported",
        ),
        (
            StreamError::Auth(AuthError::LoginRequired),
            "Session expired: run \"tidal-player login\"",
        ),
        (StreamError::Server(500), "Tidal server error (HTTP 500)"),
    ];
    for (error, want) in stream {
        assert_eq!(stream_error_message(123, "NO", &error), want, "{error:?}");
    }

    let engine = [
        (
            EngineError::Output(SinkError::Busy {
                device: "hw:1,0".into(),
                holder: Some("PipeWire".into()),
            }),
            "Output hw:1,0 is busy (used by PipeWire): close it, or use --device default",
        ),
        (
            EngineError::Output(SinkError::Busy {
                device: "hw:1,0".into(),
                holder: None,
            }),
            "Output hw:1,0 is busy: close it, or use --device default",
        ),
        (
            EngineError::Output(SinkError::NotFound("hw:5,0".into())),
            "No such output device hw:5,0: see \"tidal-player devices\"",
        ),
        (
            EngineError::Output(SinkError::Lost("hw:1,0".into())),
            "Output hw:1,0 was lost",
        ),
        (
            EngineError::Source(SourceError::Network("timed out".into())),
            "Network error while streaming track 123",
        ),
        (
            EngineError::Unsupported("HE-AAC".into()),
            "Track 123 is not playable: HE-AAC",
        ),
    ];
    for (error, want) in engine {
        assert_eq!(engine_error_message(123, &error), want, "{error:?}");
    }
}

fn started() -> Event {
    Event::Started {
        tag: 0,
        source: FLAC_24_96,
        output: output(
            "hw:1,0",
            OutputKind::Exclusive,
            SampleFormat::S32Le,
            96_000,
            None,
        ),
    }
}

fn report(tty: bool, events: &[Event]) -> (String, Option<Outcome>) {
    let mut reporter = Reporter::new(
        77_640_617,
        AudioQuality::HiResLossless,
        Some(Duration::from_secs(296)),
        tty,
    );
    let mut out = Vec::new();
    let mut outcome = None;
    for event in events {
        if let Some(o) = reporter.on_event(event, &mut out).unwrap() {
            outcome = Some(o);
            break;
        }
    }
    reporter.finish(&mut out).unwrap();
    (String::from_utf8(out).unwrap(), outcome)
}

#[test]
fn ac26_progress_only_on_terminal() {
    let events = [
        started(),
        Event::Position(Duration::from_secs(1)),
        Event::Position(Duration::from_secs(83)),
        Event::TrackEnded { tag: 0 },
    ];
    let lines = "Track 77640617: HI_RES_LOSSLESS, FLAC 24-bit 96 kHz stereo\n\
                     Output: hw:1,0 (exclusive) S32_LE 96 kHz 2 ch, bit-perfect\n";

    let (out, outcome) = report(false, &events);
    assert_eq!(out, lines);
    assert_eq!(outcome, Some(Outcome::Ended));

    let (out, outcome) = report(true, &events);
    assert_eq!(
        out,
        format!("{lines}\r\x1b[K  0:00 / 4:56\r\x1b[K  0:01 / 4:56\r\x1b[K  1:23 / 4:56\n")
    );
    assert_eq!(outcome, Some(Outcome::Ended));

    let lost = EngineError::Output(SinkError::Lost("hw:1,0".into()));
    let (out, outcome) = report(
        false,
        &[
            started(),
            Event::Error {
                tag: 0,
                error: lost,
            },
        ],
    );
    assert_eq!(out, lines);
    assert_eq!(
        outcome,
        Some(Outcome::Failed("Output hw:1,0 was lost".into()))
    );
}

// --- spec 0004 ---------------------------------------------------------------

use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use crate::player::runtime::fakes::{FakeEngine, FakeJobs, Log, Script, not_available, track};
use crate::player::runtime::parse_items;
use tidal_player_api::auth::BoxFuture;
use tidal_player_api::metadata::MetadataError;
use tidal_player_core::TrackId;
use tidal_player_core::player::{self, PlayerConfig, PlayerInput, PlayerState};

const SEED: u64 = 42;

/// Metadata from memory; records each request.
#[derive(Default)]
struct FakeMeta {
    requests: Mutex<Vec<String>>,
}

impl FakeMeta {
    fn answer<T: Send + 'static>(
        &self,
        request: String,
        result: Result<T, MetadataError>,
    ) -> BoxFuture<'_, Result<T, MetadataError>> {
        self.requests.lock().unwrap().push(request);
        Box::pin(async move { result })
    }
}

impl Metadata for FakeMeta {
    fn track(&self, id: TrackId) -> BoxFuture<'_, Result<Track, MetadataError>> {
        let result = match id.0 {
            1..=11 => Ok(track(id.0, (id.0 != 11).then_some(200))),
            _ => Err(MetadataError::NotFound(Item::Track(id))),
        };
        self.answer(format!("track {id}"), result)
    }

    fn album(&self, id: u64) -> BoxFuture<'_, Result<Vec<Track>, MetadataError>> {
        let ids: &[u64] = match id {
            10 => &[1, 2],
            20 => &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
            _ => &[],
        };
        let result = if ids.is_empty() {
            Err(MetadataError::NotFound(Item::Album(id)))
        } else {
            Ok(ids.iter().map(|i| track(*i, Some(200))).collect())
        };
        self.answer(format!("album {id}"), result)
    }

    fn playlist(&self, uuid: String) -> BoxFuture<'_, Result<Vec<Track>, MetadataError>> {
        let result = match uuid.as_str() {
            "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d" => {
                Ok(vec![track(4, Some(200)), track(5, Some(200))])
            }
            _ => Err(MetadataError::NotFound(Item::Playlist(uuid.clone()))),
        };
        self.answer(format!("playlist {uuid}"), result)
    }

    fn suggestions(&self, seed: TrackId) -> BoxFuture<'_, Result<Vec<Track>, MetadataError>> {
        self.answer(format!("suggestions {seed}"), Ok(Vec::new()))
    }
}

fn status_lines(id: u64) -> String {
    format!(
        "Track {id}: LOSSLESS, FLAC 16-bit 44.1 kHz stereo\n\
             Output: hw:1,0 (exclusive) S32_LE 44.1 kHz 2 ch, bit-perfect\n"
    )
}

/// The play order the player gives `ids` with shuffle on and `SEED`.
fn shuffled(ids: &[u64]) -> Vec<u64> {
    let mut state = PlayerState::new(PlayerConfig::default(), SEED);
    let tracks = ids.iter().map(|i| track(*i, Some(200))).collect();
    player::update(&mut state, PlayerInput::Command(Command::ToggleShuffle));
    player::update(
        &mut state,
        PlayerInput::Command(Command::LoadQueue { tracks, start: 0 }),
    );
    state
        .snapshot()
        .queue
        .iter()
        .map(|e| e.track.id.0)
        .collect()
}

#[derive(Default)]
struct Row {
    name: &'static str,
    items: &'static [&'static str],
    options: PlayOptions,
    /// Tracks whose resolution fails (not available).
    unavailable: &'static [u64],
    /// Per track, the engine's outcome of each play (default: ends).
    engine: Vec<(u64, Vec<Script>)>,
    tty: bool,
    code: u8,
    /// The tracks whose "Track"/"Output" lines are printed, in order.
    started: Vec<u64>,
    stderr: &'static str,
}

/// Runs `play` as the binary does, against the fakes: parse, expand,
/// then the queue through the player runtime.
fn run_play(row: Row) -> (u8, String, String, Vec<String>) {
    let log: Log = Arc::default();
    let (tx, rx) = mpsc::channel();
    let mut engine = FakeEngine::new(&log, Script::Ends);
    for (track, outcomes) in row.engine {
        engine = engine.script(track, outcomes);
    }
    let mut jobs = FakeJobs::new(&log, &tx, true);
    for id in row.unavailable {
        jobs.failures.insert(*id, not_available(*id));
    }
    jobs.suggestions = vec![track(11, None), track(3, Some(200))];
    let meta = FakeMeta::default();
    let args: Vec<String> = row.items.iter().map(|s| (*s).to_owned()).collect();
    let tokio = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let tracks = parse_items(&args)
        .and_then(|items| tokio.block_on(play_tracks(&meta, &items, &row.options, false)));
    let code = match tracks {
        Err(e) => {
            writeln!(err, "{e}").unwrap();
            e.exit_code()
        }
        Ok(tracks) => {
            let settings = resolve_play_config(row.options.autoplay, |_| None).unwrap();
            let mut config = settings.player;
            config.country = Some("NO".into());
            let mut runtime = PlayerRuntime::new(config, SEED, engine, jobs);
            let mut polls = 0;
            let mut interrupted = || {
                polls += 1;
                polls > 10_000
            };
            let mut output = PlayOutput {
                out: &mut out,
                err: &mut err,
                tty: row.tty,
            };
            play_queue(
                &mut runtime,
                tracks,
                &row.options,
                &rx,
                Duration::ZERO,
                &mut interrupted,
                &mut output,
            )
        }
    };
    let requests = meta.requests.lock().unwrap().clone();
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
        requests,
    )
}

/// Spec 0025 AC13: one track ID skips the metadata request unless a hook
/// needs the track's title.
#[test]
fn ac13_play_asks_for_the_hooks_metadata() {
    let tokio = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    for (hooked, title, requests) in [
        (false, "", &[][..]),
        (true, "Title 1", &["track 1".to_owned()][..]),
    ] {
        let meta = FakeMeta::default();
        let items = [Item::Track(TrackId(1))];
        let tracks = tokio
            .block_on(play_tracks(&meta, &items, &PlayOptions::default(), hooked))
            .unwrap();
        assert_eq!(tracks[0].title, title, "hooked: {hooked}");
        assert_eq!(*meta.requests.lock().unwrap(), requests, "hooked: {hooked}");
    }
}

/// AC18: several items expand in order into one queue played through
/// the player; the status lines at each start; the exit codes.
#[test]
fn ac18_queue_exit_codes() {
    let busy = || {
        EngineError::Output(SinkError::Busy {
            device: "hw:1,0".into(),
            holder: None,
        })
    };
    let lost = || EngineError::Output(SinkError::Lost("hw:1,0".into()));
    let ten = shuffled(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
    assert_eq!(ten[0], 1);
    assert_ne!(ten, vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
    let rows = vec![
        Row {
            name: "all play: album, track, playlist in order",
            items: &[
                "https://tidal.com/browse/album/10",
                "7",
                "https://tidal.com/browse/playlist/0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d",
            ],
            started: vec![1, 2, 7, 4, 5],
            ..Row::default()
        },
        Row {
            name: "one track-only failure among three",
            items: &["1", "2", "3"],
            unavailable: &[2],
            started: vec![1, 3],
            stderr: "Track 2 is not available in NO\n",
            ..Row::default()
        },
        Row {
            name: "a decode failure among three",
            items: &["1", "2", "3"],
            engine: vec![(
                2,
                vec![Script::Fails(EngineError::Decode("bad frame".into()))],
            )],
            started: vec![1, 3],
            stderr: "Track 2 could not be decoded: bad frame\n",
            ..Row::default()
        },
        Row {
            name: "the last track fails: the queue still ran out",
            items: &["1", "2"],
            unavailable: &[2],
            started: vec![1],
            stderr: "Track 2 is not available in NO\n",
            ..Row::default()
        },
        Row {
            name: "output busy",
            items: &["1", "2", "3"],
            engine: vec![(2, vec![Script::Fails(busy())])],
            code: 1,
            started: vec![1],
            stderr: "Output hw:1,0 is busy: close it, or use --device default\n",
            ..Row::default()
        },
        Row {
            name: "every track fails",
            items: &["1", "2", "3"],
            unavailable: &[1, 2, 3],
            code: 1,
            stderr: "Track 1 is not available in NO\n\
                         Track 2 is not available in NO\n\
                         Track 3 is not available in NO\n\
                         Stopped: 3 tracks in a row could not be played\n",
            ..Row::default()
        },
        Row {
            name: "bad item",
            items: &["1", "https://tidal.com/browse/artist/1"],
            code: 2,
            stderr: "Not a Tidal track, album or playlist: https://tidal.com/browse/artist/1\n",
            ..Row::default()
        },
        Row {
            name: "item not found",
            items: &["1", "https://tidal.com/browse/album/404"],
            code: 1,
            stderr: "Album 404 was not found\n",
            ..Row::default()
        },
        Row {
            name: "--shuffle before loading",
            items: &["https://tidal.com/album/20"],
            options: PlayOptions {
                shuffle: true,
                ..PlayOptions::default()
            },
            started: ten,
            ..Row::default()
        },
        Row {
            name: "--repeat track before loading",
            items: &["1", "2"],
            options: PlayOptions {
                repeat: RepeatMode::Track,
                ..PlayOptions::default()
            },
            engine: vec![(1, vec![Script::Ends, Script::Ends, Script::Fails(lost())])],
            code: 1,
            started: vec![1, 1],
            stderr: "Output hw:1,0 was lost\n",
            ..Row::default()
        },
        Row {
            name: "--repeat queue before loading",
            items: &["1", "2"],
            options: PlayOptions {
                repeat: RepeatMode::Queue,
                ..PlayOptions::default()
            },
            engine: vec![(1, vec![Script::Ends, Script::Fails(lost())])],
            code: 1,
            started: vec![1, 2],
            stderr: "Output hw:1,0 was lost\n",
            ..Row::default()
        },
        Row {
            name: "--autoplay continues with suggestions",
            items: &["11"],
            options: PlayOptions {
                autoplay: true,
                ..PlayOptions::default()
            },
            started: vec![11, 3],
            ..Row::default()
        },
        Row {
            name: "progress on a terminal",
            items: &["1", "2"],
            tty: true,
            started: vec![1, 2],
            ..Row::default()
        },
    ];
    for row in rows {
        let name = row.name;
        let (code, started, stderr, tty) = (row.code, row.started.clone(), row.stderr, row.tty);
        let (got_code, out, err, _) = run_play(row);
        let progress = if tty { "\r\x1b[K  0:00 / 3:20\n" } else { "" };
        let want_out: String = started
            .iter()
            .map(|id| format!("{}{progress}", status_lines(*id)))
            .collect();
        assert_eq!(out, want_out, "{name}: stdout");
        assert_eq!(err, stderr, "{name}: stderr");
        assert_eq!(got_code, code, "{name}: exit code");
    }

    // 0003's form: one track ID, no new flag: no metadata request, and
    // the track plays (and its errors are the stream's).
    let (code, out, err, requests) = run_play(Row {
        items: &["99"],
        ..Row::default()
    });
    assert_eq!((code, out, err), (0, status_lines(99), String::new()));
    assert!(requests.is_empty(), "{requests:?}");
    let (code, _, err, _) = run_play(Row {
        items: &["99"],
        unavailable: &[99],
        ..Row::default()
    });
    assert_eq!(
        (code, err.as_str()),
        (1, "Track 99 is not available in NO\n")
    );
}
