//! Filtering the focused window (spec 0012): `/` (`Search`) narrows the
//! focused list window, or the queue, to the rows whose fields contain
//! what is typed, ignoring case. The filter's fixed keys while typing, the
//! matching, the queue's shown entries and the `ClosePopup` that clears
//! it. A window's filter lives on the [`Window`] (its rows shown are
//! cached there); the queue's on the queue [`Page`].

use crate::track::{EntryId, Track};

use super::dispatch::step;
use super::keymap::UiCommand;
use super::page::{Load, Page, PageKind, Row, group};
use super::{Effect, Key, State, browse};

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
    /// Whether the filter narrows anything: an empty or all-space filter
    /// is no filter.
    pub fn active(&self) -> bool {
        !self.text.trim().is_empty()
    }

    /// Whether the title shows the filter (typing, or a text kept).
    pub fn shown(&self) -> bool {
        self.typing || self.active()
    }

    /// The filter in a title: `/love`, `/love▏` while typing.
    pub fn label(&self) -> String {
        if self.typing {
            format!("/{}{CURSOR}", self.text)
        } else {
            format!("/{}", self.text)
        }
    }

    /// The title's filter parts: the label, and the match count when the
    /// filter is active (`/love · 12 matches`).
    pub fn title_parts(&self, matches: usize) -> Vec<String> {
        if !self.shown() {
            return Vec::new();
        }
        let mut parts = vec![self.label()];
        if self.active() {
            let n = u32::try_from(matches).unwrap_or(u32::MAX);
            parts.push(match n {
                1 => "1 match".to_owned(),
                n => format!("{} matches", group(n)),
            });
        }
        parts
    }

    /// Appends `text` without control characters, up to [`MAX_FILTER`]
    /// characters.
    fn append(&mut self, text: impl IntoIterator<Item = char>) {
        let room = MAX_FILTER.saturating_sub(self.text.chars().count());
        self.text
            .extend(text.into_iter().filter(|c| !c.is_control()).take(room));
    }
}

/// What a window with a filter matching nothing says (`No rows match
/// "xyz"`).
pub fn no_match(text: &str) -> String {
    format!("No rows match \"{text}\"")
}

/// Whether `row` matches `filter` (spec 0012 "What matches"): the filter,
/// as typed, is contained in one of the row's fields, ignoring case. An
/// empty or all-space filter matches every row.
pub fn matches(row: Row<'_>, filter: &str) -> bool {
    if filter.trim().is_empty() {
        return true;
    }
    let needle = filter.to_lowercase();
    let has = |field: &str| field.to_lowercase().contains(&needle);
    match row {
        Row::Track(track) => track_matches(track, &has),
        Row::Credit(credit) => track_matches(&credit.track, &has),
        Row::Album(album) => {
            has(&album.title)
                || album.artists.iter().any(|a| has(&a.name))
                || album.year.is_some_and(|y| has(&y.to_string()))
        }
        Row::Playlist(playlist) => has(&playlist.title),
        Row::Artist(artist) => has(&artist.name),
        Row::Mix(mix) => has(&mix.title) || mix.subtitle.as_deref().is_some_and(has),
    }
}

fn track_matches(track: &Track, has: &impl Fn(&str) -> bool) -> bool {
    has(&track.title)
        || track.version.as_deref().is_some_and(has)
        || track.artists.iter().any(|a| has(&a.name))
        || track.album.as_ref().is_some_and(|a| has(&a.title))
}

// --- the queue ---------------------------------------------------------------

/// The queue page (the bottom of the history).
fn queue_page(state: &State) -> &Page {
    &state.history[0]
}

/// The queue's filter.
pub fn queue_filter(state: &State) -> &Filter {
    &queue_page(state).filter
}

/// The indices (into the queue) of the entries the queue page shows: all
/// of them, or those matching its filter.
pub fn queue_shown(state: &State) -> Vec<usize> {
    let filter = queue_filter(state);
    state
        .queue()
        .iter()
        .enumerate()
        .filter(|(_, e)| !filter.active() || matches(Row::Track(&e.track), &filter.text))
        .map(|(i, _)| i)
        .collect()
}

/// The queue window's title: `Queue (40)`, `Queue (40 · /veil · 9
/// matches)`.
pub fn queue_title(state: &State) -> String {
    let filter = queue_filter(state);
    let mut parts = vec![state.queue().len().to_string()];
    parts.extend(filter.title_parts(queue_shown(state).len()));
    format!("Queue ({})", parts.join(" · "))
}

/// The queue's selected entry: the cursor's, when it is shown.
pub fn queue_selected(state: &State) -> Option<EntryId> {
    let id = state.cursor?;
    let filter = queue_filter(state);
    let entry = state.queue().iter().find(|e| e.id == id)?;
    (!filter.active() || matches(Row::Track(&entry.track), &filter.text)).then_some(id)
}

/// Keeps the queue cursor on its entry when it is shown, else moves it to
/// the first shown entry (none shown: it stays, selecting nothing).
pub(super) fn keep_queue_cursor(state: &mut State) {
    if queue_selected(state).is_some() {
        return;
    }
    if let Some(&first) = queue_shown(state).first() {
        let id = state.queue()[first].id;
        state.cursor = Some(id);
        state.anchor = Some(id);
    }
}

/// Moves the queue cursor to `to(index, len)` over the shown entries.
pub(super) fn move_queue_cursor(state: &mut State, to: impl Fn(usize, usize) -> usize) {
    let shown = queue_shown(state);
    if shown.is_empty() {
        return;
    }
    let queue = state.queue();
    let at = state
        .cursor
        .and_then(|id| shown.iter().position(|&i| queue[i].id == id))
        .unwrap_or(0);
    let id = queue[shown[to(at, shown.len()).min(shown.len() - 1)]].id;
    state.cursor = Some(id);
    state.anchor = Some(id);
}

