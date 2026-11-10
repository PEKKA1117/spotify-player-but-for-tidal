//! Pure UI model: key -> `Action` -> new `State` plus `Effect`s, no I/O.
//!
//! Spec 0004 "TUI": the client keeps the player's latest snapshot as sent
//! (it never reorders the queue), a queue cursor addressed by entry ID, key
//! sequences (`g g`) and the open prompt. Everything it asks of the player
//! leaves as [`Effect::Send`], an opened item included (`Command::Open`:
//! the player expands it, spec 0005).
//!
//! Spec 0005 "The TUI as a client": the model is the same for a standalone
//! TUI and a client of another process's player. It holds nothing about
//! playback but what the player sent ([`Action::Welcome`], then events);
//! while [`Connection::Disconnected`] it keeps the last snapshot and no key
//! sends anything.
//!
//! Spec 0006 "Client model": the pages above the queue ([`State::history`],
//! bottom first, the queue at the bottom), their windows and cursors, the
//! popups and the whole-list load live in [`page`], [`popup`] and
//! `browse`. Library requests leave as [`Effect::Library`] with an ID from
//! [`State::next_request`]; the player's answer comes back as
//! [`Action::LibraryReply`] and is applied to whatever asked with that ID.
//! The list window's height (how near the end a cursor must come to load
//! the next page) is [`State::list_height`], set by [`Action::Resize`].
//!
//! Spec 0007: the search page is one more page of the history
//! ([`PageKind::Search`]), its input, top hit and focus in
//! [`Page::search`]; its keys live in [`search`].
//!
//! Spec 0008: keys are looked up in [`State::keymap`] ([`keymap`]); which
//! command acts where is decided in `dispatch`.
//!
//! Spec 0021: the lyrics page follows the current track and keeps its
//! lyrics and scroll state in [`Page::lyrics`]; see [`lyrics`].
//!
//! Spec 0012: each window's filter (and the queue page's) and its typing
//! keys live in [`filter`].
//!
//! Spec 0020: each window's selection (and the queue's), the `s …` keys
//! and the selection actions live in [`select`].
//!
//! Spec 0017: the theme list, the theme in use and the Themes popup live
//! in [`theme`]; the TUI draws [`State::theme`].

mod browse;
mod dispatch;
pub mod filter;
pub mod help;
pub mod keymap;
pub mod logs;
pub mod lyrics;
pub mod page;
pub mod popup;
pub mod search;
pub mod select;
pub mod theme;

use std::time::Duration;

use crate::library::{LibraryRequest, LibraryResponse};
use crate::log::{LogBatch, LogLine};
use crate::protocol::{
    self, Command, DeviceEntry, InsertAt, PlaybackState, PlayerSnapshot, QueueEntry,
};
use crate::track::EntryId;

pub use browse::{Moving, PLAYLIST_CHANGED, Purpose, WholeList, WholeListSource, Write};
pub use filter::{Filter, MAX_FILTER};
pub use help::{
    Help, HelpRow, HelpSection, HintEntry, Hints, help, hints, locate, no_match, visible,
};
pub use keymap::{BaseKey, Keymap};
pub use lyrics::{LyricsContent, LyricsView};
pub use page::{
    DEFAULT_PAGE_SIZE, Header, Load, MAX_HISTORY, MAX_WHOLE_LIST, Page, PageKind, ROLE_CATEGORIES,
    Row, Rows, ViewAll, Window, WindowKind, clock, group, largest_page,
};
pub use popup::{
    Confirmed, DEVICE_NOT_FOUND, DeviceList, DeviceRow, LOADING_DEVICES, MenuAction, NEW_PLAYLIST,
    PLAYLIST_NAME, Popup, Removal, RemoveFrom, Removed, RemovedTarget, SelectionAction,
    SelectionPart, SelectionRemoval, TargetLine, TrackSource, device_rows, selection_question,
    target_lines,
};
pub use search::{DEFAULT_SEARCH_PAGE_SIZE, MAX_QUERY, Search, SearchFocus};
pub use select::{Held, Marks, Origin, Selection};
pub use theme::{Theme, Themes};

