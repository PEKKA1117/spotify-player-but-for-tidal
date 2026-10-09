//! Spec 0007 AC5–AC10: the search page in the client model: the page in
//! the history, the input, sending a search and applying its reply, the
//! focus cycle and the scrolling loads, the rows and the top hit, and the
//! disconnected state.

use std::time::Duration;

use crate::item::Item;
use crate::library::{
    AlbumKind, AlbumSummary, LibraryRequest, LibraryResponse, ListItems, ListPage, ListRef,
    PageData, PageRequest, PlaylistSummary, TopHit,
};
use crate::protocol::RepeatMode;
use crate::protocol::{self, Command, InsertAt, PlaybackState, PlayerSnapshot, QueueEntry};
use crate::track::{AlbumRef, ArtistRef, EntryId, Track, TrackId};

use super::super::popup;
use super::super::{
    Action, DISCONNECTED, Effect, Key, Load, MenuAction, PageKind, Popup, SHUT_DOWN, SearchFocus,
    State, Window, WindowKind, update,
};

// --- fixtures ----------------------------------------------------------------

const GS: [Key; 2] = [Key::Char('g'), Key::Char('s')];
const GL: [Key; 2] = [Key::Char('g'), Key::Char('l')];
const GA: [Key; 2] = [Key::Char('g'), Key::Char('a')];

fn artist(id: u64) -> ArtistRef {
    ArtistRef {
        id,
        name: format!("Ar{id}"),
    }
}

fn track(id: u64) -> Track {
    Track {
        id: TrackId(id),
        title: format!("T{id}"),
        version: None,
        artists: vec![artist(20)],
        album: Some(AlbumRef {
            id: 10,
            title: "Al10".into(),
            cover: None,
        }),
        duration: Some(Duration::from_secs(100)),
        streamable: true,
    }
}

fn tracks(ids: impl IntoIterator<Item = u64>) -> Vec<Track> {
    ids.into_iter().map(track).collect()
}

fn album(id: u64) -> AlbumSummary {
    AlbumSummary {
        id,
        title: format!("Al{id}"),
        artists: vec![artist(20)],
        year: Some(2020),
        kind: AlbumKind::Album,
        tracks: Some(3),
        duration: None,
    }
}

/// A search playlist; `own` is set to prove the page never treats one as
/// the user's (spec 0007 decision 5).
fn playlist(i: u64) -> PlaylistSummary {
    PlaylistSummary {
        uuid: format!("p{i}"),
        title: format!("P{i}"),
        tracks: Some(2),
        duration: None,
        own: true,
    }
}

fn list<T>(items: Vec<T>, offset: u32, total: u32) -> ListPage<T> {
    ListPage {
        items,
        offset,
        total,
        hidden: 0,
    }
}

/// A search reply: `n` rows per list (tracks 1.., albums 10.., artists
/// 20.., playlists p0..), each list's total `total` (or 0 when empty).
fn data(top_hit: Option<TopHit>, n: [u64; 4], total: u32) -> PageData {
    let total = |n: u64| if n == 0 { 0 } else { total.max(n as u32) };
    search_data(top_hit, n, n.map(total))
}

/// A search reply with `n` rows and Tidal's total per list.
fn search_data(top_hit: Option<TopHit>, n: [u64; 4], totals: [u32; 4]) -> PageData {
    PageData::Search {
        top_hit: top_hit.map(Box::new),
        tracks: list(tracks(1..=n[0]), 0, totals[0]),
        albums: list((10..10 + n[1]).map(album).collect(), 0, totals[1]),
        artists: list((20..20 + n[2]).map(artist).collect(), 0, totals[2]),
        playlists: list((0..n[3]).map(playlist).collect(), 0, totals[3]),
    }
}

/// A snapshot whose queue holds entries `ids` (track = entry ID + 100).
fn snapshot(ids: &[u64], current: Option<u64>) -> PlayerSnapshot {
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
        state: PlaybackState::Playing,
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

/// A playing queue (so a playback key would send something).
fn with_queue() -> State {
    let mut state = State::default();
    update(
        &mut state,
        Action::Player(protocol::Event::Player(snapshot(&[1, 2], Some(1)))),
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

fn requests(effects: &[Effect]) -> Vec<(u64, LibraryRequest)> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::Library { id, request } => Some((*id, request.clone())),
            _ => None,
        })
        .collect()
}

/// The one library request among `effects`, and nothing else.
fn one_request(effects: &[Effect]) -> (u64, LibraryRequest) {
    let got = requests(effects);
    assert_eq!(
        (got.len(), effects.len()),
        (1, 1),
        "one request expected: {effects:?}"
    );
    got[0].clone()
}

/// What the effects ask for, without request IDs.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Out {
    Quit,
    Send(Command),
    Library(LibraryRequest),
}

