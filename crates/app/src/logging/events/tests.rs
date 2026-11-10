//! Spec 0027 AC16 (a run's life) and AC17 (what the user was told).

use std::path::Path;

use super::*;
use crate::logging::{Mode, capture};
use crate::player::runtime::fakes::{SOURCE, output};
use tidal_player_audio::SinkError;
use tidal_player_core::log::LogLevel::{self, Debug, Info, Warn};
use tidal_player_core::player::{PlayerConfig, PlayerState};

/// What `f` logged at `setting`, as (level, text).
fn lines(setting: LogSetting, f: impl FnOnce()) -> Captured {
    capture(setting, f)
        .into_iter()
        .map(|l| (l.level, l.text))
        .collect()
}

fn info(f: impl FnOnce()) -> Captured {
    lines(LogSetting::Info, f)
}

fn started(tag: u64) -> EngineEvent {
    EngineEvent::Started {
        tag,
        source: SOURCE,
        output: output(),
    }
}

fn playing(track: u64, title: &str) -> Option<Playing<'_>> {
    Some(Playing {
        track,
        title,
        quality: AudioQuality::Lossless,
    })
}

fn log() -> PlayerLog {
    PlayerLog::new(AudioQuality::HiResLossless, PlaybackState::Stopped)
}

fn snapshot(state: PlaybackState, message: Option<&str>) -> PlayerSnapshot {
    let mut snapshot = PlayerState::new(PlayerConfig::default(), 1).snapshot();
    snapshot.state = state;
    snapshot.message = message.map(str::to_owned);
    snapshot
}

