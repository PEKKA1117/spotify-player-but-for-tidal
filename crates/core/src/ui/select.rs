//! Selecting several rows (spec 0020): each list window's selection (and
//! the queue's), the `s …` commands, motion mode, the selection actions
//! popup and `d s`. Pure model.

use std::collections::BTreeSet;

use crate::track::EntryId;

use super::keymap::UiCommand;
use super::page::Window;
use super::{Effect, State};

/// A window's selection: the rows marked, and the motion anchor while
/// motion mode is on. `K` is what holds a row: its index in the loaded
/// list (a window), its entry ID (the queue).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection<K: Ord> {
    rows: BTreeSet<K>,
    anchor: Option<K>,
}

impl<K: Ord> Default for Selection<K> {
    fn default() -> Self {
        Self {
            rows: BTreeSet::new(),
            anchor: None,
        }
    }
}

impl<K: Ord + Copy> Selection<K> {
    /// The rows marked, motion range aside.
    pub fn rows(&self) -> &BTreeSet<K> {
        &self.rows
    }

    /// The motion anchor, while motion mode is on.
    pub fn anchor(&self) -> Option<K> {
        self.anchor
    }

    /// Whether motion mode is on.
    pub fn motion(&self) -> bool {
        self.anchor.is_some()
    }
}

/// Where a selection is: the queue's, or window `window` of page `page`
/// of the history.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    Queue,
    Window { page: usize, window: usize },
}

/// A selected row: a window's by its index in the loaded list, the
/// queue's by its entry ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Held {
    Row(usize),
    Entry(EntryId),
}

/// What the TUI draws about a selection (spec 0020 "How it looks").
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Marks {
    /// The rows selected, the motion range included.
    pub count: usize,
    /// How many of them the filter hides.
    pub hidden: usize,
    /// Whether motion mode is on.
    pub motion: bool,
}

impl Marks {
    /// The title's parts, after the filter's: `2 selected` (`2 selected,
    /// 1 hidden`), `motion`.
    pub fn title_parts(&self) -> Vec<String> {
        Vec::new()
    }
}

/// The rows (indices into `window.rows`) shown as selected: the selection
/// and the motion range.
pub fn window_rows(window: &Window) -> BTreeSet<usize> {
    let _ = window;
    BTreeSet::new()
}

/// The marks of `window`'s selection.
pub fn window_marks(window: &Window) -> Marks {
    let _ = window;
    Marks::default()
}

/// The queue entries shown as selected, in the queue's order.
pub fn queue_entries(state: &State) -> Vec<EntryId> {
    let _ = state;
    Vec::new()
}

/// The marks of the queue's selection.
pub fn queue_marks(state: &State) -> Marks {
    let _ = state;
    Marks::default()
}

/// One of the selection commands where the UI is (a page, no popup):
/// `count` is the count typed in a `SelectDown`/`SelectUp` sequence (1
/// without one).
pub(super) fn command(state: &mut State, command: UiCommand, count: usize) -> Vec<Effect> {
    let _ = (state, command, count);
    Vec::new()
}

#[cfg(test)]
mod tests;
