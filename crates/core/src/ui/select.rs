//! Selecting several rows (spec 0020): each list window's selection (and
//! the queue's), the `s …` commands, motion mode, the selection actions
//! popup and `d s`. Pure model.
//!
//! A window's selection lives on its [`Window`] (so a reloaded list loses
//! it with its rows), the queue's on the queue page at the bottom of the
//! history (beside its filter). A row is held by its index in the loaded
//! list, a queue entry by its entry ID. Writes sent for a selection are
//! remembered as a [`Batch`] until every one is answered: then the
//! selection loses the rows that succeeded (all of them: it clears), and
//! the message counts them.

use std::collections::BTreeSet;

use crate::item::Item;
use crate::library::{FavoriteKind, LibraryRequest};
use crate::protocol::{Command, InsertAt};
use crate::track::{EntryId, Track};

use super::browse::{self, Write};
use super::keymap::UiCommand;
use super::page::{Load, Page, PageKind, Row, Window, WindowKind, group};
use super::popup::{
    Popup, Removal, RemoveFrom, Removed, RemovedTarget, SelectionAction, SelectionPart,
    SelectionRemoval, TrackSource, selection_question,
};
use super::{Connection, Effect, State, filter};

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

    /// Selects `row`, or unselects it.
    fn toggle(&mut self, row: K) {
        if !self.rows.remove(&row) {
            self.rows.insert(row);
        }
    }

    /// Ends motion mode, adding `range` to the rows.
    fn end_motion(&mut self, range: impl IntoIterator<Item = K>) {
        self.rows.extend(range);
        self.anchor = None;
    }

    /// Keeps the rows (and the anchor) `keep` accepts.
    fn retain(&mut self, keep: impl Fn(&K) -> bool) {
        self.rows.retain(|row| keep(row));
        if self.anchor.as_ref().is_some_and(|a| !keep(a)) {
            self.anchor = None;
        }
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
    /// 1 hidden`), `motion`; none for an empty selection out of motion
    /// mode.
    pub fn title_parts(&self) -> Vec<String> {
        let mut parts = Vec::new();
        if self.count > 0 {
            let count = group(u32::try_from(self.count).unwrap_or(u32::MAX));
            parts.push(if self.hidden > 0 {
                let hidden = group(u32::try_from(self.hidden).unwrap_or(u32::MAX));
                format!("{count} selected, {hidden} hidden")
            } else {
                format!("{count} selected")
            });
        }
        if self.motion {
            parts.push("motion".to_owned());
        }
        parts
    }
}

// --- reading a selection ---------------------------------------------------------

/// The motion range of `window`: the rows shown between the anchor and
/// the cursor's row, both included (with a filter, the visible ones).
fn window_range(window: &Window) -> Vec<usize> {
    let (Some(anchor), Some(cursor)) = (window.selection.anchor(), window.index(window.cursor))
    else {
        return Vec::new();
    };
    let (low, high) = (anchor.min(cursor), anchor.max(cursor));
    window
        .visible()
        .into_iter()
        .filter(|i| (low..=high).contains(i))
        .collect()
}

/// The rows (indices into `window.rows`) shown as selected: the selection
/// and the motion range.
pub fn window_rows(window: &Window) -> BTreeSet<usize> {
    let mut rows = window.selection.rows().clone();
    rows.extend(window_range(window));
    rows
}

/// The marks of `window`'s selection.
pub fn window_marks(window: &Window) -> Marks {
    let rows = window_rows(window);
    let visible: BTreeSet<usize> = window.visible().into_iter().collect();
    Marks {
        count: rows.len(),
        hidden: rows.difference(&visible).count(),
        motion: window.selection.motion(),
    }
}

fn queue_selection(state: &State) -> &Selection<EntryId> {
    &state.history[0].selection
}

/// The queue's motion range: the shown entries between the anchor and the
/// cursor's, in the queue's order.
fn queue_range(state: &State) -> Vec<EntryId> {
    let queue = state.queue();
    let position = |id: EntryId| queue.iter().position(|e| e.id == id);
    let (Some(anchor), Some(cursor)) = (
        queue_selection(state).anchor().and_then(position),
        filter::queue_selected(state).and_then(position),
    ) else {
        return Vec::new();
    };
    let (low, high) = (anchor.min(cursor), anchor.max(cursor));
    filter::queue_shown(state)
        .into_iter()
        .filter(|i| (low..=high).contains(i))
        .map(|i| queue[i].id)
        .collect()
}

