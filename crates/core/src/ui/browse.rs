//! Browsing the library (spec 0006 "Client model"): the page history,
//! window focus and cursors, scrolling loads, the whole-list load,
//! playing and queueing from a page, the popups and the library writes.

use crate::library::{FavoriteKind, LibraryRequest, LibraryResponse, ListRef, PlaylistSummary};
use crate::protocol::{Command, InsertAt};
use crate::track::Track;

use super::page::{
    Load, MAX_HISTORY, MAX_WHOLE_LIST, Page, PageKind, Row, Rows, Window, WindowKind, group,
    is_search_list, largest_page,
};
use super::popup::{self, Confirmed, MenuAction, Popup, TrackSource};
use super::{Connection, DISCONNECTED, Effect, Key, SHUT_DOWN, State};

/// The message of a removal refused because the playlist changed (`412`),
/// as the player words it (spec 0006 "Talking to the player").
pub const PLAYLIST_CHANGED: &str = "The playlist changed: nothing was changed, try again";

/// A whole-list load in progress (spec 0006 "The whole list is needed").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WholeList {
    pub source: WholeListSource,
    pub purpose: Purpose,
}

/// The list being loaded whole.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WholeListSource {
    /// A window of a page in the history.
    Window { page: usize, window: usize },
    /// A list no page shows (an album row's tracks).
    Detached(Window),
}

/// What happens once the list is whole.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Purpose {
    /// `LoadQueue` from the visible row `start`.
    Play { start: usize },
    /// `AddToPlaylist` to `uuid`, asking first when a track is in a loaded
    /// copy of it (`check_duplicates`).
    AddToPlaylist {
        uuid: String,
        title: String,
        check_duplicates: bool,
    },
}

/// A library write waiting for its reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Write {
    Favorite {
        add: bool,
        kind: FavoriteKind,
    },
    /// Then adds `tracks` to the created playlist.
    Create {
        title: String,
        tracks: TrackSource,
    },
    AddToPlaylist {
        uuid: String,
        title: String,
        count: usize,
    },
    RemoveFromPlaylist {
        uuid: String,
        title: String,
    },
    DeletePlaylist {
        uuid: String,
        title: String,
    },
}

/// Whether requests can reach the player.
fn connected(state: &State) -> bool {
    state.connection == Connection::Connected
}

/// Why nothing can be asked: the connection's message.
fn connection_message(state: &State) -> String {
    match &state.connection {
        Connection::Disconnected { shut_down: true } => SHUT_DOWN.to_owned(),
        Connection::Refused(message) => message.clone(),
        _ => DISCONNECTED.to_owned(),
    }
}

/// Emits `request` with a fresh ID; `None` (nothing emitted) while not
/// connected.
fn request(state: &mut State, request: LibraryRequest, effects: &mut Vec<Effect>) -> Option<u64> {
    if !connected(state) {
        return None;
    }
    let id = state.next_request;
    state.next_request += 1;
    effects.push(Effect::Library { id, request });
    Some(id)
}

/// The `limit` of a `More` for `list`: the page size (a search list's:
/// the search page size), at most the endpoint's largest page.
fn limit(state: &State, list: &ListRef) -> u32 {
    let size = if is_search_list(list) {
        state.search_page_size
    } else {
        state.page_size
    };
    size.clamp(1, largest_page(list))
}

pub(super) fn top(state: &State) -> usize {
    state.history.len() - 1
}

/// Asks for page `index` (a page of the history): `Loading`, or failed
/// with the connection's message while disconnected.
pub(super) fn fetch_page(state: &mut State, index: usize, effects: &mut Vec<Effect>) {
    let Some(page) = state.history[index].kind.request() else {
        return;
    };
    let load = match request(state, LibraryRequest::Page(page), effects) {
        Some(id) => Load::Loading { id },
        None => Load::Failed(connection_message(state)),
    };
    state.history[index].load = load;
}

/// Fetches again every page of the history `affected` names (after a
/// successful edit).
fn refetch(state: &mut State, affected: impl Fn(&PageKind) -> bool, effects: &mut Vec<Effect>) {
    for index in 0..state.history.len() {
        if affected(&state.history[index].kind) {
            fetch_page(state, index, effects);
        }
    }
}