/// The configured steps of the volume and seek keys (spec 0004 "Settings").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Steps {
    /// `+`/`-` send `ChangeVolume(±volume)`.
    pub volume: u8,
    /// `>`/`<` send `SeekBy(±seek)`.
    pub seek: Duration,
}

impl Default for Steps {
    fn default() -> Self {
        Self {
            volume: 5,
            seek: Duration::from_secs(5),
        }
    }
}

/// The library page's window widths in percent (spec 0008 "The library
/// layout", `app.toml`'s `[layout] library`): *Playlists* and *Albums*;
/// *Artists* takes the rest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LibraryLayout {
    pub playlist_percent: u16,
    pub album_percent: u16,
}

impl Default for LibraryLayout {
    fn default() -> Self {
        Self {
            playlist_percent: 40,
            album_percent: 40,
        }
    }
}

/// A decoded key press (the terminal mapping lives in the binary).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    /// A printable character, upper case included (`G`, `O`, `A`).
    Char(char),
    /// A character pressed with Control (`C-s` is `Ctrl('s')`; `C-Space`
    /// is `Ctrl(' ')`).
    Ctrl(char),
    Enter,
    Esc,
    Backspace,
    Up,
    Down,
    /// Focus the next window.
    Tab,
    /// Shift-Tab: focus the previous window.
    BackTab,
    PageUp,
    PageDown,
    Left,
    Right,
    Home,
    End,
    Insert,
    Delete,
    /// `f1`–`f12`.
    F(u8),
    /// Alt + a key (`M-a` is `Alt(BaseKey::Char('a'))`, `M-enter`).
    Alt(BaseKey),
    /// Control + a named key (`C-enter`); Control + a character is
    /// [`Key::Ctrl`].
    CtrlKey(BaseKey),
}

/// The open prompt (`o` / `O`): what has been typed and where it adds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    /// `End` for `o` (add to queue), `Next` for `O` (play next).
    pub at: InsertAt,
    pub text: String,
}

/// The client's connection to the player (spec 0005 "The TUI as a
/// client").
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Connection {
    /// Joined: keys send commands.
    #[default]
    Connected,
    /// The connection closed or failed (`shut_down`: after the player said
    /// `ShuttingDown`); the last snapshot stays, reconnecting.
    Disconnected { shut_down: bool },
    /// The player refused this client (a version mismatch): the message;
    /// never retried.
    Refused(String),
}

/// The message row while disconnected.
pub const DISCONNECTED: &str = "Disconnected from the player: reconnecting…";
/// The message row after the player shut down.
pub const SHUT_DOWN: &str = "The player shut down: waiting for it to come back…";

