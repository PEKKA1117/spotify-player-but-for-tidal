//! Spec 0027 AC15: the export's file name, mode, content and failures.

use std::os::unix::fs::PermissionsExt;
use std::time::Duration;

use super::*;
use tidal_player_core::log::LogLevel;

/// 2026-11-10T13:04:05.123Z.
const T1: u64 = 1_794_315_845_123;
const HOUR: i32 = 3600;

fn at(ms: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_millis(ms)
}

fn lines() -> Vec<LogLine> {
    let line = |seq: u64, ms: u64, level: LogLevel, target: &str, text: &str| LogLine {
        seq,
        at: at(ms),
        level,
        target: target.into(),
        text: text.into(),
    };
    vec![
        line(1, T1 - 5_000, LogLevel::Info, "tidal_player::x", "first"),
        line(
            2,
            T1,
            LogLevel::Warn,
            "tidal_player::player::reserve",
            "cannot reserve card 1: busy",
        ),
        line(
            3,
            T1 + 86_400_000,
            LogLevel::Error,
            "reqwest::connect",
            "tomorrow, 中文",
        ),
    ]
}

fn expected(zone_secs: i32) -> String {
    lines()
        .iter()
        .map(|l| {
            format!(
                "{}\n",
                crate::logging::file_line(l.at, zone_secs, l.level, &l.target, &l.text)
            )
        })
        .collect()
}

fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

/// AC15: the name for a local time (and the `-N` for a taken one); the
/// file in the log file's format, mode 0600, the cache directory created
/// when missing, a taken name never overwritten; a directory that cannot
/// be written gives the failure and leaves no file.
#[test]
fn ac15_export_file() {
    // (millis, offset, n, the name)
    let rows = [
        (T1, 8 * HOUR, 1, "tidal-player-logs-20261110-210405.log"),
        (T1, 8 * HOUR, 2, "tidal-player-logs-20261110-210405-2.log"),
        (T1, 8 * HOUR, 13, "tidal-player-logs-20261110-210405-13.log"),
        (T1, 0, 1, "tidal-player-logs-20261110-130405.log"),
        (
            T1,
            -(3 * HOUR + 1800),
            1,
            "tidal-player-logs-20261110-093405.log",
        ),
        (
            T1 + 11 * 3_600_000,
            8 * HOUR,
            1,
            "tidal-player-logs-20261111-080405.log",
        ),
    ];
    for (ms, offset, n, want) in rows {
        assert_eq!(export_name(at(ms), offset, n), want);
    }

    // A fresh export: the directory is created, the file is private.
    let tmp = tempfile::tempdir().unwrap();
    let cache = tmp.path().join("cache").join("tidal-player");
    let zone = Zone::Fixed(8 * HOUR);
    let (count, path) = export(&lines(), &cache, at(T1), &zone).unwrap();
    let first = cache.join("tidal-player-logs-20261110-210405.log");
    assert_eq!((count, path.as_str()), (3, first.to_str().unwrap()));
    assert_eq!(std::fs::read_to_string(&first).unwrap(), expected(8 * HOUR));
    assert_eq!(
        std::fs::metadata(&first).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(names(&cache).len(), 1, "no leftover: {:?}", names(&cache));

    // Two exports in one second: `-2`, `-3`; the first is not touched.
    let (_, second) = export(&lines()[..1], &cache, at(T1), &zone).unwrap();
    let (_, third) = export(&lines()[..2], &cache, at(T1), &zone).unwrap();
    assert!(
        second.ends_with("tidal-player-logs-20261110-210405-2.log"),
        "{second}"
    );
    assert!(
        third.ends_with("tidal-player-logs-20261110-210405-3.log"),
        "{third}"
    );
    assert_eq!(std::fs::read_to_string(&first).unwrap(), expected(8 * HOUR));
    assert_eq!(std::fs::read_to_string(&second).unwrap().lines().count(), 1);
    assert_eq!(std::fs::read_to_string(&third).unwrap().lines().count(), 2);
    assert_eq!(names(&cache).len(), 3, "{:?}", names(&cache));

    // A name taken by a file the player did not write is not overwritten.
    std::fs::write(cache.join("tidal-player-logs-20261111-080405.log"), "mine").unwrap();
    let (_, other) = export(&lines(), &cache, at(T1 + 11 * 3_600_000), &zone).unwrap();
    assert!(other.ends_with("20261111-080405-2.log"), "{other}");
    assert_eq!(
        std::fs::read_to_string(cache.join("tidal-player-logs-20261111-080405.log")).unwrap(),
        "mine"
    );

    // The directory cannot be made (a file is in the way): the reason.
    let blocker = tmp.path().join("blocker");
    std::fs::write(&blocker, "x").unwrap();
    let error = export(&lines(), &blocker.join("cache"), at(T1), &zone).unwrap_err();
    assert!(!error.is_empty(), "a reason");
    assert_eq!(names(tmp.path()).len(), 3, "{:?}", names(tmp.path()));

    // The directory cannot be written: the reason, nothing left behind.
    let locked = tmp.path().join("locked");
    std::fs::create_dir(&locked).unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o500)).unwrap();
    // (As root the directory stays writable: nothing to check.)
    if std::fs::write(locked.join("probe"), "").is_err() {
        let error = export(&lines(), &locked, at(T1), &zone).unwrap_err();
        assert!(!error.is_empty(), "a reason");
        assert!(names(&locked).is_empty(), "{:?}", names(&locked));
    }
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o700)).unwrap();
}