/// Opens `kind`: pushed and fetched, unless it is the page on top (then
/// fetched again only if it failed).
pub(super) fn open(state: &mut State, kind: PageKind) -> Vec<Effect> {
    let mut effects = Vec::new();
    let page = state.page();
    if page.kind == kind {
        if matches!(page.load, Load::Failed(_)) {
            fetch_page(state, top(state), &mut effects);
        }
        return effects;
    }
    push(state, Page::new(kind));
    fetch_page(state, top(state), &mut effects);
    effects
}

/// Pushes `page` on the history, dropping the oldest page above the queue
/// past [`MAX_HISTORY`].
pub(super) fn push(state: &mut State, page: Page) {
    state.history.push(page);
    if state.history.len() > MAX_HISTORY + 1 {
        state.history.remove(1);
    }
}

/// `Backspace`/`C-q`: back to the page under; the queue stays.
pub(super) fn back(state: &mut State) -> Vec<Effect> {
    if state.history.len() > 1 {
        state.history.pop();
    }
    Vec::new()
}

/// `Tab`/`BackTab`: the next or previous pane, wrapping.
pub(super) fn cycle_focus(state: &mut State, forward: bool) -> Vec<Effect> {
    let index = top(state);
    state.history[index].cycle_pane(forward);
    let mut effects = Vec::new();
    first_credits(state, index, &mut effects);
    effects
}

/// `[`/`]`: the previous or next tab of the focused pane, wrapping.
pub(super) fn cycle_tab(state: &mut State, forward: bool) -> Vec<Effect> {
    let index = top(state);
    state.history[index].cycle_tab(forward);
    let mut effects = Vec::new();
    first_credits(state, index, &mut effects);
    effects
}

/// *All tracks* asks for its first page when first focused.
fn first_credits(state: &mut State, index: usize, effects: &mut Vec<Effect>) {
    let page = &state.history[index];
    if page.load != Load::Idle {
        return;
    }
    let focus = page.focus;
    if page.windows.get(focus).is_some_and(|w| {
        w.kind == WindowKind::AllTracks && w.total.is_none() && w.load == Load::Idle
    }) {
        ask_more(state, index, focus, effects);
    }
}

/// Asks for the next page of window `window` of page `page`.
fn ask_more(state: &mut State, page: usize, window: usize, effects: &mut Vec<Effect>) {
    let w = &state.history[page].windows[window];
    let more = LibraryRequest::More {
        list: w.list.clone(),
        offset: w.next_offset,
        limit: limit(state, &w.list),
    };
    if let Some(id) = request(state, more, effects) {
        state.history[page].windows[window].load = Load::Loading { id };
    }
}

/// Asks for the next page when the window's cursor is within one window
/// height of its last loaded row, nothing is pending and the list goes on.
fn near_end(state: &mut State, page: usize, window: usize, effects: &mut Vec<Effect>) {
    let w = &state.history[page].windows[window];
    if matches!(w.load, Load::Loading { .. }) || w.total.is_none() || w.complete() {
        return;
    }
    if w.cursor + state.list_height + 1 >= w.len() {
        ask_more(state, page, window, effects);
    }
}

/// Moves the focused window's cursor to `to(cursor, len)` (clamped); a
/// loading, failed or empty window ignores it.
pub(super) fn move_window_cursor(
    state: &mut State,
    to: impl Fn(usize, usize) -> usize,
) -> Vec<Effect> {
    let index = top(state);
    let page = &mut state.history[index];
    let focus = page.focus;
    if page.load != Load::Idle || !page.windows_focused() {
        return Vec::new();
    }
    let Some(window) = page.windows.get_mut(focus) else {
        return Vec::new();
    };
    let len = window.len();
    if len == 0 {
        return Vec::new();
    }
    window.cursor = to(window.cursor, len).min(len - 1);
    let mut effects = Vec::new();
    near_end(state, index, focus, &mut effects);
    effects
}

