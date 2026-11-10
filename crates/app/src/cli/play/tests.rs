use super::*;
use tidal_player_api::stream::Unsupported;

#[test]
fn ac26_settings_precedence() {
    type Row = (
        &'static str,
        Option<&'static str>,
        Option<&'static str>,
        &'static [(&'static str, &'static str)],
        Result<(AudioQuality, &'static str), &'static str>,
    );
    let rows: &[Row] = &[
        (
            "defaults",
            None,
            None,
            &[],
            Ok((AudioQuality::HiResLossless, "default")),
        ),
        (
            "environment",
            None,
            None,
            &[(QUALITY_VAR, "high"), (DEVICE_VAR, "hw:1,0")],
            Ok((AudioQuality::High, "hw:1,0")),
        ),
        (
            "flags beat environment",
            Some("lossless"),
            Some("hw:2,0"),
            &[(QUALITY_VAR, "high"), (DEVICE_VAR, "hw:1,0")],
            Ok((AudioQuality::Lossless, "hw:2,0")),
        ),
        (
            "empty environment counts as unset",
            None,
            None,
            &[(QUALITY_VAR, ""), (DEVICE_VAR, "")],
            Ok((AudioQuality::HiResLossless, "default")),
        ),
        (
            "flag beats a bad environment value",
            Some("hi-res"),
            None,
            &[(QUALITY_VAR, "ultra")],
            Ok((AudioQuality::HiResLossless, "default")),
        ),
        ("unknown flag", Some("ultra"), None, &[], Err("--quality")),
        ("low flag", Some("low"), None, &[], Err("--quality")),
        (
            "unknown environment",
            None,
            None,
            &[(QUALITY_VAR, "ultra")],
            Err(QUALITY_VAR),
        ),
        (
            "low environment",
            None,
            None,
            &[(QUALITY_VAR, "low")],
            Err(QUALITY_VAR),
        ),
    ];
    for (name, quality, device, env, want) in rows {
        let lookup = |key: &str| {
            env.iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| (*v).to_string())
        };
        let got = resolve_settings(*quality, *device, lookup);
        match want {
            Ok((q, d)) => assert_eq!(
                got,
                Ok(Settings {
                    quality: *q,
                    device: (*d).into()
                }),
                "{name}"
            ),
            Err(setting) => {
                let err = got.expect_err(name);
                assert_eq!(err.setting, *setting, "{name}");
                if quality == &Some("low") || env.iter().any(|(_, v)| *v == "low") {
                    assert!(err.to_string().contains("HE-AAC"), "{name}: {err}");
                }
            }
        }
    }
    assert_eq!(configured_device(|_| None), "default");
    assert_eq!(
        configured_device(|k| (k == DEVICE_VAR).then(|| "hw:1,0".into())),
        "hw:1,0"
    );
}

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
use tidal_player_core::library::DEFAULT_HIDDEN_VERSIONS;
use tidal_player_core::player::{
    self, EngineEvent, PlayerEffect, PlayerInput, PlayerState, Purpose, TrackDetails,
};
use tidal_player_core::{EntryId, TrackId};

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