/// The queue entries shown as selected, in the queue's order.
pub fn queue_entries(state: &State) -> Vec<EntryId> {
    let mut selected = queue_selection(state).rows().clone();
    selected.extend(queue_range(state));
    state
        .queue()
        .iter()
        .map(|e| e.id)
        .filter(|id| selected.contains(id))
        .collect()
}

/// The marks of the queue's selection.
pub fn queue_marks(state: &State) -> Marks {
    let entries = queue_entries(state);
    let queue = state.queue();
    let shown: BTreeSet<EntryId> = filter::queue_shown(state)
        .into_iter()
        .map(|i| queue[i].id)
        .collect();
    Marks {
        count: entries.len(),
        hidden: entries.iter().filter(|id| !shown.contains(id)).count(),
        motion: queue_selection(state).motion(),
    }
}

/// The marks of the selection the keys act on (the queue's on the queue
/// page, the focused window's elsewhere), if they act anywhere.
pub(super) fn focused_marks(state: &State) -> Option<Marks> {
    match origin(state)? {
        Origin::Queue => Some(queue_marks(state)),
        Origin::Window { page, window } => Some(window_marks(&state.history[page].windows[window])),
    }
}

// --- where the keys act ------------------------------------------------------------

/// The selection the keys act on: the queue's on the queue page, the
/// focused window's on a loaded page; none on a search's input or top
/// hit, or a page loading or failed.
fn origin(state: &State) -> Option<Origin> {
    let page = state.page();
    if page.kind == PageKind::Queue {
        return Some(Origin::Queue);
    }
    if page.load != Load::Idle || !page.windows_focused() || page.windows.get(page.focus).is_none()
    {
        return None;
    }
    Some(Origin::Window {
        page: state.history.len() - 1,
        window: page.focus,
    })
}

/// The window at `origin`, if it is a window that still exists.
fn window_at(state: &State, origin: Origin) -> Option<&Window> {
    match origin {
        Origin::Queue => None,
        Origin::Window { page, window } => state.history.get(page)?.windows.get(window),
    }
}

fn window_at_mut(state: &mut State, origin: Origin) -> Option<&mut Window> {
    match origin {
        Origin::Queue => None,
        Origin::Window { page, window } => state.history.get_mut(page)?.windows.get_mut(window),
    }
}

/// Clears the selection at `origin` (motion mode included).
fn clear(state: &mut State, origin: Origin) {
    match origin {
        Origin::Queue => state.history[0].selection = Selection::default(),
        Origin::Window { .. } => {
            if let Some(window) = window_at_mut(state, origin) {
                window.selection = Selection::default();
            }
        }
    }
}

/// Unselects `held` at `origin`.
fn unselect(state: &mut State, origin: Origin, held: &[Held]) {
    match origin {
        Origin::Queue => state.history[0]
            .selection
            .retain(|id| !held.contains(&Held::Entry(*id))),
        Origin::Window { .. } => {
            if let Some(window) = window_at_mut(state, origin) {
                window
                    .selection
                    .retain(|row| !held.contains(&Held::Row(*row)));
            }
        }
    }
}

/// Ends motion mode at `origin`, adding its range to the selection.
fn end_motion(state: &mut State, origin: Origin) {
    match origin {
        Origin::Queue => {
            let range = queue_range(state);
            state.history[0].selection.end_motion(range);
        }
        Origin::Window { .. } => {
            if let Some(window) = window_at_mut(state, origin) {
                let range = window_range(window);
                window.selection.end_motion(range);
            }
        }
    }
}

/// `Esc` on a page: clears the selection the keys act on.
pub(super) fn clear_focused(state: &mut State) {
    if let Some(origin) = origin(state) {
        clear(state, origin);
    }
}

/// Before the page on top changes (spec 0020: any page change): every
/// selection of the page left is cleared, the queue's when it is the
/// queue page.
pub(super) fn leave_page(state: &mut State) {
    let Some(page) = state.history.last_mut() else {
        return;
    };
    let queue = page.kind == PageKind::Queue;
    for window in &mut page.windows {
        window.selection = Selection::default();
    }
    if queue {
        state.history[0].selection = Selection::default();
    }
}

