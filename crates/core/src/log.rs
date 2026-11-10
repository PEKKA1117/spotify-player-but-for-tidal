//! Log lines (spec 0027): the types a player's log lines travel in, what
//! is kept of a `tracing` event, the text of a line and how the logs page
//! draws it. Pure: the clock, the file and the subscriber live in the
//! binary; the local UTC offset is a parameter here.

use std::collections::VecDeque;
use std::str::FromStr;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

/// The level of a log line, most severe first (so `a <= b` reads "`a` is
/// at least as severe as `b`").
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl LogLevel {
    /// `ERROR`, `WARN`, `INFO`, `DEBUG`, `TRACE`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Error => "ERROR",
            Self::Warn => "WARN",
            Self::Info => "INFO",
            Self::Debug => "DEBUG",
            Self::Trace => "TRACE",
        }
    }
}

/// One log line: one `tracing` event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogLine {
    /// The player's number for the line: 1, 2, 3, … for its lifetime.
    pub seq: u64,
    /// When it happened; milliseconds since the Unix epoch on the wire.
    #[serde(with = "epoch_millis")]
    pub at: SystemTime,
    pub level: LogLevel,
    /// The module path (`tidal_player::player::reserve`).
    pub target: String,
    /// The message, then ` name=value` for each other field.
    pub text: String,
}

/// The answer to a `Logs` request: the buffer's lines after the asked
/// `seq`, oldest first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogBatch {
    pub lines: Vec<LogLine>,
    /// The player's `log_level` is `off`.
    pub off: bool,
}

/// `SystemTime` as milliseconds since the Unix epoch (a time before it is
/// 0).
mod epoch_millis {
    use std::time::{Duration, SystemTime};

    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(at: &SystemTime, serializer: S) -> Result<S::Ok, S::Error> {
        let millis = at
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
        serializer.serialize_u64(millis)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<SystemTime, D::Error> {
        let millis = u64::deserialize(deserializer)?;
        Ok(SystemTime::UNIX_EPOCH + Duration::from_millis(millis))
    }
}

/// How many lines a player's (or a TUI's own) buffer keeps.
pub const BUFFER_LINES: usize = 1000;

/// A process's last log lines (spec 0027 "Who keeps what"): the oldest is
/// dropped first, and every line is numbered `seq` 1, 2, 3, … for the
/// buffer's lifetime.
#[derive(Debug, Clone)]
pub struct LogBuffer {
    lines: VecDeque<LogLine>,
    next_seq: u64,
    capacity: usize,
}

impl Default for LogBuffer {
    fn default() -> Self {
        Self::with_capacity(BUFFER_LINES)
    }
}

impl LogBuffer {
    /// A buffer that keeps the last `capacity` lines.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            lines: VecDeque::new(),
            next_seq: 1,
            capacity,
        }
    }

    /// Keeps a new line and returns its `seq`.
    pub fn push(&mut self, at: SystemTime, level: LogLevel, target: &str, text: String) -> u64 {
        let seq = self.next_seq;
        self.next_seq += 1;
        self.lines.push_back(LogLine {
            seq,
            at,
            level,
            target: target.to_owned(),
            text,
        });
        while self.lines.len() > self.capacity {
            self.lines.pop_front();
        }
        seq
    }

    /// The kept lines with `seq` greater than `after`, oldest first; all of
    /// them for `None`, and for an `after` beyond the newest `seq` (the
    /// player restarted).
    pub fn after(&self, after: Option<u64>) -> Vec<LogLine> {
        // Beyond the newest: the asker's player is another one.
        let after = after.filter(|n| self.newest().is_some_and(|newest| *n <= newest));
        self.lines
            .iter()
            .filter(|line| after.is_none_or(|n| line.seq > n))
            .cloned()
            .collect()
    }

    /// The newest `seq`; `None` while nothing was pushed.
    pub fn newest(&self) -> Option<u64> {
        self.next_seq.checked_sub(1).filter(|n| *n > 0)
    }

    /// How many lines are kept.
    pub fn len(&self) -> usize {
        self.lines.len()
    }

    /// Whether no line is kept.
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }
}

/// The configured `log_level`: one of the six values of `app.toml`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LogSetting {
    Off,
    Error,
    Warn,
    #[default]
    Info,
    Debug,
    Trace,
}

/// A `log_level` that is none of the six values.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown log level `{0}` (expected off, error, warn, info, debug or trace)")]
pub struct ParseLogSettingError(pub String);

impl FromStr for LogSetting {
    type Err = ParseLogSettingError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.to_ascii_lowercase().as_str() {
            "off" => Self::Off,
            "error" => Self::Error,
            "warn" => Self::Warn,
            "info" => Self::Info,
            "debug" => Self::Debug,
            "trace" => Self::Trace,
            _ => return Err(ParseLogSettingError(s.to_owned())),
        })
    }
}

impl LogSetting {
    /// The lowest level kept from this program's crates; `None` for `off`.
    pub fn max_level(self) -> Option<LogLevel> {
        match self {
            Self::Off => None,
            Self::Error => Some(LogLevel::Error),
            Self::Warn => Some(LogLevel::Warn),
            Self::Info => Some(LogLevel::Info),
            Self::Debug => Some(LogLevel::Debug),
            Self::Trace => Some(LogLevel::Trace),
        }
    }
}

/// This program's crates: their targets, and the modules of them, are its
/// own (`tidal_player_extra` is not).
const OWN_CRATES: [&str; 4] = [
    "tidal_player",
    "tidal_player_core",
    "tidal_player_api",
    "tidal_player_audio",
];

/// Whether `target` is one of this program's crates or a module of one.
fn is_own(target: &str) -> bool {
    OWN_CRATES.iter().any(|name| {
        target
            .strip_prefix(name)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with("::"))
    })
}

/// Whether an event with `level` and `target` is kept under `setting`
/// (spec 0027 "What is logged"): this program's crates at `setting` and
/// above, any other crate at `WARN` and `ERROR` only (HTTP crates log
/// the session's headers below that), nothing at `off`.
pub fn is_kept(setting: LogSetting, level: LogLevel, target: &str) -> bool {
    let Some(lowest) = setting.max_level() else {
        return false;
    };
    if is_own(target) {
        level <= lowest
    } else {
        level <= LogLevel::Warn
    }
}

/// `text` with every line break (`\r\n` included) as one space.
fn one_line(text: &str) -> String {
    text.replace("\r\n", " ").replace(['\r', '\n'], " ")
}

/// The text of a line: the message, then ` name=value` for each other
/// field, in the order given, line breaks as spaces. With no message the
/// first field starts the text.
pub fn build_text(message: &str, fields: &[(&str, &str)]) -> String {
    let mut text = one_line(message);
    for (name, value) in fields {
        if !text.is_empty() {
            text.push(' ');
        }
        text.push_str(name);
        text.push('=');
        text.push_str(&one_line(value));
    }
    text
}

/// How the logs page draws `line`: `HH:MM:SS LEVEL target: text` in the
/// local time `utc_offset_secs` from UTC, the level left-aligned to 5
/// columns, control characters left out.
pub fn format_line(line: &LogLine, utc_offset_secs: i32) -> String {
    let secs = line
        .at
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(0));
    let day = (secs + i64::from(utc_offset_secs)).rem_euclid(86_400);
    let (h, m, s) = (day / 3600, day / 60 % 60, day % 60);
    let drawn = format!(
        "{h:02}:{m:02}:{s:02} {:<5} {}: {}",
        line.level.as_str(),
        line.target,
        line.text
    );
    drawn.chars().filter(|c| !c.is_control()).collect()
}

#[cfg(test)]
mod tests;