/// A key on a browse page: cursor keys, `Enter`, `Z`/`C-z`, `d` and `f`;
/// `None` for the keys the page leaves to playback.
pub(super) fn browse_key(state: &mut State, key: Key) -> Option<Vec<Effect>> {
    let height = state.list_height;
    Some(match key {
        Key::Char('j') | Key::Down => move_window_cursor(state, |i, len| (i + 1).min(len - 1)),
        Key::Char('k') | Key::Up => move_window_cursor(state, |i, _| i.saturating_sub(1)),
        Key::Char('G') => move_window_cursor(state, |_, len| len - 1),
        Key::Ctrl('f') | Key::PageDown => {
            move_window_cursor(state, |i, len| (i + height).min(len - 1))
        }
        Key::Ctrl('b') | Key::PageUp => move_window_cursor(state, |i, _| i.saturating_sub(height)),
        Key::Enter => enter(state),
        Key::Char('Z') | Key::Ctrl('z') => queue_selected(state),
        Key::Char('d') => Vec::new(),
        Key::Char('f') => {
            open_role_filter(state);
            Vec::new()
        }
        _ => return None,
    })
}

/// The row under the focus on a loaded browse page (a search's top hit
/// included).
fn selected(state: &State) -> Option<Row<'_>> {
    state.page().selected()
}

/// `Enter`: opens an album, playlist or artist; plays from a track.
fn enter(state: &mut State) -> Vec<Effect> {
    match selected(state) {
        Some(row) => match row.page() {
            Some(kind) => open(state, kind),
            None => play_from(state),
        },
        None => Vec::new(),
    }
}

fn too_many(total: usize) -> String {
    let total = u32::try_from(total).unwrap_or(u32::MAX);
    format!("Too many tracks to queue at once ({})", group(total))
}

/// `Enter` on a track: the whole list from it, loading the rest first.
fn play_from(state: &mut State) -> Vec<Effect> {
    if !connected(state) {
        return Vec::new();
    }
    let page = top(state);
    let focus = state.history[page].focus;
    let window = &state.history[page].windows[focus];
    let start = window.cursor;
    if window.complete() {
        return play(state, window.tracks(), start);
    }
    let total = window.total.unwrap_or(0);
    if total > MAX_WHOLE_LIST {
        state.message = Some(too_many(total as usize));
        return Vec::new();
    }
    let loading = matches!(window.load, Load::Loading { .. });
    state.whole_list = Some(WholeList {
        source: WholeListSource::Window {
            page,
            window: focus,
        },
        purpose: Purpose::Play { start },
    });
    let mut effects = Vec::new();
    if !loading {
        ask_more(state, page, focus, &mut effects);
    }
    progress(state);
    effects
}

/// `LoadQueue` of `tracks` from `start`, unless there are too many.
fn play(state: &mut State, tracks: Vec<Track>, start: usize) -> Vec<Effect> {
    if tracks.len() > MAX_WHOLE_LIST as usize {
        state.message = Some(too_many(tracks.len()));
        return Vec::new();
    }
    vec![Effect::Send(Command::LoadQueue { tracks, start })]
}

/// The whole-list load's window.
fn whole_list_window(state: &State) -> Option<&Window> {
    match &state.whole_list.as_ref()?.source {
        WholeListSource::Window { page, window } => state.history.get(*page)?.windows.get(*window),
        WholeListSource::Detached(window) => Some(window),
    }
}

/// `Loading 300 of 1 234…` in the message row.
fn progress(state: &mut State) {
    let Some(window) = whole_list_window(state) else {
        return;
    };
    let loaded = u32::try_from(window.rows.len()).unwrap_or(u32::MAX);
    state.message = Some(match window.total {
        Some(total) => format!("Loading {} of {}…", group(loaded), group(total)),
        None => "Loading…".to_owned(),
    });
}

/// After a page of the whole-list load: asks for the next or, when the
/// list is whole, does what it was for.
fn continue_whole_list(state: &mut State, effects: &mut Vec<Effect>) {
    let Some(window) = whole_list_window(state) else {
        return;
    };
    if matches!(window.load, Load::Loading { .. }) {
        return;
    }
    if !window.complete() {
        let asked = match state.whole_list.as_ref().map(|w| &w.source) {
            Some(WholeListSource::Window { page, window }) => {
                let (page, window) = (*page, *window);
                ask_more(state, page, window, effects);
                matches!(
                    state.history[page].windows[window].load,
                    Load::Loading { .. }
                )
            }
            _ => ask_detached(state, effects),
        };
        if asked {
            progress(state);
        } else {
            cancel_whole_list(state);
        }
        return;
    }
    let tracks = window.tracks();
    let Some(whole) = state.whole_list.take() else {
        return;
    };
    state.message = None;
    match whole.purpose {
        Purpose::Play { start } => effects.extend(play(state, tracks, start)),
        Purpose::AddToPlaylist {
            uuid,
            title,
            check_duplicates,
        } => effects.extend(add_tracks(state, uuid, title, tracks, check_duplicates)),
    }
}

