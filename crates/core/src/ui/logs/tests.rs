//! Spec 0027 AC7–AC10: opening the logs page, asking for the player's
//! lines, the display state (merge, cap, following, scrolling, titles,
//! messages) and the filter.

use std::time::{Duration, SystemTime};

use crate::log::{LogBatch, LogLevel, LogLine};
use crate::protocol::{PlaybackState, PlayerSnapshot, QueueEntry, RepeatMode};
use crate::track::{AlbumRef, ArtistRef, EntryId, Track, TrackId};

use super::super::{Action, Connection, DISCONNECTED, Effect, Key, PageKind, State, update};
use super::{
    LOADING, LogsContent, MAX_LINES, NO_LINES, OFF, POLL, content, could_not_load, offset, shown,
    title,
};

// --- fixtures ----------------------------------------------------------------

const GO: [Key; 2] = [Key::Char('g'), Key::Char('o')];
const GG: [Key; 2] = [Key::Char('g'), Key::Char('g')];
const BACK: [Key; 1] = [Key::Backspace];

fn line(seq: u64, level: LogLevel, at_ms: u64, target: &str, text: &str) -> LogLine {
    LogLine {
        seq,
        at: SystemTime::UNIX_EPOCH + Duration::from_millis(at_ms),
        level,
        target: target.into(),
        text: text.into(),
    }
}

/// An `INFO` line `text` at `seq` seconds (so lines are in time order).
fn info(seq: u64, text: &str) -> LogLine {
    line(seq, LogLevel::Info, seq * 1000, "tidal_player::t", text)
}

/// Lines `from..=to`, the text `L<seq>`.
fn lines(from: u64, to: u64) -> Vec<LogLine> {
    (from..=to).map(|n| info(n, &format!("L{n}"))).collect()
}

fn batch(lines: Vec<LogLine>) -> Result<LogBatch, String> {
    Ok(LogBatch { lines, off: false })
}

fn press(state: &mut State, keys: &[Key]) -> Vec<Effect> {
    keys.iter()
        .flat_map(|k| update(state, Action::Key(*k)))
        .collect()
}

fn type_text(state: &mut State, text: &str) {
    press(state, &text.chars().map(Key::Char).collect::<Vec<_>>());
}

fn clock(state: &mut State, ms: u64) -> Vec<Effect> {
    update(state, Action::Clock(Duration::from_millis(ms)))
}

fn reply(state: &mut State, id: u64, result: Result<LogBatch, String>) -> Vec<Effect> {
    update(state, Action::LogsReply { id, result })
}

fn own(state: &mut State, lines: Vec<LogLine>) {
    update(state, Action::OwnLogs(lines));
}

fn welcome() -> Action {
    Action::Welcome {
        snapshot: snapshot(),
        login_required: false,
    }
}