fn outs(effects: &[Effect]) -> Vec<Out> {
    effects
        .iter()
        .map(|e| match e {
            Effect::Quit => Out::Quit,
            Effect::Send(c) => Out::Send(c.clone()),
            Effect::Library { request, .. } => Out::Library(request.clone()),
        })
        .collect()
}

fn reply(state: &mut State, id: u64, response: LibraryResponse) -> Vec<Effect> {
    update(
        state,
        Action::LibraryReply {
            id,
            result: Ok(response),
        },
    )
}

fn fail(state: &mut State, id: u64, message: &str) -> Vec<Effect> {
    update(
        state,
        Action::LibraryReply {
            id,
            result: Err(message.into()),
        },
    )
}

fn search_request(query: &str) -> LibraryRequest {
    LibraryRequest::Page(PageRequest::Search(query.into()))
}

fn more(list: ListRef, offset: u32, limit: u32) -> LibraryRequest {
    LibraryRequest::More {
        list,
        offset,
        limit,
    }
}

fn kinds(state: &State) -> Vec<PageKind> {
    state.history.iter().map(|p| p.kind.clone()).collect()
}

fn window(state: &State, index: usize) -> &Window {
    &state.page().windows[index]
}

/// The input of the search page on top.
fn input(state: &State) -> &str {
    &state.page().search.as_ref().expect("a search page").input
}

/// Where the focus is on the search page on top.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum F {
    Input,
    TopHit,
    W(usize),
}

fn focus(state: &State) -> F {
    let page = state.page();
    match page.search.as_ref().expect("a search page").focus {
        SearchFocus::Input => F::Input,
        SearchFocus::TopHit => F::TopHit,
        SearchFocus::Windows => F::W(page.focus),
    }
}

/// A search page over the queue, `query` typed and sent, and its reply
/// `data` applied.
fn searched(mut state: State, query: &str, data: PageData) -> State {
    assert_eq!(press(&mut state, &GS), vec![], "g s");
    press(&mut state, &typed(query));
    let (id, request) = one_request(&press(&mut state, &[Key::Enter]));
    assert_eq!(request, search_request(query.trim()));
    assert_eq!(reply(&mut state, id, LibraryResponse::Page(data)), vec![]);
    state
}

/// The labels of the open actions popup.
fn labels(state: &State) -> Vec<String> {
    match &state.popup {
        Some(Popup::Actions { actions, .. }) => actions.iter().map(MenuAction::label).collect(),
        other => panic!("no actions popup: {other:?}"),
    }
}

// --- AC5 ------------------------------------------------------------------------

/// AC5: `g s` pushes an empty search page with the input focused and
/// emits nothing; `g s` on a search page on top focuses its input and
/// pushes nothing; back and forth through the history keeps query,
/// results, cursors and focus with no request.
#[test]
fn ac5_search_page_history() {
    use Key::{Backspace, Char, Enter, Tab};
    // From the queue and from a loaded library page.
    let mut library = State::default();
    let (id, _) = one_request(&press(&mut library, &GL));
    reply(
        &mut library,
        id,
        LibraryResponse::Page(PageData::Library {
            playlists: list(vec![], 0, 0),
            albums: list(vec![], 0, 0),
            artists: list(vec![], 0, 0),
        }),
    );
    for (mut state, below) in [(State::default(), 1), (library, 2)] {
        assert_eq!(press(&mut state, &GS), vec![], "g s emits nothing");
        assert_eq!(state.history.len(), below + 1);
        let page = state.page();
        assert_eq!(page.kind, PageKind::Search(String::new()));
        assert_eq!(page.load, Load::Idle);
        assert_eq!(page.title(), "Search");
        assert_eq!(input(&state), "");
        assert_eq!(focus(&state), F::Input);
        let windows: Vec<WindowKind> = page.windows.iter().map(|w| w.kind).collect();
        assert_eq!(
            windows,
            vec![
                WindowKind::SearchTracks,
                WindowKind::SearchAlbums,
                WindowKind::SearchArtists,
                WindowKind::SearchPlaylists,
            ]
        );
        assert!(page.windows.iter().all(|w| w.rows.is_empty()));
        assert!(page.search.as_ref().unwrap().top_hit.is_none());
    }

    // `g s` with a window or the top hit focused: the input again, nothing
    // pushed, nothing sent; the results stay.
    for (top_hit, keys, from) in [
        (None, vec![Tab], F::W(1)),
        (None, vec![], F::W(0)),
        (Some(TopHit::Artist(artist(9))), vec![], F::TopHit),
    ] {
        let mut state = searched(State::default(), "abc", data(top_hit, [3, 3, 3, 3], 3));
        press(&mut state, &keys);
        assert_eq!(focus(&state), from);
        let before = state.page().windows.clone();
        assert_eq!(press(&mut state, &GS), vec![], "{from:?}");
        assert_eq!(state.history.len(), 2, "{from:?}");
        assert_eq!(focus(&state), F::Input, "{from:?}");
        assert_eq!(state.page().windows, before, "{from:?}");
        assert_eq!(input(&state), "abc");
    }

    // Back and forth: the search page keeps its query, results, cursors and
    // focus, without searching again.
    let mut state = searched(State::default(), "abc", data(None, [3, 3, 3, 3], 3));
    press(&mut state, &[Tab, Char('j')]);
    assert_eq!((focus(&state), window(&state, 1).cursor), (F::W(1), 1));
    let kept = state.page().clone();
    assert_eq!(kept.title(), "Search · \"abc\"");
    let (_, request) = one_request(&press(&mut state, &[Enter]));
    assert_eq!(request, LibraryRequest::Page(PageRequest::Album(11)));
    assert_eq!(state.page().kind, PageKind::Album(11));
    assert_eq!(press(&mut state, &[Backspace]), vec![]);
    assert_eq!(state.page(), &kept);
    // `g s` from another page pushes a new, empty search page; the earlier
    // one stays below.
    one_request(&press(&mut state, &GL));
    assert_eq!(press(&mut state, &GS), vec![]);
    assert_eq!(
        kinds(&state),
        vec![
            PageKind::Queue,
            PageKind::Search("abc".into()),
            PageKind::Library,
            PageKind::Search(String::new()),
        ]
    );
    assert_eq!((input(&state), focus(&state)), ("", F::Input));
    assert_eq!(press(&mut state, &[Key::Ctrl('q'), Key::Ctrl('q')]), vec![]);
    assert_eq!(state.page(), &kept);
}