/// What a row captured: (level, text) lines.
type Captured = Vec<(LogLevel, String)>;
/// What a row expects: (level, text) lines.
type Expected = Vec<(LogLevel, &'static str)>;

/// Every row of "A run's life": the seam's input, then the captured event.
#[test]
fn ac16_run_life() {
    let config = Path::new("/home/u/.config/tidal-player");
    let opened = "device opened device=hw:1,0 format=S32_LE rate=44100 channels=2 bit_perfect=true";
    let rows: Vec<(&str, Captured, Expected)> = vec![
        (
            "starting, a daemon",
            info(|| starting("1.4.6", Mode::Daemon, LogSetting::Info, config)),
            vec![(
                Info,
                "starting version=1.4.6 mode=daemon log_level=info config=/home/u/.config/tidal-player",
            )],
        ),
        (
            "starting, an attached TUI is a client",
            info(|| starting("1.4.6", Mode::Attached, LogSetting::Debug, config)),
            vec![(
                Info,
                "starting version=1.4.6 mode=client log_level=debug config=/home/u/.config/tidal-player",
            )],
        ),
        (
            "starting, standalone and play",
            info(|| {
                starting("1", Mode::Standalone, LogSetting::Warn, config);
                starting("1", Mode::Play, LogSetting::Trace, config);
            }),
            vec![
                (
                    Info,
                    "starting version=1 mode=standalone log_level=warn config=/home/u/.config/tidal-player",
                ),
                (
                    Info,
                    "starting version=1 mode=play log_level=trace config=/home/u/.config/tidal-player",
                ),
            ],
        ),
        (
            "the first track opens the device",
            info(|| log().engine(&started(1), None)),
            vec![(Info, opened)],
        ),
        (
            "paused long enough: released",
            info(|| {
                let mut log = log();
                log.engine(&started(1), None);
                log.engine(&EngineEvent::Released, None);
            }),
            vec![(Info, opened), (Info, "device released device=hw:1,0")],
        ),
        (
            "resumed after a release: opened again",
            info(|| {
                let mut log = log();
                log.engine(&started(1), None);
                log.engine(&EngineEvent::Released, None);
                log.engine(&EngineEvent::Resumed, None);
            }),
            vec![
                (Info, opened),
                (Info, "device released device=hw:1,0"),
                (Info, opened),
            ],
        ),
        (
            "stopped: the device is closed",
            info(|| {
                let mut log = log();
                log.engine(&started(1), None);
                log.engine(&EngineEvent::Stopped, None);
            }),
            vec![(Info, opened), (Info, "device released device=hw:1,0")],
        ),
        (
            "a track starts",
            info(|| log().engine(&started(1), playing(42, "Blue"))),
            vec![
                (Info, opened),
                (
                    Info,
                    "playing track=42 title=Blue quality=LOSSLESS requested=HI_RES_LOSSLESS",
                ),
            ],
        ),
        (
            "playback ends",
            info(|| {
                let mut log = PlayerLog::new(AudioQuality::High, PlaybackState::Playing);
                log.snapshot(&snapshot(PlaybackState::Stopped, None));
            }),
            vec![(Info, "stopped")],
        ),
        (
            "the login is lost, then restored",
            info(|| {
                login_required();
                login_restored();
            }),
            vec![(Warn, "login required"), (Info, "login restored")],
        ),
        (
            "every way of shutting down",
            info(|| {
                for reason in [
                    Shutdown::Quit,
                    Shutdown::Command,
                    Shutdown::Signal,
                    Shutdown::PlayerGone,
                ] {
                    shutting_down(reason);
                }
            }),
            vec![
                (Info, "shutting down reason=quit"),
                (Info, "shutting down reason=shutdown command"),
                (Info, "shutting down reason=signal"),
                (Info, "shutting down reason=player gone"),
            ],
        ),
        (
            "clients at DEBUG",
            lines(LogSetting::Debug, || {
                client_connected(3);
                client_disconnected(3);
            }),
            vec![
                (Debug, "client connected client=3"),
                (Debug, "client disconnected client=3"),
            ],
        ),
        (
            "clients are not INFO: a status bar connects every second",
            info(|| {
                client_connected(3);
                client_disconnected(3);
            }),
            vec![],
        ),
    ];
    for (name, got, want) in rows {
        let want: Vec<(LogLevel, String)> =
            want.into_iter().map(|(l, t)| (l, t.to_owned())).collect();
        assert_eq!(got, want, "{name}");
    }
}

/// `playing` is logged once per entry that starts, a gapless advance
/// included; a resume, a pause, a seek (positions) and a buffering stall
/// log no `playing`; a repeated device does not open again.
#[test]
fn ac16_playing_once_per_entry() {
    let mut log = log();
    let got = info(|| {
        log.engine(&started(1), playing(1, "One"));
        log.engine(
            &EngineEvent::Position(std::time::Duration::from_secs(1)),
            None,
        );
        log.engine(&EngineEvent::Paused, None);
        log.engine(&EngineEvent::Resumed, None);
        log.engine(&EngineEvent::Buffering, None);
        log.engine(&EngineEvent::Buffered, None);
        log.engine(
            &EngineEvent::Position(std::time::Duration::from_secs(9)),
            None,
        );
        // The gapless advance into the preloaded track, same device.
        log.engine(
            &EngineEvent::Transitioned {
                tag: 2,
                source: SOURCE,
                output: output(),
            },
            playing(2, "Two"),
        );
        // A stale start (nothing the player sent): no line at all.
        log.engine(&started(9), None);
    });
    let playing: Vec<&str> = got
        .iter()
        .map(|(_, t)| t.as_str())
        .filter(|t| t.starts_with("playing"))
        .collect();
    assert_eq!(
        playing,
        [
            "playing track=1 title=One quality=LOSSLESS requested=HI_RES_LOSSLESS",
            "playing track=2 title=Two quality=LOSSLESS requested=HI_RES_LOSSLESS",
        ],
        "{got:?}"
    );
    let opened = got
        .iter()
        .filter(|(_, t)| t.starts_with("device opened"))
        .count();
    assert_eq!(opened, 1, "the device stayed open: {got:?}");
}

/// A change of format opens the device again (the engine closes and
/// reopens it), and so does a change of device.
#[test]
fn ac16_device_opened_again_on_a_new_format() {
    let mut log = log();
    let mut other = output();
    other.sample_rate = 96_000;
    let mut moved = output();
    moved.device = "hw:2,0".into();
    let got = info(|| {
        log.engine(&started(1), None);
        log.engine(
            &EngineEvent::Transitioned {
                tag: 2,
                source: SOURCE,
                output: other,
            },
            None,
        );
        log.engine(&EngineEvent::OutputChanged(moved), None);
    });
    let texts: Vec<&str> = got.iter().map(|(_, t)| t.as_str()).collect();
    assert_eq!(
        texts,
        [
            "device opened device=hw:1,0 format=S32_LE rate=44100 channels=2 bit_perfect=true",
            "device opened device=hw:1,0 format=S32_LE rate=96000 channels=2 bit_perfect=true",
            "device opened device=hw:2,0 format=S32_LE rate=44100 channels=2 bit_perfect=true",
        ]
    );
}

/// A start-up with the player stopped logs no `stopped`, and a player that
/// stays stopped logs none either.
#[test]
fn ac16_stopped_only_on_the_transition() {
    let mut log = log();
    let got = info(|| {
        log.snapshot(&snapshot(PlaybackState::Stopped, None));
        log.snapshot(&snapshot(PlaybackState::Loading, None));
        log.snapshot(&snapshot(PlaybackState::Playing, None));
        log.snapshot(&snapshot(PlaybackState::Paused, None));
        log.snapshot(&snapshot(PlaybackState::Stopped, None));
        log.snapshot(&snapshot(PlaybackState::Stopped, None));
    });
    assert_eq!(got, [(Info, "stopped".to_owned())]);
}

/// Nothing that was logged carries a token, a URL's path or query, or an
/// account field: the fixtures hold recognisable ones.
#[test]
fn ac16_no_secrets() {
    const TOKEN: &str = "eyJSECRETTOKEN123";
    const URL: &str =
        "https://sp-pr-fa.audio.tidal.com/mediatracks/CAEaKQ/0.flac?token=SECRETQUERY";
    const COVER: &str = "https://resources.tidal.com/images/aaaa/bbbb/1280x1280.jpg";
    const EMAIL: &str = "someone@example.com";
    let secret_reason = format!(
        "error sending request for url ({URL}); Authorization: Bearer {TOKEN}; cover {COVER}"
    );
    let got = lines(LogSetting::Trace, || {
        request_failed(
            &request_name(&format!(
                "LoadQueue {{ tracks: [\"{EMAIL}\", \"{TOKEN}\"] }}"
            )),
            &secret_reason,
        );
        disconnected(&secret_reason);
        notice(&secret_reason);
        export_failed(&secret_reason);
        let mut log = log();
        log.snapshot(&snapshot(PlaybackState::Stopped, Some(&secret_reason)));
    });
    let all: String = got
        .iter()
        .map(|(_, t)| t.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(got.len(), 5, "{all}");
    for secret in [
        TOKEN,
        "SECRETQUERY",
        "mediatracks",
        "bbbb",
        EMAIL,
        "tracks:",
    ] {
        assert!(!all.contains(secret), "{secret} in {all}");
    }
    // The host is allowed, and the request's name is what is left of it.
    assert!(all.contains("sp-pr-fa.audio.tidal.com"), "{all}");
    assert!(all.contains("request=LoadQueue "), "{all}");
}

/// The name of a request is its variant's.
#[test]
fn ac17_request_name() {
    for (debug, name) in [
        (
            "LoadQueue { tracks: [Track { id: 1 }], start: 0 }",
            "LoadQueue",
        ),
        ("Page(Album(12))", "Page"),
        ("Pause", "Pause"),
        ("SetDevice(\"hw:1,0\")", "SetDevice"),
    ] {
        assert_eq!(request_name(debug), name, "{debug}");
    }
}

/// An `Err` answer's line: the request's name and the reason, at `WARN`.
#[test]
fn ac17_request_failed() {
    let got = info(|| {
        request_failed("SetDevice", "Device name is empty");
        request_failed("Page", "Tidal said: not found\nretry later");
    });
    assert_eq!(
        got,
        [
            (
                Warn,
                "request failed request=SetDevice reason=Device name is empty".to_owned()
            ),
            (
                Warn,
                "request failed request=Page reason=Tidal said: not found retry later".to_owned()
            ),
        ]
    );
}

/// A snapshot's `message` logs once when its text changes, and not again
/// while it stays; a cleared message and the same text again is a change.
#[test]
fn ac17_message_once() {
    let mut log = log();
    let got = info(|| {
        for message in [
            None,
            Some("Could not save the playback state: disk full"),
            Some("Could not save the playback state: disk full"),
            Some("Hook failed"),
            Some("Hook failed"),
            None,
            Some("Hook failed"),
        ] {
            log.snapshot(&snapshot(PlaybackState::Stopped, message));
        }
    });
    assert_eq!(
        got,
        [
            (
                Warn,
                "message text=Could not save the playback state: disk full".to_owned()
            ),
            (Warn, "message text=Hook failed".to_owned()),
            (Warn, "message text=Hook failed".to_owned()),
        ]
    );
}

/// The TUI's rows, each at its level.
#[test]
fn ac17_tui_rows() {
    let rows: Vec<(&str, Captured, (LogLevel, &str))> = vec![
        (
            "connection lost",
            info(|| disconnected("Disconnected from the player: reconnecting…")),
            (
                Warn,
                "disconnected reason=Disconnected from the player: reconnecting…",
            ),
        ),
        (
            "versions differ",
            info(|| disconnected("This client speaks protocol 2, the player 3")),
            (
                Warn,
                "disconnected reason=This client speaks protocol 2, the player 3",
            ),
        ),
        ("reconnected", info(reconnected), (Info, "reconnected")),
        (
            "a start-up notice",
            info(|| notice("keymap.toml: skipped 2 commands")),
            (Warn, "notice text=keymap.toml: skipped 2 commands"),
        ),
        (
            "an export fails",
            info(|| export_failed("Permission denied (os error 13)")),
            (Warn, "export failed reason=Permission denied (os error 13)"),
        ),
    ];
    for (name, got, (level, text)) in rows {
        assert_eq!(got, [(level, text.to_owned())], "{name}");
    }
}

/// An output that failed to open is the player's message, not a log line of
/// its own here: a `SinkError` is not logged by the tracker.
#[test]
fn ac16_resume_failed_logs_no_device_line() {
    let mut log = log();
    let got = info(|| {
        log.engine(&started(1), None);
        log.engine(&EngineEvent::Released, None);
        log.engine(&EngineEvent::ResumeFailed(SinkError::NotOpen), None);
    });
    let texts: Vec<&str> = got.iter().map(|(_, t)| t.as_str()).collect();
    assert_eq!(texts.len(), 2, "{texts:?}");
    assert!(texts[1].starts_with("device released"));
}
