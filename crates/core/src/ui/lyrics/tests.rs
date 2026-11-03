//! Spec 0021 AC5–AC7: opening the lyrics page, following the current
//! track, the current line and the scrolling.

use std::time::Duration;

use crate::library::{LibraryRequest, LibraryResponse, PageData, PageRequest};
use crate::lyrics::{LyricLine, Lyrics};
use crate::protocol::{self, PlaybackState, PlayerSnapshot, QueueEntry, RepeatMode};
use crate::track::{AlbumRef, ArtistRef, EntryId, Track, TrackId};

use super::super::{
    Action, Connection, DISCONNECTED, Effect, Key, Load, LyricsContent, PageKind, State, update,
};
use super::{LOADING, NO_LYRICS, NOTHING_PLAYING};

// --- fixtures ----------------------------------------------------------------

const L: [Key; 1] = [Key::Char('l')];
const GL_UPPER: [Key; 2] = [Key::Char('g'), Key::Char('L')];
const GL: [Key; 2] = [Key::Char('g'), Key::Char('l')];

fn track(id: u64) -> Track {
    Track {
        id: TrackId(id),
        title: format!("T{id}"),
        version: None,
        artists: vec![
            ArtistRef {
                id: 20,
                name: "Ar".into(),
            },
            ArtistRef {
                id: 21,
                name: "Br".into(),
            },
        ],
        album: Some(AlbumRef {
            id: 10,
            title: "Al".into(),
            cover: None,
        }),
        duration: Some(Duration::from_secs(300)),
        streamable: true,
    }
}