/// Asks for the next page of the detached whole-list window.
fn ask_detached(state: &mut State, effects: &mut Vec<Effect>) -> bool {
    let Some(WholeList {
        source: WholeListSource::Detached(window),
        ..
    }) = state.whole_list.as_ref()
    else {
        return false;
    };
    let more = LibraryRequest::More {
        list: window.list.clone(),
        offset: window.next_offset,
        limit: limit(state, &window.list),
    };
    let Some(id) = request(state, more, effects) else {
        return false;
    };
    if let Some(WholeList {
        source: WholeListSource::Detached(window),
        ..
    }) = state.whole_list.as_mut()
    {
        window.load = Load::Loading { id };
    }
    true
}

fn cancel_whole_list(state: &mut State) {
    if state.whole_list.take().is_some() {
        state.message = None;
    }
}

/// `Z`/`C-z`: the selected track, album or playlist at the end of the
/// queue (`Open`, the player expands it); nothing for an artist.
fn queue_selected(state: &mut State) -> Vec<Effect> {
    match selected(state).and_then(Row::item) {
        Some(item) => vec![Effect::Send(Command::Open {
            items: vec![item],
            at: Some(InsertAt::End),
        })],
        None => Vec::new(),
    }
}

/// `f` in *All tracks*: the role filter.
fn open_role_filter(state: &mut State) {
    let page = state.page();
    if page.load != Load::Idle {
        return;
    }
    if let Some(window) = page.focused().filter(|w| w.kind == WindowKind::AllTracks) {
        state.popup = Some(Popup::Roles {
            checked: window.roles,
            cursor: 0,
        });
    }
}

/// `g a`/`C-Space`: the actions popup for the selected row (the queue's
/// entry under the cursor on the queue page).
pub(super) fn actions_on_selected(state: &mut State) -> Vec<Effect> {
    let page = state.page();
    let popup = if page.kind == PageKind::Queue {
        state
            .cursor
            .and_then(|id| state.queue().iter().find(|e| e.id == id))
            .map(|entry| {
                (
                    entry.track.title.clone(),
                    popup::entry_actions(&entry.track, entry.id, false),
                )
            })
    } else {
        selected(state).map(|row| match row {
            Row::Track(_) | Row::Credit(_) => {
                let track = row.track().expect("a track row");
                let own = page.own_playlist().and_then(|playlist| {
                    let window = page.focused()?;
                    if window.kind != WindowKind::PlaylistTracks {
                        return None;
                    }
                    let position = *window.positions.get(window.cursor)?;
                    Some((playlist, position, page.etag()))
                });
                (track.title.clone(), popup::track_actions(track, own))
            }
            Row::Album(album) => (album.title.clone(), popup::album_actions(album)),
            // A search playlist is never the user's own (spec 0007).
            Row::Playlist(playlist) if page.search.is_some() => {
                let playlist = PlaylistSummary {
                    own: false,
                    ..playlist.clone()
                };
                (playlist.title.clone(), popup::playlist_actions(&playlist))
            }
            Row::Playlist(playlist) => (playlist.title.clone(), popup::playlist_actions(playlist)),
            Row::Artist(artist) => (artist.name.clone(), popup::artist_actions(artist)),
        })
    };
    if let Some((title, actions)) = popup {
        state.popup = Some(Popup::Actions {
            title,
            actions,
            cursor: 0,
        });
    }
    Vec::new()
}

/// `a`: the actions popup for the playing track.
pub(super) fn actions_on_playing(state: &mut State) -> Vec<Effect> {
    if let Some(entry) = state.current() {
        state.popup = Some(Popup::Actions {
            title: entry.track.title.clone(),
            actions: popup::entry_actions(&entry.track, entry.id, true),
            cursor: 0,
        });
    }
    Vec::new()
}