// --- AC6 ------------------------------------------------------------------------

/// AC6: while the input has the focus, printable keys (`Space`, `q`, `n`,
/// `g` included) type; `Backspace` deletes one character and does nothing
/// on an empty input; `C-u` clears; the 201st character is not appended;
/// `C-c` quits; `C-q` goes back; no key but `Enter` asks anything, and
/// none sends a player command.
#[test]
fn ac6_input_editing() {
    use Key::{BackTab, Backspace, Char, Ctrl, Down, Esc, PageDown, PageUp, Tab, Up};
    let long: String = "x".repeat(201);
    let ignored = [
        Up,
        Down,
        PageUp,
        PageDown,
        Esc,
        Ctrl('s'),
        Ctrl('r'),
        Ctrl(' '),
        Ctrl('f'),
        Ctrl('b'),
        Ctrl('z'),
    ];
    // (keys on a new search page, the input after them, the effects, the
    // pages left in the history)
    let mut cases: Vec<(Vec<Key>, String, Vec<Out>, usize)> = vec![
        (typed("hello"), "hello".into(), vec![], 2),
        (vec![Char(' ')], " ".into(), vec![], 2),
        (vec![Char('q')], "q".into(), vec![], 2),
        (vec![Char('n')], "n".into(), vec![], 2),
        (vec![Char('p')], "p".into(), vec![], 2),
        (GS.to_vec(), "gs".into(), vec![], 2),
        (GL.to_vec(), "gl".into(), vec![], 2),
        (GA.to_vec(), "ga".into(), vec![], 2),
        (
            typed("zoOaA+-_[]/<>^GjkdfZ"),
            "zoOaA+-_[]/<>^GjkdfZ".into(),
            vec![],
            2,
        ),
        (typed("Sigur Rós 米津"), "Sigur Rós 米津".into(), vec![], 2),
        (
            [typed("ab"), vec![Backspace]].concat(),
            "a".into(),
            vec![],
            2,
        ),
        (vec![Backspace], String::new(), vec![], 2),
        (
            [typed("é"), vec![Backspace, Backspace]].concat(),
            String::new(),
            vec![],
            2,
        ),
        (
            [typed("ab"), vec![Ctrl('u')]].concat(),
            String::new(),
            vec![],
            2,
        ),
        (typed(&long), "x".repeat(200), vec![], 2),
        (vec![Ctrl('c')], String::new(), vec![Out::Quit], 2),
        (vec![Ctrl('q')], String::new(), vec![], 1),
    ];
    for key in ignored {
        cases.push(([typed("ab"), vec![key]].concat(), "ab".into(), vec![], 2));
    }
    for (keys, text, effects, pages) in cases {
        let mut state = with_queue();
        press(&mut state, &GS);
        let got = press(&mut state, &keys);
        assert_eq!(outs(&got), effects, "{keys:?}");
        assert_eq!(state.history.len(), pages, "{keys:?}: pages");
        assert!(state.popup.is_none() && state.prompt.is_none(), "{keys:?}");
        if pages == 2 {
            assert_eq!(input(&state), text, "{keys:?}");
            assert_eq!(focus(&state), F::Input, "{keys:?}: focus");
        }
    }

    // `Tab`/`BackTab` leave the input; they send nothing either.
    for key in [Tab, BackTab] {
        let mut state = with_queue();
        press(&mut state, &GS);
        assert_eq!(press(&mut state, &[key]), vec![], "{key:?}");
        assert_ne!(focus(&state), F::Input, "{key:?}");
    }

    // Pasted text is appended, without line breaks, up to 200 characters.
    let mut state = with_queue();
    press(&mut state, &GS);
    press(&mut state, &typed("ab "));
    assert_eq!(
        update(&mut state, Action::Paste("cd\nef\r\n".into())),
        vec![]
    );
    assert_eq!(input(&state), "ab cdef");
    update(&mut state, Action::Paste(long));
    assert_eq!(input(&state).chars().count(), 200);
}

