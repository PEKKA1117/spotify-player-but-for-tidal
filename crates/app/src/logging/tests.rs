//! Spec 0027 AC2 (the layer), AC3 (the roles) and AC4 (the file).

use std::os::unix::fs::PermissionsExt;
use std::time::Duration;

use super::*;
use tidal_player_core::log::LogLevel::{Debug, Error, Info, Trace, Warn};

/// 2026-11-10T13:04:05.123Z.
const T1: u64 = 1_794_315_845_123;
/// 2026-11-10T23:30:00.007Z.
const T2: u64 = 1_794_353_400_007;
const HOUR: i32 = 3600;

fn at(ms: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_millis(ms)
}

/// A stderr that remembers what it was given.
#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl Write for Capture {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        locked(&self.0).extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Capture {
    fn text(&self) -> String {
        String::from_utf8(locked(&self.0).clone()).unwrap()
    }
}

fn make(
    setting: LogSetting,
    roles: Roles,
    dir: &Path,
    limit: u64,
    zone: Zone,
) -> (Logger, Capture) {
    let stderr = Capture::default();
    (
        Logger::new(setting, roles, dir, limit, Box::new(stderr.clone()), zone),
        stderr,
    )
}

fn texts(handle: &LogHandle) -> Vec<String> {
    handle
        .batch(None)
        .lines
        .into_iter()
        .map(|l| l.text)
        .collect()
}

/// AC3: each run mode installs what the "Who keeps what" table says
/// (buffer, file, stderr), and a logger with those roles does just that
/// with one line.
#[test]
fn ac3_roles() {
    // (mode, buffer, file, stderr)
    let rows = [
        (Mode::Standalone, true, true, false),
        (Mode::Daemon, true, true, true),
        (Mode::Play, true, true, false),
        (Mode::Attached, true, false, false),
        (Mode::Other, false, false, false),
    ];
    for (mode, buffer, file, stderr) in rows {
        let want = Roles {
            buffer,
            file,
            stderr,
        };
        assert_eq!(mode.roles(), want, "{mode:?}");
        let dir = tempfile::tempdir().unwrap();
        let (logger, captured) = make(
            LogSetting::Info,
            mode.roles(),
            dir.path(),
            FILE_LIMIT,
            Zone::Fixed(0),
        );
        logger.record(at(T1), Warn, "tidal_player::x", "one line");
        let in_buffer = !logger.handle().batch(None).lines.is_empty();
        // A mode with no buffer role installs nothing at all.
        assert_eq!(in_buffer, buffer || !buffer, "{mode:?}");
        assert_eq!(dir.path().join(LOG_FILE).exists(), file, "{mode:?} file");
        assert_eq!(
            captured.text(),
            if stderr {
                "WARN tidal_player::x: one line\n"
            } else {
                ""
            },
            "{mode:?} stderr"
        );
    }
    // `install` gives a mode that logs nothing no handle, and touches
    // nothing.
    let dir = tempfile::tempdir().unwrap();
    assert!(install(Mode::Other, LogSetting::Info, dir.path()).is_none());
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}

/// AC4: a line of the file is `time offset  LEVEL target: text`, for each
/// level, with the offset applied, written to a file made with mode 0600.
#[test]
fn ac4_file_format() {
    // (millis, offset in seconds, level, target, text, the line)
    type Row = (u64, i32, LogLevel, &'static str, &'static str, &'static str);
    let rows: [Row; 8] = [
        (
            T1,
            8 * HOUR,
            Warn,
            "tidal_player::player::reserve",
            "cannot reserve card 1: busy",
            "2026-11-10T21:04:05.123+08:00  WARN tidal_player::player::reserve: cannot reserve card 1: busy",
        ),
        (
            T1,
            8 * HOUR,
            Error,
            "tidal_player",
            "boom",
            "2026-11-10T21:04:05.123+08:00 ERROR tidal_player: boom",
        ),
        (
            T1,
            8 * HOUR,
            Info,
            "a::b",
            "x",
            "2026-11-10T21:04:05.123+08:00  INFO a::b: x",
        ),
        (
            T1,
            8 * HOUR,
            Debug,
            "a",
            "x",
            "2026-11-10T21:04:05.123+08:00 DEBUG a: x",
        ),
        (
            T1,
            8 * HOUR,
            Trace,
            "a",
            "x",
            "2026-11-10T21:04:05.123+08:00 TRACE a: x",
        ),
        (
            T1,
            0,
            Info,
            "a",
            "x",
            "2026-11-10T13:04:05.123+00:00  INFO a: x",
        ),
        // Past midnight in the zone, with milliseconds padded.
        (
            T2,
            8 * HOUR,
            Info,
            "a",
            "x",
            "2026-11-11T07:30:00.007+08:00  INFO a: x",
        ),
        (
            T1,
            -(3 * HOUR + 1800),
            Warn,
            "a",
            "x",
            "2026-11-10T09:34:05.123-03:30  WARN a: x",
        ),
    ];
    for (ms, offset, level, target, text, want) in rows {
        assert_eq!(file_line(at(ms), offset, level, target, text), want);
    }

    // On disk: appended, one line each, mode 0600, in the zone it was given.
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state");
    let roles = Mode::Standalone.roles();
    let (logger, _) = make(
        LogSetting::Info,
        roles,
        &state,
        FILE_LIMIT,
        Zone::Fixed(8 * HOUR),
    );
    logger.record(
        at(T1),
        Warn,
        "tidal_player::player::reserve",
        "cannot reserve card 1: busy",
    );
    logger.record(at(T2), Info, "tidal_player::x", "two");
    let path = state.join(LOG_FILE);
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "2026-11-10T21:04:05.123+08:00  WARN tidal_player::player::reserve: cannot reserve card 1: busy\n\
         2026-11-11T07:30:00.007+08:00  INFO tidal_player::x: two\n"
    );
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    // A second run appends.
    let (again, _) = make(LogSetting::Info, roles, &state, FILE_LIMIT, Zone::Fixed(0));
    again.record(at(T1), Info, "tidal_player::x", "three");
    let text = std::fs::read_to_string(&path).unwrap();
    assert_eq!(text.lines().count(), 3, "{text}");
    assert!(text.ends_with("2026-11-10T13:04:05.123+00:00  INFO tidal_player::x: three\n"));
    // Nothing the setting drops reaches the file.
    again.record(at(T1), Debug, "tidal_player::x", "dropped");
    assert_eq!(std::fs::read_to_string(&path).unwrap().lines().count(), 3);
}

