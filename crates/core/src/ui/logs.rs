//! The logs page (spec 0027): one page of the history that shows the
//! player's log lines and the TUI's own, merged by time. Pure model: the
//! lines ([`LogStore`], in [`State::logs`]), when to ask the player for
//! more, and what the page shows (follow or scroll, filter, titles,
//! messages); the rendering reads [`shown`], [`offset`], [`title`] and
//! [`content`].
//!
//! Asking: nothing here has a clock of its own. The caller feeds
//! [`Action::Clock`] (a monotonic time) before every action and on every
//! tick; a request is due one [`POLL`] after each answer. Opening the
//! page, coming back to it and a `Welcome` ask at once.
//!
//! Scrolling is counted in **lines** (a log line, wrapped or not), as the
//! lyrics page's is: the model does not know the window's width, so a
//! renderer that wraps long lines turns [`offset`] (the first line to
//! draw) into the first row. [`State::list_height`] is the window's
//! height in rows. While following, the last line is the one on the
//! window's last row.

use std::time::Duration;

use crate::log::{LogBatch, LogLevel, LogLine, format_line};

use super::dispatch::step;
use super::filter::{self, Filter};
use super::keymap::UiCommand;
use super::page::{Page, PageKind};
use super::{Connection, Effect, State, browse};

/// How long after an answer the next request is due.
pub const POLL: Duration = Duration::from_secs(1);
/// How many lines the page keeps, after merging.
pub const MAX_LINES: usize = 1000;
/// What the window says before the first answer.
pub const LOADING: &str = "Loading…";
/// What the window says for no lines.
pub const NO_LINES: &str = "No log lines yet";
/// What the window says when the player keeps no log.
pub const OFF: &str = "Logging is off: set log_level in app.toml";

/// How a failed answer reads in the window.
pub fn could_not_load(reason: &str) -> String {
    format!("Could not load the log: {reason}")
}

/// Where a line came from: the player's buffer, or the TUI's own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Origin {
    Player,
    Own,
}

/// A kept line and where it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    origin: Origin,
    line: LogLine,
}

impl Entry {
    /// What names the line among the kept ones: its origin and `seq`.
    fn key(&self) -> (Origin, u64) {
        (self.origin, self.line.seq)
    }
}

/// The lines the logs page shows and its asking (spec 0027).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LogStore {
    /// The kept lines, merged by time (the player's first on equal
    /// times), the last [`MAX_LINES`].
    entries: Vec<Entry>,
    /// The newest `seq` received from the player.
    last_seq: Option<u64>,
    /// The newest `seq` taken from the TUI's own lines.
    own_seq: Option<u64>,
    /// The player's last answer said its log is off.
    off: bool,
    /// An answer came since the last `Welcome`.
    answered: bool,
    /// The last answer's failure, until an answer succeeds.
    error: Option<String>,
    /// The ID of the request waiting for its answer.
    in_flight: Option<u64>,
    /// When the next request is due; `None`: at once.
    due: Option<Duration>,
    /// The last time [`Action::Clock`](super::Action::Clock) gave.
    now: Duration,
    /// The local time's offset from UTC in seconds, for the time each
    /// line is drawn with (the core has no clock or zone); 0 until the
    /// caller sets it.
    pub utc_offset_secs: i32,
}

impl LogStore {
    /// Merges `line` in: after the last line of its own origin, and by
    /// time against the other's (equal times: the player's first). The
    /// oldest line goes when there are more than [`MAX_LINES`].
    fn insert(&mut self, origin: Origin, line: LogLine) {
        let mut at = self
            .entries
            .iter()
            .rposition(|e| e.origin == origin)
            .map_or(0, |i| i + 1);
        while let Some(entry) = self.entries.get(at) {
            let passed = match origin {
                Origin::Player => entry.origin == Origin::Own && entry.line.at < line.at,
                Origin::Own => entry.origin == Origin::Player && entry.line.at <= line.at,
            };
            if !passed {
                break;
            }
            at += 1;
        }
        self.entries.insert(at, Entry { origin, line });
        if self.entries.len() > MAX_LINES {
            self.entries.remove(0);
        }
    }
}

/// The logs page's view: whether it follows the newest line, and where it
/// holds still when it does not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogsView {
    /// Whether new lines scroll the view (it is at the bottom).
    pub following: bool,
    /// The line at the view's top while not following.
    anchor: Option<(Origin, u64)>,
}

impl Default for LogsView {
    fn default() -> Self {
        Self {
            following: true,
            anchor: None,
        }
    }
}