// --- AC7 ------------------------------------------------------------------------

/// AC7: `Enter` on an empty or all-space input asks nothing; otherwise one
/// `Page(Search(trimmed))` with a fresh ID, the windows loading; the reply
/// fills the top hit and the windows and focuses the top hit, else the
/// first non-empty window, else the input; a newer search drops the older
/// reply; a reply for a page lower in the history lands there.
#[test]
fn ac7_send_and_reply() {
    use Key::{Char, Enter, Tab};
    // Nothing to send.
    for text in ["", "   "] {
        let mut state = with_queue();
        press(&mut state, &GS);
        press(&mut state, &typed(text));
        assert_eq!(press(&mut state, &[Enter]), vec![], "{text:?}");
        assert_eq!(state.page().load, Load::Idle);
        assert_eq!(state.page().kind, PageKind::Search(String::new()));
    }

    // One request, trimmed, with a fresh ID; the windows are loading.
    let mut state = with_queue();
    let (earlier, _) = one_request(&press(&mut state, &GL));
    press(&mut state, &GS);
    press(&mut state, &typed("  pierce the veil "));
    let (id, request) = one_request(&press(&mut state, &[Enter]));
    assert!(id > earlier, "a fresh ID");
    assert_eq!(request, search_request("pierce the veil"));
    let page = state.page();
    assert_eq!(page.load, Load::Loading { id });
    assert_eq!(page.kind, PageKind::Search("pierce the veil".into()));
    assert_eq!(page.title(), "Search · \"pierce the veil\"");
    assert_eq!(input(&state), "  pierce the veil ");
    let lists: Vec<ListRef> = page.windows.iter().map(|w| w.list.clone()).collect();
    let q = || "pierce the veil".to_owned();
    assert_eq!(
        lists,
        vec![
            ListRef::SearchTracks(q()),
            ListRef::SearchAlbums(q()),
            ListRef::SearchArtists(q()),
            ListRef::SearchPlaylists(q()),
        ]
    );
    // The reply fills the top hit and the windows, with Tidal's totals.
    let hit = TopHit::Artist(artist(9));
    assert_eq!(
        reply(
            &mut state,
            id,
            LibraryResponse::Page(data(Some(hit.clone()), [3, 2, 1, 4], 300))
        ),
        vec![]
    );
    let page = state.page();
    assert_eq!(page.load, Load::Idle);
    assert_eq!(page.search.as_ref().unwrap().top_hit, Some(hit));
    let got: Vec<(usize, Option<u32>)> = page
        .windows
        .iter()
        .map(|w| (w.rows.len(), w.total))
        .collect();
    assert_eq!(
        got,
        vec![
            (3, Some(300)),
            (2, Some(300)),
            (1, Some(300)),
            (4, Some(300))
        ]
    );
    assert_eq!(window(&state, 0).title(), "Tracks (300)");
    assert_eq!(focus(&state), F::TopHit);

    // The focus after a reply: (top hit, rows per list, focus).
    let cases: Vec<(Option<TopHit>, [u64; 4], F)> = vec![
        (Some(TopHit::Track(track(2))), [3, 3, 3, 3], F::TopHit),
        (Some(TopHit::Album(album(10))), [0, 0, 0, 0], F::TopHit),
        (None, [3, 3, 3, 3], F::W(0)),
        (None, [0, 2, 3, 3], F::W(1)),
        (None, [0, 0, 1, 0], F::W(2)),
        (None, [0, 0, 0, 1], F::W(3)),
        (None, [0, 0, 0, 0], F::Input),
    ];
    for (top_hit, n, expected) in cases {
        let state = searched(with_queue(), "abc", data(top_hit.clone(), n, 5));
        assert_eq!(focus(&state), expected, "{top_hit:?} {n:?}");
        assert_eq!(state.page().search.as_ref().unwrap().top_hit, top_hit);
    }

    // A new search on the same page replaces the results; cursors return
    // to the top.
    let mut state = searched(
        with_queue(),
        "abc",
        data(Some(TopHit::Artist(artist(9))), [3, 3, 3, 3], 3),
    );
    press(&mut state, &[Tab, Char('j'), Char('j')]);
    assert_eq!(window(&state, 0).cursor, 2);
    press(&mut state, &[Char('/'), Char('d')]);
    let (id, request) = one_request(&press(&mut state, &[Enter]));
    assert_eq!(request, search_request("abcd"));
    assert!(state.page().windows.iter().all(|w| w.rows.is_empty()));
    assert!(state.page().search.as_ref().unwrap().top_hit.is_none());
    reply(
        &mut state,
        id,
        LibraryResponse::Page(data(None, [2, 0, 0, 0], 2)),
    );
    assert_eq!(
        (window(&state, 0).cursor, window(&state, 0).rows.len()),
        (0, 2)
    );
    assert_eq!(window(&state, 1).rows.len(), 0);

    // A second `Enter` before the first reply: the first reply is dropped.
    let mut state = with_queue();
    press(&mut state, &GS);
    press(&mut state, &typed("a"));
    let (first, _) = one_request(&press(&mut state, &[Enter]));
    press(&mut state, &typed("b"));
    let (second, request) = one_request(&press(&mut state, &[Enter]));
    assert_eq!(request, search_request("ab"));
    let hit = Some(TopHit::Artist(artist(9)));
    assert_eq!(
        reply(
            &mut state,
            first,
            LibraryResponse::Page(data(hit.clone(), [3, 3, 3, 3], 3))
        ),
        vec![]
    );
    assert_eq!(state.page().load, Load::Loading { id: second });
    assert!(state.page().windows.iter().all(|w| w.rows.is_empty()));
    assert!(state.page().search.as_ref().unwrap().top_hit.is_none());
    reply(
        &mut state,
        second,
        LibraryResponse::Page(data(None, [1, 0, 0, 0], 1)),
    );
    assert_eq!(state.page().load, Load::Idle);
    assert_eq!(window(&state, 0).rows.len(), 1);

    // A reply for a search on a page lower in the history lands there.
    let mut state = with_queue();
    press(&mut state, &GS);
    press(&mut state, &typed("a"));
    let (id, _) = one_request(&press(&mut state, &[Enter]));
    press(&mut state, &[Tab]);
    one_request(&press(&mut state, &GL));
    assert_eq!(state.page().kind, PageKind::Library);
    reply(
        &mut state,
        id,
        LibraryResponse::Page(data(None, [2, 1, 0, 0], 2)),
    );
    let below = &state.history[1];
    assert_eq!(below.load, Load::Idle);
    assert_eq!(below.windows[0].rows.len(), 2);
    assert_eq!(below.windows[1].rows.len(), 1);
    assert!(matches!(state.page().load, Load::Loading { .. }));

    // A failed search shows its message in the page.
    let mut state = with_queue();
    press(&mut state, &GS);
    press(&mut state, &typed("a"));
    let (id, _) = one_request(&press(&mut state, &[Enter]));
    fail(&mut state, id, "Tidal answered 429: try again in a moment");
    assert_eq!(
        state.page().load,
        Load::Failed("Could not search: Tidal answered 429: try again in a moment".into())
    );
}

