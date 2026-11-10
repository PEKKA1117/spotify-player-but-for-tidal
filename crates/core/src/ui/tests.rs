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
            cover: None,
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
        device: "default".into(),
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
