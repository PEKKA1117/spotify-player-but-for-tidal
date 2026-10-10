//! Filtering the focused window (spec 0012): `/` (`Search`) narrows the
//! focused list window, or the queue, to the rows whose fields contain
//! what is typed, ignoring case.
//!
//! Red stub (spec 0012 "Test plan"): `matches` keeps every row, the title
//! ignores the filter and `/` keeps today's behaviour.

use crate::track::EntryId;

use super::State;
use super::page::{Page, Row};

/// The longest filter: typing stops here.
pub const MAX_FILTER: usize = 100;
/// The cursor block after the filter's text while typing.
pub const CURSOR: char = '▏';

/// A window's (or the queue's) filter.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Filter {
    /// What has been typed.
    pub text: String,
    /// Whether keys go to the filter (after `/`, until `Enter` or `Esc`).
    pub typing: bool,
}

impl Filter {
    /// Whether the filter narrows anything.
    pub fn active(&self) -> bool {
        !self.text.trim().is_empty()
    }

    /// The title's filter parts (stub: none).
    pub fn title_parts(&self, _matches: usize) -> Vec<String> {
        Vec::new()
    }
}

/// What a window with a filter matching nothing says.
pub fn no_match(text: &str) -> String {
    format!("No rows match \"{text}\"")
}

/// Whether `row` matches `filter` (stub: every row).
pub fn matches(_row: Row<'_>, _filter: &str) -> bool {
    true
}

fn queue_page(state: &State) -> &Page {
    &state.history[0]
}

/// The queue's filter.
pub fn queue_filter(state: &State) -> &Filter {
    &queue_page(state).filter
}

/// The indices of the entries the queue page shows (stub: all).
pub fn queue_shown(state: &State) -> Vec<usize> {
    (0..state.queue().len()).collect()
}

/// The queue window's title (stub: no filter).
pub fn queue_title(state: &State) -> String {
    format!("Queue ({})", state.queue().len())
}

/// The queue's selected entry (stub: the cursor's).
pub fn queue_selected(state: &State) -> Option<EntryId> {
    state.cursor
}

#[cfg(test)]
mod tests;