// --- AC8 ------------------------------------------------------------------------

/// AC8: `Tab`/`BackTab` cycle input → Top hit (when shown) → Tracks →
/// Albums → Artists → Playlists → input; `/` on a window focuses the
/// input; `Esc` on the input focuses what a reply would when there are
/// results; scrolling a window asks for the next page of its search list
/// at the search page size (0006 AC10's rules).
#[test]
fn ac8_windows_and_scroll() {
    use Key::{BackTab, Char, Esc, Tab};
    let hit = || Some(TopHit::Track(track(1)));
    // (top hit, the key, the focus sequence from the input)
    let all = [F::W(0), F::W(1), F::W(2), F::W(3)];
    let cases: Vec<(Option<TopHit>, Key, Vec<F>)> = vec![
        (
            hit(),
            Tab,
            [vec![F::TopHit], all.to_vec(), vec![F::Input, F::TopHit]].concat(),
        ),
        (
            hit(),
            BackTab,
            [
                all.iter().rev().copied().collect(),
                vec![F::TopHit, F::Input, F::W(3)],
            ]
            .concat(),
        ),
        (None, Tab, [all.to_vec(), vec![F::Input, F::W(0)]].concat()),
        (
            None,
            BackTab,
            [all.iter().rev().copied().collect(), vec![F::Input, F::W(3)]].concat(),
        ),
    ];
    for (top_hit, key, sequence) in cases {
        let mut state = searched(with_queue(), "abc", data(top_hit.clone(), [3, 3, 3, 3], 3));
        // `/` gives the input the focus.
        assert_eq!(press(&mut state, &[Char('/')]), vec![]);
        assert_eq!(focus(&state), F::Input);
        for expected in sequence {
            assert_eq!(press(&mut state, &[key]), vec![], "{key:?}");
            assert_eq!(focus(&state), expected, "{top_hit:?} {key:?}");
            // `/` from here focuses the input; `Tab` order goes on from
            // where it was.
            if expected != F::Input {
                let mut other = state.clone();
                assert_eq!(press(&mut other, &[Char('/')]), vec![]);
                assert_eq!(focus(&other), F::Input, "/ from {expected:?}");
                assert_eq!(input(&other), "abc", "/ types nothing");
            }
        }
    }

    // `Esc` on the input: what a reply would focus, when there are results.
    let cases: Vec<(Option<TopHit>, [u64; 4], F)> = vec![
        (hit(), [3, 3, 3, 3], F::TopHit),
        (None, [3, 3, 3, 3], F::W(0)),
        (None, [0, 0, 2, 0], F::W(2)),
        (None, [0, 0, 0, 0], F::Input),
    ];
    for (top_hit, n, expected) in cases {
        let mut state = searched(with_queue(), "abc", data(top_hit.clone(), n, 3));
        press(&mut state, &[Char('/')]);
        assert_eq!(press(&mut state, &[Esc]), vec![]);
        assert_eq!(focus(&state), expected, "{top_hit:?} {n:?}");
    }
    // Before any search, and while one loads: `Esc` does nothing.
    let mut state = with_queue();
    press(&mut state, &GS);
    assert_eq!(press(&mut state, &[Esc]), vec![]);
    assert_eq!(focus(&state), F::Input);
    press(&mut state, &typed("a"));
    press(&mut state, &[Key::Enter]);
    assert_eq!(press(&mut state, &[Esc]), vec![]);
    assert_eq!(focus(&state), F::Input);

    // Scrolling: each window asks for its list's next page when the cursor
    // comes within a window height of the last loaded row.
    let q = || "abc".to_owned();
    let lists = [
        ListRef::SearchTracks(q()),
        ListRef::SearchAlbums(q()),
        ListRef::SearchArtists(q()),
        ListRef::SearchPlaylists(q()),
    ];
    for (index, list) in lists.iter().enumerate() {
        let start = State {
            list_height: 5,
            ..with_queue()
        };
        let mut state = searched(start, "abc", data(None, [20, 20, 20, 20], 300));
        while focus(&state) != F::W(index) {
            press(&mut state, &[Tab]);
        }
        // Rows 1–13: more than a window height above row 19.
        for _ in 0..13 {
            assert_eq!(press(&mut state, &[Char('j')]), vec![], "{list:?}");
        }
        let (id, request) = one_request(&press(&mut state, &[Char('j')]));
        assert_eq!(request, more(list.clone(), 20, 20), "{list:?}");
        assert_eq!(window(&state, index).load, Load::Loading { id });
        assert_eq!(press(&mut state, &[Char('j')]), vec![], "asked twice");
        // Appended; `G` asks for the next.
        let items = match index {
            0 => ListItems::Tracks(list_of(tracks(21..=40), 20)),
            1 => ListItems::Albums(list_of((30..50).map(album).collect(), 20)),
            2 => ListItems::Artists(list_of((40..60).map(artist).collect(), 20)),
            _ => ListItems::Playlists(list_of((20..40).map(playlist).collect(), 20)),
        };
        assert_eq!(reply(&mut state, id, LibraryResponse::Items(items)), vec![]);
        assert_eq!(window(&state, index).rows.len(), 40, "{list:?}");
        let (_, request) = one_request(&press(&mut state, &[Char('G')]));
        assert_eq!(request, more(list.clone(), 40, 20), "{list:?}");
    }

    // The search page size, not the library's, clamped to the largest
    // page; a complete list asks nothing:
    // (library page size, search page size, rows loaded, total, `G` asks)
    let cases: Vec<(u32, u32, u64, u32, Option<LibraryRequest>)> = vec![
        (100, 20, 20, 300, Some(more(lists[0].clone(), 20, 20))),
        (100, 30, 30, 300, Some(more(lists[0].clone(), 30, 30))),
        (5, 50, 50, 300, Some(more(lists[0].clone(), 50, 50))),
        (
            100,
            5000,
            1000,
            3000,
            Some(more(lists[0].clone(), 1000, 1000)),
        ),
        (100, 20, 20, 20, None),
    ];
    for (page_size, search_page_size, n, total, expected) in cases {
        let start = State {
            page_size,
            search_page_size,
            list_height: 5,
            ..with_queue()
        };
        let mut state = searched(start, "abc", data(None, [n, 0, 0, 0], total));
        assert_eq!(focus(&state), F::W(0));
        let got = requests(&press(&mut state, &[Char('G')]));
        let got: Vec<LibraryRequest> = got.into_iter().map(|(_, r)| r).collect();
        assert_eq!(
            got,
            expected.into_iter().collect::<Vec<_>>(),
            "{search_page_size}"
        );
    }
}