/// Before the focus leaves the focused window: its motion range is added
/// to its selection, as `s m` does.
pub(super) fn leave_focus(state: &mut State) {
    let page = state.page();
    if page.kind == PageKind::Queue || !page.windows_focused() || page.windows.is_empty() {
        return;
    }
    let origin = Origin::Window {
        page: state.history.len() - 1,
        window: page.focus,
    };
    end_motion(state, origin);
}

/// After a snapshot: the entries that left the queue leave its selection
/// (the anchor too, which ends motion mode).
pub(super) fn prune_queue(state: &mut State) {
    let ids: BTreeSet<EntryId> = state.queue().iter().map(|e| e.id).collect();
    state.history[0].selection.retain(|id| ids.contains(id));
}

// --- the commands ------------------------------------------------------------------

/// One of the selection commands on a page: `count` is the count typed in
/// a `SelectDown`/`SelectUp` sequence (1 without one).
pub(super) fn command(state: &mut State, command: UiCommand, count: usize) -> Vec<Effect> {
    let Some(origin) = origin(state) else {
        return Vec::new();
    };
    match command {
        UiCommand::SelectCurrent => toggle_current(state, origin),
        UiCommand::SelectDown => return select_rows(state, origin, true, count.max(1)),
        UiCommand::SelectUp => return select_rows(state, origin, false, count.max(1)),
        UiCommand::SelectMotion => motion(state, origin),
        UiCommand::SelectionActions => open_actions(state, origin),
        UiCommand::RemoveSelection => return remove_selection(state, origin),
        _ => {}
    }
    Vec::new()
}

/// `s c`: the cursor's row in or out; nothing in motion mode (the cursor
/// is the range's moving end) or with no row under the cursor.
fn toggle_current(state: &mut State, origin: Origin) {
    match origin {
        Origin::Queue => {
            if queue_selection(state).motion() {
                return;
            }
            if let Some(id) = filter::queue_selected(state) {
                state.history[0].selection.toggle(id);
            }
        }
        Origin::Window { .. } => {
            if let Some(window) = window_at_mut(state, origin)
                && !window.selection.motion()
                && let Some(row) = window.index(window.cursor)
            {
                window.selection.toggle(row);
            }
        }
    }
}

/// The shown positions `count` rows from `at` (of `len`), downwards or
/// upwards, the cursor's included; and where the cursor ends.
fn span(
    at: usize,
    len: usize,
    down: bool,
    count: usize,
) -> (std::ops::RangeInclusive<usize>, usize) {
    if down {
        let end = at.saturating_add(count - 1).min(len - 1);
        (at..=end, end)
    } else {
        let end = at.saturating_sub(count - 1);
        (end..=at, end)
    }
}

/// `s <N> j` / `s <N> k`: selects `count` shown rows from the cursor's,
/// the cursor ending on the last; the move loads as any move does.
fn select_rows(state: &mut State, origin: Origin, down: bool, count: usize) -> Vec<Effect> {
    match origin {
        Origin::Queue => {
            let shown = filter::queue_shown(state);
            if shown.is_empty() {
                return Vec::new();
            }
            let queue = state.queue();
            let at = state
                .cursor
                .and_then(|id| shown.iter().position(|&i| queue[i].id == id))
                .unwrap_or(0);
            let (rows, end) = span(at, shown.len(), down, count);
            let ids: Vec<EntryId> = rows.map(|p| queue[shown[p]].id).collect();
            state.history[0].selection.rows.extend(ids);
            filter::move_queue_cursor(state, |_, _| end);
            Vec::new()
        }
        Origin::Window { .. } => {
            let Some(window) = window_at_mut(state, origin) else {
                return Vec::new();
            };
            let len = window.len();
            if len == 0 {
                return Vec::new();
            }
            let (rows, end) = span(window.cursor.min(len - 1), len, down, count);
            let picked: Vec<usize> = rows.filter_map(|p| window.index(p)).collect();
            window.selection.rows.extend(picked);
            browse::move_window_cursor(state, |_, _| end)
        }
    }
}

