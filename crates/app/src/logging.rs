//! The log (spec 0027): a `tracing` subscriber that keeps what the program's
//! events say in a buffer (for the logs page), in `tidal-player.log` in the
//! state directory, and on a daemon's stderr, as the run mode says.
//!
//! The pure parts take their inputs as parameters (the time, its UTC offset,
//! the size limit), so tests wait for nothing and write no megabyte. The
//! global subscriber can be set once per process, so tests drive a
//! [`Logger`] (or a layer under `with_default`) directly.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::SystemTime;

use tidal_player_core::log::{
    LogBatch, LogBuffer, LogLevel, LogLine, LogSetting, build_text, is_kept,
};
use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::{Layer, Registry};

/// The log file's name in the state directory.
pub const LOG_FILE: &str = "tidal-player.log";
/// What the log file becomes when it is rotated.
pub const OLD_FILE: &str = "tidal-player.log.old";
/// The size the log file does not pass (1 MiB).
pub const FILE_LIMIT: u64 = 1 << 20;
/// The target of the log's own line (`cannot write …`).
const OWN_TARGET: &str = "tidal_player::logging";

/// Which kind of process is logging (spec 0027 "Who keeps what").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// A TUI that is the player.
    Standalone,
    /// `tidal-player daemon`.
    Daemon,
    /// `tidal-player play`.
    Play,
    /// A TUI attached to another process's player: its own lines only.
    Attached,
    /// Every other command: it logs nothing.
    Other,
}

/// Where a mode's lines go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Roles {
    pub buffer: bool,
    pub file: bool,
    pub stderr: bool,
}

impl Mode {
    /// The sinks this mode installs.
    pub fn roles(self) -> Roles {
        let (buffer, file, stderr) = match self {
            Self::Standalone | Self::Play => (true, true, false),
            Self::Daemon => (true, true, true),
            // Only the player writes the file: a client writes none.
            Self::Attached => (true, false, false),
            Self::Other => (false, false, false),
        };
        Roles {
            buffer,
            file,
            stderr,
        }
    }
}

/// The local time zone, as the offset a line is written with.
#[derive(Debug, Clone)]
pub enum Zone {
    /// A fixed offset from UTC, in seconds (tests).
    Fixed(i32),
    /// The system's zone, read once.
    System(jiff::tz::TimeZone),
}

impl Zone {
    /// The machine's local zone.
    pub fn system() -> Self {
        Self::System(jiff::tz::TimeZone::system())
    }

    /// The offset from UTC in seconds at `at`.
    pub fn offset_at(&self, at: SystemTime) -> i32 {
        match self {
            Self::Fixed(secs) => *secs,
            Self::System(zone) => jiff::Timestamp::try_from(at)
                .map_or(0, |timestamp| zone.to_offset(timestamp).seconds()),
        }
    }
}

/// The local UTC offset now, in seconds: what the logs page draws times
/// with.
pub fn local_offset_secs() -> i32 {
    Zone::system().offset_at(SystemTime::now())
}

/// `at` in the zone `offset_secs` from UTC (UTC when the offset is out of
/// range).
pub(crate) fn zoned(at: SystemTime, offset_secs: i32) -> jiff::Zoned {
    let offset = jiff::tz::Offset::from_seconds(offset_secs).unwrap_or(jiff::tz::Offset::UTC);
    jiff::Timestamp::try_from(at)
        .unwrap_or(jiff::Timestamp::UNIX_EPOCH)
        .to_zoned(jiff::tz::TimeZone::fixed(offset))
}

/// A line of the log file: `2026-11-10T21:04:05.123+08:00  WARN target: text`,
/// the time in the zone `offset_secs` from UTC, the level right-aligned to
/// 5 columns. No line break at the end.
pub fn file_line(
    at: SystemTime,
    offset_secs: i32,
    level: LogLevel,
    target: &str,
    text: &str,
) -> String {
    let zoned = zoned(at, offset_secs);
    let offset = zoned.offset();
    let sign = if offset.seconds() < 0 { '-' } else { '+' };
    let minutes = offset.seconds().unsigned_abs() / 60;
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}{sign}{:02}:{:02} {:>5} {target}: {text}",
        zoned.year(),
        zoned.month(),
        zoned.day(),
        zoned.hour(),
        zoned.minute(),
        zoned.second(),
        zoned.millisecond(),
        minutes / 60,
        minutes % 60,
        level.as_str(),
    )
}

/// A daemon's stderr line: `LEVEL target: text`, without the time (the
/// journal has it).
pub fn stderr_line(level: LogLevel, target: &str, text: &str) -> String {
    format!("{} {target}: {text}", level.as_str())
}

/// Whether a file of `size` bytes is rotated before `next` more bytes are
/// written: it is not empty and would pass `limit`. A file already over the
/// limit at start is rotated with `next` = 0.
pub fn needs_rotation(size: u64, next: u64, limit: u64) -> bool {
    size > 0 && size.saturating_add(next) > limit
}