/// AC25: the player settings from the environment, their ranges, and
/// `--autoplay` over the environment.
#[test]
fn ac25_player_config() {
    type Want = Result<(u8, u64, u64, bool), (&'static str, &'static str)>;
    type Env = &'static [(&'static str, &'static str)];
    let rows: &[(&str, Env, Want)] = &[
        ("defaults", &[], Ok((5, 5, 3, false))),
        (
            "empty counts as unset",
            &[
                (VOLUME_STEP_VAR, ""),
                (SEEK_STEP_VAR, ""),
                (PREVIOUS_RESTART_VAR, ""),
                (AUTOPLAY_VAR, ""),
            ],
            Ok((5, 5, 3, false)),
        ),
        (
            "lowest values",
            &[
                (VOLUME_STEP_VAR, "1"),
                (SEEK_STEP_VAR, "1"),
                (PREVIOUS_RESTART_VAR, "0"),
                (AUTOPLAY_VAR, "off"),
            ],
            Ok((1, 1, 0, false)),
        ),
        (
            "highest values",
            &[
                (VOLUME_STEP_VAR, "25"),
                (SEEK_STEP_VAR, "600"),
                (PREVIOUS_RESTART_VAR, "60"),
                (AUTOPLAY_VAR, "on"),
            ],
            Ok((25, 600, 60, true)),
        ),
        (
            "volume step 0",
            &[(VOLUME_STEP_VAR, "0")],
            Err((VOLUME_STEP_VAR, "1 to 25")),
        ),
        (
            "volume step 26",
            &[(VOLUME_STEP_VAR, "26")],
            Err((VOLUME_STEP_VAR, "1 to 25")),
        ),
        (
            "volume step text",
            &[(VOLUME_STEP_VAR, "five")],
            Err((VOLUME_STEP_VAR, "1 to 25")),
        ),
        (
            "volume step negative",
            &[(VOLUME_STEP_VAR, "-5")],
            Err((VOLUME_STEP_VAR, "1 to 25")),
        ),
        (
            "seek step 0",
            &[(SEEK_STEP_VAR, "0")],
            Err((SEEK_STEP_VAR, "1 to 600")),
        ),
        (
            "seek step 601",
            &[(SEEK_STEP_VAR, "601")],
            Err((SEEK_STEP_VAR, "1 to 600")),
        ),
        (
            "seek step fraction",
            &[(SEEK_STEP_VAR, "2.5")],
            Err((SEEK_STEP_VAR, "1 to 600")),
        ),
        (
            "previous 61",
            &[(PREVIOUS_RESTART_VAR, "61")],
            Err((PREVIOUS_RESTART_VAR, "0 to 60")),
        ),
        (
            "previous text",
            &[(PREVIOUS_RESTART_VAR, "3s")],
            Err((PREVIOUS_RESTART_VAR, "0 to 60")),
        ),
        (
            "autoplay yes",
            &[(AUTOPLAY_VAR, "yes")],
            Err((AUTOPLAY_VAR, "on or off")),
        ),
        (
            "autoplay 1",
            &[(AUTOPLAY_VAR, "1")],
            Err((AUTOPLAY_VAR, "on or off")),
        ),
    ];
    let lookup = |env: &'static [(&'static str, &'static str)]| {
        move |key: &str| {
            env.iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| (*v).to_string())
        }
    };
    for (name, env, want) in rows {
        let got = resolve_player_config(lookup(env));
        match want {
            Ok((volume, seek, previous, autoplay)) => {
                let got = got.unwrap_or_else(|e| panic!("{name}: {e}"));
                assert_eq!(
                    got.steps,
                    Steps {
                        volume: *volume,
                        seek: Duration::from_secs(*seek)
                    },
                    "{name}"
                );
                assert_eq!(
                    got.player,
                    PlayerConfig {
                        previous_restart: Duration::from_secs(*previous),
                        autoplay: *autoplay,
                        country: None,
                        device: "default".into(),
                    },
                    "{name}"
                );
            }
            Err((var, range)) => {
                let err = got.expect_err(name);
                assert_eq!(err.setting, *var, "{name}");
                let text = err.to_string();
                assert!(text.contains(var) && text.contains(range), "{name}: {text}");
            }
        }
    }

    // 0005 AC23: the release delay, 0–3600 s or `never`.
    // Ok(None): never; Err: refused.
    type ReleaseWant = Result<Option<u64>, ()>;
    let release: &[(&str, Option<&str>, ReleaseWant)] = &[
        ("unset", None, Ok(Some(10))),
        ("empty counts as unset", Some(""), Ok(Some(10))),
        ("at once", Some("0"), Ok(Some(0))),
        ("an hour", Some("3600"), Ok(Some(3600))),
        ("never", Some("never"), Ok(None)),
        ("over an hour", Some("3601"), Err(())),
        ("negative", Some("-1"), Err(())),
        ("text", Some("x"), Err(())),
    ];
    for (name, value, want) in release {
        let got = resolve_player_config(|key: &str| {
            (key == RELEASE_PAUSED_VAR)
                .then_some(*value)
                .flatten()
                .map(Into::into)
        });
        match want {
            Ok(secs) => assert_eq!(
                got.map(|s| s.release_paused).map_err(|e| e.to_string()),
                Ok(secs.map(Duration::from_secs)),
                "release delay: {name}"
            ),
            Err(()) => {
                let err = got.expect_err(name);
                assert_eq!(err.setting, RELEASE_PAUSED_VAR, "{name}");
                let text = err.to_string();
                assert!(
                    text.contains(RELEASE_PAUSED_VAR)
                        && text.contains("0 to 3600")
                        && text.contains("never"),
                    "{name}: {text}"
                );
            }
        }
    }

    // 0006 AC3: the page size (empty = unset) and the hidden words
    // (empty = hide nothing).
    let default_words: Vec<String> = DEFAULT_HIDDEN_VERSIONS
        .iter()
        .map(|w| (*w).to_owned())
        .collect();
    let words = |list: &[&str]| list.iter().map(|w| (*w).to_owned()).collect::<Vec<_>>();
    type PageWant = Result<u32, ()>;
    let pages: &[(&str, Option<&str>, PageWant)] = &[
        ("unset", None, Ok(100)),
        ("empty counts as unset", Some(""), Ok(100)),
        ("lowest", Some("1"), Ok(1)),
        ("highest", Some("10000"), Ok(10000)),
        ("zero", Some("0"), Err(())),
        ("over", Some("10001"), Err(())),
        ("text", Some("x"), Err(())),
    ];
    for (name, value, want) in pages {
        let got = resolve_player_config(|key: &str| {
            (key == PAGE_SIZE_VAR)
                .then_some(*value)
                .flatten()
                .map(Into::into)
        });
        match want {
            Ok(n) => assert_eq!(
                got.map(|s| s.library.page_size).map_err(|e| e.to_string()),
                Ok(*n),
                "page size: {name}"
            ),
            Err(()) => {
                let err = got.expect_err(name);
                assert_eq!(err.setting, PAGE_SIZE_VAR, "{name}");
                let text = err.to_string();
                assert!(
                    text.contains(PAGE_SIZE_VAR) && text.contains("1 to 10000"),
                    "{name}: {text}"
                );
            }
        }
    }
    // 0007 AC1: the search page size (empty = unset).
    let search_pages: &[(&str, Option<&str>, PageWant)] = &[
        ("unset", None, Ok(20)),
        ("empty counts as unset", Some(""), Ok(20)),
        ("lowest", Some("1"), Ok(1)),
        ("highest", Some("1000"), Ok(1000)),
        ("zero", Some("0"), Err(())),
        ("over", Some("1001"), Err(())),
        ("text", Some("x"), Err(())),
    ];
    for (name, value, want) in search_pages {
        let got = resolve_player_config(|key: &str| {
            (key == SEARCH_PAGE_SIZE_VAR)
                .then_some(*value)
                .flatten()
                .map(Into::into)
        });
        match want {
            Ok(n) => assert_eq!(
                got.map(|s| s.library.search_page_size)
                    .map_err(|e| e.to_string()),
                Ok(*n),
                "search page size: {name}"
            ),
            Err(()) => {
                let err = got.expect_err(name);
                assert_eq!(err.setting, SEARCH_PAGE_SIZE_VAR, "{name}");
                let text = err.to_string();
                assert!(
                    text.contains(SEARCH_PAGE_SIZE_VAR) && text.contains("1 to 1000"),
                    "{name}: {text}"
                );
            }
        }
    }
    let hidden: &[(&str, Option<&str>, Vec<String>)] = &[
        ("unset", None, default_words),
        ("empty hides nothing", Some(""), Vec::new()),
        ("one word", Some("remix"), words(&["remix"])),
        (
            "trimmed list",
            Some(" live , demo,,  8d audio "),
            words(&["live", "demo", "8d audio"]),
        ),
    ];
    for (name, value, want) in hidden {
        let got = resolve_player_config(|key: &str| {
            (key == HIDE_VERSIONS_VAR)
                .then_some(*value)
                .flatten()
                .map(Into::into)
        });
        assert_eq!(
            got.map(|s| s.library.hidden_words)
                .map_err(|e| e.to_string()),
            Ok(want.clone()),
            "hidden words: {name}"
        );
    }

    // `play --autoplay` beats the environment, even a bad value.
    let precedence: &[(bool, Env, bool)] = &[
        (true, &[(AUTOPLAY_VAR, "off")], true),
        (true, &[(AUTOPLAY_VAR, "maybe")], true),
        (false, &[(AUTOPLAY_VAR, "on")], true),
        (false, &[(AUTOPLAY_VAR, "")], false),
    ];
    for (flag, env, want) in precedence {
        let got = resolve_play_config(*flag, lookup(env)).expect("valid");
        assert_eq!(got.player.autoplay, *want, "--autoplay {flag}, {env:?}");
    }

    // The player's previous uses the configured threshold: at 0:05,
    // 10 s goes back to the entry before, the default 3 s restarts.
    let previous_at_5s = |threshold: &'static str| {
        let settings = resolve_player_config(lookup(match threshold {
            "10" => &[(PREVIOUS_RESTART_VAR, "10")],
            _ => &[],
        }))
        .unwrap();
        let mut state = PlayerState::new(settings.player, SEED);
        let tracks = vec![track(1, Some(200)), track(2, Some(200))];
        let fx = player::update(
            &mut state,
            PlayerInput::Command(Command::LoadQueue { tracks, start: 1 }),
        );
        let Some(PlayerEffect::Resolve { tag, .. }) = fx.first().cloned() else {
            panic!("no resolve: {fx:?}");
        };
        player::update(
            &mut state,
            PlayerInput::Resolved {
                tag,
                result: Ok(AudioQuality::Lossless),
            },
        );
        let details = TrackDetails {
            source: "FLAC".into(),
            output: "hw:1,0".into(),
            bit_perfect: true,
            reason: None,
        };
        player::update(
            &mut state,
            PlayerInput::Engine(EngineEvent::Started { tag, details }),
        );
        player::update(
            &mut state,
            PlayerInput::Engine(EngineEvent::Position(Duration::from_secs(5))),
        );
        player::update(&mut state, PlayerInput::Command(Command::Previous))
    };
    let back = previous_at_5s("10");
    assert!(
        back.iter().any(|e| matches!(
            e,
            PlayerEffect::Resolve {
                entry: EntryId(1),
                purpose: Purpose::Play,
                ..
            }
        )),
        "10 s threshold: {back:?}"
    );
    let restart = previous_at_5s("default");
    assert!(
        restart.contains(&PlayerEffect::EngineSeek(Duration::ZERO)),
        "3 s threshold: {restart:?}"
    );
}

