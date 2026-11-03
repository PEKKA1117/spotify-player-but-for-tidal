//! The lyrics page (spec 0021): one page of the history that follows the
//! player's current track. Pure model: which track the page shows, its
//! lyrics, the current line and the scroll state; the rendering reads
//! [`LyricsView`] and [`Page::lyrics_content`].
//!
//! Scrolling is counted in **lines** (a line of the lyrics, wrapped or
//! not): the model does not know the window's width, so a renderer that
//! wraps long lines turns [`LyricsView::offset`] (a line index) into the
//! first row to draw. [`State::list_height`] (set by `Resize`) is the
//! window's height; it places the "middle row" and sizes page moves.

use std::time::Duration;

use crate::lyrics::{LyricLine, Lyrics, current_line};
use crate::track::Track;

use super::dispatch::step;
use super::keymap::UiCommand;
use super::page::{Load, Page, PageKind};
use super::{Effect, State, browse};

/// What the window says while a fetch is pending.
pub const LOADING: &str = "Loading…";
/// What the window says for a track without lyrics.
pub const NO_LYRICS: &str = "No lyrics for this track";
/// What the window says with no current entry.
pub const NOTHING_PLAYING: &str = "Nothing playing";

/// What the lyrics page shows: the track it follows and its lyrics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LyricsView {
    /// The track the page shows; `None` with no current entry.
    pub track: Option<Track>,
    /// The track's lyrics once fetched; `None` while loading or when
    /// Tidal has none.
    pub lyrics: Option<Lyrics>,
    /// Whether the view follows the current line (synced lyrics). A
    /// movement key clears it, `Esc` and a new track set it.
    pub following: bool,
    /// The first line shown while not following, and for plain lyrics
    /// (a line index).
    pub scroll: usize,
}

/// What the lyrics window holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LyricsContent<'a> {
    /// No current entry: [`NOTHING_PLAYING`].
    NothingPlaying,
    /// [`LOADING`].
    Loading,
    /// The failure to show, as it reads in the page.
    Failed(&'a str),
    /// [`NO_LYRICS`]: Tidal has none, or both texts are empty.
    NoLyrics,
    /// Plain text, scrolled with the movement keys.
    Plain(&'a str),
    /// Timed lines.
    Synced(&'a [LyricLine]),
}

impl LyricsContent<'_> {
    /// The message that replaces the lyrics, if this is not lyrics.
    pub fn message(&self) -> Option<String> {
        Some(match self {
            Self::NothingPlaying => NOTHING_PLAYING.to_owned(),
            Self::Loading => LOADING.to_owned(),
            Self::Failed(message) => (*message).to_owned(),
            Self::NoLyrics => NO_LYRICS.to_owned(),
            Self::Plain(_) | Self::Synced(_) => return None,
        })
    }
}

impl LyricsView {
    /// A view of `track`, following, with nothing fetched yet.
    pub fn new(track: Option<Track>) -> Self {
        Self {
            track,
            lyrics: None,
            following: true,
            scroll: 0,
        }
    }

    /// Whether the lyrics have timed lines.
    pub fn is_synced(&self) -> bool {
        self.lyrics.as_ref().is_some_and(|l| !l.synced.is_empty())
    }

    /// Whether the lyrics are drawn right-aligned.
    pub fn right_to_left(&self) -> bool {
        self.lyrics.as_ref().is_some_and(|l| l.right_to_left)
    }

    /// The number of lines scrolled over: the timed lines, else the lines
    /// of the plain text.
    pub fn line_count(&self) -> usize {
        match &self.lyrics {
            Some(l) if !l.synced.is_empty() => l.synced.len(),
            Some(l) if !l.plain.trim().is_empty() => l.plain.lines().count(),
            _ => 0,
        }
    }

    /// The current line at `position` (synced lyrics only): the last line
    /// whose time is at or before it.
    pub fn current(&self, position: Duration) -> Option<usize> {
        self.lyrics
            .as_ref()
            .and_then(|l| current_line(&l.synced, position))
    }

    /// How many lines are drawn as played: all those before the current
    /// one (none before the first line's time).
    pub fn played(&self, position: Duration) -> usize {
        self.current(position).unwrap_or(0)
    }

    /// Where a following view starts: the current line at the middle row
    /// of a window `height` rows tall once it is past the middle, the top
    /// before that.
    pub fn followed_offset(&self, position: Duration, height: usize) -> usize {
        self.current(position)
            .map_or(0, |line| line.saturating_sub(height / 2))
    }

    /// The first line to draw at `position` in a window `height` rows
    /// tall: the followed place while following synced lyrics, else the
    /// scroll.
    pub fn offset(&self, position: Duration, height: usize) -> usize {
        if self.is_synced() && self.following {
            self.followed_offset(position, height)
        } else {
            self.scroll.min(self.line_count().saturating_sub(1))
        }
    }

    /// The furthest the view scrolls by hand: the last line at the bottom
    /// of the window, or further where following puts the view.
    fn max_offset(&self, position: Duration, height: usize) -> usize {
        let last = self.line_count().saturating_sub(height.max(1));
        if self.is_synced() {
            last.max(self.followed_offset(position, height))
        } else {
            last
        }
    }

    /// The window's title: `Lyrics`, `Lyrics (synced)`, and `· scrolled`
    /// while synced lyrics are not followed.
    pub fn window_title(&self) -> String {
        match (self.is_synced(), self.following) {
            (false, _) => "Lyrics".into(),
            (true, true) => "Lyrics (synced)".into(),
            (true, false) => "Lyrics (synced) · scrolled".into(),
        }
    }

    /// Scrolls by `command` (a list move) from where the view is; synced
    /// lyrics stop following.
    fn scroll_by(&mut self, command: UiCommand, position: Duration, height: usize) {
        if self.line_count() == 0 {
            return;
        }
        let start = self.offset(position, height);
        let max = self.max_offset(position, height);
        self.scroll = step(command, start.min(max), max + 1, height.max(1));
        if self.is_synced() {
            self.following = false;
        }
    }
}

impl Page {
    /// What the lyrics window holds; `None` on any other page.
    pub fn lyrics_content(&self) -> Option<LyricsContent<'_>> {
        let view = self.lyrics.as_ref()?;
        if view.track.is_none() {
            return Some(LyricsContent::NothingPlaying);
        }
        Some(match &self.load {
            Load::Loading { .. } => LyricsContent::Loading,
            Load::Failed(message) => LyricsContent::Failed(message),
            Load::Idle => match &view.lyrics {
                Some(l) if !l.synced.is_empty() => LyricsContent::Synced(&l.synced),
                Some(l) if !l.plain.trim().is_empty() => LyricsContent::Plain(&l.plain),
                _ => LyricsContent::NoLyrics,
            },
        })
    }
}