/// `s m`: motion mode on at the cursor's row, or off (its range added).
fn motion(state: &mut State, origin: Origin) {
    let on = match origin {
        Origin::Queue => queue_selection(state).motion(),
        Origin::Window { .. } => window_at(state, origin).is_some_and(|w| w.selection.motion()),
    };
    if on {
        end_motion(state, origin);
        return;
    }
    match origin {
        Origin::Queue => {
            if let Some(id) = filter::queue_selected(state) {
                state.history[0].selection.anchor = Some(id);
            }
        }
        Origin::Window { .. } => {
            if let Some(window) = window_at_mut(state, origin)
                && let Some(row) = window.index(window.cursor)
            {
                window.selection.anchor = Some(row);
            }
        }
    }
}

// --- the selected rows -------------------------------------------------------------

/// The selection at `origin` (motion mode ended first by the callers):
/// each row held, in the list's order.
fn held(state: &State, origin: Origin) -> Vec<Held> {
    match origin {
        Origin::Queue => queue_entries(state).into_iter().map(Held::Entry).collect(),
        Origin::Window { .. } => window_at(state, origin)
            .map(|w| w.selection.rows().iter().copied().map(Held::Row).collect())
            .unwrap_or_default(),
    }
}

/// A selected row: what it is as a list row, for the actions.
enum Picked<'a> {
    Entry(EntryId, &'a Track),
    Row(usize, Row<'a>),
}

impl<'a> Picked<'a> {
    fn held(&self) -> Held {
        match self {
            Self::Entry(id, _) => Held::Entry(*id),
            Self::Row(i, _) => Held::Row(*i),
        }
    }

    fn row(&self) -> Row<'a> {
        match self {
            Self::Entry(_, track) => Row::Track(track),
            Self::Row(_, row) => *row,
        }
    }

    /// The row's title, as the list shows it.
    fn title(&self) -> String {
        match self.row() {
            Row::Track(t) => t.title.clone(),
            Row::Credit(c) => c.track.title.clone(),
            Row::Album(a) => a.title.clone(),
            Row::Playlist(p) => p.title.clone(),
            Row::Artist(a) => a.name.clone(),
            Row::Mix(m) => m.title.clone(),
        }
    }
}

/// The selected rows at `origin`, in the list's order (hidden ones
/// included), with the page and window they are in.
fn picked(state: &State, origin: Origin) -> (Vec<Picked<'_>>, Option<(&Page, &Window)>) {
    match origin {
        Origin::Queue => {
            let selected: BTreeSet<EntryId> = queue_entries(state).into_iter().collect();
            let rows = state
                .queue()
                .iter()
                .filter(|e| selected.contains(&e.id))
                .map(|e| Picked::Entry(e.id, &e.track))
                .collect();
            (rows, None)
        }
        Origin::Window { page, window } => {
            let Some(page) = state.history.get(page) else {
                return (Vec::new(), None);
            };
            let Some(window) = page.windows.get(window) else {
                return (Vec::new(), None);
            };
            let rows = window
                .selection
                .rows()
                .iter()
                .filter_map(|&i| Some(Picked::Row(i, window.rows.get(i)?)))
                .collect();
            (rows, Some((page, window)))
        }
    }
}

/// How a row stands with the favorites: its kind and ID, and whether the
/// list it is in knows it is one (its window lists favorites); `None` for
/// what cannot be a favorite (a mix, an own playlist).
fn favorite(row: Row<'_>, at: Option<(&Page, &Window)>) -> Option<(FavoriteKind, String, bool)> {
    let kind = at.map(|(_, w)| w.kind);
    let in_search = at.is_some_and(|(p, _)| p.search.is_some());
    Some(match row {
        Row::Track(t) => (
            FavoriteKind::Track,
            t.id.to_string(),
            kind == Some(WindowKind::FavoriteTracks),
        ),
        Row::Credit(c) => (FavoriteKind::Track, c.track.id.to_string(), false),
        Row::Album(a) => (
            FavoriteKind::Album,
            a.id.to_string(),
            kind == Some(WindowKind::Albums),
        ),
        Row::Artist(a) => (
            FavoriteKind::Artist,
            a.id.to_string(),
            kind == Some(WindowKind::Artists),
        ),
        // A search playlist is never the user's own (spec 0007).
        Row::Playlist(p) if p.own && !in_search => return None,
        Row::Playlist(p) => (
            FavoriteKind::Playlist,
            p.uuid.clone(),
            kind == Some(WindowKind::Playlists),
        ),
        Row::Mix(_) => return None,
    })
}

