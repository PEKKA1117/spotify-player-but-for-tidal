//! Pure UI model: key -> `Action` -> new `State` plus `Effect`s, no I/O.
//!
//! Spec 0004 "TUI": the client keeps the player's latest snapshot as sent
//! (it never reorders the queue), a queue cursor addressed by entry ID, key
//! sequences (`g g`) and the open prompt. Everything it asks of the player
//! leaves as [`Effect::Send`]; expanding a pasted item is
//! [`Effect::Expand`], run by the caller off the UI thread, whose result
//! comes back as [`Action::Expanded`].

use std::time::Duration;

use crate::item::Item;
use crate::protocol::{self, Command, InsertAt, PlayerSnapshot, QueueEntry};
use crate::track::{EntryId, Track};

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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// A printable character, upper case included (`G`, `O`, `A`).
    Char(char),
    /// A character pressed with Control (`C-s` is `Ctrl('s')`).
    Ctrl(char),
    Enter,
    Esc,
    Backspace,
    Up,
    Down,
}

/// The open prompt (`o` / `O`): what has been typed and where it adds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    /// `End` for `o` (add to queue), `Next` for `O` (play next).
    pub at: InsertAt,
    pub text: String,
}

/// The whole UI state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State {
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
    /// A message of the client's own (an invalid item, a failed fetch, the
    /// startup expansion); shown instead of the player's while set.
    pub message: Option<String>,
    /// `g` was pressed: a second `g` moves to the top.
    pub pending_g: bool,
    /// Tracks were added to start the first of them: the entry IDs the
    /// queue held when `AddToQueue` was sent. The first entry of a later
    /// snapshot not among them is played.
    pub start_added: Option<Vec<EntryId>>,
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

    /// The message to show in the playback window: the client's own, else
    /// the player's.
    pub fn message(&self) -> Option<&str> {
        self.message
            .as_deref()
            .or_else(|| self.player.as_ref()?.message.as_deref())
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
    /// The result of an [`Effect::Expand`]: the item's tracks, or the
    /// message saying why there are none.
    Expanded {
        at: InsertAt,
        result: Result<Vec<Track>, String>,
    },
}

/// A side effect the caller must perform on behalf of the model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Leave the application.
    Quit,
    /// Send a command to the player.
    Send(Command),
    /// Expand `item` into tracks (as on the command line) and answer with
    /// [`Action::Expanded`].
    Expand { item: Item, at: InsertAt },
}