fn list_of<T>(items: Vec<T>, offset: u32) -> ListPage<T> {
    list(items, offset, 300)
}

// --- AC9 ------------------------------------------------------------------------

/// AC9: rows and the top hit act as rows of 0006 pages: `Enter` on a track
/// queues the loaded *Tracks* rows from it (nothing fetched, before or
/// after); on a top-hit track among them the same, else the top hit then
/// the rows; `Enter` on an album, playlist or artist opens its page; `Z`
/// sends `Open`; the actions popup lists 0006's actions, a search
/// playlist never as own.
#[test]
fn ac9_rows_act() {
    use Key::{Char, Ctrl, Enter, Tab};
    let load = |ids: Vec<u64>, start| {
        vec![Out::Send(Command::LoadQueue {
            tracks: tracks(ids),
            start,
        })]
    };
    let open_page = |request| vec![Out::Library(LibraryRequest::Page(request))];
    let queue = |item| {
        vec![Out::Send(Command::Open {
            items: vec![item],
            at: Some(InsertAt::End),
        })]
    };
    // (top hit, keys after the reply, the effects, the page on top after)
    type Case = (Option<TopHit>, Vec<Key>, Vec<Out>, PageKind);
    let search = || PageKind::Search("abc".into());
    let cases: Vec<Case> = vec![
        // Tracks 1–5 of 300: the loaded rows from the chosen one.
        (None, vec![Enter], load((1..=5).collect(), 0), search()),
        (
            None,
            vec![Char('j'), Char('j'), Enter],
            load((1..=5).collect(), 2),
            search(),
        ),
        // The top hit, a track among the loaded rows, or not.
        (
            Some(TopHit::Track(track(3))),
            vec![Enter],
            load((1..=5).collect(), 2),
            search(),
        ),
        (
            Some(TopHit::Track(track(99))),
            vec![Enter],
            load([99, 1, 2, 3, 4, 5].to_vec(), 0),
            search(),
        ),
        // The top hit, an album, playlist or artist: its page.
        (
            Some(TopHit::Album(album(70))),
            vec![Enter],
            open_page(PageRequest::Album(70)),
            PageKind::Album(70),
        ),
        (
            Some(TopHit::Playlist(playlist(7))),
            vec![Enter],
            open_page(PageRequest::Playlist("p7".into())),
            PageKind::Playlist("p7".into()),
        ),
        (
            Some(TopHit::Artist(artist(77))),
            vec![Enter],
            open_page(PageRequest::Artist(77)),
            PageKind::Artist(77),
        ),
        // Album, artist and playlist rows: their page.
        (
            None,
            vec![Tab, Char('j'), Enter],
            open_page(PageRequest::Album(11)),
            PageKind::Album(11),
        ),
        (
            None,
            vec![Tab, Tab, Enter],
            open_page(PageRequest::Artist(20)),
            PageKind::Artist(20),
        ),
        (
            None,
            vec![Tab, Tab, Tab, Char('j'), Enter],
            open_page(PageRequest::Playlist("p1".into())),
            PageKind::Playlist("p1".into()),
        ),
        // `Z`/`C-z`: 0006's `Open`; nothing for an artist.
        (
            None,
            vec![Char('j'), Char('Z')],
            queue(Item::Track(TrackId(2))),
            search(),
        ),
        (None, vec![Tab, Ctrl('z')], queue(Item::Album(10)), search()),
        (
            None,
            vec![Tab, Tab, Tab, Char('Z')],
            queue(Item::Playlist("p0".into())),
            search(),
        ),
        (None, vec![Tab, Tab, Char('Z')], vec![], search()),
        (
            Some(TopHit::Track(track(99))),
            vec![Char('Z')],
            queue(Item::Track(TrackId(99))),
            search(),
        ),
        (
            Some(TopHit::Album(album(70))),
            vec![Char('Z')],
            queue(Item::Album(70)),
            search(),
        ),
    ];
    for (top_hit, keys, expected, page) in cases {
        // Tracks 1–5 of 300 (more to load), the other lists complete; the
        // cursor moves ask nothing.
        let start = State {
            list_height: 1,
            ..with_queue()
        };
        let data = search_data(top_hit.clone(), [5, 3, 3, 3], [300, 3, 3, 3]);
        let mut state = searched(start, "abc", data);
        let got = press(&mut state, &keys);
        assert_eq!(outs(&got), expected, "{top_hit:?} {keys:?}");
        assert_eq!(state.page().kind, page, "{top_hit:?} {keys:?}");
        assert!(state.whole_list.is_none(), "{top_hit:?} {keys:?}");
        // After the player's reply, nothing more is asked.
        assert_eq!(update(&mut state, Action::Reply(Ok(()))), vec![]);
    }

    // The actions popup: 0006's actions per kind of row; a search playlist
    // is never own (no *Delete playlist*, both favorites actions).
    let actions = |hit: Option<TopHit>, keys: Vec<Key>| {
        let mut state = searched(with_queue(), "abc", data(hit, [5, 3, 3, 3], 300));
        press(&mut state, &keys);
        assert_eq!(press(&mut state, &GA), vec![]);
        let got = labels(&state);
        state.popup = None;
        press(&mut state, &[Ctrl(' ')]);
        assert_eq!(labels(&state), got, "C-Space as g a");
        got
    };
    let names =
        |actions: Vec<MenuAction>| actions.iter().map(MenuAction::label).collect::<Vec<_>>();
    let not_own = PlaylistSummary {
        own: false,
        ..playlist(0)
    };
    let cases: Vec<(Option<TopHit>, Vec<Key>, Vec<String>)> = vec![
        (None, vec![], names(popup::track_actions(&track(1), None))),
        (None, vec![Tab], names(popup::album_actions(&album(10)))),
        (
            None,
            vec![Tab, Tab],
            names(popup::artist_actions(&artist(20))),
        ),
        (
            None,
            vec![Tab, Tab, Tab],
            names(popup::playlist_actions(&not_own)),
        ),
        (
            Some(TopHit::Track(track(99))),
            vec![],
            names(popup::track_actions(&track(99), None)),
        ),
        (
            Some(TopHit::Album(album(70))),
            vec![],
            names(popup::album_actions(&album(70))),
        ),
        (
            Some(TopHit::Playlist(playlist(0))),
            vec![],
            names(popup::playlist_actions(&not_own)),
        ),
        (
            Some(TopHit::Artist(artist(77))),
            vec![],
            names(popup::artist_actions(&artist(77))),
        ),
    ];
    for (hit, keys, expected) in cases {
        let got = actions(hit.clone(), keys.clone());
        assert_eq!(got, expected, "{hit:?} {keys:?}");
    }
    let playlist_labels = actions(None, vec![Tab, Tab, Tab]);
    assert!(!playlist_labels.contains(&"Delete playlist".to_owned()));
    for label in ["Add to favorites", "Remove from favorites"] {
        assert!(playlist_labels.contains(&label.to_owned()), "{label}");
    }
}