/// A key while a popup is open: only the popup's keys act.
pub(super) fn popup_key(state: &mut State, key: Key) -> Vec<Effect> {
    let Some(popup) = state.popup.take() else {
        return Vec::new();
    };
    let down = matches!(key, Key::Char('j') | Key::Down);
    let up = matches!(key, Key::Char('k') | Key::Up);
    match popup {
        Popup::Actions {
            title,
            actions,
            mut cursor,
        } => match key {
            Key::Esc => {}
            Key::Enter => {
                if let Some(action) = actions.get(cursor).cloned() {
                    return run(state, action);
                }
            }
            _ => {
                if down {
                    cursor = (cursor + 1).min(actions.len().saturating_sub(1));
                } else if up {
                    cursor = cursor.saturating_sub(1);
                }
                state.popup = Some(Popup::Actions {
                    title,
                    actions,
                    cursor,
                });
            }
        },
        Popup::AddToPlaylist {
            tracks,
            playlists,
            mut cursor,
        } => {
            let own: Vec<(String, String)> = popup::own(&playlists)
                .iter()
                .map(|p| (p.uuid.clone(), p.title.clone()))
                .collect();
            match key {
                Key::Esc => {}
                Key::Enter if cursor == 0 => {
                    state.popup = Some(Popup::NewPlaylist {
                        tracks,
                        name: String::new(),
                    });
                }
                Key::Enter => {
                    if let Some((uuid, title)) = own.get(cursor - 1).cloned() {
                        return add_source(state, tracks, uuid, title, true);
                    }
                }
                _ => {
                    if down {
                        cursor = (cursor + 1).min(own.len());
                    } else if up {
                        cursor = cursor.saturating_sub(1);
                    }
                    state.popup = Some(Popup::AddToPlaylist {
                        tracks,
                        playlists,
                        cursor,
                    });
                }
            }
        }
        Popup::NewPlaylist { tracks, mut name } => match key {
            Key::Esc => {}
            Key::Enter => {
                let title = name.trim().to_owned();
                if !title.is_empty() {
                    return write(
                        state,
                        LibraryRequest::CreatePlaylist {
                            title: title.clone(),
                        },
                        Write::Create { title, tracks },
                    );
                }
            }
            _ => {
                match key {
                    Key::Char(c) => name.push(c),
                    Key::Backspace => {
                        name.pop();
                    }
                    _ => {}
                }
                state.popup = Some(Popup::NewPlaylist { tracks, name });
            }
        },
        Popup::Confirm { question, on_yes } => match key {
            Key::Char('y') => return confirmed(state, on_yes),
            Key::Char('n') | Key::Esc => {}
            _ => state.popup = Some(Popup::Confirm { question, on_yes }),
        },
        Popup::Roles {
            mut checked,
            mut cursor,
        } => match key {
            Key::Esc => {}
            Key::Enter => return apply_roles(state, checked),
            _ => {
                if down {
                    cursor = (cursor + 1).min(checked.len() - 1);
                } else if up {
                    cursor = cursor.saturating_sub(1);
                } else if key == Key::Char(' ') {
                    checked[cursor] = !checked[cursor];
                }
                state.popup = Some(Popup::Roles { checked, cursor });
            }
        },
    }
    Vec::new()
}

/// The role filter applied to *All tracks*: the cursor stays among the
/// shown rows, which may need the next page.
fn apply_roles(state: &mut State, checked: [bool; 4]) -> Vec<Effect> {
    let index = top(state);
    let page = &mut state.history[index];
    let focus = page.focus;
    let Some(window) = page
        .windows
        .get_mut(focus)
        .filter(|w| w.kind == WindowKind::AllTracks)
    else {
        return Vec::new();
    };
    window.roles = checked;
    window.cursor = window.cursor.min(window.len().saturating_sub(1));
    let mut effects = Vec::new();
    near_end(state, index, focus, &mut effects);
    effects
}