/// The whole UI state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct State {
    /// Whether the player is reachable (spec 0005).
    pub connection: Connection,
    /// Whether the session has expired and login is required.
    pub login_required: bool,
    /// The volume and seek steps the keys send.
    pub steps: Steps,
    /// The player's latest snapshot, as sent; `None` until the first.
    pub player: Option<PlayerSnapshot>,
    /// The position in the current entry (snapshot, then `Position` events).
    pub position: Duration,
    /// The queue cursor, by entry ID (never by index).
    pub cursor: Option<EntryId>,
    /// The entry the queue view keeps in view: the current one when it
    /// changes, the cursor when it moves.
    pub anchor: Option<EntryId>,
    /// The open prompt, if open.
    pub prompt: Option<Prompt>,
    /// A message of the client's own (an invalid item, a command's error
    /// reply); shown instead of the player's while set.
    pub message: Option<String>,
    /// The keymap's notice of skipped spotify-player names (spec 0008
    /// decision 3), and `theme.toml`'s after it (spec 0017), kept until the
    /// first `Welcome`, which shows it as the message instead of clearing
    /// it.
    pub notice: Option<String>,
    /// The library page's window widths (spec 0008 "The library layout").
    pub library_layout: LibraryLayout,
    /// The bindings keys are looked up in (spec 0008); the defaults until
    /// [`apply_keymap`].
    pub keymap: Keymap,
    /// The keys of a sequence being collected (spec 0008 "Sequences":
    /// they start a binding and are none yet).
    pub pending: Vec<Key>,
    /// Whether a sequence is being collected (with the defaults: `g` was
    /// pressed); always `!pending.is_empty()` (0004 AC21 reads it).
    pub pending_g: bool,
    /// Whether the hint for a pending key sequence is shown (spec 0013,
    /// `key_hints`); the TUI decides when.
    pub key_hints: bool,
    /// Whether removing from favorites, a playlist or the queue asks
    /// first (spec 0019, `confirm_removals`).
    pub confirm_removals: bool,
    /// The browse pages whose links are not shown, by the name after
    /// `pages/` (spec 0024, `hide_browse_pages`); applied when a page
    /// arrives.
    pub hide_browse_pages: Vec<String>,
    /// The page history, bottom first: the queue page at the bottom, the
    /// page shown on top (spec 0006 "Pages").
    pub history: Vec<Page>,
    /// The open popup, if any.
    pub popup: Option<Popup>,
    /// The keys help, if open (spec 0008); over whatever else is open,
    /// which it leaves as it is.
    pub help: Option<Help>,
    /// A whole-list load in progress (before `Enter` on a track or an
    /// *Add to playlist…* of an album).
    pub whole_list: Option<WholeList>,
    /// The library writes waiting for their reply, by request ID.
    pub writes: Vec<(u64, Write)>,
    /// The playlist move on its way, if any (spec 0022).
    pub moving: Option<Moving>,
    /// The writes sent for a selection, until all are answered (spec
    /// 0020).
    pub batches: Vec<select::Batch>,
    /// The selection an open *Add to playlist…* adds (spec 0020): its
    /// write, once sent, clears it when it succeeds.
    pub selection_add: Option<(Origin, Vec<Held>)>,
    /// The page size of every list request (`TIDAL_PLAYER_PAGE_SIZE`).
    pub page_size: u32,
    /// The page size of every search request, the first page of each
    /// result list included (`TIDAL_PLAYER_SEARCH_PAGE_SIZE`, spec 0007).
    pub search_page_size: u32,
    /// A list window's height in rows: the next page loads when the cursor
    /// comes within this many rows of the last loaded one, and `C-f`/`C-b`
    /// move by it. Set by [`Action::Resize`]; 20 until then.
    pub list_height: usize,
    /// The ID of the next library request (unique for the client's run).
    pub next_request: u64,
    /// The start page waits for the first `Welcome` to be fetched.
    pub start_pending: bool,
    /// The themes (spec 0017), `default` first; `default` alone until
    /// [`apply_themes`].
    pub themes: Themes,
    /// The index in [`State::themes`] of the theme drawn: the one in use,
    /// or the one under the Themes popup's cursor while it is open.
    pub theme: usize,
    /// The log lines the logs page shows and its asking (spec 0027).
    pub logs: logs::LogStore,
}

impl Default for State {
    fn default() -> Self {
        Self {
            connection: Connection::default(),
            login_required: false,
            steps: Steps::default(),
            player: None,
            position: Duration::ZERO,
            cursor: None,
            anchor: None,
            prompt: None,
            message: None,
            notice: None,
            library_layout: LibraryLayout::default(),
            keymap: Keymap::default(),
            pending: Vec::new(),
            pending_g: false,
            key_hints: true,
            confirm_removals: true,
            hide_browse_pages: Vec::new(),
            history: vec![Page::new(PageKind::Queue)],
            popup: None,
            help: None,
            whole_list: None,
            writes: Vec::new(),
            moving: None,
            batches: Vec::new(),
            selection_add: None,
            page_size: DEFAULT_PAGE_SIZE,
            search_page_size: DEFAULT_SEARCH_PAGE_SIZE,
            list_height: 20,
            next_request: 1,
            start_pending: false,
            themes: Themes::default(),
            theme: 0,
            logs: logs::LogStore::default(),
        }
    }
}