/// A lyrics page showing `track`, not yet fetched.
pub fn page(track: Option<Track>) -> Page {
    let mut page = Page::new(PageKind::Lyrics);
    page.lyrics = Some(LyricsView::new(track));
    page
}

/// The page on top, when it is the lyrics page.
fn top_view(state: &mut State) -> Option<&mut LyricsView> {
    state.history.last_mut()?.lyrics.as_mut()
}

/// The page on top follows the current entry (spec 0021 "The page"):
/// called when a snapshot arrives and when a page is popped. A new track
/// resets the view (following, from the top), shows `Loading…` and asks
/// once; the same track, a page not on top and a `Position` event ask
/// nothing.
pub(super) fn follow_track(state: &mut State) -> Vec<Effect> {
    let current = state.current().map(|e| e.track.clone());
    let index = browse::top(state);
    let page = &mut state.history[index];
    let Some(view) = page.lyrics.as_mut() else {
        return Vec::new();
    };
    if view.track.as_ref().map(|t| t.id) == current.as_ref().map(|t| t.id) {
        return Vec::new();
    }
    *view = LyricsView::new(current);
    page.load = Load::Idle;
    let mut effects = Vec::new();
    browse::fetch_page(state, index, &mut effects);
    effects
}

/// A page command on the lyrics page: the list moves scroll; every other
/// command that acts on rows does nothing there.
pub(super) fn command(state: &mut State, command: UiCommand) -> Vec<Effect> {
    if command.is_list_move() {
        let (position, height) = (state.position, state.list_height);
        if let Some(view) = top_view(state) {
            view.scroll_by(command, position, height);
        }
    }
    Vec::new()
}

/// `Esc` on the lyrics page: follow the current line again.
pub(super) fn follow(state: &mut State) -> Vec<Effect> {
    if let Some(view) = top_view(state) {
        view.following = true;
    }
    Vec::new()
}

#[cfg(test)]
mod tests;