/// The actions on the selection at `origin` (spec 0020 "The selection
/// actions popup"), in the table's order.
fn actions(state: &State, origin: Origin) -> Vec<SelectionAction> {
    let (rows, at) = picked(state, origin);
    if rows.is_empty() {
        return Vec::new();
    }
    let mut actions = Vec::new();
    let items: Option<Vec<Item>> = rows.iter().map(|r| r.row().item()).collect();
    if let Some(items) = items {
        actions.extend([
            SelectionAction::Play(items.clone()),
            SelectionAction::PlayNext(items.clone()),
            SelectionAction::AddToQueue(items),
        ]);
    }
    let tracks: Option<Vec<Track>> = rows.iter().map(|r| r.row().track().cloned()).collect();
    if let Some(tracks) = tracks {
        actions.push(SelectionAction::AddToPlaylist(tracks));
    }
    let favorites: Option<Vec<(FavoriteKind, String, bool, Held)>> = rows
        .iter()
        .map(|r| favorite(r.row(), at).map(|(kind, id, known)| (kind, id, known, r.held())))
        .collect();
    if let Some(favorites) = &favorites {
        let unknown: Vec<(Held, String)> = favorites
            .iter()
            .filter(|(_, _, known, _)| !known)
            .map(|(_, id, _, held)| (*held, id.clone()))
            .collect();
        if let (Some((kind, ..)), false) = (favorites.first(), unknown.is_empty()) {
            actions.push(SelectionAction::AddFavorites(*kind, unknown));
        }
    }
    match at {
        None => actions.push(SelectionAction::RemoveFromQueue),
        Some((page, window)) if browse::removable(page, window.kind) => match window.kind {
            WindowKind::PlaylistTracks => actions.push(SelectionAction::RemoveFromPlaylist),
            // In the library's Playlists only followed ones leave the
            // favorites; `d s` deletes own ones, which is not that.
            WindowKind::Playlists
                if favorites
                    .as_ref()
                    .is_none_or(|f| f.iter().any(|(_, _, known, _)| !known)) => {}
            _ => actions.push(SelectionAction::RemoveFavorites),
        },
        Some(_) => {}
    }
    actions
}

/// `s a`: the selection actions popup, motion mode ended first; nothing
/// with nothing selected (or nothing that applies).
fn open_actions(state: &mut State, origin: Origin) {
    end_motion(state, origin);
    let count = held(state, origin).len();
    let actions = actions(state, origin);
    if count == 0 || actions.is_empty() {
        return;
    }
    state.popup = Some(Popup::SelectionActions {
        title: format!(
            "{} selected",
            group(u32::try_from(count).unwrap_or(u32::MAX))
        ),
        actions,
        cursor: 0,
        origin,
    });
}

fn connected(state: &State) -> bool {
    state.connection == Connection::Connected
}

/// Runs a selection action (the popup closed by the caller) on the
/// selection at `origin`.
pub(super) fn run(state: &mut State, origin: Origin, action: SelectionAction) -> Vec<Effect> {
    let open = |state: &mut State, items: Vec<Item>, at: Option<InsertAt>| {
        // Sent only while connected (spec 0005); what is not sent does
        // not succeed, so the selection stays.
        if !connected(state) {
            return Vec::new();
        }
        clear(state, origin);
        vec![Effect::Send(Command::Open { items, at })]
    };
    match action {
        SelectionAction::Play(items) => open(state, items, None),
        SelectionAction::PlayNext(items) => open(state, items, Some(InsertAt::Next)),
        SelectionAction::AddToQueue(items) => open(state, items, Some(InsertAt::End)),
        SelectionAction::AddToPlaylist(tracks) => {
            let effects = browse::open_picker(state, TrackSource::Tracks(tracks));
            state.selection_add = Some((origin, held(state, origin)));
            effects
        }
        SelectionAction::AddFavorites(kind, rows) => {
            let mut effects = Vec::new();
            let mut requests = Vec::new();
            for (held, id) in rows {
                let (sent, request) = browse::write_id(
                    state,
                    LibraryRequest::AddFavorite(kind, id),
                    Write::Favorite { add: true, kind },
                );
                effects.extend(sent);
                if let Some(request) = request {
                    requests.push(Request::new(request, vec![held], Part::AddFavorite(kind)));
                }
            }
            remember(state, origin, requests);
            effects
        }
        SelectionAction::RemoveFavorites
        | SelectionAction::RemoveFromPlaylist
        | SelectionAction::RemoveFromQueue => remove_selection(state, origin),
    }
}

