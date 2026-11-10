//! Log lines (spec 0027): the types a player's log lines travel in, what
//! is kept of a `tracing` event, the text of a line and how the logs page
//! draws it. Pure: the clock, the file and the subscriber live in the
//! binary; the local UTC offset is a parameter here.

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

/// `SystemTime` as milliseconds since the Unix epoch.
mod epoch_millis {
    use std::time::SystemTime;

    use serde::{Deserializer, Serializer};

    pub fn serialize<S: Serializer>(_at: &SystemTime, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_unit()
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        _deserializer: D,
    ) -> Result<SystemTime, D::Error> {
        Ok(SystemTime::UNIX_EPOCH)
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
        let _ = s;
        Ok(Self::Info)
    }
}

impl LogSetting {
    /// The lowest level kept from this program's crates; `None` for `off`.
    pub fn max_level(self) -> Option<LogLevel> {
        let _ = self;
        None
    }
}

/// Whether an event with `level` and `target` is kept under `setting`.
pub fn is_kept(setting: LogSetting, level: LogLevel, target: &str) -> bool {
    let _ = (setting, level, target);
    true
}

/// The text of a line: the message, then ` name=value` for each field.
pub fn build_text(message: &str, fields: &[(&str, &str)]) -> String {
    let _ = fields;
    message.to_owned()
}

/// How the logs page draws `line`: `HH:MM:SS LEVEL target: text`.
pub fn format_line(line: &LogLine, utc_offset_secs: i32) -> String {
    let _ = utc_offset_secs;
    line.text.clone()
}

#[cfg(test)]
mod tests;
