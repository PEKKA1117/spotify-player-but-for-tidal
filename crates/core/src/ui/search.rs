//! The search page (spec 0007 "The search page"): its input, top hit and
//! focus, the keys while the input has the focus, sending a search and
//! acting on the top hit. The four result windows are 0006 windows.

use crate::library::TopHit;
use crate::protocol::Command;

use super::browse;
use super::page::{Page, PageKind, WindowKind};
use super::{Effect, Key, State};

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

impl Search {
    /// Appends `text` to the input, without control characters, up to
    /// [`MAX_QUERY`] characters.
    fn append(&mut self, text: impl IntoIterator<Item = char>) {
        let room = MAX_QUERY.saturating_sub(self.input.chars().count());
        self.input
            .extend(text.into_iter().filter(|c| !c.is_control()).take(room));
    }
}

/// The search on top, if the page on top is a search page.
fn search_mut(state: &mut State) -> Option<&mut Search> {
    state.history.last_mut()?.search.as_mut()
}

/// Whether the page on top is a search page with its input focused.
pub(super) fn input_focused(state: &State) -> bool {
    state
        .page()
        .search
        .as_ref()
        .is_some_and(|s| s.focus == SearchFocus::Input)
}

/// `g s`: a new, empty search page; on a search page on top, its input
/// gets the focus instead.
pub(super) fn open(state: &mut State) -> Vec<Effect> {
    match search_mut(state) {
        Some(search) => search.focus = SearchFocus::Input,
        None => browse::push(state, Page::new(PageKind::Search(String::new()))),
    }
    Vec::new()
}

/// Pasted text, while the search input has the focus.
pub(super) fn paste(state: &mut State, text: &str) {
    if input_focused(state)
        && let Some(search) = search_mut(state)
    {
        search.append(text.chars());
    }
}

/// A key while the search input has the focus: printable keys type, and
/// only `Enter` asks anything (spec 0007 "Input keys").
pub(super) fn input_key(state: &mut State, key: Key) -> Vec<Effect> {
    let Some(search) = search_mut(state) else {
        return Vec::new();
    };
    match key {
        Key::Char(c) => search.append([c]),
        Key::Backspace => {
            search.input.pop();
        }
        Key::Ctrl('u') => search.input.clear(),
        Key::Ctrl('c') => return vec![Effect::Quit],
        Key::Ctrl('q') => return browse::back(state),
        Key::Enter => return send(state),
        Key::Tab => return cycle(state, true),
        Key::BackTab => return cycle(state, false),
        Key::Esc => {
            let page = state
                .history
                .last_mut()
                .expect("the queue page is never popped");
            if let Some((focus, window)) = page.result_focus() {
                page.focus_on(focus, window);
            }
        }
        _ => {}
    }
    Vec::new()
}

/// `Search` (`/`) on a search page: back to its input; elsewhere nothing
/// (spec 0008: the in-page search of other pages is out of scope).
pub(super) fn focus_input(state: &mut State) -> Vec<Effect> {
    if let Some(search) = search_mut(state) {
        search.focus = SearchFocus::Input;
    }
    Vec::new()
}

/// `Enter` on the input: the trimmed query, sent with a fresh ID; the page
/// starts over (no results, cursors at the top). Failed with the
/// connection's message while disconnected.
fn send(state: &mut State) -> Vec<Effect> {
    let index = browse::top(state);
    let Some(search) = state.history[index].search.as_ref() else {
        return Vec::new();
    };
    let query = search.input.trim().to_owned();
    if query.is_empty() {
        return Vec::new();
    }
    let input = search.input.clone();
    let mut page = Page::new(PageKind::Search(query));
    page.search = Some(Search {
        input,
        ..Search::default()
    });
    state.history[index] = page;
    let mut effects = Vec::new();
    browse::fetch_page(state, index, &mut effects);
    effects
}

/// `Tab`/`BackTab`: input → top hit (when shown) → each window → input,
/// wrapping.
pub(super) fn cycle(state: &mut State, forward: bool) -> Vec<Effect> {
    let page = state
        .history
        .last_mut()
        .expect("the queue page is never popped");
    let Some(search) = page.search.as_ref() else {
        return Vec::new();
    };
    let mut stops = vec![(SearchFocus::Input, page.focus)];
    if search.top_hit.is_some() {
        stops.push((SearchFocus::TopHit, page.focus));
    }
    stops.extend((0..page.windows.len()).map(|w| (SearchFocus::Windows, w)));
    let at = stops
        .iter()
        .position(|&(focus, window)| {
            focus == search.focus && (focus != SearchFocus::Windows || window == page.focus)
        })
        .unwrap_or(0);
    let count = stops.len();
    let next = if forward {
        (at + 1) % count
    } else {
        (at + count - 1) % count
    };
    let (focus, window) = stops[next];
    page.focus_on(focus, window);
    Vec::new()
}

/// `Enter` on a row or the top hit: an album, playlist or artist opens its
/// page; a track replaces the queue with the loaded rows of *Tracks*,
/// nothing fetched (spec 0007 decision 4), the top hit first when it is
/// not among them.
pub(super) fn enter(state: &mut State) -> Vec<Effect> {
    let page = state.page();
    let Some(row) = page.selected() else {
        return Vec::new();
    };
    if let Some(kind) = row.page() {
        return browse::open(state, kind);
    }
    let Some(chosen) = row.track() else {
        return Vec::new();
    };
    let Some(window) = page
        .windows
        .iter()
        .find(|w| w.kind == WindowKind::SearchTracks)
    else {
        return Vec::new();
    };
    let mut tracks = window.tracks();
    let start = match page.focused() {
        Some(focused) if focused.kind == WindowKind::SearchTracks => focused.cursor,
        _ => match tracks.iter().position(|t| t.id == chosen.id) {
            Some(index) => index,
            None => {
                tracks.insert(0, chosen.clone());
                0
            }
        },
    };
    vec![Effect::Send(Command::LoadQueue { tracks, start })]
}

#[cfg(test)]
mod tests;