/// The open log file and what it holds.
#[derive(Debug)]
struct FileLog {
    path: PathBuf,
    old: PathBuf,
    limit: u64,
    file: File,
    size: u64,
}

fn open_append(path: &Path) -> std::io::Result<File> {
    OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(path)
}

impl FileLog {
    /// Opens `dir`'s log file, creating the directory, rotating a file that
    /// is already over `limit`.
    fn open(dir: &Path, limit: u64) -> std::io::Result<Self> {
        std::fs::create_dir_all(dir)?;
        let path = dir.join(LOG_FILE);
        let old = dir.join(OLD_FILE);
        let size = std::fs::metadata(&path).map_or(0, |m| m.len());
        if needs_rotation(size, 0, limit) {
            std::fs::rename(&path, &old)?;
        }
        let file = open_append(&path)?;
        let size = file.metadata()?.len();
        Ok(Self {
            path,
            old,
            limit,
            file,
            size,
        })
    }

    /// Appends `line` and a line break, first rotating when the line would
    /// pass the limit.
    fn append(&mut self, line: &str) -> std::io::Result<()> {
        let bytes = u64::try_from(line.len() + 1).unwrap_or(u64::MAX);
        if needs_rotation(self.size, bytes, self.limit) {
            std::fs::rename(&self.path, &self.old)?;
            self.file = open_append(&self.path)?;
            self.size = 0;
        }
        self.file.write_all(format!("{line}\n").as_bytes())?;
        self.size += bytes;
        Ok(())
    }
}

/// What the file sink is doing.
#[derive(Debug)]
enum FileState {
    /// This mode (or `off`) keeps no file.
    None,
    Active(FileLog),
    /// A write failed: no more tries for the rest of the run.
    GivenUp,
}

/// A process's lines: the buffer, the file and stderr that the mode has.
pub struct Logger {
    setting: LogSetting,
    buffer: Arc<Mutex<LogBuffer>>,
    file: Mutex<FileState>,
    /// The file's path, for the line that says it was given up.
    path: PathBuf,
    stderr: Option<Mutex<Box<dyn Write + Send>>>,
    zone: Zone,
}

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Logger {
    /// A logger for a process with `roles`, keeping `setting`'s lines: its
    /// file `dir`/[`LOG_FILE`] (never created at `off`, or without the file
    /// role), rotated at `limit`, and `stderr` written when the mode has
    /// that role. A file that cannot be opened is given up at once, with
    /// the line that says so.
    pub fn new(
        setting: LogSetting,
        roles: Roles,
        dir: &Path,
        limit: u64,
        stderr: Box<dyn Write + Send>,
        zone: Zone,
    ) -> Self {
        let logger = Self {
            setting,
            buffer: Arc::new(Mutex::new(LogBuffer::default())),
            file: Mutex::new(FileState::None),
            path: dir.join(LOG_FILE),
            stderr: (roles.stderr && setting != LogSetting::Off).then(|| Mutex::new(stderr)),
            zone,
        };
        if roles.file && setting != LogSetting::Off {
            match FileLog::open(dir, limit) {
                Ok(file) => *locked(&logger.file) = FileState::Active(file),
                Err(e) => {
                    *locked(&logger.file) = FileState::GivenUp;
                    logger.gave_up(&e);
                }
            }
        }
        logger
    }

    /// What clients ask for the lines through.
    pub fn handle(&self) -> LogHandle {
        LogHandle {
            buffer: Arc::clone(&self.buffer),
            off: self.setting == LogSetting::Off,
        }
    }

    /// Keeps one line of `level` from `target` that happened at `at`, when
    /// the setting keeps it.
    pub fn record(&self, at: SystemTime, level: LogLevel, target: &str, text: &str) {
        if !is_kept(self.setting, level, target) {
            return;
        }
        locked(&self.buffer).push(at, level, target, text.to_owned());
        self.to_stderr(level, target, text);
        let mut state = locked(&self.file);
        if let FileState::Active(file) = &mut *state {
            let offset = self.zone.offset_at(at);
            if let Err(e) = file.append(&file_line(at, offset, level, target, text)) {
                *state = FileState::GivenUp;
                drop(state);
                self.gave_up(&e);
            }
        }
    }

    fn to_stderr(&self, level: LogLevel, target: &str, text: &str) {
        if let Some(stderr) = &self.stderr {
            // A closed stderr is not the player's problem.
            let _ = writeln!(locked(stderr), "{}", stderr_line(level, target, text));
        }
    }

    /// The one line that says the file is given up: in the buffer and on
    /// stderr, never in the file.
    fn gave_up(&self, error: &std::io::Error) {
        let text = format!("cannot write {}: {error}", self.path.display());
        locked(&self.buffer).push(SystemTime::now(), LogLevel::Warn, OWN_TARGET, text.clone());
        self.to_stderr(LogLevel::Warn, OWN_TARGET, &text);
    }
}