// --- `d s` -------------------------------------------------------------------------

/// What `d s` removes at `origin`: one [`SelectionPart`] per list, in the
/// list's order; none where `d d` does nothing or nothing is selected.
fn removal(state: &State, origin: Origin) -> Option<SelectionRemoval> {
    let (rows, at) = picked(state, origin);
    if rows.is_empty() {
        return None;
    }
    let item = |r: &Picked<'_>, target| Removed {
        title: r.title(),
        target,
    };
    let part = |from, items: Vec<(Held, Removed)>| {
        let (held, items) = items.into_iter().unzip();
        SelectionPart {
            removal: Removal { from, items },
            held,
        }
    };
    let favorites = |kind| {
        let items = rows
            .iter()
            .filter_map(|r| {
                let (_, id, _) = favorite(r.row(), at)?;
                Some((r.held(), item(r, RemovedTarget::Favorite(id))))
            })
            .collect();
        part(RemoveFrom::Favorites(kind), items)
    };
    let parts = match at {
        None => {
            let items = rows
                .iter()
                .filter_map(|r| match r {
                    Picked::Entry(id, _) => Some((r.held(), item(r, RemovedTarget::Entry(*id)))),
                    Picked::Row(..) => None,
                })
                .collect();
            vec![part(RemoveFrom::Queue, items)]
        }
        Some((page, window)) if browse::removable(page, window.kind) => match window.kind {
            WindowKind::PlaylistTracks => {
                let playlist = page.own_playlist()?;
                let items = rows
                    .iter()
                    .filter_map(|r| match r {
                        Picked::Row(i, _) => {
                            let position = *window.positions.get(*i)?;
                            Some((r.held(), item(r, RemovedTarget::Position(position))))
                        }
                        Picked::Entry(..) => None,
                    })
                    .collect();
                vec![part(
                    RemoveFrom::Playlist {
                        uuid: playlist.uuid.clone(),
                        title: playlist.title.clone(),
                        etag: page.etag().map(Into::into),
                    },
                    items,
                )]
            }
            WindowKind::FavoriteTracks => vec![favorites(FavoriteKind::Track)],
            WindowKind::Albums => vec![favorites(FavoriteKind::Album)],
            WindowKind::Artists => vec![favorites(FavoriteKind::Artist)],
            WindowKind::Playlists => {
                let own = rows
                    .iter()
                    .filter_map(|r| match r.row() {
                        Row::Playlist(p) if p.own => {
                            Some((r.held(), item(r, RemovedTarget::Playlist(p.uuid.clone()))))
                        }
                        _ => None,
                    })
                    .collect();
                vec![
                    part(RemoveFrom::OwnPlaylists, own),
                    favorites(FavoriteKind::Playlist),
                ]
            }
            _ => return None,
        },
        Some(_) => return None,
    };
    let parts: Vec<SelectionPart> = parts
        .into_iter()
        .filter(|p| !p.removal.items.is_empty())
        .collect();
    (!parts.is_empty()).then_some(SelectionRemoval { origin, parts })
}

/// `d s`: the question for the selection at `origin` (motion mode ended
/// first), or the removal at once with `confirm_removals` off; deleting
/// own playlists asks whatever it says (spec 0019).
fn remove_selection(state: &mut State, origin: Origin) -> Vec<Effect> {
    end_motion(state, origin);
    let Some(removal) = removal(state, origin) else {
        return Vec::new();
    };
    let deletes = removal
        .parts
        .iter()
        .any(|p| p.removal.from == RemoveFrom::OwnPlaylists);
    if !state.confirm_removals && !deletes {
        return remove(state, removal);
    }
    state.popup = Some(Popup::Confirm {
        question: selection_question(&removal),
        on_yes: super::Confirmed::RemoveSelection(removal),
        scroll: 0,
    });
    Vec::new()
}