/// Runs one action of the actions popup (closed by the caller).
fn run(state: &mut State, action: MenuAction) -> Vec<Effect> {
    let queue = |item, at| {
        vec![Effect::Send(Command::Open {
            items: vec![item],
            at: Some(at),
        })]
    };
    match action {
        MenuAction::Open(kind) => open(state, kind),
        MenuAction::GoToAlbum(id) => open(state, PageKind::Album(id)),
        MenuAction::GoToArtist(artist) => open(state, PageKind::Artist(artist.id)),
        MenuAction::AddToQueue(item) => queue(item, InsertAt::End),
        MenuAction::PlayNext(item) => queue(item, InsertAt::Next),
        MenuAction::AddFavorite(kind, id) => write(
            state,
            LibraryRequest::AddFavorite(kind, id),
            Write::Favorite { add: true, kind },
        ),
        MenuAction::RemoveFavorite(kind, id) => write(
            state,
            LibraryRequest::RemoveFavorite(kind, id),
            Write::Favorite { add: false, kind },
        ),
        MenuAction::AddToPlaylist(tracks) => {
            let mut playlists = Window::new(WindowKind::Playlists, ListRef::Playlists);
            let more = LibraryRequest::More {
                list: ListRef::Playlists,
                offset: 0,
                limit: limit(state, &ListRef::Playlists),
            };
            let mut effects = Vec::new();
            playlists.load = match request(state, more, &mut effects) {
                Some(id) => Load::Loading { id },
                None => Load::Failed(connection_message(state)),
            };
            state.popup = Some(Popup::AddToPlaylist {
                tracks,
                playlists,
                cursor: 0,
            });
            effects
        }
        MenuAction::RemoveFromPlaylist {
            uuid,
            title,
            index,
            etag,
        } => write(
            state,
            LibraryRequest::RemoveFromPlaylist {
                uuid: uuid.clone(),
                index,
                etag: etag.unwrap_or_default(),
            },
            Write::RemoveFromPlaylist { uuid, title },
        ),
        MenuAction::RemoveFromQueue(entry) => {
            vec![Effect::Send(Command::RemoveFromQueue(entry))]
        }
        MenuAction::DeletePlaylist { uuid, title } => {
            state.popup = Some(Popup::Confirm {
                question: format!("Delete {title}? (y/n)"),
                on_yes: Confirmed::DeletePlaylist { uuid, title },
            });
            Vec::new()
        }
    }
}

/// Sends a library write and remembers it for its reply.
fn write(state: &mut State, request_: LibraryRequest, write: Write) -> Vec<Effect> {
    let mut effects = Vec::new();
    if let Some(id) = request(state, request_, &mut effects) {
        state.writes.push((id, write));
    }
    effects
}

/// `y` to a question.
fn confirmed(state: &mut State, on_yes: Confirmed) -> Vec<Effect> {
    match on_yes {
        Confirmed::AddToPlaylist {
            uuid,
            title,
            tracks,
        } => send_add(state, uuid, title, tracks, true),
        Confirmed::DeletePlaylist { uuid, title } => write(
            state,
            LibraryRequest::DeletePlaylist { uuid: uuid.clone() },
            Write::DeletePlaylist { uuid, title },
        ),
    }
}

/// Adds `source`'s tracks to playlist `uuid`: an album's are fetched
/// whole first.
fn add_source(
    state: &mut State,
    source: TrackSource,
    uuid: String,
    title: String,
    check_duplicates: bool,
) -> Vec<Effect> {
    match source {
        TrackSource::Tracks(tracks) => add_tracks(state, uuid, title, tracks, check_duplicates),
        TrackSource::Album(id) => {
            if !connected(state) {
                return Vec::new();
            }
            state.whole_list = Some(WholeList {
                source: WholeListSource::Detached(Window::new(
                    WindowKind::AlbumTracks,
                    ListRef::AlbumTracks(id),
                )),
                purpose: Purpose::AddToPlaylist {
                    uuid,
                    title,
                    check_duplicates,
                },
            });
            let mut effects = Vec::new();
            if ask_detached(state, &mut effects) {
                progress(state);
            } else {
                cancel_whole_list(state);
            }
            effects
        }
    }
}

/// Adds `tracks`, asking first when one is in a loaded copy of the
/// playlist (`check_duplicates`).
fn add_tracks(
    state: &mut State,
    uuid: String,
    title: String,
    tracks: Vec<Track>,
    check_duplicates: bool,
) -> Vec<Effect> {
    if check_duplicates && in_loaded_copy(state, &uuid, &tracks) {
        state.popup = Some(Popup::Confirm {
            question: format!("Already in {title}: add again? (y/n)"),
            on_yes: Confirmed::AddToPlaylist {
                uuid,
                title,
                tracks,
            },
        });
        return Vec::new();
    }
    send_add(state, uuid, title, tracks, false)
}

/// Whether one of `tracks` is in a loaded copy of playlist `uuid`.
fn in_loaded_copy(state: &State, uuid: &str, tracks: &[Track]) -> bool {
    state
        .history
        .iter()
        .filter(|page| matches!(&page.kind, PageKind::Playlist(u) if u == uuid))
        .flat_map(|page| &page.windows)
        .any(|window| match &window.rows {
            Rows::Tracks(rows) => rows.iter().any(|r| tracks.iter().any(|t| t.id == r.id)),
            _ => false,
        })
}