/// AC4: a file over the limit at start, and a line that would pass it,
/// rotate to `tidal-player.log.old`, replacing the old one.
#[test]
fn ac4_rotation() {
    let roles = Mode::Standalone.roles();
    let line = file_line(at(T1), 0, Info, "tidal_player::x", "line 1");
    let size = u64::try_from(line.len() + 1).unwrap();
    // Two lines fit, a third passes the limit.
    let limit = size * 2 + size / 2;

    // At start: not over the limit is kept, over it is rotated.
    for (existing, rotated) in [(limit - size, false), (limit + 1, true)] {
        let dir = tempfile::tempdir().unwrap();
        let (main, old) = (dir.path().join(LOG_FILE), dir.path().join(OLD_FILE));
        std::fs::write(&main, "x".repeat(usize::try_from(existing).unwrap())).unwrap();
        std::fs::write(&old, "older").unwrap();
        let (logger, _) = make(LogSetting::Info, roles, dir.path(), limit, Zone::Fixed(0));
        logger.record(at(T1), Info, "tidal_player::x", "line 1");
        let main_text = std::fs::read_to_string(&main).unwrap();
        let old_text = std::fs::read_to_string(&old).unwrap();
        if rotated {
            assert_eq!(
                old_text.len(),
                usize::try_from(existing).unwrap(),
                "{existing}"
            );
            assert_eq!(main_text, format!("{line}\n"));
        } else {
            assert_eq!(old_text, "older", "{existing}");
            assert!(main_text.ends_with(&format!("{line}\n")), "{existing}");
        }
    }

    // While running: the third line starts a new file; the fifth the next.
    let dir = tempfile::tempdir().unwrap();
    let (main, old) = (dir.path().join(LOG_FILE), dir.path().join(OLD_FILE));
    let (logger, _) = make(LogSetting::Info, roles, dir.path(), limit, Zone::Fixed(0));
    let full = |n: u32| file_line(at(T1), 0, Info, "tidal_player::x", &format!("line {n}"));
    for n in 1..=5 {
        logger.record(at(T1), Info, "tidal_player::x", &format!("line {n}"));
        let read = |p: &Path| std::fs::read_to_string(p).unwrap_or_default();
        match n {
            1 | 2 => assert!(!old.exists(), "after {n}"),
            3 => {
                assert_eq!(read(&old), format!("{}\n{}\n", full(1), full(2)));
                assert_eq!(read(&main), format!("{}\n", full(3)));
            }
            4 => assert_eq!(read(&main), format!("{}\n{}\n", full(3), full(4))),
            _ => {
                assert_eq!(read(&old), format!("{}\n{}\n", full(3), full(4)));
                assert_eq!(read(&main), format!("{}\n", full(5)));
            }
        }
    }
    assert_eq!(logger.handle().batch(None).lines.len(), 5);
}

