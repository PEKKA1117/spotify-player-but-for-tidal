//! The keys help (spec 0008 "The keys help"): stub.

use super::State;
use super::keymap::{Binding, KeySequence};

/// The help popup's state: the filter and the highlighted row.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Help {
    /// What the filter holds.
    pub filter: String,
    /// Whether typed characters go to the filter.
    pub typing: bool,
    /// The highlighted binding row, counted over the visible rows only.
    pub cursor: usize,
}

/// One row: the keys of a binding, what it does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelpRow {
    pub keys: String,
    pub command: String,
    pub text: String,
    pub dim: bool,
    pub binding: Option<Binding>,
    pub sequences: Vec<KeySequence>,
}

/// A titled group of rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelpSection {
    pub title: String,
    pub rows: Vec<HelpRow>,
}

/// The sections of the help for `state`.
pub fn help(state: &State) -> Vec<HelpSection> {
    let rows = state
        .keymap
        .bindings()
        .iter()
        .map(|(sequence, binding)| HelpRow {
            keys: sequence.to_string(),
            command: binding.name().to_owned(),
            text: String::new(),
            dim: false,
            binding: Some(*binding),
            sequences: vec![sequence.clone()],
        })
        .collect();
    vec![HelpSection {
        title: "Keys".into(),
        rows,
    }]
}

/// The sections the filter keeps.
pub fn visible(state: &State) -> Vec<HelpSection> {
    help(state)
}

#[cfg(test)]
mod tests;