fn snapshot() -> PlayerSnapshot {
    let track = Track {
        id: TrackId(1),
        title: "T".into(),
        version: None,
        artists: vec![ArtistRef {
            id: 2,
            name: "A".into(),
        }],
        album: Some(AlbumRef {
            id: 3,
            title: "Al".into(),
            cover: None,
        }),
        duration: Some(Duration::from_secs(100)),
        streamable: true,
    };
    PlayerSnapshot {
        queue: vec![QueueEntry {
            id: EntryId(1),
            track,
            suggested: false,
        }],
        current: Some(EntryId(1)),
        state: PlaybackState::Paused,
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

/// The request in `effects`: `(id, after)`; panics unless it is the one
/// effect.
fn the_request(effects: &[Effect]) -> (u64, Option<u64>) {
    match effects {
        [Effect::Logs { id, after }] => (*id, *after),
        other => panic!("expected one Logs request, got {other:?}"),
    }
}

fn no_request(effects: &[Effect], what: &str) {
    assert!(
        !effects.iter().any(|e| matches!(e, Effect::Logs { .. })),
        "{what}: {effects:?}"
    );
}

/// A connected state with the logs page open (a window of `height` rows)
/// and its first request's ID.
fn opened(height: usize) -> (State, u64) {
    let mut state = State::default();
    update(
        &mut state,
        Action::Resize {
            list_height: height,
        },
    );
    let (id, after) = the_request(&press(&mut state, &GO));
    assert_eq!(after, None);
    (state, id)
}

/// The logs page with `lines` shown (answered at 0 ms).
fn with_lines(height: usize, lines: Vec<LogLine>) -> State {
    let (mut state, id) = opened(height);
    clock(&mut state, 0);
    reply(&mut state, id, batch(lines));
    state
}

/// The drawn lines' texts without time, level and target.
fn texts(state: &State) -> Vec<String> {
    shown(state)
        .into_iter()
        .map(|d| {
            d.text
                .rsplit_once(": ")
                .map_or(d.text.clone(), |(_, t)| t.into())
        })
        .collect()
}

/// The texts of the rows a window of `height` rows draws.
fn window(state: &State) -> Vec<String> {
    let all = texts(state);
    let top = offset(state);
    all.into_iter().skip(top).take(state.list_height).collect()
}

fn message(state: &State) -> Option<String> {
    content(state).message()
}

// --- AC7 ---------------------------------------------------------------------

/// AC7: `g o` pushes the logs page and asks for every line; on top
/// already it does nothing; going back pops it.
#[test]
fn ac7_open_logs() {
    // From anywhere: the queue, another page.
    for before in [None, Some([Key::Char('g'), Key::Char('l')])] {
        let mut state = State::default();
        if let Some(keys) = before {
            press(&mut state, &keys);
        }
        let depth = state.history.len();
        let id = state.next_request;
        let effects = press(&mut state, &GO);
        assert_eq!(effects, vec![Effect::Logs { id, after: None }]);
        assert_eq!(state.page().kind, PageKind::Logs, "{before:?}");
        assert_eq!(state.history.len(), depth + 1);
        assert_eq!(state.next_request, id + 1);
        assert_eq!(title(&state), "Logs");

        // On top already: nothing, nothing asked.
        assert_eq!(press(&mut state, &GO), vec![]);
        assert_eq!(state.history.len(), depth + 1);
        assert_eq!(state.next_request, id + 1);

        // Back leaves it.
        press(&mut state, &BACK);
        assert_eq!(state.history.len(), depth);
        assert_ne!(state.page().kind, PageKind::Logs);
    }

    // Opened again later: it asks after the newest line it has.
    let mut state = with_lines(5, lines(1, 4));
    press(&mut state, &BACK);
    let (_, after) = the_request(&press(&mut state, &GO));
    assert_eq!(after, Some(4));
    assert_eq!(texts(&state), ["L1", "L2", "L3", "L4"]);

    // Disconnected: the page opens, nothing is asked.
    let mut state = State::default();
    update(&mut state, Action::Disconnected { shut_down: false });
    assert_eq!(press(&mut state, &GO), vec![]);
    assert_eq!(state.page().kind, PageKind::Logs);
}

// --- AC8 ---------------------------------------------------------------------

/// AC8: after an answer the next request is due one second later, carries
/// the newest `seq`, and is not sent while one is in flight.
#[test]
fn ac8_asking() {
    let (mut state, id) = opened(5);
    let second = POLL.as_millis() as u64;
    assert_eq!(second, 1000);

    // In flight: no request, however long it takes.
    for ms in [0, 500, 1000, 60_000] {
        no_request(&clock(&mut state, ms), "in flight");
    }

    // Answered at 60 s with lines 1..=3: due at 61 s, not before.
    clock(&mut state, 60_000);
    assert_eq!(reply(&mut state, id, batch(lines(1, 3))), vec![]);
    for ms in [60_000, 60_001, 60_999] {
        no_request(&clock(&mut state, ms), "not yet due");
    }
    let (id, after) = the_request(&clock(&mut state, 61_000));
    assert_eq!(after, Some(3));
    // In flight again.
    no_request(&clock(&mut state, 70_000), "in flight again");

    // An answer with another id is ignored: no lines, still in flight.
    reply(&mut state, id + 100, batch(lines(4, 9)));
    assert_eq!(texts(&state), ["L1", "L2", "L3"]);
    no_request(&clock(&mut state, 80_000), "still in flight");

    // An empty answer: the next request is due a second later, same `after`.
    clock(&mut state, 80_000);
    reply(&mut state, id, batch(vec![]));
    no_request(&clock(&mut state, 80_500), "not yet due");
    let (id, after) = the_request(&clock(&mut state, 81_000));
    assert_eq!(after, Some(3));

    // A failed answer keeps the lines and asks again a second later.
    clock(&mut state, 90_000);
    reply(&mut state, id, Err("boom".into()));
    assert_eq!(texts(&state), ["L1", "L2", "L3"]);
    no_request(&clock(&mut state, 90_999), "not yet due");
    let (id, after) = the_request(&clock(&mut state, 91_000));
    assert_eq!(after, Some(3));
    clock(&mut state, 91_000);
    reply(&mut state, id, batch(lines(4, 5)));
    assert_eq!(texts(&state), ["L1", "L2", "L3", "L4", "L5"]);

    // Not on top: nothing is asked, however late; coming back asks at once.
    press(&mut state, &[Key::Char('z')]);
    assert_ne!(state.page().kind, PageKind::Logs);
    for ms in [92_000, 95_000, 200_000] {
        no_request(&clock(&mut state, ms), "not on top");
    }
    let (id, after) = the_request(&press(&mut state, &BACK));
    assert_eq!(after, Some(5));
    // The one request that was in flight when the page was left is the
    // only one: leaving and coming back never makes two.
    clock(&mut state, 200_000);
    reply(&mut state, id, batch(vec![]));
    press(&mut state, &[Key::Char('z')]);
    assert_eq!(
        press(&mut state, &BACK),
        vec![Effect::Logs {
            id: id + 1,
            after: Some(5)
        }]
    );
    assert_eq!(press(&mut state, &[Key::Char('z')]), vec![]);
    assert_eq!(press(&mut state, &BACK), vec![], "one is in flight");
}

/// AC8: while disconnected nothing is asked; a `Welcome` clears the
/// player's lines (the TUI's own stay) and the next request has
/// `after: None`.
#[test]
fn ac8_asking_reconnect() {
    let mut state = with_lines(5, lines(1, 3));
    own(
        &mut state,
        vec![line(
            1,
            LogLevel::Warn,
            500,
            "tidal_player::tui::cover",
            "own",
        )],
    );
    assert_eq!(texts(&state), ["own", "L1", "L2", "L3"]);

    // A request in flight when the connection goes is dropped with it.
    let (id, _) = the_request(&clock(&mut state, 1000));
    let effects = update(&mut state, Action::Disconnected { shut_down: false });
    assert_eq!(effects, vec![]);
    no_request(&clock(&mut state, 5000), "disconnected");
    assert_eq!(
        state.connection,
        Connection::Disconnected { shut_down: false }
    );
    // The lines already shown stay.
    assert_eq!(texts(&state), ["own", "L1", "L2", "L3"]);
    // A late answer is dropped.
    reply(&mut state, id, batch(lines(4, 4)));
    assert_eq!(texts(&state), ["own", "L1", "L2", "L3"]);

    // Reconnected: the old player's lines go, the next request is at once.
    let effects = update(&mut state, welcome());
    let (id, after) = the_request(&effects);
    assert_eq!(after, None);
    assert_eq!(texts(&state), ["own"]);
    // The new player starts numbering again; its lines are not mistaken
    // for old ones.
    clock(&mut state, 6000);
    reply(&mut state, id, batch(lines(1, 2)));
    assert_eq!(texts(&state), ["own", "L1", "L2"]);
    let (_, after) = the_request(&clock(&mut state, 7000));
    assert_eq!(after, Some(2));

    // A `Welcome` while the page is not on top asks nothing; the page
    // asks with `after: None` when it comes back.
    let mut state = with_lines(5, lines(1, 3));
    press(&mut state, &[Key::Char('z')]);
    assert_eq!(update(&mut state, welcome()), vec![]);
    assert_eq!(
        press(&mut state, &BACK),
        vec![Effect::Logs { id: 2, after: None }]
    );
}

// --- AC9 ---------------------------------------------------------------------

/// AC9: the display state, in its parts.
#[test]
fn ac9_display_state() {
    ac9_display_state_merge_and_cap();
    ac9_display_state_scrolling();
    ac9_display_state_follow_and_hold();
    ac9_display_state_messages();
    ac9_display_state_row_keys_inert();
}

/// AC9: lines merged by time, the player's first on equal times, capped.
fn ac9_display_state_merge_and_cap() {
    let at = |seq, ms, text: &str| line(seq, LogLevel::Info, ms, "t", text);
    let player = || {
        vec![
            at(1, 1000, "P1"),
            at(2, 3000, "P2"),
            at(3, 3000, "P3"),
            at(4, 5000, "P4"),
        ]
    };
    let mine = || vec![at(1, 2000, "O1"), at(2, 3000, "O2"), at(3, 4000, "O3")];
    let want = ["P1", "O1", "P2", "P3", "O2", "O3", "P4"];

    // Whichever arrives first.
    let mut state = with_lines(5, player());
    own(&mut state, mine());
    assert_eq!(texts(&state), want, "own after the player's");
    let (mut state, id) = opened(5);
    own(&mut state, mine());
    reply(&mut state, id, batch(player()));
    assert_eq!(texts(&state), want, "own before the player's");

    // The TUI's lines come in pieces; ones it already gave are ignored.
    let (mut state, id) = opened(5);
    reply(&mut state, id, batch(player()));
    own(&mut state, mine()[..2].to_vec());
    own(&mut state, mine());
    assert_eq!(texts(&state), want, "own in pieces");

    // A clock that stepped back: the player's lines keep their order.
    let state = with_lines(
        5,
        vec![at(1, 5000, "A"), at(2, 1000, "B"), at(3, 9000, "C")],
    );
    assert_eq!(texts(&state), ["A", "B", "C"]);

    // The cap: the last 1000 lines are kept, the oldest dropped first.
    let state = with_lines(5, lines(1, 1200));
    let all = texts(&state);
    assert_eq!(all.len(), MAX_LINES);
    assert_eq!(all.first().map(String::as_str), Some("L201"));
    assert_eq!(all.last().map(String::as_str), Some("L1200"));
    // After merging: 800 of the player's and 400 of the TUI's.
    let mut state = with_lines(5, lines(1, 800));
    own(
        &mut state,
        (1..=400)
            .map(|n| line(n, LogLevel::Info, 800_000 + n, "t", &format!("O{n}")))
            .collect(),
    );
    let all = texts(&state);
    assert_eq!(all.len(), MAX_LINES);
    assert_eq!(all.first().map(String::as_str), Some("L201"));
    assert_eq!(all.last().map(String::as_str), Some("O400"));
}

/// AC9: the page follows at the bottom, each movement key scrolls by
/// rows clamped to the lines; away from the bottom it does not follow.
fn ac9_display_state_scrolling() {
    use Key::{Char, Ctrl, Down, PageDown, PageUp, Up};
    // 12 lines in a window of 5 rows: the bottom starts at line 8.
    let mut state = with_lines(5, lines(1, 12));
    assert_eq!(offset(&state), 7);
    assert_eq!(window(&state), ["L8", "L9", "L10", "L11", "L12"]);
    assert_eq!(title(&state), "Logs");

    // (keys, the offset after them, whether it follows)
    let steps: [(&[Key], usize, bool); 18] = [
        (&[Char('j')], 7, true),
        (&[Down], 7, true),
        (&[Char('k')], 6, false),
        (&[Up], 5, false),
        (&[Char('k')], 4, false),
        (&[Char('j')], 5, false),
        (&[Down], 6, false),
        // Back at the bottom: it follows again.
        (&[Char('j')], 7, true),
        (&[Ctrl('b')], 2, false),
        (&[PageUp], 0, false),
        (&[Char('k')], 0, false),
        (&[Ctrl('f')], 5, false),
        (&[PageDown], 7, true),
        (&[Char('k'), Char('k'), Char('k')], 4, false),
        (&GG, 0, false),
        (&[Char('G')], 7, true),
        (&GG, 0, false),
        (&[Char('G')], 7, true),
    ];
    for (keys, want, following) in steps {
        press(&mut state, keys);
        assert_eq!(offset(&state), want, "{keys:?}");
        let scrolled = title(&state).ends_with("scrolled");
        assert_eq!(scrolled, !following, "{keys:?}: {}", title(&state));
    }

    // Fewer lines than rows: nothing to scroll, it follows.
    let mut state = with_lines(5, lines(1, 3));
    for keys in [&[Char('k')][..], &GG, &[Ctrl('b')]] {
        press(&mut state, keys);
        assert_eq!(offset(&state), 0);
        assert_eq!(title(&state), "Logs");
    }
    assert_eq!(window(&state), ["L1", "L2", "L3"]);
}

/// AC9: following, new lines scroll the view; scrolled away, it holds its
/// rows as lines arrive and are dropped; `Esc` follows again.
fn ac9_display_state_follow_and_hold() {
    let mut state = with_lines(5, lines(1, 12));
    let (id, after) = the_request(&clock(&mut state, 1000));
    assert_eq!(after, Some(12));
    reply(&mut state, id, batch(lines(13, 14)));
    assert_eq!(
        window(&state),
        ["L10", "L11", "L12", "L13", "L14"],
        "follows"
    );

    // Scrolled to L6..L10: three more lines do not move it.
    press(&mut state, &GG);
    press(&mut state, &[Key::Char('j'); 5]);
    assert_eq!(window(&state), ["L6", "L7", "L8", "L9", "L10"]);
    assert!(title(&state).ends_with("scrolled"));
    let (id, _) = the_request(&clock(&mut state, 2000));
    reply(&mut state, id, batch(lines(15, 17)));
    assert_eq!(window(&state), ["L6", "L7", "L8", "L9", "L10"], "holds");
    assert_eq!(title(&state), "Logs · scrolled");

    // `Esc` follows again.
    press(&mut state, &[Key::Esc]);
    assert_eq!(window(&state), ["L13", "L14", "L15", "L16", "L17"]);
    assert_eq!(title(&state), "Logs");
    // `Esc` while following changes nothing.
    press(&mut state, &[Key::Esc]);
    assert_eq!(title(&state), "Logs");

    // A full buffer: lines are dropped as new ones arrive. A view at
    // line 11 holds it until its own top row is dropped.
    let mut state = with_lines(5, lines(1, 1000));
    press(&mut state, &GG);
    press(&mut state, &[Key::Char('j'); 10]);
    assert_eq!(window(&state)[0], "L11");
    let mut next = 1001;
    let mut push = |state: &mut State, n: u64, ms: u64| {
        let (id, _) = the_request(&clock(state, ms));
        reply(state, id, batch(lines(next, next + n - 1)));
        next += n;
    };
    // L1..L5 dropped: L11 is still its top row.
    push(&mut state, 5, 10_000);
    assert_eq!(window(&state)[0], "L11");
    assert_eq!(offset(&state), 5);
    // Dropped up to L10: still held.
    push(&mut state, 5, 20_000);
    assert_eq!(window(&state)[0], "L11");
    assert_eq!(offset(&state), 0);
    // Its own top row dropped: it moves to what is oldest.
    push(&mut state, 5, 30_000);
    assert_eq!(window(&state)[0], "L16");
    assert_eq!(offset(&state), 0);
    assert!(title(&state).ends_with("scrolled"));
}

/// AC9: the window's messages for each state, and the page's title row.
fn ac9_display_state_messages() {
    // Before the first answer.
    let (mut state, id) = opened(5);
    assert_eq!(content(&state), LogsContent::Loading);
    assert_eq!(message(&state).as_deref(), Some(LOADING));

    // A failure with nothing shown.
    clock(&mut state, 0);
    reply(&mut state, id, Err("the player said no".into()));
    assert_eq!(message(&state), Some(could_not_load("the player said no")));
    assert_eq!(
        message(&state).as_deref(),
        Some("Could not load the log: the player said no")
    );

    // An empty answer.
    let (id, _) = the_request(&clock(&mut state, 1000));
    reply(&mut state, id, batch(vec![]));
    assert_eq!(content(&state), LogsContent::NoLines);
    assert_eq!(message(&state).as_deref(), Some(NO_LINES));
    assert_eq!(NO_LINES, "No log lines yet");

    // Logging off.
    let (id, _) = the_request(&clock(&mut state, 2000));
    reply(
        &mut state,
        id,
        Ok(LogBatch {
            lines: vec![],
            off: true,
        }),
    );
    assert_eq!(content(&state), LogsContent::Off);
    assert_eq!(
        message(&state).as_deref(),
        Some("Logging is off: set log_level in app.toml")
    );
    assert_eq!(OFF, "Logging is off: set log_level in app.toml");
    // ... unless the TUI has lines of its own.
    own(
        &mut state,
        vec![line(1, LogLevel::Warn, 1, "tidal_player::tui", "mine")],
    );
    assert_eq!(content(&state), LogsContent::Lines);
    assert_eq!(texts(&state), ["mine"]);

    // Lost connection with nothing shown: 0006's message as is.
    let mut state = State::default();
    press(&mut state, &GO);
    update(&mut state, Action::Disconnected { shut_down: false });
    assert_eq!(message(&state).as_deref(), Some(DISCONNECTED));

    // With lines shown the lines stay, whatever happens.
    let mut state = with_lines(5, lines(1, 3));
    let (id, _) = the_request(&clock(&mut state, 1000));
    reply(&mut state, id, Err("boom".into()));
    assert_eq!(content(&state), LogsContent::Lines);
    assert_eq!(texts(&state), ["L1", "L2", "L3"]);
    update(&mut state, Action::Disconnected { shut_down: true });
    assert_eq!(content(&state), LogsContent::Lines);
    assert_eq!(texts(&state), ["L1", "L2", "L3"]);
}

/// AC9: keys that act on rows do nothing on the logs page.
fn ac9_display_state_row_keys_inert() {
    use Key::{Char, Ctrl, Enter};
    let mut state = with_lines(5, lines(1, 12));
    // Scrolled, so that a change to the view would show too.
    press(&mut state, &[Char('k'), Char('k')]);
    let before = state.clone();
    for keys in [
        &[Enter][..],
        &[Char('Z')],
        &[Ctrl('z')],
        &[Char('g'), Char('a')],
        &[Char('g'), Ctrl(' ')],
        &[Char('s'), Char('c')],
        &[Char('s'), Char('a')],
        &[Char('s'), Char('m')],
        &[Char('d'), Char('d')],
        &[Char('d'), Char('s')],
        &[Char('r')],
        &[Char('f')],
        &[Key::Tab],
        &[Char('['), Char(']')],
    ] {
        let effects = press(&mut state, keys);
        assert_eq!(effects, vec![], "{keys:?}");
        assert_eq!(state, before, "{keys:?}");
    }
}

// --- AC10 --------------------------------------------------------------------

fn filter_lines() -> Vec<LogLine> {
    vec![
        line(
            1,
            LogLevel::Info,
            1000,
            "tidal_player::player::reserve",
            "card 1 taken",
        ),
        line(
            2,
            LogLevel::Warn,
            2000,
            "tidal_player::auth",
            "refresh failed",
        ),
        line(
            3,
            LogLevel::Error,
            3000,
            "tidal_player::player::reserve",
            "cannot reserve card 1",
        ),
        line(
            4,
            LogLevel::Debug,
            4000,
            "tidal_player_api::http",
            "GET /v1/tracks",
        ),
        line(
            5,
            LogLevel::Info,
            5000,
            "tidal_player::player::engine",
            "Reserve released",
        ),
    ]
}

/// AC10: `/` filters the lines by their drawn text, ignoring case, with
/// 0012's typing, `Enter`, `Esc` and title.
#[test]
fn ac10_filter() {
    let mut state = with_lines(10, filter_lines());
    assert_eq!(texts(&state).len(), 5);

    // Typing narrows as it goes, the title shows the label and count.
    press(&mut state, &[Key::Char('/')]);
    assert_eq!(title(&state), "Logs · /▏");
    type_text(&mut state, "RESER");
    assert_eq!(
        texts(&state),
        ["card 1 taken", "cannot reserve card 1", "Reserve released"]
    );
    assert_eq!(title(&state), "Logs · /RESER▏ · 3 matches");
    // Keys go to the filter while typing: `q` is a letter.
    type_text(&mut state, "ve c");
    assert_eq!(texts(&state), ["cannot reserve card 1"]);
    assert_eq!(title(&state), "Logs · /RESERve c▏ · 1 match");
    press(&mut state, &[Key::Backspace; 4]);
    assert_eq!(title(&state), "Logs · /RESER▏ · 3 matches");

    // `Enter` keeps it.
    press(&mut state, &[Key::Enter]);
    assert_eq!(title(&state), "Logs · /RESER · 3 matches");
    assert_eq!(texts(&state).len(), 3);

    // The drawn text counts: the level and the target, the time.
    for (typed, want) in [
        ("warn", vec!["refresh failed"]),
        ("ERROR", vec!["cannot reserve card 1"]),
        ("::auth", vec!["refresh failed"]),
        ("00:00:04", vec!["GET /v1/tracks"]),
        ("tidal_player_api", vec!["GET /v1/tracks"]),
    ] {
        press(&mut state, &[Key::Char('/'), Key::Ctrl('u')]);
        type_text(&mut state, typed);
        press(&mut state, &[Key::Enter]);
        assert_eq!(texts(&state), want, "{typed}");
    }

    // Nothing matches: 0012's message.
    press(&mut state, &[Key::Char('/'), Key::Ctrl('u')]);
    type_text(&mut state, "zzz");
    press(&mut state, &[Key::Enter]);
    assert_eq!(texts(&state), Vec::<String>::new());
    assert_eq!(message(&state).as_deref(), Some("No rows match \"zzz\""));
    assert_eq!(title(&state), "Logs · /zzz · 0 matches");

    // New lines that match appear as they arrive, the others do not.
    press(&mut state, &[Key::Char('/'), Key::Ctrl('u')]);
    type_text(&mut state, "reserve");
    press(&mut state, &[Key::Enter]);
    assert_eq!(texts(&state).len(), 3);
    let (id, _) = the_request(&clock(&mut state, 1000));
    reply(
        &mut state,
        id,
        batch(vec![
            line(6, LogLevel::Info, 6000, "tidal_player::x", "no match here"),
            line(
                7,
                LogLevel::Warn,
                7000,
                "tidal_player::player::reserve",
                "busy",
            ),
        ]),
    );
    assert_eq!(
        texts(&state),
        [
            "card 1 taken",
            "cannot reserve card 1",
            "Reserve released",
            "busy"
        ]
    );
    assert_eq!(title(&state), "Logs · /reserve · 4 matches");
}

/// AC10: `Esc` clears a kept filter before it follows again; `Esc` while
/// typing clears it; the movement keys work over the matches.
#[test]
fn ac10_filter_esc_order() {
    let mut state = with_lines(2, filter_lines());
    press(&mut state, &[Key::Char('/')]);
    type_text(&mut state, "reserve");
    press(&mut state, &[Key::Enter]);
    assert_eq!(offset(&state), 1, "3 matches, 2 rows: at the bottom");
    // Scroll up over the matches.
    press(&mut state, &[Key::Char('k')]);
    assert_eq!(offset(&state), 0);
    assert_eq!(title(&state), "Logs · scrolled · /reserve · 3 matches");

    // The first `Esc` clears the filter and leaves the view scrolled; the
    // second follows.
    press(&mut state, &[Key::Esc]);
    assert_eq!(texts(&state).len(), 5);
    assert!(title(&state).ends_with("scrolled"), "{}", title(&state));
    assert!(!title(&state).contains('/'));
    press(&mut state, &[Key::Esc]);
    assert_eq!(title(&state), "Logs");
    assert_eq!(offset(&state), 3);

    // While typing: the arrow keys scroll, `Esc` clears and stops typing.
    press(&mut state, &[Key::Char('/')]);
    type_text(&mut state, "e");
    press(&mut state, &[Key::Up]);
    assert_eq!(title(&state), "Logs · scrolled · /e▏ · 5 matches");
    press(&mut state, &[Key::Esc]);
    assert_eq!(title(&state), "Logs · scrolled");
    // Typing again works: `Esc` was one press for the filter.
    press(&mut state, &[Key::Char('/')]);
    type_text(&mut state, "q");
    assert_eq!(title(&state), "Logs · scrolled · /q▏ · 0 matches");

    // An all-space filter is no filter.
    press(&mut state, &[Key::Ctrl('u')]);
    type_text(&mut state, "  ");
    press(&mut state, &[Key::Enter]);
    assert_eq!(title(&state), "Logs · scrolled");
}

// --- AC14 --------------------------------------------------------------------

const E: [Key; 1] = [Key::Char('e')];

fn exported(effects: &[Effect]) -> Option<Vec<u64>> {
    match effects {
        [Effect::ExportLogs { lines }] => Some(lines.iter().map(|l| l.seq).collect()),
        [] => None,
        other => panic!("{other:?}"),
    }
}

/// AC14: `e` on the logs page exports the lines held, filtered, in the
/// page's order, wherever the view is and whether connected or not; with
/// none it says so; elsewhere it does nothing; the result sets the
/// message row.
#[test]
fn ac14_export() {
    const NO_LINES: &str = "No log lines to export";
    // All of them, not the rows in view; scrolled or not.
    let mut state = with_lines(2, lines(1, 6));
    assert_eq!(
        exported(&press(&mut state, &E)),
        Some(vec![1, 2, 3, 4, 5, 6])
    );
    press(&mut state, &GG);
    assert_eq!(offset(&state), 0);
    assert_eq!(
        exported(&press(&mut state, &E)),
        Some(vec![1, 2, 3, 4, 5, 6])
    );
    assert_eq!(state.message, None);

    // The player's and the TUI's own, in the page's order.
    own(
        &mut state,
        vec![line(1, LogLevel::Warn, 2500, "tidal_player::tui", "mine")],
    );
    let sent = match press(&mut state, &E).as_slice() {
        [Effect::ExportLogs { lines }] => lines.iter().map(|l| l.text.clone()).collect::<Vec<_>>(),
        other => panic!("{other:?}"),
    };
    assert_eq!(sent, ["L1", "L2", "mine", "L3", "L4", "L5", "L6"]);

    // Disconnected: the lines already shown are still exported.
    update(&mut state, Action::Disconnected { shut_down: false });
    assert_eq!(exported(&press(&mut state, &E)).map(|v| v.len()), Some(7));

    // A kept filter narrows the export to the matches.
    let mut state = with_lines(10, filter_lines());
    press(&mut state, &[Key::Char('/')]);
    type_text(&mut state, "reserve");
    press(&mut state, &[Key::Enter]);
    assert_eq!(exported(&press(&mut state, &E)), Some(vec![1, 3, 5]));
    // Typing: `e` is a letter of the filter, exporting nothing.
    press(&mut state, &[Key::Char('/')]);
    assert_eq!(press(&mut state, &E), vec![]);
    assert!(title(&state).contains("reservee"), "{}", title(&state));
    press(&mut state, &[Key::Esc]);

    // Nothing matches, or nothing is held: no effect, the message says so.
    let mut state = with_lines(10, filter_lines());
    press(&mut state, &[Key::Char('/')]);
    type_text(&mut state, "zzz");
    press(&mut state, &[Key::Enter]);
    assert_eq!(press(&mut state, &E), vec![]);
    assert_eq!(state.message.as_deref(), Some(NO_LINES));
    let (mut state, _) = opened(5);
    assert_eq!(press(&mut state, &E), vec![]);
    assert_eq!(state.message.as_deref(), Some(NO_LINES));

    // Elsewhere: nothing at all.
    let mut state = State::default();
    let before = state.clone();
    assert_eq!(press(&mut state, &E), vec![]);
    assert_eq!(state, before);
    let mut state = with_lines(5, lines(1, 3));
    press(&mut state, &[Key::Char('z')]);
    let before = state.clone();
    assert_eq!(press(&mut state, &E), vec![]);
    assert_eq!(state, before);

    // The result sets the message row.
    let cases = [
        (
            Ok((42, "/c/a.log".to_owned())),
            "Exported 42 log lines to /c/a.log",
        ),
        (
            Ok((1, "/c/b.log".to_owned())),
            "Exported 1 log line to /c/b.log",
        ),
        (
            Err("disk full".to_owned()),
            "Could not export the log: disk full",
        ),
    ];
    for (result, want) in cases {
        let mut state = with_lines(5, lines(1, 3));
        assert_eq!(update(&mut state, Action::LogsExported(result)), vec![]);
        assert_eq!(state.message.as_deref(), Some(want));
    }
}