fn send_add(
    state: &mut State,
    uuid: String,
    title: String,
    tracks: Vec<Track>,
    allow_duplicates: bool,
) -> Vec<Effect> {
    if tracks.is_empty() {
        return Vec::new();
    }
    let count = tracks.len();
    write(
        state,
        LibraryRequest::AddToPlaylist {
            uuid: uuid.clone(),
            tracks: tracks.iter().map(|t| t.id).collect(),
            allow_duplicates,
        },
        Write::AddToPlaylist { uuid, title, count },
    )
}

/// A key during a whole-list load: `Esc` cancels it; `q`/`C-c` quit;
/// nothing else acts until it ends.
pub(super) fn whole_list_key(state: &mut State, key: Key) -> Vec<Effect> {
    match key {
        Key::Esc => {
            cancel_whole_list(state);
            Vec::new()
        }
        Key::Char('q') | Key::Ctrl('c') => vec![Effect::Quit],
        _ => Vec::new(),
    }
}

/// How a page's fetch failure reads in the page.
fn page_error(kind: &PageKind, message: String) -> String {
    match kind {
        PageKind::Library => format!("Could not load the library: {message}"),
        PageKind::FavoriteTracks => format!("Could not load the favorite tracks: {message}"),
        PageKind::Search(_) => format!("Could not search: {message}"),
        _ => message,
    }
}

/// The player's answer to request `id`: applied to the page, window or
/// write that asked; dropped when nothing waits for it.
pub(super) fn reply(
    state: &mut State,
    id: u64,
    result: Result<LibraryResponse, String>,
) -> Vec<Effect> {
    let mut effects = Vec::new();
    let waiting = Load::Loading { id };
    // A page.
    if let Some(index) = state.history.iter().position(|p| p.load == waiting) {
        let page = &state.history[index];
        let page_size = if page.search.is_some() {
            state.search_page_size
        } else {
            state.page_size
        };
        let page = &mut state.history[index];
        match result {
            Ok(LibraryResponse::Page(data)) => {
                page.apply(data, page_size);
                first_credits(state, index, &mut effects);
            }
            Ok(other) => page.load = Load::Failed(format!("Unexpected answer: {other:?}")),
            Err(message) => page.load = Load::Failed(page_error(&page.kind, message)),
        }
        return effects;
    }
    // A window of a page.
    let found = state.history.iter().enumerate().find_map(|(p, page)| {
        let w = page.windows.iter().position(|w| w.load == waiting)?;
        Some((p, w))
    });
    if let Some((p, w)) = found {
        let limit = limit(state, &state.history[p].windows[w].list);
        let ours = matches!(
            state.whole_list.as_ref().map(|w| &w.source),
            Some(WholeListSource::Window { page, window }) if (*page, *window) == (p, w)
        );
        let window = &mut state.history[p].windows[w];
        match result {
            Ok(LibraryResponse::Items(items)) => {
                window.append(items, limit);
                if ours {
                    continue_whole_list(state, &mut effects);
                }
            }
            Ok(_) | Err(_) => {
                let message = result.err().unwrap_or_else(|| "Unexpected answer".into());
                window.load = Load::Failed(message.clone());
                if ours {
                    cancel_whole_list(state);
                    state.message = Some(message);
                }
            }
        }
        return effects;
    }
    // The playlists of *Add to playlist…*: loaded to their end.
    if let Some(Popup::AddToPlaylist { playlists, .. }) = state.popup.as_mut()
        && playlists.load == waiting
    {
        match result {
            Ok(LibraryResponse::Items(items)) => {
                playlists.append(items, largest_page(&ListRef::Playlists));
                if !playlists.complete() {
                    let more = LibraryRequest::More {
                        list: ListRef::Playlists,
                        offset: playlists.next_offset,
                        limit: largest_page(&ListRef::Playlists),
                    };
                    let mut asked = Vec::new();
                    if let Some(id) = request(state, more, &mut asked)
                        && let Some(Popup::AddToPlaylist { playlists, .. }) = state.popup.as_mut()
                    {
                        playlists.load = Load::Loading { id };
                    }
                    effects.extend(asked);
                }
            }
            Ok(_) => playlists.load = Load::Failed("Unexpected answer".into()),
            Err(message) => playlists.load = Load::Failed(message),
        }
        return effects;
    }
    // The detached list of a whole-list load.
    if let Some(WholeList {
        source: WholeListSource::Detached(window),
        ..
    }) = state.whole_list.as_mut()
        && window.load == waiting
    {
        match result {
            Ok(LibraryResponse::Items(items)) => {
                let limit = state.page_size.clamp(1, largest_page(&window.list));
                window.append(items, limit);
                continue_whole_list(state, &mut effects);
            }
            Ok(_) | Err(_) => {
                cancel_whole_list(state);
                state.message = Some(result.err().unwrap_or_else(|| "Unexpected answer".into()));
            }
        }
        return effects;
    }
    // A write.
    if let Some(index) = state.writes.iter().position(|(w, _)| *w == id) {
        let (_, write) = state.writes.remove(index);
        written(state, write, result, &mut effects);
    }
    effects
}

