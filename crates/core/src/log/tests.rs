//! Spec 0027 AC2: what is kept of an event, and its text; the setting's
//! parsing and the page's line format.

use std::time::{Duration, SystemTime};

use super::LogLevel::{self, Debug, Error, Info, Trace, Warn};
use super::*;

const ALL: [LogLevel; 5] = [Error, Warn, Info, Debug, Trace];

const OWN: [&str; 7] = [
    "tidal_player",
    "tidal_player::player::reserve",
    "tidal_player_core",
    "tidal_player_core::ui::logs",
    "tidal_player_api::http",
    "tidal_player_audio",
    "tidal_player_audio::alsa::open",
];
const OTHER: [&str; 5] = [
    "reqwest::connect",
    "hyper",
    "zbus::connection",
    "symphonia_core",
    "",
];
const LOOK_ALIKE: [&str; 6] = [
    "tidal_player_extra",
    "tidal_player_extra::x",
    "tidal_playerx",
    "tidal_player_",
    "my_tidal_player",
    "tidal-player",
];

/// AC2: a line is kept exactly when the setting is not `off` and either
/// the target is this program's and the level is at or above the setting,
/// or it is another crate's and the level is `WARN` or `ERROR`.
#[test]
fn ac2_kept() {
    use LogSetting as S;
    // (setting, the levels kept from this program's crates)
    let own: [(S, &[LogLevel]); 6] = [
        (S::Off, &[]),
        (S::Error, &[Error]),
        (S::Warn, &[Error, Warn]),
        (S::Info, &[Error, Warn, Info]),
        (S::Debug, &[Error, Warn, Info, Debug]),
        (S::Trace, &ALL),
    ];
    for (setting, kept) in own {
        for target in OWN {
            for level in ALL {
                assert_eq!(
                    is_kept(setting, level, target),
                    kept.contains(&level),
                    "{setting:?} {level:?} {target}"
                );
            }
        }
        // Other crates: `WARN` and `ERROR` at every setting but `off`.
        let others: &[LogLevel] = if setting == S::Off {
            &[]
        } else {
            &[Error, Warn]
        };
        for target in OTHER.into_iter().chain(LOOK_ALIKE) {
            for level in ALL {
                assert_eq!(
                    is_kept(setting, level, target),
                    others.contains(&level),
                    "{setting:?} {level:?} {target:?}"
                );
            }
        }
    }
    assert_eq!(LogSetting::default(), S::Info);
    assert_eq!(S::Off.max_level(), None);
    assert_eq!(S::Warn.max_level(), Some(Warn));
    assert_eq!(S::Trace.max_level(), Some(Trace));
}

/// AC2: the text is the message, then ` name=value` in order; line breaks
/// are spaces.
#[test]
fn ac2_text() {
    type Case = (
        &'static str,
        &'static [(&'static str, &'static str)],
        &'static str,
    );
    let cases: [Case; 8] = [
        ("plain", &[], "plain"),
        (
            "could not save the refreshed session",
            &[("error", "disk full")],
            "could not save the refreshed session error=disk full",
        ),
        ("m", &[("a", "1"), ("b", "2"), ("a", "3")], "m a=1 b=2 a=3"),
        ("two\nlines", &[], "two lines"),
        ("a\r\nb\rc", &[], "a b c"),
        ("m", &[("err", "x\ny")], "m err=x y"),
        ("", &[("k", "v")], "k=v"),
        ("", &[], ""),
    ];
    for (message, fields, want) in cases {
        assert_eq!(build_text(message, fields), want, "{message:?} {fields:?}");
    }
}

/// AC2: `log_level` is one of six values in any letter case.
#[test]
fn ac2_setting_parse() {
    use LogSetting as S;
    for (text, want) in [
        ("off", Some(S::Off)),
        ("error", Some(S::Error)),
        ("warn", Some(S::Warn)),
        ("info", Some(S::Info)),
        ("debug", Some(S::Debug)),
        ("trace", Some(S::Trace)),
        ("OFF", Some(S::Off)),
        ("Info", Some(S::Info)),
        ("dEbUg", Some(S::Debug)),
        ("TRACE", Some(S::Trace)),
        ("", None),
        ("warning", None),
        ("verbose", None),
        (" info", None),
    ] {
        assert_eq!(text.parse::<S>().ok(), want, "{text:?}");
    }
    let bad = "loud".parse::<S>().unwrap_err();
    assert!(bad.to_string().contains("`loud`"), "{bad}");
}

fn line(level: LogLevel, at_ms: u64, target: &str, text: &str) -> LogLine {
    LogLine {
        seq: 1,
        at: SystemTime::UNIX_EPOCH + Duration::from_millis(at_ms),
        level,
        target: target.into(),
        text: text.into(),
    }
}

/// AC9: a line is drawn `HH:MM:SS LEVEL target: text`, in the local time
/// of the offset it is given, the level left-aligned to 5 columns, control
/// characters left out.
#[test]
fn ac9_format_line() {
    const DAY: u64 = 86_400_000;
    let at = 20_000 * DAY + ((13 * 60 + 4) * 60 + 5) * 1000 + 123;
    let h = 3600;
    let cases = [
        (Warn, 8 * h, "21:04:05 WARN  tidal_player::x: busy"),
        (Error, 0, "13:04:05 ERROR tidal_player::x: busy"),
        (Info, -5 * h, "08:04:05 INFO  tidal_player::x: busy"),
        (Debug, 11 * h, "00:04:05 DEBUG tidal_player::x: busy"),
        (Trace, 5 * h + 1800, "18:34:05 TRACE tidal_player::x: busy"),
    ];
    for (level, offset, want) in cases {
        assert_eq!(
            format_line(&line(level, at, "tidal_player::x", "busy"), offset),
            want
        );
    }
    // Across midnight both ways.
    let late = 20_000 * DAY + (23 * 3600 + 59 * 60 + 59) * 1000;
    assert_eq!(
        format_line(&line(Info, late, "t", "x"), 2 * h),
        "01:59:59 INFO  t: x"
    );
    assert_eq!(
        format_line(&line(Info, 20_000 * DAY + 30 * 60 * 1000, "t", "x"), -h),
        "23:30:00 INFO  t: x"
    );
    // Control characters are not drawn.
    assert_eq!(
        format_line(&line(Info, at, "t", "a\tb\u{1b}[31mc"), 0),
        "13:04:05 INFO  t: ab[31mc"
    );
}