/// Applies `action` to `state` and returns the effects the caller must run.
pub fn update(state: &mut State, action: Action) -> Vec<Effect> {
    match action {
        Action::Quit => vec![Effect::Quit],
        Action::Tick | Action::Paste(_) | Action::Expanded { .. } => Vec::new(),
        Action::Key(key) => match key {
            Key::Char('q') | Key::Esc => vec![Effect::Quit],
            Key::Char('g') => {
                state.cursor = state.queue().first().map(|e| e.id);
                Vec::new()
            }
            Key::Char('G') => {
                state.cursor = state.queue().last().map(|e| e.id);
                Vec::new()
            }
            Key::Char('j') | Key::Char('k') => {
                let ids: Vec<EntryId> = state.queue().iter().map(|e| e.id).collect();
                if let Some(i) = ids.iter().position(|id| Some(*id) == state.cursor) {
                    let i = if key == Key::Char('j') {
                        (i + 1).min(ids.len() - 1)
                    } else {
                        i.saturating_sub(1)
                    };
                    state.cursor = Some(ids[i]);
                }
                Vec::new()
            }
            _ => Vec::new(),
        },
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
            protocol::Event::Player(mut snapshot) => {
                snapshot.queue.sort_by_key(|e| e.id);
                if state.cursor.is_none() {
                    state.cursor = snapshot.queue.first().map(|e| e.id);
                }
                state.player = Some(snapshot);
                Vec::new()
            }
            protocol::Event::Position { .. } => Vec::new(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{PlaybackState, RepeatMode};
    use crate::track::TrackId;

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
            artists: vec!["Artist".into()],
            album: Some("Album".into()),
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
            (Esc, vec![Effect::Quit], 3, vec![Effect::Quit]),
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

    /// AC28: the open prompt (state, mode, editing, effects), then the
    /// expansion's answer: `AddToQueue`, and `PlayEntry` of the first added
    /// entry when nothing was current or the queue was empty.
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
        let cases: Vec<(Vec<Action>, Option<Prompt>, Vec<Effect>, Option<&str>)> = vec![
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
                vec![Effect::Expand {
                    item: Item::Track(TrackId(123)),
                    at: InsertAt::End,
                }],
                None,
            ),
            (
                vec![k(Char('O')), Action::Paste(album.into()), k(Enter)],
                None,
                vec![Effect::Expand {
                    item: Item::Album(10),
                    at: InsertAt::Next,
                }],
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

        // The expansion's answer.
        let tracks = vec![track(500), track(501)];
        let expanded = |at, result| Action::Expanded { at, result };
        let add = |at| {
            send(Command::AddToQueue {
                tracks: tracks.clone(),
                at,
            })
        };
        let snap = |state: &mut State, s: PlayerSnapshot| {
            update(state, Action::Player(protocol::Event::Player(s)))
        };
        // (queue before, current, state, at, queue after, plays)
        let rows: Vec<(
            &[u64],
            Option<u64>,
            PlaybackState,
            InsertAt,
            &[u64],
            Option<u64>,
        )> = vec![
            // Empty queue: the first added entry plays.
            (
                &[],
                None,
                PlaybackState::Stopped,
                InsertAt::End,
                &[20, 21],
                Some(20),
            ),
            (
                &[],
                None,
                PlaybackState::Stopped,
                InsertAt::Next,
                &[20, 21],
                Some(20),
            ),
            // Stopped with nothing current.
            (
                &[1],
                None,
                PlaybackState::Stopped,
                InsertAt::End,
                &[1, 20, 21],
                Some(20),
            ),
            // Something current: added only.
            (
                &[1, 2],
                Some(1),
                PlaybackState::Playing,
                InsertAt::End,
                &[1, 2, 20, 21],
                None,
            ),
            (
                &[1, 2],
                Some(1),
                PlaybackState::Playing,
                InsertAt::Next,
                &[1, 20, 21, 2],
                None,
            ),
            (
                &[1, 2],
                Some(2),
                PlaybackState::Stopped,
                InsertAt::End,
                &[1, 2, 20, 21],
                None,
            ),
        ];
        for (before, current, playback, at, after, plays) in rows {
            let mut state = with(snapshot(before, current, playback));
            let got = update(&mut state, expanded(at, Ok(tracks.clone())));
            assert_eq!(got, add(at), "{before:?} {at:?}");
            // An unrelated snapshot (no new entry) starts nothing.
            assert_eq!(
                snap(&mut state, snapshot(before, current, playback)),
                vec![]
            );
            let got = snap(&mut state, snapshot(after, current, playback));
            let want = plays.map_or_else(Vec::new, |id| send(Command::PlayEntry(EntryId(id))));
            assert_eq!(got, want, "{before:?} {at:?}");
            // Once only.
            assert_eq!(snap(&mut state, snapshot(after, current, playback)), vec![]);
        }

        // A failed expansion sets the message and sends nothing.
        let mut state = with(snapshot(&[1], Some(1), PlaybackState::Playing));
        let got = update(
            &mut state,
            expanded(InsertAt::End, Err("Album 1 was not found".into())),
        );
        assert_eq!(got, vec![]);
        assert_eq!(state.message(), Some("Album 1 was not found"));
        // A new track start clears the client's message.
        let mut playing = snapshot(&[1, 2], Some(2), PlaybackState::Playing);
        snap(&mut state, playing.clone());
        assert_eq!(state.message(), None);
        // The player's message shows when the client has none.
        playing.message = Some("Track 102 is not available in NO".into());
        snap(&mut state, playing);
        assert_eq!(state.message(), Some("Track 102 is not available in NO"));
    }
}