/// What `y` to `d s`'s question sends (spec 0020 "What `y` sends"): one
/// command per queue entry, one write per favorite and per own playlist,
/// one write for every position of a playlist.
pub(super) fn remove(state: &mut State, removal: SelectionRemoval) -> Vec<Effect> {
    let SelectionRemoval { origin, parts } = removal;
    let mut effects = Vec::new();
    let mut requests = Vec::new();
    for SelectionPart { removal, held } in parts {
        let Removal { from, items } = removal;
        match from {
            RemoveFrom::Queue => {
                if !connected(state) {
                    continue;
                }
                let count = items.len();
                effects.extend(items.into_iter().filter_map(|item| match item.target {
                    RemovedTarget::Entry(entry) => {
                        Some(Effect::Send(Command::RemoveFromQueue(entry)))
                    }
                    _ => None,
                }));
                clear(state, origin);
                if count > 1 {
                    state.message = Some(format!("Removed {count} tracks from the queue"));
                }
            }
            RemoveFrom::Favorites(kind) => {
                for (item, held) in items.into_iter().zip(held) {
                    let RemovedTarget::Favorite(id) = item.target else {
                        continue;
                    };
                    let (sent, request) = browse::write_id(
                        state,
                        LibraryRequest::RemoveFavorite(kind, id),
                        Write::Favorite { add: false, kind },
                    );
                    effects.extend(sent);
                    if let Some(request) = request {
                        requests.push(Request::new(request, vec![held], Part::Favorite(kind)));
                    }
                }
            }
            RemoveFrom::Playlist { uuid, title, etag } => {
                let indices: Vec<u32> = items
                    .iter()
                    .filter_map(|item| match item.target {
                        RemovedTarget::Position(i) => Some(i),
                        _ => None,
                    })
                    .collect();
                let (sent, request) = browse::write_id(
                    state,
                    LibraryRequest::RemoveFromPlaylist {
                        uuid: uuid.clone(),
                        indices,
                        etag: etag.unwrap_or_default(),
                    },
                    Write::RemoveFromPlaylist {
                        uuid,
                        title: title.clone(),
                    },
                );
                effects.extend(sent);
                if let Some(request) = request {
                    requests.push(Request::new(request, held, Part::Playlist(title)));
                }
            }
            RemoveFrom::OwnPlaylists => {
                for (item, held) in items.into_iter().zip(held) {
                    let RemovedTarget::Playlist(uuid) = item.target else {
                        continue;
                    };
                    let (sent, request) = browse::write_id(
                        state,
                        LibraryRequest::DeletePlaylist { uuid: uuid.clone() },
                        Write::DeletePlaylist {
                            uuid,
                            title: item.title,
                        },
                    );
                    effects.extend(sent);
                    if let Some(request) = request {
                        requests.push(Request::new(request, vec![held], Part::Delete));
                    }
                }
            }
        }
    }
    remember(state, origin, requests);
    effects
}

// --- writes for a selection ----------------------------------------------------------

/// The library writes sent for one selection, until all are answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Batch {
    origin: Origin,
    requests: Vec<Request>,
}

/// One write of a [`Batch`]: the rows it is for, and its answer (`None`
/// while waiting; `Some(true)` when it succeeded).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Request {
    id: u64,
    held: Vec<Held>,
    part: Part,
    ok: Option<bool>,
}

impl Request {
    fn new(id: u64, held: Vec<Held>, part: Part) -> Self {
        Self {
            id,
            held,
            part,
            ok: None,
        }
    }
}

/// What a write of a batch does, for its message.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Part {
    Favorite(FavoriteKind),
    AddFavorite(FavoriteKind),
    Playlist(String),
    Delete,
    AddToPlaylist,
}

/// Remembers the writes sent for the selection at `origin`.
fn remember(state: &mut State, origin: Origin, requests: Vec<Request>) {
    if !requests.is_empty() {
        state.batches.push(Batch { origin, requests });
    }
}

/// After *Add to playlist…* of the selection sent its write `id`: the
/// selection clears once it succeeds.
pub(super) fn adding(state: &mut State, id: Option<u64>) {
    let Some((origin, held)) = state.selection_add.take() else {
        return;
    };
    if let Some(id) = id {
        remember(
            state,
            origin,
            vec![Request::new(id, held, Part::AddToPlaylist)],
        );
    }
}