/// A shared view of a process's buffer: what serves `Logs` requests and
/// feeds an attached TUI's own lines to its page.
#[derive(Debug, Clone)]
pub struct LogHandle {
    buffer: Arc<Mutex<LogBuffer>>,
    off: bool,
}

impl LogHandle {
    /// A handle on an empty buffer of its own (a player that keeps no log
    /// installs none; tests fill one with [`LogHandle::push`]).
    pub fn detached(off: bool) -> Self {
        Self {
            buffer: Arc::new(Mutex::new(LogBuffer::default())),
            off,
        }
    }

    /// Keeps a line in the buffer only (tests).
    pub fn push(&self, at: SystemTime, level: LogLevel, target: &str, text: &str) -> u64 {
        locked(&self.buffer).push(at, level, target, text.to_owned())
    }

    /// The lines after `after`, and whether the log is off (spec 0027
    /// "Talking to the player").
    pub fn batch(&self, after: Option<u64>) -> LogBatch {
        LogBatch {
            lines: locked(&self.buffer).after(after),
            off: self.off,
        }
    }

    /// The lines after `after`.
    pub fn lines_after(&self, after: Option<u64>) -> Vec<LogLine> {
        locked(&self.buffer).after(after)
    }
}

/// The `tracing` layer that hands each kept event to a [`Logger`].
pub struct LogLayer {
    logger: Arc<Logger>,
}

impl LogLayer {
    pub fn new(logger: Arc<Logger>) -> Self {
        Self { logger }
    }
}

/// The event's message and its other fields, as text.
#[derive(Default)]
struct Fields {
    message: String,
    fields: Vec<(&'static str, String)>,
}

impl Visit for Fields {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            value.clone_into(&mut self.message);
        } else {
            self.fields.push((field.name(), value.to_owned()));
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        let text = format!("{value:?}");
        if field.name() == "message" {
            self.message = text;
        } else {
            self.fields.push((field.name(), text));
        }
    }
}

fn level_of(level: &Level) -> LogLevel {
    match *level {
        Level::ERROR => LogLevel::Error,
        Level::WARN => LogLevel::Warn,
        Level::INFO => LogLevel::Info,
        Level::DEBUG => LogLevel::Debug,
        Level::TRACE => LogLevel::Trace,
    }
}

impl<S: Subscriber> Layer<S> for LogLayer {
    fn enabled(&self, metadata: &tracing::Metadata<'_>, _: Context<'_, S>) -> bool {
        is_kept(
            self.logger.setting,
            level_of(metadata.level()),
            metadata.target(),
        )
    }

    fn max_level_hint(&self) -> Option<tracing::level_filters::LevelFilter> {
        Some(match self.logger.setting {
            LogSetting::Off => tracing::level_filters::LevelFilter::OFF,
            LogSetting::Error => tracing::level_filters::LevelFilter::ERROR,
            LogSetting::Warn => tracing::level_filters::LevelFilter::WARN,
            LogSetting::Info => tracing::level_filters::LevelFilter::INFO,
            LogSetting::Debug => tracing::level_filters::LevelFilter::DEBUG,
            LogSetting::Trace => tracing::level_filters::LevelFilter::TRACE,
        })
    }

    fn on_event(&self, event: &Event<'_>, _: Context<'_, S>) {
        let mut fields = Fields::default();
        event.record(&mut fields);
        let pairs: Vec<(&str, &str)> = fields
            .fields
            .iter()
            .map(|(name, value)| (*name, value.as_str()))
            .collect();
        let metadata = event.metadata();
        self.logger.record(
            SystemTime::now(),
            level_of(metadata.level()),
            metadata.target(),
            &build_text(&fields.message, &pairs),
        );
    }
}

/// Installs the log of a process in `mode` as the global `tracing`
/// subscriber: the buffer, the file in `state_dir`, stderr, as the mode
/// has them. `None` for a mode that logs nothing. At `off` nothing is
/// installed and no file is touched; the handle says so.
pub fn install(mode: Mode, setting: LogSetting, state_dir: &Path) -> Option<LogHandle> {
    let roles = mode.roles();
    if !roles.buffer {
        return None;
    }
    let logger = Arc::new(Logger::new(
        setting,
        roles,
        state_dir,
        FILE_LIMIT,
        Box::new(std::io::stderr()),
        Zone::system(),
    ));
    let handle = logger.handle();
    if setting != LogSetting::Off {
        // Set once per process: a second call (a test binary) keeps the
        // first subscriber.
        let _ = tracing::subscriber::set_global_default(
            Registry::default().with(LogLayer::new(logger)),
        );
    }
    Some(handle)
}

#[cfg(test)]
mod tests;