/// A write's answer: its message, and the pages showing the changed list
/// fetched again.
fn written(
    state: &mut State,
    write: Write,
    result: Result<LibraryResponse, String>,
    effects: &mut Vec<Effect>,
) {
    let response = match result {
        Ok(response) => response,
        Err(message) => {
            if let Write::RemoveFromPlaylist { uuid, .. } = &write
                && message == PLAYLIST_CHANGED
            {
                refetch(
                    state,
                    |k| matches!(k, PageKind::Playlist(u) if u == uuid),
                    effects,
                );
            }
            state.message = Some(message);
            return;
        }
    };
    let is_playlist =
        |uuid: String| move |k: &PageKind| matches!(k, PageKind::Playlist(u) if *u == uuid);
    match write {
        Write::Favorite { add, kind } => {
            state.message = Some(if add {
                "Added to favorites".into()
            } else {
                "Removed from favorites".into()
            });
            if kind == FavoriteKind::Track {
                refetch(state, |k| *k == PageKind::FavoriteTracks, effects);
            } else {
                refetch(state, |k| *k == PageKind::Library, effects);
            }
        }
        Write::Create { title, tracks } => {
            let LibraryResponse::Created(playlist) = response else {
                state.message = Some(format!("Created {title}"));
                return;
            };
            state.message = Some(format!("Created {}", playlist.title));
            refetch(state, |k| *k == PageKind::Library, effects);
            effects.extend(add_source(
                state,
                tracks,
                playlist.uuid,
                playlist.title,
                false,
            ));
        }
        Write::AddToPlaylist { uuid, title, count } => {
            state.message = Some(match count {
                1 => format!("Added 1 track to {title}"),
                n => format!("Added {n} tracks to {title}"),
            });
            refetch(state, is_playlist(uuid), effects);
        }
        Write::RemoveFromPlaylist { uuid, title } => {
            state.message = Some(format!("Removed from {title}"));
            refetch(state, is_playlist(uuid), effects);
        }
        Write::DeletePlaylist { title, .. } => {
            state.message = Some(format!("Deleted {title}"));
            refetch(state, |k| *k == PageKind::Library, effects);
        }
    }
}

/// The connection is gone: pending loads fail with its message, pending
/// writes and a whole-list load are dropped.
pub(super) fn disconnected(state: &mut State) {
    let message = connection_message(state);
    let fail = |load: &mut Load| {
        if matches!(load, Load::Loading { .. }) {
            *load = Load::Failed(message.clone());
        }
    };
    for page in &mut state.history {
        fail(&mut page.load);
        for window in &mut page.windows {
            fail(&mut window.load);
        }
    }
    if let Some(Popup::AddToPlaylist { playlists, .. }) = state.popup.as_mut() {
        fail(&mut playlists.load);
    }
    cancel_whole_list(state);
    state.writes.clear();
}

/// Joined again: the page on top is fetched again if it failed because
/// the player was gone.
pub(super) fn reconnected(state: &mut State) -> Vec<Effect> {
    let mut effects = Vec::new();
    let start = std::mem::take(&mut state.start_pending);
    if start || matches!(&state.page().load, Load::Failed(m) if m == DISCONNECTED || m == SHUT_DOWN)
    {
        fetch_page(state, top(state), &mut effects);
    }
    effects
}

#[cfg(test)]
mod tests;