impl State {
    /// An empty state whose keys send `steps`.
    pub fn new(steps: Steps) -> Self {
        Self {
            steps,
            ..Self::default()
        }
    }

    /// The queue in play order, exactly as the last snapshot sent it.
    pub fn queue(&self) -> &[QueueEntry] {
        self.player.as_ref().map_or(&[], |p| p.queue.as_slice())
    }

    /// The current entry, if any.
    pub fn current(&self) -> Option<&QueueEntry> {
        let id = self.player.as_ref()?.current?;
        self.queue().iter().find(|e| e.id == id)
    }

    /// The message to show in the playback window: the connection's, else
    /// the client's own, else the player's.
    pub fn message(&self) -> Option<&str> {
        match &self.connection {
            Connection::Connected => self
                .message
                .as_deref()
                .or_else(|| self.player.as_ref()?.message.as_deref()),
            Connection::Disconnected { shut_down: false } => Some(DISCONNECTED),
            Connection::Disconnected { shut_down: true } => Some(SHUT_DOWN),
            Connection::Refused(message) => Some(message),
        }
    }

    /// Whether the client is trying to reach the player again.
    pub fn reconnecting(&self) -> bool {
        matches!(self.connection, Connection::Disconnected { .. })
    }

    /// The page shown: the top of the history.
    pub fn page(&self) -> &Page {
        self.history.last().expect("the queue page is never popped")
    }

    /// The theme the TUI draws in (spec 0017).
    pub fn theme(&self) -> &Theme {
        let list = self.themes.list();
        list.get(self.theme).unwrap_or(&list[0])
    }
}

/// An input to the UI model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// The user asked to quit.
    Quit,
    /// A periodic timer tick.
    Tick,
    /// A key press.
    Key(Key),
    /// Pasted text (bracketed paste).
    Paste(String),
    /// An event from the player.
    Player(protocol::Event),
    /// The player's answer to `Subscribe`: its state and the login status,
    /// replacing everything (spec 0005).
    Welcome {
        snapshot: PlayerSnapshot,
        login_required: bool,
    },
    /// The player's answer to a command: applied, or why not.
    Reply(Result<(), String>),
    /// The connection to the player is gone (`shut_down`: it said so).
    Disconnected { shut_down: bool },
    /// The player refused this client: the message.
    Refused(String),
    /// The player's answer to [`Effect::Library`] with the same `id`.
    LibraryReply {
        id: u64,
        result: Result<LibraryResponse, String>,
    },
    /// The terminal was resized: a list window now shows `list_height`
    /// rows.
    Resize { list_height: usize },
    /// The player's answer to [`Effect::Devices`] with the same `id`
    /// (spec 0014).
    DevicesReply {
        id: u64,
        result: Result<Vec<DeviceEntry>, String>,
    },
    /// The player's answer to [`Effect::Logs`] with the same `id` (spec
    /// 0027).
    LogsReply {
        id: u64,
        result: Result<LogBatch, String>,
    },
    /// The time now on a monotonic clock (since the TUI started), fed
    /// before every other action and on every tick: the logs page asks
    /// again a second after each answer (spec 0027).
    Clock(Duration),
    /// Lines of the TUI's own log, new since the last ones given (spec
    /// 0027): merged into the logs page by time.
    OwnLogs(Vec<LogLine>),
}

/// A side effect the caller must perform on behalf of the model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Leave the application.
    Quit,
    /// Send a command to the player.
    Send(Command),
    /// Ask the player about the library; its answer comes back as
    /// [`Action::LibraryReply`] with the same `id`.
    Library { id: u64, request: LibraryRequest },
    /// Ask the player for its output devices (spec 0014); its answer comes
    /// back as [`Action::DevicesReply`] with the same `id`.
    Devices { id: u64 },
    /// Ask the player for its log lines after `after` (spec 0027); its
    /// answer comes back as [`Action::LogsReply`] with the same `id`.
    Logs { id: u64, after: Option<u64> },
    /// Spec 0017 addendum 67055c40c1e2.
    Envy(theme::Envy),
}

