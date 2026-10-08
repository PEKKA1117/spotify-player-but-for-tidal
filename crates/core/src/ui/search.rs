//! The search page (spec 0007 "The search page"): its input, top hit and
//! focus, the keys while the input has the focus, sending a search and
//! acting on the top hit. The four result windows are 0006 windows.

use crate::library::TopHit;

/// The longest query: typing stops here (spec 0007 "Sending").
pub const MAX_QUERY: usize = 200;
/// The default search page size (`TIDAL_PLAYER_SEARCH_PAGE_SIZE`).
pub const DEFAULT_SEARCH_PAGE_SIZE: u32 = 20;

/// What has the focus on a search page.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SearchFocus {
    /// The input row: printable keys type.
    #[default]
    Input,
    /// The *Top hit* row.
    TopHit,
    /// The window [`super::Page::focus`] names.
    Windows,
}

/// A search page's own state, beside its four windows.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Search {
    /// The text in the input (what `Enter` sends, trimmed).
    pub input: String,
    /// Tidal's top hit of the last search, when it named one.
    pub top_hit: Option<TopHit>,
    pub focus: SearchFocus,
}

#[cfg(test)]
mod tests;
