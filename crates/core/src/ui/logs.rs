//! The logs page (spec 0027). Stub.

use std::time::Duration;

use crate::log::{LogBatch, LogLevel, LogLine};

use super::{Effect, State};

pub const POLL: Duration = Duration::from_secs(1);
pub const MAX_LINES: usize = 1000;
pub const LOADING: &str = "Loading…";
pub const NO_LINES: &str = "No log lines yet";
pub const OFF: &str = "Logging is off: set log_level in app.toml";

/// How a failure reads in the window.
pub fn could_not_load(reason: &str) -> String {
    format!("Could not load the log: {reason}")
}

/// What the logs window holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogsContent {
    Lines,
    Loading,
    Failed(String),
    Disconnected(String),
    Off,
    NoLines,
    NoMatch(String),
}

impl LogsContent {
    pub fn message(&self) -> Option<String> {
        None
    }
}

/// A line as the page draws it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Drawn {
    pub level: LogLevel,
    pub text: String,
}

pub fn shown(_state: &State) -> Vec<Drawn> {
    Vec::new()
}

pub fn offset(_state: &State) -> usize {
    0
}

pub fn title(_state: &State) -> String {
    "Logs".into()
}

pub fn content(_state: &State) -> LogsContent {
    LogsContent::NoLines
}

/// Everything the page keeps of the logs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LogStore;

pub(super) fn clock(_state: &mut State, _now: Duration) -> Vec<Effect> {
    Vec::new()
}

pub(super) fn reply(_state: &mut State, _id: u64, _result: Result<LogBatch, String>) {}

pub(super) fn own_lines(_state: &mut State, _lines: Vec<LogLine>) {}

#[cfg(test)]
mod tests;