/// Puts the library over the queue as the start page (spec 0006 "Pages"),
/// fetched on the first `Welcome`.
pub fn start_on_library(state: &mut State) -> Vec<Effect> {
    let mut library = Page::new(PageKind::Library);
    // ID 0 is never given to a request, so no reply lands here early.
    library.load = Load::Loading { id: 0 };
    state.history.push(library);
    state.start_pending = true;
    Vec::new()
}

/// Puts `keymap` in force; its notice of skipped spotify-player names
/// (spec 0008 decision 3), if any, becomes the message, and stays it
/// through the player's first `Welcome`.
pub fn apply_keymap(state: &mut State, keymap: Keymap) {
    if let Some(notice) = keymap.notice() {
        // Before `theme.toml`'s, if that is in already (spec 0017 AC6).
        let notice = match state.notice.take() {
            Some(themes) => format!("{notice} · {themes}"),
            None => notice,
        };
        state.message = Some(notice.clone());
        state.notice = Some(notice);
    }
    state.keymap = keymap;
    dispatch::set_pending(state, Vec::new());
}

/// Puts `themes` in force with `active` (an index in it) the theme in use
/// (spec 0017); `theme.toml`'s notice of skipped spotify-player names joins
/// the keymap's, after it (AC6).
pub fn apply_themes(state: &mut State, themes: Themes, active: usize) {
    if let Some(notice) = themes.notice() {
        let notice = match state.notice.take() {
            Some(keymap) => format!("{keymap} · {notice}"),
            None => notice,
        };
        state.message = Some(notice.clone());
        state.notice = Some(notice);
    }
    state.theme = active.min(themes.list().len() - 1);
    state.themes = themes;
}

/// Applies `action` to `state` and returns the effects the caller must run.
pub fn update(state: &mut State, action: Action) -> Vec<Effect> {
    match action {
        Action::Quit => vec![Effect::Quit],
        Action::Tick => Vec::new(),
        Action::Key(key) => {
            let mut effects = dispatch::key(state, key);
            // Nothing reaches a player that is not there (spec 0005).
            if state.connection != Connection::Connected {
                effects.retain(|e| {
                    !matches!(
                        e,
                        Effect::Send(_)
                            | Effect::Library { .. }
                            | Effect::Devices { .. }
                            | Effect::Logs { .. }
                    )
                });
            }
            effects
        }
        Action::Paste(text) => {
            // The help takes pasted text into its filter, and nothing else.
            if let Some(open) = state.help.as_mut() {
                if open.typing {
                    open.filter.extend(text.chars().filter(|c| !c.is_control()));
                    open.cursor = 0;
                }
                return Vec::new();
            }
            // Line breaks and other control characters never reach the
            // prompt: a pasted link often ends with a newline.
            let text = text.chars().filter(|c| !c.is_control());
            if let Some(prompt) = state.prompt.as_mut() {
                prompt.text.extend(text);
            } else if let Some(Popup::NewPlaylist {
                name, description, ..
            }) = state.popup.as_mut()
            {
                description.as_mut().unwrap_or(name).extend(text);
            } else if let Some(Popup::EditPlaylist { text: held, .. }) = state.popup.as_mut() {
                held.extend(text);
            } else if state.popup.is_none() {
                let text: String = text.collect();
                search::paste(state, &text);
                return filter::paste(state, &text);
            }
            Vec::new()
        }
        Action::Welcome {
            snapshot,
            login_required,
        } => {
            state.connection = Connection::Connected;
            state.login_required = login_required;
            state.message = None;
            let mut effects = apply_snapshot(state, snapshot);
            // The keymap's notice outlives the first `Welcome` only.
            if let Some(notice) = state.notice.take() {
                state.message = Some(notice);
            }
            effects.extend(browse::reconnected(state));
            effects.extend(logs::welcome(state));
            effects
        }
        Action::LibraryReply { id, result } => browse::reply(state, id, result),
        Action::DevicesReply { id, result } => {
            browse::devices_reply(state, id, result);
            Vec::new()
        }
        Action::LogsReply { id, result } => {
            logs::reply(state, id, result);
            Vec::new()
        }
        Action::Clock(now) => logs::clock(state, now),
        Action::OwnLogs(lines) => {
            logs::own_lines(state, lines);
            Vec::new()
        }
        Action::Resize { list_height } => {
            state.list_height = list_height.max(1);
            Vec::new()
        }
        Action::Reply(result) => {
            if let Err(message) = result {
                state.message = Some(message);
            }
            Vec::new()
        }
        Action::Disconnected { shut_down } => {
            if !matches!(state.connection, Connection::Refused(_)) {
                state.connection = Connection::Disconnected { shut_down };
            }
            browse::disconnected(state);
            Vec::new()
        }
        Action::Refused(message) => {
            state.connection = Connection::Refused(message);
            browse::disconnected(state);
            Vec::new()
        }
        Action::Player(event) => match event {
            protocol::Event::LoginRequired => {
                state.login_required = true;
                Vec::new()
            }
            protocol::Event::LoginRestored => {
                state.login_required = false;
                Vec::new()
            }
            protocol::Event::ShuttingDown => Vec::new(),
            protocol::Event::Player(snapshot) => apply_snapshot(state, snapshot),
            protocol::Event::Position { entry, position } => {
                if state.player.as_ref().and_then(|p| p.current) == Some(entry) {
                    state.position = position;
                }
                Vec::new()
            }
        },
    }
}