/// AC4: a path that cannot be written is given up after exactly one
/// `cannot write <path>: <reason>` line in the buffer (and the daemon's
/// stderr); later lines still reach the buffer; nothing panics or exits.
#[test]
fn ac4_gives_up_once() {
    // At start: the state directory cannot be made (a file is in the way).
    let dir = tempfile::tempdir().unwrap();
    let blocker = dir.path().join("file");
    std::fs::write(&blocker, "x").unwrap();
    let state = blocker.join("state");
    let (logger, stderr) = make(
        LogSetting::Info,
        Mode::Daemon.roles(),
        &state,
        FILE_LIMIT,
        Zone::Fixed(0),
    );
    for n in 1..=3 {
        logger.record(at(T1), Info, "tidal_player::x", &format!("later {n}"));
    }
    let lines = logger.handle().batch(None).lines;
    assert_eq!(lines.len(), 4, "{lines:?}");
    let first = &lines[0];
    assert_eq!((first.seq, first.level), (1, Warn));
    let prefix = format!("cannot write {}: ", state.join(LOG_FILE).display());
    assert!(first.text.starts_with(&prefix), "{}", first.text);
    assert!(first.text.len() > prefix.len(), "a reason follows");
    assert_eq!(
        texts(&logger.handle())[1..],
        ["later 1", "later 2", "later 3"].map(String::from)
    );
    assert!(!state.exists());
    assert_eq!(
        stderr.text().matches("cannot write").count(),
        1,
        "{}",
        stderr.text()
    );
    assert!(
        stderr
            .text()
            .starts_with("WARN tidal_player::logging: cannot write")
    );

    // While running: a write that fails (a full disk) gives it up once.
    let full = Path::new("/dev/full");
    if full.exists() {
        let log = FileLog {
            path: full.to_owned(),
            old: full.to_owned(),
            limit: FILE_LIMIT,
            file: OpenOptions::new().append(true).open(full).unwrap(),
            size: 0,
        };
        let logger = Logger {
            setting: LogSetting::Info,
            buffer: Arc::new(Mutex::new(LogBuffer::default())),
            file: Mutex::new(FileState::Active(log)),
            path: full.to_owned(),
            stderr: None,
            zone: Zone::Fixed(0),
        };
        for n in 1..=4 {
            logger.record(at(T1), Info, "tidal_player::x", &format!("line {n}"));
        }
        let all = texts(&logger.handle());
        let warnings: Vec<&String> = all
            .iter()
            .filter(|t| t.starts_with("cannot write"))
            .collect();
        assert_eq!(warnings.len(), 1, "{all:?}");
        assert!(
            warnings[0].starts_with("cannot write /dev/full: "),
            "{all:?}"
        );
        assert_eq!(all.len(), 5, "{all:?}");
    }
}

/// AC4: `off` creates no file, not even the directory, and keeps nothing.
#[test]
fn ac4_off_creates_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state");
    let (logger, stderr) = make(
        LogSetting::Off,
        Mode::Daemon.roles(),
        &state,
        FILE_LIMIT,
        Zone::Fixed(0),
    );
    logger.record(at(T1), Error, "tidal_player::x", "boom");
    assert!(!state.exists());
    assert!(logger.handle().batch(None).lines.is_empty());
    assert!(logger.handle().batch(None).off);
    assert_eq!(stderr.text(), "");
    // `install` at `off` touches nothing either, and says so in the handle.
    let handle = install(Mode::Standalone, LogSetting::Off, &state).unwrap();
    assert!(handle.batch(None).off && handle.batch(None).lines.is_empty());
    assert!(!state.exists());
}

/// AC2 (through the layer): events become lines with their fields; other
/// crates' debug lines and this program's lines below the setting are
/// dropped; line breaks are spaces.
#[test]
fn ac2_layer_capture() {
    let dir = tempfile::tempdir().unwrap();
    let (logger, _) = make(
        LogSetting::Info,
        Mode::Attached.roles(),
        dir.path(),
        FILE_LIMIT,
        Zone::Fixed(0),
    );
    let logger = Arc::new(logger);
    let subscriber = Registry::default().with(LogLayer::new(Arc::clone(&logger)));
    tracing::subscriber::with_default(subscriber, || {
        tracing::warn!(target: "tidal_player::player::reserve", "cannot reserve card {}: busy", 1);
        tracing::info!(target: "tidal_player::x", count = 3, name = "a b", "two\nlines");
        tracing::warn!(target: "tidal_player_api::auth", error = %"boom", "could not save");
        tracing::debug!(target: "tidal_player::x", "below the setting");
        tracing::info!(target: "reqwest::connect", "another crate's info");
        tracing::warn!(target: "reqwest::connect", "another crate's warning");
        tracing::error!(target: "tidal_player_extra", "a look-alike's error");
    });
    let lines = logger.handle().batch(None).lines;
    let got: Vec<(LogLevel, &str, &str)> = lines
        .iter()
        .map(|l| (l.level, l.target.as_str(), l.text.as_str()))
        .collect();
    assert_eq!(
        got,
        vec![
            (
                Warn,
                "tidal_player::player::reserve",
                "cannot reserve card 1: busy"
            ),
            (Info, "tidal_player::x", "two lines count=3 name=a b"),
            (Warn, "tidal_player_api::auth", "could not save error=boom"),
            (Warn, "reqwest::connect", "another crate's warning"),
            (Error, "tidal_player_extra", "a look-alike's error"),
        ]
    );
    // The attached TUI keeps no file.
    assert!(!dir.path().join(LOG_FILE).exists());
}