/// What the logs window holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogsContent {
    /// Lines: draw [`shown`] from [`offset`].
    Lines,
    /// [`LOADING`].
    Loading,
    /// A failed answer, as it reads in the window.
    Failed(String),
    /// The connection's message while no line is held.
    Disconnected(String),
    /// [`OFF`].
    Off,
    /// [`NO_LINES`].
    NoLines,
    /// The filter matches nothing: its text.
    NoMatch(String),
}

impl LogsContent {
    /// The message that replaces the lines, if this is not lines.
    pub fn message(&self) -> Option<String> {
        Some(match self {
            Self::Lines => return None,
            Self::Loading => LOADING.to_owned(),
            Self::Failed(message) | Self::Disconnected(message) => message.clone(),
            Self::Off => OFF.to_owned(),
            Self::NoLines => NO_LINES.to_owned(),
            Self::NoMatch(text) => filter::no_match(text),
        })
    }
}

/// A line as the page draws it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Drawn {
    /// Its level: `ERROR` and `WARN` lines are coloured.
    pub level: LogLevel,
    /// `HH:MM:SS LEVEL target: text`, without control characters.
    pub text: String,
}

/// The filter of the page on top, or none when it is not the logs page.
fn filter_of(state: &State) -> &Filter {
    static NONE: Filter = Filter {
        text: String::new(),
        typing: false,
    };
    let page = state.page();
    if page.kind == PageKind::Logs {
        &page.filter
    } else {
        &NONE
    }
}

/// The indices (into the kept lines) of the lines shown: all of them, or
/// those whose drawn text contains the filter, ignoring case.
fn shown_indices(state: &State) -> Vec<usize> {
    let filter = filter_of(state);
    let store = &state.logs;
    let needle = filter.text.to_lowercase();
    store
        .entries
        .iter()
        .enumerate()
        .filter(|(_, e)| {
            !filter.active()
                || format_line(&e.line, store.utc_offset_secs)
                    .to_lowercase()
                    .contains(&needle)
        })
        .map(|(i, _)| i)
        .collect()
}

/// The lines the page shows, oldest first, as they are drawn.
pub fn shown(state: &State) -> Vec<Drawn> {
    let store = &state.logs;
    shown_indices(state)
        .into_iter()
        .map(|i| {
            let line = &store.entries[i].line;
            Drawn {
                level: line.level,
                text: format_line(line, store.utc_offset_secs),
            }
        })
        .collect()
}

/// How far the view can scroll: the last line at the bottom of the
/// window.
fn max_offset(shown: usize, height: usize) -> usize {
    shown.saturating_sub(height.max(1))
}

/// The first of the [`shown`] lines to draw: the bottom while following,
/// else the line the view holds (the oldest line when its own was
/// dropped).
pub fn offset(state: &State) -> usize {
    let shown = shown_indices(state);
    let max = max_offset(shown.len(), state.list_height);
    let Some(view) = state.page().logs.as_ref().filter(|v| !v.following) else {
        return max;
    };
    let store = &state.logs;
    let held = view
        .anchor
        .and_then(|key| store.entries.iter().position(|e| e.key() == key))
        .map_or(0, |at| {
            shown.iter().position(|&i| i >= at).unwrap_or(shown.len())
        });
    held.min(max)
}

/// The window's title: `Logs`, `Logs · scrolled` while not following,
/// then the filter's parts (`Logs · /reserve · 3 matches`).
pub fn title(state: &State) -> String {
    let mut parts = vec!["Logs".to_owned()];
    if state.page().logs.as_ref().is_some_and(|v| !v.following) {
        parts.push("scrolled".to_owned());
    }
    parts.extend(filter_of(state).title_parts(shown_indices(state).len()));
    parts.join(" · ")
}

/// What the logs window holds.
pub fn content(state: &State) -> LogsContent {
    let store = &state.logs;
    if !shown_indices(state).is_empty() {
        return LogsContent::Lines;
    }
    // Lines held, none shown: the filter hides them all.
    if !store.entries.is_empty() {
        return LogsContent::NoMatch(filter_of(state).text.clone());
    }
    if state.connection != Connection::Connected {
        return LogsContent::Disconnected(browse::connection_message(state));
    }
    if let Some(error) = &store.error {
        return LogsContent::Failed(could_not_load(error));
    }
    if !store.answered {
        return LogsContent::Loading;
    }
    if store.off {
        LogsContent::Off
    } else {
        LogsContent::NoLines
    }
}

// --- asking ------------------------------------------------------------------