/// Takes the snapshot as is; keeps the cursor on its entry (clamped to
/// the same index when it was removed). The effects are the lyrics page's
/// request for a new current track (spec 0021).
fn apply_snapshot(state: &mut State, snapshot: PlayerSnapshot) -> Vec<Effect> {
    let old = state.player.take();
    let old_queue = old.as_ref().map_or(&[][..], |p| p.queue.as_slice());
    let ids: Vec<EntryId> = snapshot.queue.iter().map(|e| e.id).collect();

    state.cursor = match state.cursor {
        _ if ids.is_empty() => None,
        Some(id) if ids.contains(&id) => Some(id),
        Some(id) => {
            let index = old_queue.iter().position(|e| e.id == id).unwrap_or(0);
            Some(ids[index.min(ids.len() - 1)])
        }
        None => snapshot.current.or(ids.first().copied()),
    };
    let old_current = old.as_ref().and_then(|p| p.current);
    if snapshot.current.is_some() && snapshot.current != old_current {
        state.anchor = snapshot.current;
    }

    // The client's message gives way to a newer one from the player, and
    // to a track start.
    let old_message = old.as_ref().and_then(|p| p.message.as_deref());
    let newer_message = snapshot.message.is_some() && snapshot.message.as_deref() != old_message;
    let was_running = old.as_ref().is_some_and(|p| {
        matches!(
            p.state,
            PlaybackState::Playing | PlaybackState::Buffering | PlaybackState::Paused
        ) && p.current == snapshot.current
    });
    let started = snapshot.state == PlaybackState::Playing && !was_running;
    if newer_message || started {
        state.message = None;
    }

    state.position = snapshot.position;
    state.player = Some(snapshot);
    // Entries that left the queue leave its selection (spec 0020).
    select::prune_queue(state);
    // A filtered queue: the cursor keeps its entry while it matches, else
    // goes to the first match (spec 0012).
    if filter::queue_filter(state).active() {
        filter::keep_queue_cursor(state);
    }
    lyrics::follow_track(state)
}

#[cfg(test)]
mod tests;