/// A snapshot whose entries are `(entry ID, track ID)`.
fn snapshot(entries: &[(u64, u64)], current: Option<u64>) -> PlayerSnapshot {
    PlayerSnapshot {
        queue: entries
            .iter()
            .map(|&(entry, track_id)| QueueEntry {
                id: EntryId(entry),
                track: track(track_id),
                suggested: false,
            })
            .collect(),
        current: current.map(EntryId),
        state: PlaybackState::Playing,
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

/// Entries 1 and 2 are tracks 101 and 102.
fn entries() -> [(u64, u64); 2] {
    [(1, 101), (2, 102)]
}

fn playing(current: Option<u64>) -> State {
    let mut state = State::default();
    update(
        &mut state,
        Action::Player(protocol::Event::Player(snapshot(&entries(), current))),
    );
    state
}

fn press(state: &mut State, keys: &[Key]) -> Vec<Effect> {
    keys.iter()
        .flat_map(|k| update(state, Action::Key(*k)))
        .collect()
}

fn set_current(state: &mut State, current: Option<u64>) -> Vec<Effect> {
    update(
        state,
        Action::Player(protocol::Event::Player(snapshot(&entries(), current))),
    )
}

fn position(state: &mut State, entry: u64, millis: u64) -> Vec<Effect> {
    update(
        state,
        Action::Player(protocol::Event::Position {
            entry: EntryId(entry),
            position: Duration::from_millis(millis),
        }),
    )
}

/// The lyrics requests among `effects`, and nothing else.
fn asked(effects: &[Effect]) -> Vec<(u64, u64)> {
    effects
        .iter()
        .map(|e| match e {
            Effect::Library {
                id,
                request: LibraryRequest::Page(PageRequest::Lyrics(track_id)),
            } => (*id, *track_id),
            other => panic!("not a lyrics request: {other:?}"),
        })
        .collect()
}

fn answer(state: &mut State, id: u64, track_id: u64, lyrics: Option<Lyrics>) -> Vec<Effect> {
    update(
        state,
        Action::LibraryReply {
            id,
            result: Ok(LibraryResponse::Page(PageData::Lyrics {
                track: track_id,
                lyrics,
            })),
        },
    )
}

/// 20 lines, `10 s` apart from `0:10`: line `i` (0-based) at `(i + 1) * 10 s`.
fn synced() -> Lyrics {
    Lyrics {
        synced: (1..=20)
            .map(|i| LyricLine {
                at: Duration::from_secs(i * 10),
                text: format!("line {i}"),
            })
            .collect(),
        plain: "line 1\nline 2".into(),
        right_to_left: false,
    }
}

/// 30 plain lines.
fn plain() -> Lyrics {
    Lyrics {
        synced: vec![],
        plain: (1..=30)
            .map(|i| format!("row {i}"))
            .collect::<Vec<_>>()
            .join("\n"),
        right_to_left: false,
    }
}

/// The lyrics page for entry 1 (track 101), loaded with `lyrics`, over a
/// window 10 rows tall.
fn with_lyrics(lyrics: Lyrics) -> State {
    let mut state = playing(Some(1));
    update(&mut state, Action::Resize { list_height: 10 });
    let (id, track_id) = asked(&press(&mut state, &L))[0];
    answer(&mut state, id, track_id, Some(lyrics));
    state
}

fn view(state: &State) -> &super::LyricsView {
    state.page().lyrics.as_ref().expect("the lyrics page")
}

fn content(state: &State) -> LyricsContent<'_> {
    state.page().lyrics_content().expect("the lyrics page")
}

fn shown_track(state: &State) -> Option<u64> {
    view(state).track.as_ref().map(|t| t.id.0)
}

// --- AC5 ---------------------------------------------------------------------

/// AC5: `l` and `g L` push the lyrics page and ask for the current
/// track's lyrics; on top already, nothing; with no current entry the
/// page says `Nothing playing` and asks nothing; back onto the page with
/// the same track asks nothing.
#[test]
fn ac5_open_lyrics() {
    // (name, keys)
    for (name, keys) in [("l", &L[..]), ("g L", &GL_UPPER[..])] {
        let mut state = playing(Some(1));
        let effects = press(&mut state, keys);
        let [(id, track_id)] = asked(&effects)[..] else {
            panic!("{name}: one request expected: {effects:?}");
        };
        assert_eq!(track_id, 101, "{name}: the current entry's track");
        assert_eq!(state.page().kind, PageKind::Lyrics, "{name}");
        assert_eq!(state.history.len(), 2, "{name}");
        assert_eq!(state.page().load, Load::Loading { id }, "{name}");
        assert_eq!(content(&state), LyricsContent::Loading, "{name}");
        assert_eq!(state.page().title(), "Lyrics · T101 · Ar, Br", "{name}");

        // On top already: nothing.
        assert_eq!(press(&mut state, keys), vec![], "{name}: on top");
        assert_eq!(state.history.len(), 2, "{name}: on top");
        assert_eq!(state.page().load, Load::Loading { id }, "{name}: on top");
    }

    // No current entry (an empty queue, or a queue with none current).
    for current in [None::<u64>] {
        let mut state = playing(current);
        assert_eq!(press(&mut state, &L), vec![], "nothing playing");
        assert_eq!(state.page().kind, PageKind::Lyrics);
        assert_eq!(content(&state), LyricsContent::NothingPlaying);
        assert_eq!(state.page().title(), "Lyrics");
        assert_eq!(content(&state).message().as_deref(), Some(NOTHING_PLAYING));
    }
    let mut state = State::default();
    assert_eq!(press(&mut state, &L), vec![], "an empty queue");
    assert_eq!(content(&state), LyricsContent::NothingPlaying);

    // Back onto the page with the same track: the kept lyrics, no request.
    let mut state = with_lyrics(synced());
    assert!(matches!(content(&state), LyricsContent::Synced(_)));
    let (library_id, _) = match &press(&mut state, &GL)[..] {
        [Effect::Library { id, request }] => (*id, request.clone()),
        other => panic!("{other:?}"),
    };
    assert_eq!(state.history.len(), 3);
    assert_eq!(press(&mut state, &[Key::Backspace]), vec![], "back");
    assert_eq!(state.page().kind, PageKind::Lyrics);
    assert!(matches!(content(&state), LyricsContent::Synced(_)), "kept");
    // The page above it was dropped with its request.
    let _ = library_id;

    // Disconnected: the page opens failed, nothing is asked.
    let mut state = playing(Some(1));
    update(&mut state, Action::Disconnected { shut_down: false });
    assert_eq!(press(&mut state, &L), vec![]);
    assert_eq!(state.page().load, Load::Failed(DISCONNECTED.into()));
    assert_eq!(content(&state), LyricsContent::Failed(DISCONNECTED));

    // A failed page on top is fetched again by `l`.
    let mut state = playing(Some(1));
    let (id, _) = asked(&press(&mut state, &L))[0];
    update(
        &mut state,
        Action::LibraryReply {
            id,
            result: Err("Tidal answered 429: try again in a moment".into()),
        },
    );
    assert_eq!(
        content(&state),
        LyricsContent::Failed(
            "Could not load the lyrics: Tidal answered 429: try again in a moment"
        )
    );
    assert_eq!(asked(&press(&mut state, &L)).len(), 1);
}

// --- AC6 ---------------------------------------------------------------------

/// AC6: the page follows the current entry's track.
#[test]
fn ac6_follow_track() {
    // A new current track while the page is on top: one request, `Loading…`.
    let mut state = with_lyrics(synced());
    let effects = set_current(&mut state, Some(2));
    let [(id, track_id)] = asked(&effects)[..] else {
        panic!("{effects:?}");
    };
    assert_eq!(track_id, 102);
    assert_eq!(shown_track(&state), Some(102));
    assert_eq!(state.page().load, Load::Loading { id });
    assert_eq!(content(&state), LyricsContent::Loading);
    assert_eq!(content(&state).message().as_deref(), Some(LOADING));
    assert_eq!(state.page().title(), "Lyrics · T102 · Ar, Br");

    // A snapshot with the same track, and `Position` events: nothing.
    assert_eq!(set_current(&mut state, Some(2)), vec![], "same track");
    assert_eq!(position(&mut state, 2, 12_000), vec![], "position");
    assert_eq!(state.page().load, Load::Loading { id });

    // Repeat-one style: the same track under a new entry ID asks nothing.
    let mut state = with_lyrics(synced());
    let mut same = snapshot(&[(1, 101), (3, 101)], Some(3));
    same.position = Duration::ZERO;
    assert_eq!(
        update(&mut state, Action::Player(protocol::Event::Player(same))),
        vec![],
        "the same track again"
    );
    assert!(matches!(content(&state), LyricsContent::Synced(_)));

    // A late answer for a track no longer the page's is dropped.
    let mut state = playing(Some(1));
    let (old, _) = asked(&press(&mut state, &L))[0];
    let (new, _) = asked(&set_current(&mut state, Some(2)))[0];
    assert_ne!(old, new);
    assert_eq!(answer(&mut state, old, 101, Some(synced())), vec![]);
    assert_eq!(content(&state), LyricsContent::Loading, "still Loading…");
    assert_eq!(state.page().load, Load::Loading { id: new });
    // An answer under the right ID for another track is dropped too.
    answer(&mut state, new, 101, Some(synced()));
    assert_eq!(content(&state), LyricsContent::Loading);
    // The right one lands.
    answer(&mut state, new, 102, Some(plain()));
    assert!(matches!(content(&state), LyricsContent::Plain(_)));

    // A new track while the page is not on top: nothing; coming back
    // asks once.
    let mut state = with_lyrics(synced());
    press(&mut state, &GL);
    assert_eq!(state.page().kind, PageKind::Library);
    assert_eq!(set_current(&mut state, Some(2)), vec![], "not on top");
    assert_eq!(shown_track(&state_below(&state)), Some(101));
    let effects = press(&mut state, &[Key::Backspace]);
    assert_eq!(asked(&effects).len(), 1, "coming back asks once");
    assert_eq!(asked(&effects)[0].1, 102);
    assert_eq!(content(&state), LyricsContent::Loading);

    // The current entry goes away: `Nothing playing`, no request, and the
    // late answer is dropped.
    let mut state = playing(Some(1));
    let (id, _) = asked(&press(&mut state, &L))[0];
    assert_eq!(set_current(&mut state, None), vec![]);
    assert_eq!(content(&state), LyricsContent::NothingPlaying);
    assert_eq!(answer(&mut state, id, 101, Some(synced())), vec![]);
    assert_eq!(content(&state), LyricsContent::NothingPlaying);
    // And a track coming back asks again.
    let effects = set_current(&mut state, Some(1));
    assert_eq!(asked(&effects).len(), 1);

    // Disconnected and back: a new track at the first `Welcome` is asked
    // once, not twice.
    let mut state = with_lyrics(synced());
    update(&mut state, Action::Disconnected { shut_down: false });
    let effects = update(
        &mut state,
        Action::Welcome {
            snapshot: snapshot(&entries(), Some(2)),
            login_required: false,
        },
    );
    assert_eq!(state.connection, Connection::Connected);
    assert_eq!(asked(&effects).len(), 1, "{effects:?}");
    assert_eq!(asked(&effects)[0].1, 102);
    // The same track: a failed page is fetched again by the `Welcome`.
    let mut state = playing(Some(1));
    press(&mut state, &L);
    update(&mut state, Action::Disconnected { shut_down: false });
    assert_eq!(state.page().load, Load::Failed(DISCONNECTED.into()));
    let effects = update(
        &mut state,
        Action::Welcome {
            snapshot: snapshot(&entries(), Some(1)),
            login_required: false,
        },
    );
    assert_eq!(asked(&effects), vec![(2, 101)]);
}

/// The lyrics page under the page on top.
fn state_below(state: &State) -> State {
    State {
        history: state.history[..state.history.len() - 1].to_vec(),
        ..state.clone()
    }
}

// --- AC7 ---------------------------------------------------------------------

/// AC7: the current and the played lines follow the position, seeking
/// backwards included.
#[test]
fn ac7_display_state() {
    let mut state = with_lyrics(synced());
    // (position in ms, current, played, offset of a following view)
    let rows: [(u64, Option<usize>, usize, usize); 8] = [
        (0, None, 0, 0),
        (9_999, None, 0, 0),
        (10_000, Some(0), 0, 0),
        (25_000, Some(1), 1, 0),
        (60_000, Some(5), 5, 0),
        (100_000, Some(9), 9, 4),
        (125_000, Some(11), 11, 6),
        (400_000, Some(19), 19, 14),
    ];
    for (millis, current, played, offset) in rows {
        position(&mut state, 1, millis);
        let v = view(&state);
        assert_eq!(v.current(state.position), current, "{millis} ms");
        assert_eq!(v.played(state.position), played, "{millis} ms");
        assert_eq!(v.offset(state.position, 10), offset, "{millis} ms");
        assert!(v.following);
    }
    // Seeking backwards moves it back.
    position(&mut state, 1, 100_000);
    assert_eq!(view(&state).current(state.position), Some(9));
    position(&mut state, 1, 25_000);
    assert_eq!(view(&state).current(state.position), Some(1));
    assert_eq!(view(&state).offset(state.position, 10), 0);
    // A `Position` for another entry is not the position.
    position(&mut state, 2, 200_000);
    assert_eq!(state.position, Duration::from_millis(25_000));
    // Titles.
    assert_eq!(view(&state).window_title(), "Lyrics (synced)");
}

/// AC7: plain lyrics scroll with the movement keys and clamp.
#[test]
fn ac7_plain_scrolls() {
    use Key::{Char, Ctrl};
    let mut state = with_lyrics(plain());
    assert!(matches!(content(&state), LyricsContent::Plain(_)));
    assert_eq!(view(&state).window_title(), "Lyrics");
    let off = |s: &State| view(s).offset(s.position, 10);
    assert_eq!(off(&state), 0, "starts at the top");
    // (keys, expected offset)
    let rows: [(&[Key], usize); 12] = [
        (&[Char('k')], 0),
        (&[Char('j')], 1),
        (&[Char('j')], 2),
        (&[Char('k')], 1),
        (&[Key::Down], 2),
        (&[Key::Up], 1),
        (&[Ctrl('f')], 11),
        (&[Ctrl('f')], 20),
        (&[Ctrl('f')], 20),
        (&[Ctrl('b')], 10),
        (&[Char('G')], 20),
        (&[Char('g'), Char('g')], 0),
    ];
    for (keys, want) in rows {
        assert_eq!(press(&mut state, keys), vec![], "{keys:?}");
        assert_eq!(off(&state), want, "{keys:?}");
    }
    // Plain lyrics never show `· scrolled` and stay "following".
    assert_eq!(view(&state).window_title(), "Lyrics");
    // Position events do not move it.
    position(&mut state, 1, 100_000);
    assert_eq!(off(&state), 0);
}

/// AC7: on synced lyrics a movement key scrolls from the view's place and
/// stops following; `Esc` follows again.
#[test]
fn ac7_synced_scroll_and_follow() {
    use Key::{Char, Esc};
    let mut state = with_lyrics(synced());
    // At 100 s the current line is index 9 and the following offset 4.
    position(&mut state, 1, 100_000);
    assert_eq!(view(&state).offset(state.position, 10), 4);

    assert_eq!(press(&mut state, &[Char('j')]), vec![]);
    assert!(!view(&state).following);
    assert_eq!(
        view(&state).offset(state.position, 10),
        5,
        "from where the view was"
    );
    assert_eq!(view(&state).window_title(), "Lyrics (synced) · scrolled");
    // The highlight keeps moving; the view does not.
    position(&mut state, 1, 150_000);
    assert_eq!(view(&state).current(state.position), Some(14));
    assert_eq!(view(&state).offset(state.position, 10), 5);
    press(&mut state, &[Char('k'), Char('k')]);
    assert_eq!(view(&state).offset(state.position, 10), 3);
    press(&mut state, &[Char('G')]);
    assert_eq!(
        view(&state).offset(state.position, 10),
        14,
        "the last line at the bottom... or further"
    );

    // `Esc` follows again: the view back on the current line.
    assert_eq!(press(&mut state, &[Esc]), vec![]);
    assert!(view(&state).following);
    assert_eq!(view(&state).offset(state.position, 10), 9);
    assert_eq!(view(&state).window_title(), "Lyrics (synced)");
    // Following already: nothing changes.
    let before = state.clone();
    assert_eq!(press(&mut state, &[Esc]), vec![]);
    assert_eq!(state, before);

    // `Esc` with a popup open closes it and still does not follow.
    press(&mut state, &[Char('j')]);
    press(&mut state, &[Char('a')]);
    assert!(state.popup.is_some(), "the playing track's actions");
    press(&mut state, &[Esc]);
    assert!(state.popup.is_none());
    assert!(!view(&state).following);
    press(&mut state, &[Esc]);
    assert!(view(&state).following, "a second Esc follows");

    // A new track follows again, from the top of its own lyrics.
    press(&mut state, &[Char('j')]);
    assert!(!view(&state).following);
    set_current(&mut state, Some(2));
    assert!(view(&state).following);
    assert_eq!(view(&state).scroll, 0);

    // Reopening the page follows.
    let mut state = with_lyrics(synced());
    press(&mut state, &[Char('j')]);
    assert!(!view(&state).following);
    press(&mut state, &[Key::Backspace]);
    assert_eq!(state.page().kind, PageKind::Queue);
    let (id, track_id) = asked(&press(&mut state, &L))[0];
    answer(&mut state, id, track_id, Some(synced()));
    assert!(view(&state).following);
}

/// AC7: keys that act on rows do nothing on the lyrics page.
#[test]
fn ac7_row_keys_are_inert() {
    use Key::{Char, Ctrl, Enter};
    let mut state = with_lyrics(synced());
    let sequences: [&[Key]; 8] = [
        &[Enter],
        &[Char('Z')],
        &[Ctrl('z')],
        &[Char('/')],
        &[Char('s'), Char('c')],
        &[Char('d'), Char('d')],
        &[Char('g'), Char('a')],
        &[Key::Tab],
    ];
    for keys in sequences {
        let before = state.clone();
        assert_eq!(press(&mut state, keys), vec![], "{keys:?}");
        assert_eq!(state, before, "{keys:?}");
    }
    // `NO_LYRICS` for a track Tidal has none for.
    let mut state = playing(Some(1));
    let (id, track_id) = asked(&press(&mut state, &L))[0];
    answer(&mut state, id, track_id, None);
    assert_eq!(content(&state), LyricsContent::NoLyrics);
    assert_eq!(content(&state).message().as_deref(), Some(NO_LYRICS));
    // Both texts empty: the same.
    let mut state = playing(Some(1));
    let (id, track_id) = asked(&press(&mut state, &L))[0];
    answer(
        &mut state,
        id,
        track_id,
        Some(Lyrics {
            synced: vec![],
            plain: " \n".into(),
            right_to_left: false,
        }),
    );
    assert_eq!(content(&state), LyricsContent::NoLyrics);
}
