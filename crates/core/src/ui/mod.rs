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

mod browse;
mod dispatch;
pub mod help;
pub mod keymap;
pub mod page;
pub mod popup;
pub mod search;

use std::time::Duration;

use crate::library::{LibraryRequest, LibraryResponse};
use crate::protocol::{self, Command, InsertAt, PlaybackState, PlayerSnapshot, QueueEntry};
use crate::track::EntryId;

pub use browse::{PLAYLIST_CHANGED, Purpose, WholeList, WholeListSource, Write};
pub use help::{Help, HelpRow, HelpSection, help, locate, no_match, visible};
pub use keymap::{BaseKey, Keymap};
pub use page::{
    DEFAULT_PAGE_SIZE, Header, Load, MAX_HISTORY, MAX_WHOLE_LIST, Page, PageKind, ROLE_CATEGORIES,
    Row, Rows, Window, WindowKind, clock, group, largest_page,
};
pub use popup::{Confirmed, MenuAction, NEW_PLAYLIST, PLAYLIST_NAME, Popup, TrackSource};
pub use search::{DEFAULT_SEARCH_PAGE_SIZE, MAX_QUERY, Search, SearchFocus};

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
    /// The bindings keys are looked up in (spec 0008); the defaults until
    /// [`apply_keymap`].
    pub keymap: Keymap,
    /// The keys of a sequence being collected (spec 0008 "Sequences":
    /// they start a binding and are none yet).
    pub pending: Vec<Key>,
    /// Whether a sequence is being collected (with the defaults: `g` was
    /// pressed); always `!pending.is_empty()` (0004 AC21 reads it).
    pub pending_g: bool,
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
            keymap: Keymap::default(),
            pending: Vec::new(),
            pending_g: false,
            history: vec![Page::new(PageKind::Queue)],
            popup: None,
            help: None,
            whole_list: None,
            writes: Vec::new(),
            page_size: DEFAULT_PAGE_SIZE,
            search_page_size: DEFAULT_SEARCH_PAGE_SIZE,
            list_height: 20,
            next_request: 1,
            start_pending: false,
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
/// (spec 0008 decision 3), if any, becomes the message.
pub fn apply_keymap(state: &mut State, keymap: Keymap) {
    if let Some(notice) = keymap.notice() {
        state.message = Some(notice);
    }
    state.keymap = keymap;
    dispatch::set_pending(state, Vec::new());
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
                effects.retain(|e| !matches!(e, Effect::Send(_) | Effect::Library { .. }));
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
            } else if let Some(Popup::NewPlaylist { name, .. }) = state.popup.as_mut() {
                name.extend(text);
            } else if state.popup.is_none() {
                search::paste(state, &text.collect::<String>());
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
            apply_snapshot(state, snapshot);
            browse::reconnected(state)
        }
        Action::LibraryReply { id, result } => browse::reply(state, id, result),
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
            protocol::Event::Player(snapshot) => {
                apply_snapshot(state, snapshot);
                Vec::new()
            }
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
/// the same index when it was removed).
fn apply_snapshot(state: &mut State, snapshot: PlayerSnapshot) {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::Item;
    use crate::protocol::RepeatMode;
    use crate::track::{AlbumRef, ArtistRef, Track, TrackId};

    #[test]
    fn ac5_quit_emits_quit() {
        assert_eq!(
            update(&mut State::default(), Action::Quit),
            vec![Effect::Quit]
        );
    }

    #[test]
    fn ac5_other_actions_emit_nothing() {
        // 0004 adds keys; on the default state (empty queue, prompt closed)
        // an unmapped key, a playback key and a paste do nothing.
        let others = [
            Action::Tick,
            Action::Player(protocol::Event::ShuttingDown),
            Action::Player(protocol::Event::LoginRestored),
            Action::Key(Key::Char('x')),
            Action::Key(Key::Char(' ')),
            Action::Paste("123".into()),
        ];
        for action in others {
            let mut state = State::default();
            assert_eq!(update(&mut state, action.clone()), vec![], "{action:?}");
            assert_eq!(state, State::default(), "{action:?}");
        }
    }

    #[test]
    fn ac14_login_status() {
        // Table-driven test: (start_state, event, expected_login_required, expected_effects)
        let cases = vec![
            (
                State::default(),
                protocol::Event::LoginRequired,
                true,
                vec![],
            ),
            (
                State {
                    login_required: true,
                    ..State::default()
                },
                protocol::Event::LoginRestored,
                false,
                vec![],
            ),
            (
                State::default(),
                protocol::Event::LoginRestored,
                false,
                vec![],
            ),
        ];

        for (mut state, event, expected_login_required, expected_effects) in cases {
            let effects = update(&mut state, Action::Player(event.clone()));
            assert_eq!(
                state.login_required, expected_login_required,
                "login_required mismatch for {:?}",
                event
            );
            assert_eq!(
                effects, expected_effects,
                "effects mismatch for {:?}",
                event
            );
        }
    }

    // --- spec 0004 -------------------------------------------------------------

    fn track(id: u64) -> Track {
        Track {
            id: TrackId(id),
            title: format!("Title {id}"),
            version: None,
            artists: vec![ArtistRef {
                id: 1,
                name: "Artist".into(),
            }],
            album: Some(AlbumRef {
                id: 1,
                title: "Album".into(),
            }),
            duration: Some(Duration::from_secs(200)),
            streamable: true,
        }
    }

    /// A snapshot whose queue holds entries `ids` (track = entry ID + 100).
    fn snapshot(ids: &[u64], current: Option<u64>, state: PlaybackState) -> PlayerSnapshot {
        PlayerSnapshot {
            queue: ids
                .iter()
                .map(|id| QueueEntry {
                    id: EntryId(*id),
                    track: track(id + 100),
                    suggested: false,
                })
                .collect(),
            current: current.map(EntryId),
            state,
            position: Duration::ZERO,
            shuffle: false,
            repeat: RepeatMode::Off,
            autoplay: false,
            volume: 100,
            muted: false,
            now_playing: None,
            message: None,
        }
    }

    fn with(snapshot: PlayerSnapshot) -> State {
        let mut state = State::default();
        update(
            &mut state,
            Action::Player(protocol::Event::Player(snapshot)),
        );
        state
    }

    fn press(state: &mut State, keys: &[Key]) -> Vec<Effect> {
        keys.iter()
            .flat_map(|k| update(state, Action::Key(*k)))
            .collect()
    }

    fn typed(text: &str) -> Vec<Key> {
        text.chars().map(Key::Char).collect()
    }

    fn send(command: Command) -> Vec<Effect> {
        vec![Effect::Send(command)]
    }

    /// AC20: each key under "Keys" maps to its command (or cursor move);
    /// `Enter` plays the entry under the cursor by ID; on an empty queue
    /// only the volume and mode keys send anything.
    #[test]
    fn ac20_keys_to_commands() {
        use Key::{Char, Ctrl, Down, Enter, Esc, Up};
        // Entry IDs differ from indices; the cursor starts on the current.
        let queue = || with(snapshot(&[7, 3, 9], Some(3), PlaybackState::Playing));
        // (key, effects with a queue, cursor after it, effects on an empty queue)
        let cases: Vec<(Key, Vec<Effect>, u64, Vec<Effect>)> = vec![
            (Char(' '), send(Command::TogglePause), 3, vec![]),
            (Char('n'), send(Command::Next), 3, vec![]),
            (Char('p'), send(Command::Previous), 3, vec![]),
            (Char('>'), send(Command::SeekBy(5000)), 3, vec![]),
            (Char('<'), send(Command::SeekBy(-5000)), 3, vec![]),
            (Char('^'), send(Command::SeekTo(Duration::ZERO)), 3, vec![]),
            (
                Ctrl('s'),
                send(Command::ToggleShuffle),
                3,
                send(Command::ToggleShuffle),
            ),
            (
                Ctrl('r'),
                send(Command::CycleRepeat),
                3,
                send(Command::CycleRepeat),
            ),
            (
                Char('A'),
                send(Command::ToggleAutoplay),
                3,
                send(Command::ToggleAutoplay),
            ),
            (
                Char('+'),
                send(Command::ChangeVolume(5)),
                3,
                send(Command::ChangeVolume(5)),
            ),
            (
                Char('-'),
                send(Command::ChangeVolume(-5)),
                3,
                send(Command::ChangeVolume(-5)),
            ),
            (
                Char('_'),
                send(Command::ToggleMute),
                3,
                send(Command::ToggleMute),
            ),
            (Char('j'), vec![], 9, vec![]),
            (Down, vec![], 9, vec![]),
            (Char('k'), vec![], 7, vec![]),
            (Up, vec![], 7, vec![]),
            (Char('G'), vec![], 9, vec![]),
            (Enter, send(Command::PlayEntry(EntryId(3))), 3, vec![]),
            (Char('q'), vec![Effect::Quit], 3, vec![Effect::Quit]),
            // Spec 0006 AC15: `Esc` no longer quits.
            (Esc, vec![], 3, vec![]),
            (Ctrl('c'), vec![Effect::Quit], 3, vec![Effect::Quit]),
        ];
        for (key, effects, cursor, empty_effects) in cases {
            let mut state = queue();
            assert_eq!(press(&mut state, &[key]), effects, "{key:?}");
            assert_eq!(state.cursor, Some(EntryId(cursor)), "{key:?}: cursor");

            let mut empty = with(snapshot(&[], None, PlaybackState::Stopped));
            assert_eq!(press(&mut empty, &[key]), empty_effects, "{key:?}: empty");
            let mut none = State::default();
            assert_eq!(
                press(&mut none, &[key]),
                empty_effects,
                "{key:?}: no snapshot"
            );
        }

        // `Enter` sends the ID of the entry under the cursor, not its index.
        let mut state = queue();
        assert_eq!(
            press(&mut state, &[Char('j'), Enter]),
            send(Command::PlayEntry(EntryId(9)))
        );
        assert_eq!(
            press(&mut state, &[Char('k'), Char('k'), Enter]),
            send(Command::PlayEntry(EntryId(7)))
        );
        // The cursor stops at both ends.
        assert_eq!(press(&mut state, &[Char('k')]), vec![]);
        assert_eq!(state.cursor, Some(EntryId(7)));
        press(&mut state, &[Char('G'), Char('j')]);
        assert_eq!(state.cursor, Some(EntryId(9)));
        // A cursor move keeps that entry in view.
        assert_eq!(state.anchor, Some(EntryId(9)));
    }

    /// AC21: `g g` moves to the top; `g` then another key does that key.
    #[test]
    fn ac21_key_sequence() {
        use Key::Char;
        let mut state = with(snapshot(&[2, 4, 7, 9], Some(7), PlaybackState::Playing));
        press(&mut state, &[Char('G')]);
        assert_eq!(state.cursor, Some(EntryId(9)));

        // One `g` alone does nothing yet.
        assert_eq!(press(&mut state, &[Char('g')]), vec![]);
        assert_eq!(state.cursor, Some(EntryId(9)), "g alone moved the cursor");
        // The second one moves to the top.
        assert_eq!(press(&mut state, &[Char('g')]), vec![]);
        assert_eq!(state.cursor, Some(EntryId(2)));
        assert!(!state.pending_g);

        // `g` then `j`: down by one, from the top.
        assert_eq!(press(&mut state, &[Char('g'), Char('j')]), vec![]);
        assert_eq!(state.cursor, Some(EntryId(4)));
        // `g` then `Space`: play/pause, and the cursor stays.
        assert_eq!(
            press(&mut state, &[Char('g'), Char(' ')]),
            send(Command::TogglePause)
        );
        assert_eq!(state.cursor, Some(EntryId(4)));
        // `g` `j` `g` `g`: down, then top.
        press(&mut state, &[Char('g'), Char('j'), Char('g'), Char('g')]);
        assert_eq!(state.cursor, Some(EntryId(2)));
    }

    /// AC22: the snapshot is taken as is (order, current); a position for
    /// another entry is ignored; the cursor follows its entry ID and is
    /// clamped when that entry is removed.
    #[test]
    fn ac22_snapshot_applied_as_is() {
        let ids = |state: &State| state.queue().iter().map(|e| e.id.0).collect::<Vec<_>>();
        // A shuffled play order is displayed exactly as sent.
        let mut state = with(snapshot(&[5, 2, 9, 1], Some(9), PlaybackState::Playing));
        assert_eq!(ids(&state), vec![5, 2, 9, 1]);
        assert_eq!(state.current().map(|e| e.id), Some(EntryId(9)));
        assert_eq!(state.cursor, Some(EntryId(9)), "cursor starts on current");

        // Positions: the current entry's are taken, any other's ignored.
        let position = |entry: u64, secs: u64| {
            Action::Player(protocol::Event::Position {
                entry: EntryId(entry),
                position: Duration::from_secs(secs),
            })
        };
        assert_eq!(update(&mut state, position(9, 42)), vec![]);
        assert_eq!(state.position, Duration::from_secs(42));
        assert_eq!(update(&mut state, position(5, 7)), vec![]);
        assert_eq!(state.position, Duration::from_secs(42), "stale position");

        let apply = |state: &mut State, ids: &[u64], current: Option<u64>| {
            update(
                state,
                Action::Player(protocol::Event::Player(snapshot(
                    ids,
                    current,
                    PlaybackState::Playing,
                ))),
            )
        };
        // Reordered (shuffle toggled): the cursor stays on entry 9.
        assert_eq!(apply(&mut state, &[9, 1, 5, 2], Some(9)), vec![]);
        assert_eq!(ids(&state), vec![9, 1, 5, 2]);
        assert_eq!(state.cursor, Some(EntryId(9)));
        // Cursor on entry 5 (index 2); 5 is removed: index 2 again.
        press(&mut state, &[Key::Char('j'), Key::Char('j')]);
        assert_eq!(state.cursor, Some(EntryId(5)));
        apply(&mut state, &[9, 1, 2, 8], Some(9));
        assert_eq!(state.cursor, Some(EntryId(2)));
        // The cursor's entry was last and is removed: the new last.
        press(&mut state, &[Key::Char('G')]);
        apply(&mut state, &[9, 1, 2], Some(9));
        assert_eq!(state.cursor, Some(EntryId(2)));
        // An empty queue: no cursor.
        apply(&mut state, &[], None);
        assert_eq!(state.cursor, None);

        // A new current entry is kept in view; a snapshot replaces the
        // position.
        let mut state = with(snapshot(&[1, 2, 3], Some(1), PlaybackState::Playing));
        assert_eq!(state.anchor, Some(EntryId(1)));
        let mut next = snapshot(&[1, 2, 3], Some(3), PlaybackState::Loading);
        next.position = Duration::from_secs(3);
        update(&mut state, Action::Player(protocol::Event::Player(next)));
        assert_eq!(state.anchor, Some(EntryId(3)));
        assert_eq!(state.position, Duration::from_secs(3));
        assert_eq!(
            state.cursor,
            Some(EntryId(1)),
            "the cursor moves on its own"
        );
    }

    /// AC25 (TUI half): `+`/`-` and `>`/`<` send the configured steps.
    #[test]
    fn ac25_steps_from_config() {
        use Key::Char;
        for (volume, seek_ms) in [(1u8, 1_000i64), (10, 30_000), (25, 600_000)] {
            let mut state = State::new(Steps {
                volume,
                seek: Duration::from_millis(seek_ms as u64),
            });
            update(
                &mut state,
                Action::Player(protocol::Event::Player(snapshot(
                    &[1],
                    Some(1),
                    PlaybackState::Playing,
                ))),
            );
            let v = volume as i8;
            for (key, command) in [
                (Char('+'), Command::ChangeVolume(v)),
                (Char('-'), Command::ChangeVolume(-v)),
                (Char('>'), Command::SeekBy(seek_ms)),
                (Char('<'), Command::SeekBy(-seek_ms)),
            ] {
                assert_eq!(press(&mut state, &[key]), send(command), "{key:?}");
            }
        }
    }

    /// AC28: the open prompt (state, mode, editing, effects); `Enter` sends
    /// `Open` (spec 0005 "Opening items in the player": the player expands
    /// the item and starts it when idle, 0005 AC7); the reply's error is
    /// the message, and nothing changes locally.
    #[test]
    fn ac28_open_prompt() {
        use Key::{Backspace, Char, Ctrl, Enter, Esc};
        let album = "https://tidal.com/browse/album/10?u";
        let artist = "https://tidal.com/browse/artist/1";
        let open = |at, text: &str| {
            Some(Prompt {
                at,
                text: text.into(),
            })
        };
        let k = |key| Action::Key(key);
        let keys = |text: &str| typed(text).into_iter().map(Action::Key).collect::<Vec<_>>();
        let cat = |parts: Vec<Vec<Action>>| parts.concat();
        // (actions, prompt after, effects, message after)
        type Case<'a> = (Vec<Action>, Option<Prompt>, Vec<Effect>, Option<&'a str>);
        let cases: Vec<Case> = vec![
            (vec![k(Char('o'))], open(InsertAt::End, ""), vec![], None),
            (vec![k(Char('O'))], open(InsertAt::Next, ""), vec![], None),
            (
                cat(vec![vec![k(Char('o'))], keys("123")]),
                open(InsertAt::End, "123"),
                vec![],
                None,
            ),
            (
                vec![k(Char('o')), Action::Paste(format!("{album}\n"))],
                open(InsertAt::End, album),
                vec![],
                None,
            ),
            (
                cat(vec![vec![k(Char('o'))], keys("12"), vec![k(Backspace)]]),
                open(InsertAt::End, "1"),
                vec![],
                None,
            ),
            (
                cat(vec![vec![k(Char('o'))], keys("12"), vec![k(Esc)]]),
                None,
                vec![],
                None,
            ),
            // While open, no key reaches the player: they type.
            (
                cat(vec![
                    vec![k(Char('O'))],
                    keys(" qnGg+"),
                    vec![k(Ctrl('s')), k(Key::Down)],
                ]),
                open(InsertAt::Next, " qnGg+"),
                vec![],
                None,
            ),
            (
                cat(vec![vec![k(Char('o'))], keys("123"), vec![k(Enter)]]),
                None,
                send(Command::Open {
                    items: vec![Item::Track(TrackId(123))],
                    at: Some(InsertAt::End),
                }),
                None,
            ),
            (
                vec![k(Char('O')), Action::Paste(album.into()), k(Enter)],
                None,
                send(Command::Open {
                    items: vec![Item::Album(10)],
                    at: Some(InsertAt::Next),
                }),
                None,
            ),
            (
                vec![k(Char('o')), Action::Paste(artist.into()), k(Enter)],
                None,
                vec![],
                Some("Not a Tidal track, album or playlist: https://tidal.com/browse/artist/1"),
            ),
        ];
        for (actions, prompt, effects, message) in cases {
            let mut state = with(snapshot(&[1, 2], Some(1), PlaybackState::Playing));
            let before = state.cursor;
            let got: Vec<Effect> = actions
                .iter()
                .flat_map(|a| update(&mut state, a.clone()))
                .collect();
            assert_eq!(state.prompt, prompt, "{actions:?}");
            assert_eq!(got, effects, "{actions:?}");
            assert_eq!(state.message(), message, "{actions:?}");
            assert_eq!(state.cursor, before, "{actions:?}: cursor moved");
        }

        let snap = |state: &mut State, s: PlayerSnapshot| {
            update(state, Action::Player(protocol::Event::Player(s)))
        };
        // The player's answer: an error is the message; the queue is the
        // player's, untouched until its snapshot says otherwise.
        let mut state = with(snapshot(&[1], Some(1), PlaybackState::Playing));
        let before = state.clone();
        assert_eq!(update(&mut state, Action::Reply(Ok(()))), vec![]);
        assert_eq!(state, before, "an Ok reply changes nothing");
        let got = update(
            &mut state,
            Action::Reply(Err("Album 1 was not found".into())),
        );
        assert_eq!(got, vec![]);
        assert_eq!(state.message(), Some("Album 1 was not found"));
        assert_eq!(state.player, before.player, "the queue changed locally");
        // A new track start clears the client's message.
        let mut playing = snapshot(&[1, 2], Some(2), PlaybackState::Playing);
        snap(&mut state, playing.clone());
        assert_eq!(state.message(), None);
        // The player's message shows when the client has none.
        playing.message = Some("Track 102 is not available in NO".into());
        snap(&mut state, playing);
        assert_eq!(state.message(), Some("Track 102 is not available in NO"));
    }

    // --- spec 0005 ---------------------------------------------------------------

    /// AC12: `Welcome` replaces the snapshot and the login status;
    /// `Disconnected` keeps the snapshot and sets its message; while
    /// disconnected no key sends, cursor keys and `q` work; `q` never sends
    /// `Shutdown`; a refusal (version mismatch) sets its message and stops
    /// reconnecting.
    #[test]
    fn ac12_client_connection_states() {
        use Key::{Char, Ctrl, Down, Enter, Esc, Up};
        let ids = |state: &State| state.queue().iter().map(|e| e.id.0).collect::<Vec<_>>();
        let welcome = |ids: &[u64], current: u64, login_required| Action::Welcome {
            snapshot: snapshot(ids, Some(current), PlaybackState::Playing),
            login_required,
        };

        // `Welcome` replaces everything: queue, current, login status and
        // the client's own message.
        for login_required in [true, false] {
            let mut state = with(snapshot(&[1, 2], Some(1), PlaybackState::Playing));
            state.login_required = !login_required;
            state.message = Some("Album 1 was not found".into());
            assert_eq!(
                update(&mut state, welcome(&[7, 8, 9], 8, login_required)),
                vec![]
            );
            assert_eq!(ids(&state), vec![7, 8, 9]);
            assert_eq!(state.current().map(|e| e.id), Some(EntryId(8)));
            assert_eq!(state.login_required, login_required);
            assert_eq!(state.message(), None);
            assert_eq!(state.connection, Connection::Connected);
        }

        // Every key that sends a command while connected.
        let command_keys = [
            Char(' '),
            Char('n'),
            Char('p'),
            Char('>'),
            Char('<'),
            Char('^'),
            Ctrl('s'),
            Ctrl('r'),
            Char('A'),
            Char('+'),
            Char('-'),
            Char('_'),
            Enter,
        ];
        for key in command_keys {
            let mut state = with(snapshot(&[1, 2, 3], Some(2), PlaybackState::Playing));
            assert_ne!(press(&mut state, &[key]), vec![], "{key:?} sends nothing");
        }

        for (shut_down, message) in [(false, DISCONNECTED), (true, SHUT_DOWN)] {
            let mut state = with(snapshot(&[1, 2, 3], Some(2), PlaybackState::Playing));
            let before = state.player.clone();
            let got = update(&mut state, Action::Disconnected { shut_down });
            assert_eq!(got, vec![]);
            assert_eq!(state.player, before, "{message}: the snapshot went");
            assert_eq!(state.message(), Some(message));
            assert!(state.reconnecting(), "{message}");
            for key in command_keys {
                assert_eq!(press(&mut state, &[key]), vec![], "{message}: {key:?}");
            }
            // The open prompt sends nothing either.
            assert_eq!(press(&mut state, &[Char('o'), Char('1'), Enter]), vec![]);
            // Cursor keys work.
            for (keys, cursor) in [
                (vec![Char('j')], 3),
                (vec![Char('k')], 2),
                (vec![Down], 3),
                (vec![Up], 2),
                (vec![Char('G')], 3),
                (vec![Char('g'), Char('g')], 1),
            ] {
                assert_eq!(press(&mut state, &keys), vec![], "{keys:?}");
                assert_eq!(state.cursor, Some(EntryId(cursor)), "{message}: {keys:?}");
            }
            assert_eq!(state.player, before);
            // `q` quits, without a `Shutdown`; `Esc` does nothing (spec
            // 0006 AC15).
            assert_eq!(press(&mut state.clone(), &[Char('q')]), vec![Effect::Quit]);
            assert_eq!(press(&mut state.clone(), &[Esc]), vec![]);
            // Back: the next `Welcome` replaces everything and keys send.
            update(&mut state, welcome(&[4], 4, false));
            assert_eq!(ids(&state), vec![4]);
            assert_eq!(state.message(), None);
            assert!(!state.reconnecting());
            assert_eq!(press(&mut state, &[Char(' ')]), send(Command::TogglePause));
        }

        // Connected, `q` quits and sends nothing (a client detaches).
        let mut state = with(snapshot(&[1], Some(1), PlaybackState::Playing));
        assert_eq!(press(&mut state, &[Char('q')]), vec![Effect::Quit]);

        // A version mismatch: its message, no reconnecting, no commands.
        let mismatch = "The running player is tidal-player 0.0.1, this is 0.1.0: restart it";
        let mut state = with(snapshot(&[1], Some(1), PlaybackState::Playing));
        assert_eq!(update(&mut state, Action::Refused(mismatch.into())), vec![]);
        assert_eq!(state.message(), Some(mismatch));
        assert!(!state.reconnecting());
        assert_eq!(press(&mut state, &[Char(' ')]), vec![]);
        assert_eq!(state.connection, Connection::Refused(mismatch.into()));
    }
}