// --- where the filter acts ---------------------------------------------------

/// The filter `/` acts on: the queue's, or a window of the page on top.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    Queue,
    Window(usize),
}

/// Where `/` acts (spec 0012 "Where it acts"): the queue page, or the
/// focused window of a loaded page; nothing on a loading or failed page,
/// a search's input or top hit.
fn target(state: &State) -> Option<Target> {
    let page = state.page();
    if page.kind == PageKind::Queue {
        return Some(Target::Queue);
    }
    if page.load != Load::Idle || !page.windows_focused() || page.windows.get(page.focus).is_none()
    {
        return None;
    }
    Some(Target::Window(page.focus))
}

/// The filter being typed into, if any (the queue's on the queue page, the
/// focused window's elsewhere).
fn typing_target(state: &State) -> Option<Target> {
    let page = state.page();
    if page.kind == PageKind::Queue {
        return page.filter.typing.then_some(Target::Queue);
    }
    if !page.windows_focused() {
        return None;
    }
    page.windows
        .get(page.focus)
        .filter(|w| w.filter.typing)
        .map(|_| Target::Window(page.focus))
}

/// Whether keys go to a filter now.
pub(super) fn typing(state: &State) -> bool {
    typing_target(state).is_some()
}

fn filter_mut(state: &mut State, target: Target) -> &mut Filter {
    let index = browse::top(state);
    let page = &mut state.history[index];
    match target {
        Target::Queue => &mut page.filter,
        Target::Window(w) => &mut page.windows[w].filter,
    }
}

/// `Search` (`/`): starts typing into the focused window's filter (its
/// text kept); nothing where the filter does not act.
pub(super) fn start(state: &mut State) -> Vec<Effect> {
    if let Some(target) = target(state) {
        filter_mut(state, target).typing = true;
    }
    Vec::new()
}

/// `ClosePopup` (`Esc`) on a page: clears the focused window's filter,
/// and nothing else.
pub(super) fn clear(state: &mut State) -> Vec<Effect> {
    if let Some(target) = target(state) {
        filter_mut(state, target).typing = false;
        set_text(state, target, String::new());
    }
    Vec::new()
}

/// Pasted text, while a filter is being typed.
pub(super) fn paste(state: &mut State, text: &str) -> Vec<Effect> {
    let Some(target) = typing_target(state) else {
        return Vec::new();
    };
    let mut filter = filter_mut(state, target).clone();
    filter.append(text.chars());
    set_text(state, target, filter.text)
}

/// A key while a filter is being typed (spec 0012 "Typing"): it edits the
/// filter or moves over the matches; `C-c` quits; nothing else acts.
pub(super) fn key(state: &mut State, key: Key) -> Vec<Effect> {
    let Some(target) = typing_target(state) else {
        return Vec::new();
    };
    let mut text = filter_mut(state, target).text.clone();
    match key {
        Key::Char(c) => {
            let mut filter = Filter { text, typing: true };
            filter.append([c]);
            text = filter.text;
        }
        Key::Backspace => {
            text.pop();
        }
        Key::Ctrl('u') => text.clear(),
        Key::Enter => {
            let filter = filter_mut(state, target);
            filter.typing = false;
            // An all-space filter is no filter: nothing is kept.
            if !filter.active() {
                filter.text.clear();
            }
            return Vec::new();
        }
        Key::Esc => {
            filter_mut(state, target).typing = false;
            text.clear();
        }
        Key::Up | Key::Down | Key::PageUp | Key::PageDown => {
            let command = match key {
                Key::Up => UiCommand::SelectPreviousOrScrollUp,
                Key::Down => UiCommand::SelectNextOrScrollDown,
                Key::PageUp => UiCommand::PageSelectPreviousOrScrollUp,
                _ => UiCommand::PageSelectNextOrScrollDown,
            };
            let height = state.list_height;
            return match target {
                Target::Queue => {
                    move_queue_cursor(state, |i, len| step(command, i, len, height));
                    Vec::new()
                }
                Target::Window(_) => {
                    browse::move_window_cursor(state, |i, len| step(command, i, len, height))
                }
            };
        }
        Key::Ctrl('c') => return vec![Effect::Quit],
        _ => return Vec::new(),
    }
    set_text(state, target, text)
}

/// Sets the filter's text: the shown rows follow, the cursor stays on its
/// row when it still matches (else the first match), and a list that
/// loads as you scroll asks for more when too few rows match.
fn set_text(state: &mut State, target: Target, text: String) -> Vec<Effect> {
    match target {
        Target::Queue => {
            filter_mut(state, target).text = text;
            keep_queue_cursor(state);
            Vec::new()
        }
        Target::Window(w) => {
            let index = browse::top(state);
            let window = &mut state.history[index].windows[w];
            window.set_filter(text);
            let mut effects = Vec::new();
            if window.filter.active() && !matches!(window.load, Load::Failed(_)) {
                browse::near_end(state, index, w, &mut effects);
            }
            effects
        }
    }
}

/// After a list page arrived in window `w` of page `p`: a filtered window
/// asks for more while too few rows match (spec 0012 "Lists that load as
/// you scroll").
pub(super) fn arrived(state: &mut State, p: usize, w: usize, effects: &mut Vec<Effect>) {
    if state.history[p].windows[w].filter.active() {
        browse::near_end(state, p, w, effects);
    }
}

#[cfg(test)]
mod tests;