/// Spec 0009 AC12: the autoplay a player starts with: flag > environment
/// > remembered > `app.toml` > default.
#[test]
fn ac12_autoplay_precedence() {
    type Env = &'static [(&'static str, &'static str)];
    // name, app.toml, environment, --autoplay, remembered, want
    type Row = (&'static str, &'static str, Env, bool, Option<bool>, bool);
    let rows: &[Row] = &[
        ("default", "", &[], false, None, false),
        ("app.toml", "autoplay = true", &[], false, None, true),
        (
            "remembered over app.toml (off)",
            "autoplay = true",
            &[],
            false,
            Some(false),
            false,
        ),
        (
            "remembered over app.toml (on)",
            "autoplay = false",
            &[],
            false,
            Some(true),
            true,
        ),
        (
            "remembered over the default",
            "",
            &[],
            false,
            Some(true),
            true,
        ),
        (
            "environment over remembered (off)",
            "autoplay = true",
            &[(AUTOPLAY_VAR, "off")],
            false,
            Some(true),
            false,
        ),
        (
            "environment over remembered (on)",
            "",
            &[(AUTOPLAY_VAR, "on")],
            false,
            Some(false),
            true,
        ),
        (
            "empty environment falls through to remembered",
            "autoplay = true",
            &[(AUTOPLAY_VAR, "")],
            false,
            Some(false),
            false,
        ),
        (
            "--autoplay over all",
            "autoplay = false",
            &[(AUTOPLAY_VAR, "off")],
            true,
            Some(false),
            true,
        ),
    ];
    for (name, file, env, flag, remembered, want) in rows {
        let file = crate::config::parse_app_toml(std::path::Path::new("/c/app.toml"), file)
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        let env = |key: &str| {
            env.iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| (*v).to_owned())
        };
        let settings =
            resolve_play_config_with(&file, *flag, env).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(start_autoplay(&settings, *remembered), *want, "{name}");
    }
}