/// The answer to write `id` (`error`: why it failed), after its own
/// message was set (`before`: the message before it). Once a batch is
/// answered whole, its selection loses the rows that succeeded (all of
/// them: it clears) and the message counts them; until then, the message
/// of one of several writes is not shown.
pub(super) fn answered(state: &mut State, id: u64, error: Option<String>, before: Option<String>) {
    let Some(at) = state
        .batches
        .iter()
        .position(|b| b.requests.iter().any(|r| r.id == id))
    else {
        return;
    };
    let batch = &mut state.batches[at];
    for request in batch.requests.iter_mut().filter(|r| r.id == id) {
        request.ok = Some(error.is_none());
    }
    if batch.requests.iter().any(|r| r.ok.is_none()) {
        if batch.requests.len() > 1 {
            state.message = before;
        }
        return;
    }
    let batch = state.batches.remove(at);
    if batch.requests.iter().all(|r| r.ok == Some(true)) {
        clear(state, batch.origin);
    } else {
        let done: Vec<Held> = batch
            .requests
            .iter()
            .filter(|r| r.ok == Some(true))
            .flat_map(|r| r.held.iter().copied())
            .collect();
        unselect(state, batch.origin, &done);
    }
    if let Some(message) = message(&batch.requests) {
        state.message = Some(message);
    }
}

/// `1 track`, `3 albums`.
fn things(n: usize, kind: FavoriteKind) -> String {
    let thing = match kind {
        FavoriteKind::Track => "track",
        FavoriteKind::Album => "album",
        FavoriteKind::Artist => "artist",
        FavoriteKind::Playlist => "playlist",
    };
    let plural = if n == 1 { "" } else { "s" };
    format!("{n} {thing}{plural}")
}

/// The message of an answered batch (spec 0020: `Removed 3 tracks from
/// Road trip`, `Removed 2 albums from favorites; 1 failed`); `None` keeps
/// the last write's own: when it was one write for one row (0019's
/// messages), an *Add to playlist…*, or when every write failed.
fn message(requests: &[Request]) -> Option<String> {
    let rows = |r: &Request| r.held.len();
    let ok: Vec<&Request> = requests.iter().filter(|r| r.ok == Some(true)).collect();
    let failed: usize = requests
        .iter()
        .filter(|r| r.ok != Some(true))
        .map(rows)
        .sum();
    let single = matches!(requests, [one] if rows(one) == 1);
    if ok.is_empty() || single || requests.iter().any(|r| r.part == Part::AddToPlaylist) {
        return None;
    }
    let count = |wanted: fn(&Part) -> bool| -> usize {
        ok.iter().filter(|r| wanted(&r.part)).map(|r| rows(r)).sum()
    };
    let mut clauses = Vec::new();
    if let Some(Part::Playlist(title)) = ok.first().map(|r| &r.part) {
        let n = count(|p| matches!(p, Part::Playlist(_)));
        clauses.push(format!(
            "Removed {} from {title}",
            things(n, FavoriteKind::Track)
        ));
    }
    let deleted = count(|p| *p == Part::Delete);
    if deleted > 0 {
        clauses.push(format!(
            "Deleted {}",
            things(deleted, FavoriteKind::Playlist)
        ));
    }
    let kind_of = |add: bool| {
        ok.iter().find_map(|r| match r.part {
            Part::Favorite(kind) if !add => Some(kind),
            Part::AddFavorite(kind) if add => Some(kind),
            _ => None,
        })
    };
    if let Some(kind) = kind_of(false) {
        let n = count(|p| matches!(p, Part::Favorite(_)));
        clauses.push(if deleted > 0 {
            format!("removed {n} from favorites")
        } else {
            format!("Removed {} from favorites", things(n, kind))
        });
    }
    if let Some(kind) = kind_of(true) {
        let n = count(|p| matches!(p, Part::AddFavorite(_)));
        clauses.push(format!("Added {} to favorites", things(n, kind)));
    }
    let mut message = clauses.join(" and ");
    if failed > 0 {
        message.push_str(&format!("; {failed} failed"));
    }
    Some(message)
}

#[cfg(test)]
mod tests;