// --- AC10 -----------------------------------------------------------------------

/// AC10: `Enter` while disconnected fails the page with the connection's
/// message and sends nothing; the next `Welcome` sends the search again if
/// the page on top failed that way (0006 AC16's rules); a session-expired
/// error shows in the page.
#[test]
fn ac10_disconnected() {
    use Key::{Enter, Tab};
    let welcome = || Action::Welcome {
        snapshot: snapshot(&[1], Some(1)),
        login_required: false,
    };
    for (shut_down, message) in [(false, DISCONNECTED), (true, SHUT_DOWN)] {
        // `Enter` while disconnected: failed, nothing sent; typing works.
        let mut state = with_queue();
        press(&mut state, &GS);
        update(&mut state, Action::Disconnected { shut_down });
        press(&mut state, &typed(" abc"));
        assert_eq!(input(&state), " abc");
        assert_eq!(press(&mut state, &[Enter]), vec![], "{message}");
        assert_eq!(state.page().kind, PageKind::Search("abc".into()));
        assert_eq!(state.page().load, Load::Failed(message.into()));
        // The next `Welcome` sends it again.
        let (id, request) = one_request(&update(&mut state, welcome()));
        assert_eq!(request, search_request("abc"));
        assert_eq!(state.page().load, Load::Loading { id });
        reply(
            &mut state,
            id,
            LibraryResponse::Page(data(None, [1, 0, 0, 0], 1)),
        );
        assert_eq!(window(&state, 0).rows.len(), 1);
        assert_eq!(update(&mut state, welcome()), vec![], "once");

        // A search pending when the connection goes: failed, its late reply
        // dropped, sent again on `Welcome`.
        let mut state = with_queue();
        press(&mut state, &GS);
        press(&mut state, &typed("abc"));
        let (id, _) = one_request(&press(&mut state, &[Enter]));
        update(&mut state, Action::Disconnected { shut_down });
        assert_eq!(state.page().load, Load::Failed(message.into()));
        reply(
            &mut state,
            id,
            LibraryResponse::Page(data(None, [1, 0, 0, 0], 1)),
        );
        assert_eq!(state.page().load, Load::Failed(message.into()));
        let (_, request) = one_request(&update(&mut state, welcome()));
        assert_eq!(request, search_request("abc"));

        // Only the page on top: a failed search below it waits.
        let mut state = with_queue();
        press(&mut state, &GS);
        update(&mut state, Action::Disconnected { shut_down });
        press(&mut state, &typed("abc"));
        press(&mut state, &[Enter, Tab]);
        assert_eq!(press(&mut state, &GL), vec![]);
        let (_, request) = one_request(&update(&mut state, welcome()));
        assert_eq!(request, LibraryRequest::Page(PageRequest::Library));
    }

    // Session expired: the player's error shows in the page.
    let expired = "Session expired: run \"tidal-player login\"";
    let mut state = with_queue();
    press(&mut state, &GS);
    press(&mut state, &typed("abc"));
    let (id, _) = one_request(&press(&mut state, &[Enter]));
    update(&mut state, Action::Player(protocol::Event::LoginRequired));
    fail(&mut state, id, expired);
    match &state.page().load {
        Load::Failed(m) => assert!(m.contains(expired), "{m}"),
        other => panic!("{other:?}"),
    }
}