/// Asks for the lines after the newest one: nothing while disconnected or
/// while a request waits for its answer.
fn ask(state: &mut State) -> Vec<Effect> {
    if state.connection != Connection::Connected || state.logs.in_flight.is_some() {
        return Vec::new();
    }
    let id = state.next_request;
    state.next_request += 1;
    state.logs.in_flight = Some(id);
    vec![Effect::Logs {
        id,
        after: state.logs.last_seq,
    }]
}

/// `OpenLogs` (`g o`): pushes the page and asks at once; nothing when it
/// is on top already.
pub(super) fn open(state: &mut State) -> Vec<Effect> {
    if state.page().kind == PageKind::Logs {
        return Vec::new();
    }
    browse::push(state, Page::new(PageKind::Logs));
    ask(state)
}

/// A page was popped: the logs page coming back on top asks at once.
pub(super) fn come_back(state: &mut State) -> Vec<Effect> {
    if state.page().kind == PageKind::Logs {
        ask(state)
    } else {
        Vec::new()
    }
}

/// [`Action::Clock`](super::Action::Clock): the page on top asks when an
/// answer was a second ago.
pub(super) fn clock(state: &mut State, now: Duration) -> Vec<Effect> {
    state.logs.now = now;
    if state.page().kind != PageKind::Logs || state.logs.due.is_some_and(|due| now < due) {
        return Vec::new();
    }
    ask(state)
}

/// The answer to request `id`; dropped when no request waits for it.
pub(super) fn reply(state: &mut State, id: u64, result: Result<LogBatch, String>) {
    let store = &mut state.logs;
    if store.in_flight != Some(id) {
        return;
    }
    store.in_flight = None;
    store.due = Some(store.now + POLL);
    match result {
        Ok(batch) => {
            store.error = None;
            store.answered = true;
            store.off = batch.off;
            for line in batch.lines {
                if store.last_seq.is_some_and(|seen| line.seq <= seen) {
                    continue;
                }
                store.last_seq = Some(line.seq);
                store.insert(Origin::Player, line);
            }
        }
        Err(message) => store.error = Some(message),
    }
}

/// The TUI's own lines, new since the last ones given.
pub(super) fn own_lines(state: &mut State, lines: Vec<LogLine>) {
    let store = &mut state.logs;
    for line in lines {
        if store.own_seq.is_some_and(|seen| line.seq <= seen) {
            continue;
        }
        store.own_seq = Some(line.seq);
        store.insert(Origin::Own, line);
    }
}

/// The connection is gone: the request waiting is lost with it.
pub(super) fn disconnected(state: &mut State) {
    state.logs.in_flight = None;
}

/// A `Welcome`: the old player's lines are forgotten (a restarted player
/// numbers its lines again), and the page on top asks for every line.
pub(super) fn welcome(state: &mut State) -> Vec<Effect> {
    let store = &mut state.logs;
    store.entries.retain(|e| e.origin == Origin::Own);
    store.last_seq = None;
    store.off = false;
    store.answered = false;
    store.error = None;
    store.in_flight = None;
    store.due = None;
    come_back(state)
}

// --- keys --------------------------------------------------------------------

/// A page command on the logs page: the list moves scroll the lines;
/// every other command that acts on rows does nothing there.
pub(super) fn command(state: &mut State, command: UiCommand) -> Vec<Effect> {
    if !command.is_list_move() {
        return Vec::new();
    }
    let shown = shown_indices(state);
    if shown.is_empty() {
        return Vec::new();
    }
    let height = state.list_height.max(1);
    let max = max_offset(shown.len(), height);
    let to = step(command, offset(state), max + 1, height);
    let anchor = state.logs.entries[shown[to]].key();
    if let Some(view) = state.history.last_mut().and_then(|p| p.logs.as_mut()) {
        // At the bottom it follows; anywhere else it holds the line at
        // its top.
        view.following = to >= max;
        view.anchor = (!view.following).then_some(anchor);
    }
    Vec::new()
}

/// `ExportLogs` (`e`). Stub (red).
pub(super) fn export(state: &mut State) -> Vec<Effect> {
    let lines = state.logs.entries.iter().map(|e| e.line.clone()).collect();
    vec![Effect::ExportLogs { lines }]
}

/// The result of an export. Stub (red).
pub(super) fn exported(_state: &mut State, _result: Result<(usize, String), String>) {}

/// `Esc` on the logs page: a kept filter is cleared first; with none, the
/// view follows again.
pub(super) fn escape(state: &mut State) -> Vec<Effect> {
    if filter::kept(state) {
        return filter::clear(state);
    }
    if let Some(view) = state.history.last_mut().and_then(|p| p.logs.as_mut()) {
        *view = LogsView::default();
    }
    Vec::new()
}

#[cfg(test)]
mod tests;
