//! Spec 0020 AC1, AC2, AC4, AC5, AC7, AC8 and AC9: selecting rows, counts,
//! motion mode, clearing, the selection actions popup and what it sends,
//! and the cursor-row keys beside a selection.

use std::time::Duration;

use crate::item::Item;
use crate::library::{
    AlbumKind, AlbumSummary, FavoriteKind, LibraryRequest, LibraryResponse, ListItems, ListPage,
    ListRef, MixSummary, PageData, PlaylistSummary,
};
use crate::protocol::{
    self, Command, InsertAt, PlaybackState, PlayerSnapshot, QueueEntry, RepeatMode,
};
use crate::track::{AlbumRef, ArtistRef, EntryId, Track, TrackId};

use super::super::keymap::{ActionEntry, KeymapFile, Target, build};
use super::super::{
    Action, BaseKey, Effect, Key, Popup, SelectionAction, State, TrackSource, Window, apply_keymap,
    help, update,
};
use super::{queue_entries, queue_marks, window_marks, window_rows};

// --- fixtures ----------------------------------------------------------------

const GL: [Key; 2] = [Key::Char('g'), Key::Char('l')];
const GY: [Key; 2] = [Key::Char('g'), Key::Char('y')];
const GS: [Key; 2] = [Key::Char('g'), Key::Char('s')];
const GM: [Key; 2] = [Key::Char('g'), Key::Char('m')];
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

fn playlist(uuid: &str, title: &str, own: bool) -> PlaylistSummary {
    PlaylistSummary {
        uuid: uuid.into(),
        title: title.into(),
        tracks: Some(2),
        duration: None,
        own,
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

/// Playlists Mine (own), Theirs (followed), Other (own); albums 10–12;
/// artists 20–22.
fn library_data() -> PageData {
    PageData::Library {
        playlists: list(
            vec![
                playlist("p1", "Mine", true),
                playlist("p2", "Theirs", false),
                playlist("p3", "Other", true),
            ],
            0,
            3,
        ),
        albums: list(vec![album(10), album(11), album(12)], 0, 3),
        artists: list(vec![artist(20), artist(21), artist(22)], 0, 3),
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
        device: "default".into(),
    }
}

fn apply(state: &mut State, ids: &[u64], current: Option<u64>) {
    update(
        state,
        Action::Player(protocol::Event::Player(snapshot(ids, current))),
    );
}

/// The queue page with entries `ids`, the cursor on the first.
fn queue_of(ids: &[u64]) -> State {
    let mut state = State::default();
    apply(&mut state, ids, ids.first().copied());
    state
}

fn press(state: &mut State, keys: &[Key]) -> Vec<Effect> {
    keys.iter()
        .flat_map(|k| update(state, Action::Key(*k)))
        .collect()
}

/// The keys of `text`, one per character (`"s3j"`).
fn keys(text: &str) -> Vec<Key> {
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

/// What the effects ask for, without request IDs.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Out {
    Send(Command),
    Library(LibraryRequest),
    Other,
}

fn outs(effects: &[Effect]) -> Vec<Out> {
    effects
        .iter()
        .map(|e| match e {
            Effect::Send(c) => Out::Send(c.clone()),
            Effect::Library { request, .. } => Out::Library(request.clone()),
            _ => Out::Other,
        })
        .collect()
}

fn reply(state: &mut State, id: u64, result: Result<LibraryResponse, String>) -> Vec<Effect> {
    update(state, Action::LibraryReply { id, result })
}

/// Presses `keys`, which must ask for one page, and answers it.
fn open(state: &mut State, keys: &[Key], data: PageData) {
    let effects = press(state, keys);
    let asked = requests(&effects);
    assert_eq!(asked.len(), 1, "one request: {effects:?}");
    reply(state, asked[0].0, Ok(LibraryResponse::Page(data)));
}

/// Favorite tracks `ids`, all loaded, over the queue (entries 1–3).
fn favorites(ids: impl IntoIterator<Item = u64>) -> State {
    let tracks = tracks(ids);
    let total = tracks.len() as u32;
    let mut state = queue_of(&[1, 2, 3]);
    open(
        &mut state,
        &GY,
        PageData::FavoriteTracks {
            tracks: list(tracks, 0, total),
        },
    );
    state
}

/// The library page, loaded, the Playlists window focused.
fn library() -> State {
    let mut state = queue_of(&[1, 2, 3]);
    open(&mut state, &GL, library_data());
    state
}

/// Album 10's page (tracks 1–3), opened from the library's Albums.
fn album_page() -> State {
    let mut state = library();
    open(
        &mut state,
        &[Key::Tab, Key::Enter],
        PageData::Album {
            album: album(10),
            tracks: list(tracks(1..4), 0, 3),
        },
    );
    state
}

/// Playlist p1 Mine (`own`), tracks 4, 6, 8, 9, ETag `e1`.
fn playlist_page(own: bool) -> State {
    let mut state = library();
    open(
        &mut state,
        &[Key::Enter],
        PageData::Playlist {
            playlist: playlist("p1", "Mine", own),
            etag: Some("e1".into()),
            tracks: list(tracks([4, 6, 8, 9]), 0, 4),
        },
    );
    state
}

/// A search page with results; `window` (0 tracks … 3 playlists) focused.
fn searched(window: usize) -> State {
    let mut state = queue_of(&[1, 2, 3]);
    press(&mut state, &GS);
    press(&mut state, &keys("q"));
    open(
        &mut state,
        &[Key::Enter],
        PageData::Search {
            top_hit: None,
            tracks: list(tracks(1..4), 0, 3),
            albums: list(vec![album(10), album(11)], 0, 2),
            artists: list(vec![artist(20), artist(21)], 0, 2),
            playlists: list(
                vec![playlist("s1", "S1", false), playlist("s2", "S2", false)],
                0,
                2,
            ),
        },
    );
    for _ in 0..window {
        press(&mut state, &[Key::Tab]);
    }
    state
}

/// Mix m1's page (tracks 1–3), opened from the mixes page.
fn mix_page() -> State {
    let mix = MixSummary {
        id: "m1".into(),
        title: "Mix 1".into(),
        subtitle: None,
    };
    let mut state = queue_of(&[1, 2, 3]);
    open(
        &mut state,
        &GM,
        PageData::Mixes {
            mixes: list(vec![mix.clone()], 0, 1),
        },
    );
    open(
        &mut state,
        &[Key::Enter],
        PageData::Mix {
            mix,
            tracks: list(tracks(1..4), 0, 3),
        },
    );
    state
}

/// The focused window of the page on top.
fn focused(state: &State) -> &Window {
    let page = state.page();
    &page.windows[page.focus]
}

/// The rows selected in the focused window (indices into its rows).
fn rows(state: &State) -> Vec<usize> {
    window_rows(focused(state)).into_iter().collect()
}

/// The rows selected in window `w` of the page on top.
fn rows_of(state: &State, w: usize) -> Vec<usize> {
    window_rows(&state.page().windows[w]).into_iter().collect()
}

/// The queue entries selected.
fn entries(state: &State) -> Vec<u64> {
    queue_entries(state).iter().map(|e| e.0).collect()
}

/// The open selection popup's title and labels.
fn popup(state: &State) -> Option<(String, Vec<&'static str>)> {
    match &state.popup {
        Some(Popup::SelectionActions { title, actions, .. }) => Some((
            title.clone(),
            actions.iter().map(SelectionAction::label).collect(),
        )),
        _ => None,
    }
}

/// Moves the open selection popup's cursor to `label`.
fn to_label(state: &mut State, label: &str) {
    let at = popup(state)
        .unwrap_or_else(|| panic!("no selection popup: {:?}", state.popup))
        .1
        .iter()
        .position(|l| *l == label)
        .unwrap_or_else(|| panic!("no {label:?}"));
    press(state, &vec![Key::Char('j'); at]);
}

fn open_items(items: Vec<Item>, at: Option<InsertAt>) -> Vec<Out> {
    vec![Out::Send(Command::Open { items, at })]
}

fn track_items(ids: &[u64]) -> Vec<Item> {
    ids.iter().map(|id| Item::Track(TrackId(*id))).collect()
}

// --- AC1 -----------------------------------------------------------------------

/// AC1: `s c` selects an unselected cursor row and unselects a selected
/// one; the cursor stays; an empty window changes nothing; with a filter,
/// the row under the cursor is the filtered one.
#[test]
fn ac1_select_current() {
    use Key::{Char, Enter, Tab};
    let sc = keys("sc");
    // (name, state, keys moving the cursor, the row selected)
    type Case = (&'static str, fn() -> State, Vec<Key>, usize);
    let cases: Vec<Case> = vec![
        ("favorite tracks", || favorites(1..5), vec![Char('j')], 1),
        ("library albums", library, vec![Tab, Char('j')], 1),
        ("an album's tracks", album_page, vec![Char('G')], 2),
        ("a search window", || searched(1), vec![Char('j')], 1),
        (
            "favorite tracks, filtered",
            || favorites(1..5),
            [vec![Char('/')], keys("3"), vec![Enter]].concat(),
            2,
        ),
    ];
    for (name, setup, moves, row) in cases {
        let mut state = setup();
        press(&mut state, &moves);
        let cursor = focused(&state).cursor;
        assert_eq!(press(&mut state, &sc), vec![], "{name}");
        assert_eq!(rows(&state), vec![row], "{name}: selected");
        assert_eq!(focused(&state).cursor, cursor, "{name}: cursor moved");
        assert_eq!(state.pending, vec![], "{name}: pending");
        press(&mut state, &sc);
        assert_eq!(rows(&state), Vec::<usize>::new(), "{name}: unselected");
        assert_eq!(focused(&state).cursor, cursor, "{name}: cursor moved");
    }

    // The queue: by entry ID.
    let mut state = queue_of(&[7, 3, 9]);
    press(&mut state, &[Char('j')]);
    press(&mut state, &sc);
    assert_eq!(entries(&state), vec![3]);
    assert_eq!(state.cursor, Some(EntryId(3)));
    press(&mut state, &sc);
    assert_eq!(entries(&state), Vec::<u64>::new());

    // An empty window and an empty queue: nothing changes.
    for mut state in [favorites([]), State::default()] {
        let before = state.clone();
        assert_eq!(press(&mut state, &sc), vec![]);
        assert_eq!(state, before);
    }
}

// --- AC2 -----------------------------------------------------------------------

/// AC2: `s j`, `s <N> j`, `s <N> k`: the rows from the cursor, the cursor
/// on the last; the ends of the list; already selected rows stay; `s 0 j`,
/// `s 3 x` and `s 3 esc` select nothing and leave nothing pending; a lazy
/// list's next page is asked for as `j` asks for it.
#[test]
fn ac2_select_rows_with_count() {
    use Key::Esc;
    // (name, list length, keys, rows selected, cursor after)
    type Case = (&'static str, u64, Vec<Key>, Vec<usize>, usize);
    let cases: Vec<Case> = vec![
        ("s j: the cursor row", 5, keys("sj"), vec![0], 0),
        ("s 3 j", 5, keys("js3j"), vec![1, 2, 3], 3),
        ("s 3 k", 5, keys("Gs3k"), vec![2, 3, 4], 2),
        ("s 1 2 j", 15, keys("s12j"), (0..12).collect(), 11),
        ("s 1 j is s c", 5, keys("js1j"), vec![1], 1),
        ("past the end", 5, keys("Gks9j"), vec![3, 4], 4),
        ("past the start", 5, keys("js9k"), vec![0, 1], 0),
        ("selected rows stay", 5, keys("scs3j"), vec![0, 1, 2], 2),
        ("s j never unselects", 5, keys("scsj"), vec![0], 0),
        ("s 0 j", 5, keys("s0j"), vec![], 0),
        ("s 3 x", 5, keys("s3x"), vec![], 0),
        ("s 3 esc", 5, [keys("s3"), vec![Esc]].concat(), vec![], 0),
        ("s 3 k at the top", 5, keys("s3k"), vec![0], 0),
    ];
    for (name, n, pressed, selected, cursor) in cases {
        let mut state = favorites(1..=n);
        press(&mut state, &pressed);
        assert_eq!(rows(&state), selected, "{name}: rows");
        assert_eq!(focused(&state).cursor, cursor, "{name}: cursor");
        assert_eq!(state.pending, vec![], "{name}: pending");
        assert_eq!(state.popup, None, "{name}: popup");
    }

    // The queue: by entry ID, the cursor on the last.
    let mut state = queue_of(&[7, 3, 9, 5]);
    press(&mut state, &keys("js2j"));
    assert_eq!(entries(&state), vec![3, 9]);
    assert_eq!(state.cursor, Some(EntryId(9)));
    press(&mut state, &keys("Gs9k"));
    assert_eq!(entries(&state), vec![7, 3, 9, 5]);
    assert_eq!(state.cursor, Some(EntryId(7)));

    // A lazy list (5 of 100 loaded): the next page is asked for as the
    // cursor's move by `j` asks for it.
    let lazy = || {
        let mut state = queue_of(&[1]);
        state.list_height = 1;
        open(
            &mut state,
            &GY,
            PageData::FavoriteTracks {
                tracks: list(tracks(1..6), 0, 100),
            },
        );
        state
    };
    let by_j = press(&mut lazy(), &keys("jjj"));
    assert_eq!(requests(&by_j).len(), 1, "j asks: {by_j:?}");
    let mut state = lazy();
    let by_count = press(&mut state, &keys("s4j"));
    assert_eq!(outs(&by_count), outs(&by_j));
    assert_eq!(rows(&state), vec![0, 1, 2, 3]);
}

// --- AC4 -----------------------------------------------------------------------

/// AC4: motion mode: the range from the anchor to the cursor reads as
/// selected; `s m` adds it and ends the mode; both directions, `G` and
/// `g g`; `Esc` ends it and clears; a focus change adds the range, a page
/// change clears; `s a` and `d s` act on selection and range and end the
/// mode; `s c` changes nothing and the keys help dims it.
#[test]
fn ac4_motion_mode() {
    use Key::{Backspace, Esc, Tab};
    let motion = |state: &State| window_marks(focused(state)).motion;
    // (name, keys, rows read as selected, motion on after)
    type Case = (&'static str, Vec<Key>, Vec<usize>, bool);
    let cases: Vec<Case> = vec![
        ("s m alone: the anchor", keys("sm"), vec![0], true),
        ("s m, j j", keys("smjj"), vec![0, 1, 2], true),
        ("s m, j j, s m", keys("smjjsm"), vec![0, 1, 2], false),
        (
            "earlier rows stay",
            keys("Gscggsmjsm"),
            vec![0, 1, 5],
            false,
        ),
        (
            "back past the anchor",
            keys("jjjsmkkk"),
            vec![0, 1, 2, 3],
            true,
        ),
        ("G extends it", keys("jsmG"), vec![1, 2, 3, 4, 5], true),
        ("g g extends it", keys("Gsmgg"), (0..6).collect(), true),
        ("s c changes nothing", keys("smjsc"), vec![0, 1], true),
        (
            "Esc clears",
            [keys("scjjsmj"), vec![Esc]].concat(),
            vec![],
            false,
        ),
    ];
    for (name, pressed, selected, on) in cases {
        let mut state = favorites(1..7);
        press(&mut state, &pressed);
        assert_eq!(rows(&state), selected, "{name}: rows");
        assert_eq!(motion(&state), on, "{name}: motion");
        assert_eq!(state.pending, vec![], "{name}: pending");
    }

    // The queue too.
    let mut state = queue_of(&[7, 3, 9, 5]);
    press(&mut state, &keys("jsmj"));
    assert_eq!(entries(&state), vec![3, 9]);
    assert!(queue_marks(&state).motion);
    press(&mut state, &keys("sm"));
    assert!(!queue_marks(&state).motion);
    assert_eq!(entries(&state), vec![3, 9]);

    // A focus change adds the range to the window left.
    let mut state = library();
    press(&mut state, &[Tab]);
    press(&mut state, &keys("smj"));
    press(&mut state, &[Tab]);
    assert_eq!(rows_of(&state, 1), vec![0, 1]);
    assert!(!window_marks(&state.page().windows[1]).motion);

    // A page change clears; coming back does not restore it.
    let mut state = favorites(1..7);
    press(&mut state, &keys("smj"));
    open(&mut state, &GL, library_data());
    press(&mut state, &[Backspace]);
    assert_eq!(rows(&state), Vec::<usize>::new());
    assert!(!motion(&state));

    // `s a` acts on selection and range, and ends the mode.
    let mut state = favorites(1..7);
    press(&mut state, &keys("Gscggsmj"));
    press(&mut state, &keys("sa"));
    assert_eq!(popup(&state).map(|(t, _)| t).as_deref(), Some("3 selected"));
    assert_eq!(rows(&state), vec![0, 1, 5]);
    assert!(!motion(&state));

    // `d s` too.
    let mut state = favorites(1..7);
    press(&mut state, &keys("smj"));
    press(&mut state, &keys("ds"));
    match &state.popup {
        Some(Popup::Confirm { question, .. }) => {
            assert_eq!(question, "Remove 2 tracks from favorites? (y/n)")
        }
        other => panic!("no question: {other:?}"),
    }
    assert!(!motion(&state));

    // The keys help dims `s c` in motion mode.
    let mut state = favorites(1..7);
    let dim = |state: &State| {
        help(state)
            .into_iter()
            .flat_map(|s| s.rows)
            .find(|r| r.command == "SelectCurrent")
            .map(|r| r.dim)
    };
    assert_eq!(dim(&state), Some(false));
    press(&mut state, &keys("sm"));
    assert_eq!(dim(&state), Some(true));
}

// --- AC5 -----------------------------------------------------------------------

/// AC5: what clears a selection and what keeps it.
#[test]
fn ac5_clearing() {
    use Key::{BackTab, Char, Esc, Tab};
    // `Esc` with nothing open clears the focused window's selection only.
    let mut state = library();
    press(&mut state, &keys("sc"));
    press(&mut state, &[Tab]);
    press(&mut state, &keys("sc"));
    press(&mut state, &[Esc]);
    assert_eq!(rows_of(&state, 0), vec![0], "Esc: the other window");
    assert_eq!(rows_of(&state, 1), Vec::<usize>::new(), "Esc: focused");

    // `Esc` closing a popup, the help or a filter input clears nothing.
    for (name, pressed) in [
        ("a popup", GA.to_vec()),
        ("the help", vec![Char('?')]),
        ("a filter input", [vec![Char('/')], keys("T")].concat()),
    ] {
        let mut state = favorites(1..4);
        press(&mut state, &keys("sc"));
        press(&mut state, &pressed);
        press(&mut state, &[Esc]);
        assert_eq!(rows(&state), vec![0], "Esc closing {name}");
    }

    // Focus changes keep each window's selection.
    let mut state = library();
    press(&mut state, &keys("sc"));
    press(&mut state, &[Tab, Tab, Tab, BackTab, BackTab]);
    press(&mut state, &keys("jsc"));
    assert_eq!(rows_of(&state, 0), vec![0]);
    assert_eq!(rows_of(&state, 1), vec![1]);

    // A page change clears every window of the page left, for good.
    let mut state = library();
    press(&mut state, &keys("sc"));
    press(&mut state, &[Tab]);
    press(&mut state, &keys("sc"));
    press(&mut state, &[Char('z')]);
    press(&mut state, &[Key::Backspace]);
    assert_eq!(rows_of(&state, 0), Vec::<usize>::new(), "page change");
    assert_eq!(rows_of(&state, 1), Vec::<usize>::new(), "page change");
    // The queue's too.
    let mut state = queue_of(&[7, 3]);
    press(&mut state, &keys("sc"));
    open(&mut state, &GL, library_data());
    press(&mut state, &[Key::Backspace]);
    assert_eq!(entries(&state), Vec::<u64>::new(), "queue page left");

    // A successful action clears; a failed one keeps the selection.
    let mut state = favorites(1..4);
    press(&mut state, &keys("s2j"));
    let sent = press(&mut state, &keys("saa"));
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_eq!(rows(&state), Vec::<usize>::new(), "sent: cleared");
    let mut state = favorites(1..4);
    state.confirm_removals = false;
    press(&mut state, &keys("s2j"));
    let effects = press(&mut state, &keys("ds"));
    let asked = requests(&effects);
    assert_eq!(asked.len(), 2, "{effects:?}");
    for (id, _) in &asked {
        reply(&mut state, *id, Err("Network error".into()));
    }
    assert_eq!(rows(&state), vec![0, 1], "failed: kept");
    for (id, _) in requests(&press(&mut state, &keys("ds"))) {
        reply(&mut state, id, Ok(LibraryResponse::Done));
    }
    assert_eq!(rows(&state), Vec::<usize>::new(), "removed: cleared");

    // Disconnected: nothing is sent, so nothing clears.
    let mut state = favorites(1..4);
    press(&mut state, &keys("s2j"));
    update(&mut state, Action::Disconnected { shut_down: false });
    assert_eq!(press(&mut state, &keys("sap")), vec![]);
    assert_eq!(rows(&state), vec![0, 1], "disconnected: kept");

    // A reloaded window loses its selection.
    let mut state = favorites(1..4);
    state.confirm_removals = false;
    press(&mut state, &keys("jsc"));
    // `d d` on row 0 (not the selection); its reply refetches the page.
    press(&mut state, &[Char('g'), Char('g')]);
    let (id, _) = requests(&press(&mut state, &keys("dd")))[0].clone();
    assert_eq!(rows(&state), vec![1], "d d leaves the selection");
    let refetch = reply(&mut state, id, Ok(LibraryResponse::Done));
    let (page, _) = requests(&refetch)[0].clone();
    reply(
        &mut state,
        page,
        Ok(LibraryResponse::Page(PageData::FavoriteTracks {
            tracks: list(tracks(2..4), 0, 2),
        })),
    );
    assert_eq!(rows(&state), Vec::<usize>::new(), "reloaded");

    // A queue entry that leaves the queue leaves the selection.
    let mut state = queue_of(&[7, 3, 9]);
    press(&mut state, &keys("s3j"));
    apply(&mut state, &[3, 9, 4], Some(3));
    assert_eq!(entries(&state), vec![3, 9]);
}

// --- AC7 -----------------------------------------------------------------------

/// AC7: `s a` with nothing selected opens nothing; with a selection the
/// popup is titled `N selected` and lists the actions that apply to every
/// selected row, never a navigation action.
#[test]
fn ac7_selection_actions_listed() {
    use Key::Tab;
    let play = vec!["Play", "Play next", "Add to queue"];
    let with = |more: &[&'static str]| [&play[..], more].concat();
    let track_rows = with(&["Add to playlist…", "Add to favorites"]);
    // (name, state, keys selecting the rows, title, labels)
    type Case = (
        &'static str,
        fn() -> State,
        Vec<Key>,
        &'static str,
        Vec<&'static str>,
    );
    let cases: Vec<Case> = vec![
        (
            "an album's tracks",
            album_page,
            keys("s2j"),
            "2 selected",
            track_rows.clone(),
        ),
        (
            "an own playlist",
            || playlist_page(true),
            keys("s3j"),
            "3 selected",
            [&track_rows[..], &["Remove from this playlist"]].concat(),
        ),
        (
            "a followed playlist",
            || playlist_page(false),
            keys("s2j"),
            "2 selected",
            track_rows.clone(),
        ),
        (
            "the queue",
            || queue_of(&[1, 2, 3]),
            keys("s2j"),
            "2 selected",
            [&track_rows[..], &["Remove from queue"]].concat(),
        ),
        (
            "favorite tracks",
            || favorites(1..4),
            keys("s2j"),
            "2 selected",
            with(&["Add to playlist…", "Remove from favorites"]),
        ),
        (
            "library albums",
            library,
            [vec![Tab], keys("s2j")].concat(),
            "2 selected",
            with(&["Remove from favorites"]),
        ),
        (
            "library artists",
            library,
            [vec![Tab, Tab], keys("s2j")].concat(),
            "2 selected",
            vec!["Remove from favorites"],
        ),
        (
            "library playlists, followed only",
            library,
            keys("jsc"),
            "1 selected",
            with(&["Remove from favorites"]),
        ),
        (
            "library playlists, own and followed",
            library,
            keys("s2j"),
            "2 selected",
            play.clone(),
        ),
        (
            "search tracks",
            || searched(0),
            keys("s2j"),
            "2 selected",
            track_rows.clone(),
        ),
        (
            "search albums",
            || searched(1),
            keys("s2j"),
            "2 selected",
            with(&["Add to favorites"]),
        ),
        (
            "search artists",
            || searched(2),
            keys("s2j"),
            "2 selected",
            vec!["Add to favorites"],
        ),
        (
            "search playlists",
            || searched(3),
            keys("s2j"),
            "2 selected",
            with(&["Add to favorites"]),
        ),
        (
            "a mix",
            mix_page,
            keys("s2j"),
            "2 selected",
            track_rows.clone(),
        ),
    ];
    for (name, setup, select, title, labels) in cases {
        let mut state = setup();
        // Nothing selected: nothing opens.
        assert_eq!(press(&mut state, &keys("sa")), vec![], "{name}");
        assert_eq!(state.popup, None, "{name}: opened with nothing selected");
        press(&mut state, &select);
        assert_eq!(press(&mut state, &keys("sa")), vec![], "{name}");
        assert_eq!(
            popup(&state),
            Some((title.to_owned(), labels)),
            "{name}: the popup"
        );
    }
}

// --- AC8 -----------------------------------------------------------------------

/// AC8: what the actions send: `Open` with the selected items in list
/// order and the table's `at`, then the selection clears; `Enter` on a row
/// does what its key does; *Add to playlist…* opens the picker with the
/// tracks, the selection clearing once added and staying on `Esc`; *Add
/// to favorites* sends one `AddFavorite` per row not known to be one.
#[test]
fn ac8_selection_actions_send() {
    use Key::{Char, Enter, Esc, Tab};
    // Rows 0 and 2 of the album's tracks (1, 3).
    let select = keys("scjjsc");
    // (name, keys after `s a`, what is sent)
    let cases: Vec<(&str, Vec<Key>, Vec<Out>)> = vec![
        ("p", keys("p"), open_items(track_items(&[1, 3]), None)),
        (
            "N",
            keys("N"),
            open_items(track_items(&[1, 3]), Some(InsertAt::Next)),
        ),
        (
            "a",
            keys("a"),
            open_items(track_items(&[1, 3]), Some(InsertAt::End)),
        ),
        (
            "Enter on Play",
            vec![Enter],
            open_items(track_items(&[1, 3]), None),
        ),
        (
            "Enter on Play next",
            vec![Char('j'), Enter],
            open_items(track_items(&[1, 3]), Some(InsertAt::Next)),
        ),
        (
            "Enter on Add to queue",
            vec![Char('j'), Char('j'), Enter],
            open_items(track_items(&[1, 3]), Some(InsertAt::End)),
        ),
        (
            "Add to favorites",
            vec![Char('G'), Enter],
            vec![
                Out::Library(LibraryRequest::AddFavorite(FavoriteKind::Track, "1".into())),
                Out::Library(LibraryRequest::AddFavorite(FavoriteKind::Track, "3".into())),
            ],
        ),
    ];
    for (name, after, sent) in cases {
        let mut state = album_page();
        press(&mut state, &select);
        press(&mut state, &keys("sa"));
        let effects = press(&mut state, &after);
        assert_eq!(outs(&effects), sent, "{name}");
        assert_eq!(state.popup, None, "{name}: the popup stayed");
        if name == "Add to favorites" {
            // Cleared once the writes succeed.
            assert_eq!(rows(&state), vec![0, 2], "{name}: before the replies");
            for (id, _) in requests(&effects) {
                reply(&mut state, id, Ok(LibraryResponse::Done));
            }
        }
        assert_eq!(rows(&state), Vec::<usize>::new(), "{name}: cleared");
    }

    // The library's albums: *Play* sends the albums.
    let mut state = library();
    press(&mut state, &[Tab]);
    press(&mut state, &keys("s2jsa"));
    assert_eq!(
        outs(&press(&mut state, &keys("p"))),
        open_items(vec![Item::Album(10), Item::Album(11)], None)
    );

    // The queue: its entries' tracks, in the queue's order.
    let mut state = queue_of(&[7, 3, 9]);
    press(&mut state, &keys("Gscggsc"));
    press(&mut state, &keys("sa"));
    assert_eq!(
        outs(&press(&mut state, &keys("N"))),
        open_items(track_items(&[107, 109]), Some(InsertAt::Next))
    );

    // *Add to playlist…*: the picker with the tracks in order.
    let picker = || {
        let mut state = album_page();
        press(&mut state, &select);
        press(&mut state, &keys("sa"));
        to_label(&mut state, "Add to playlist…");
        let effects = press(&mut state, &[Enter]);
        let (id, request) = requests(&effects)[0].clone();
        assert_eq!(
            request,
            LibraryRequest::More {
                list: ListRef::Playlists,
                offset: 0,
                limit: 50,
            }
        );
        match &state.popup {
            Some(Popup::AddToPlaylist { tracks: source, .. }) => {
                assert_eq!(*source, TrackSource::Tracks(tracks([1, 3])))
            }
            other => panic!("no picker: {other:?}"),
        }
        reply(
            &mut state,
            id,
            Ok(LibraryResponse::Items(ListItems::Playlists(list(
                vec![playlist("p9", "Nine", true)],
                0,
                1,
            )))),
        );
        state
    };
    // `Esc` keeps the selection.
    let mut state = picker();
    press(&mut state, &[Esc]);
    assert_eq!(state.popup, None);
    assert_eq!(rows(&state), vec![0, 2], "picker cancelled: kept");
    // Added: cleared once the write succeeds.
    let mut state = picker();
    let effects = press(&mut state, &[Char('j'), Enter]);
    let (id, request) = requests(&effects)[0].clone();
    assert_eq!(
        request,
        LibraryRequest::AddToPlaylist {
            uuid: "p9".into(),
            tracks: vec![TrackId(1), TrackId(3)],
            allow_duplicates: false,
        }
    );
    assert_eq!(rows(&state), vec![0, 2], "before the reply");
    reply(&mut state, id, Ok(LibraryResponse::Done));
    assert_eq!(rows(&state), Vec::<usize>::new(), "added: cleared");
    // Failed: kept.
    let mut state = picker();
    let effects = press(&mut state, &[Char('j'), Enter]);
    let (id, _) = requests(&effects)[0].clone();
    reply(&mut state, id, Err("Network error".into()));
    assert_eq!(rows(&state), vec![0, 2], "failed: kept");

    // *Add to favorites* on the queue: one per entry, in the queue's order.
    let mut state = queue_of(&[7, 3]);
    press(&mut state, &keys("s2jsa"));
    to_label(&mut state, "Add to favorites");
    assert_eq!(
        outs(&press(&mut state, &[Enter])),
        vec![
            Out::Library(LibraryRequest::AddFavorite(
                FavoriteKind::Track,
                "107".into()
            )),
            Out::Library(LibraryRequest::AddFavorite(
                FavoriteKind::Track,
                "103".into()
            )),
        ]
    );
}

// --- AC9 -----------------------------------------------------------------------

/// AC9: with rows selected, `Enter`, `a`, `d d` and an `[[actions]]`
/// binding act on the row under the cursor exactly as without a
/// selection, and leave the selection as it was.
#[test]
fn ac9_cursor_keys_ignore_selection() {
    use Key::{Char, Enter};
    let mx = Key::Alt(BaseKey::Char('x'));
    let bound = |mut state: State| {
        let file = KeymapFile {
            keymaps: vec![],
            actions: vec![ActionEntry {
                action: "AddToQueue".into(),
                key_sequence: "M-x".into(),
                target: Target::SelectedItem,
            }],
        };
        apply_keymap(&mut state, build(&file).unwrap());
        state
    };
    let selection = |state: &State, queue: bool| {
        if queue {
            format!("{:?}", entries(state))
        } else {
            format!("{:?}", rows(state))
        }
    };
    let cases: Vec<(&str, Vec<Key>)> = vec![
        ("Enter", vec![Enter]),
        ("a", vec![Char('a')]),
        ("d d", keys("dd")),
        ("an [[actions]] binding", vec![mx]),
    ];
    // (window, setup, the selection `j j s 2 j k k` makes)
    let windows: [(&str, fn() -> State, &str); 2] = [
        ("favorite tracks", || favorites(1..5), "[2, 3]"),
        ("the queue", || queue_of(&[1, 2, 3, 4]), "[3, 4]"),
    ];
    for (name, pressed) in cases {
        for (window, setup, selected) in windows {
            let context = format!("{name} on {window}");
            let queue = window == "the queue";
            // The cursor on row 1 either way.
            let mut without = bound(setup());
            press(&mut without, &[Char('j')]);
            let expected = press(&mut without, &pressed);
            let mut with = bound(setup());
            press(&mut with, &keys("jjs2jkk"));
            assert_eq!(selection(&with, queue), selected, "{context}: setup");
            assert_eq!(press(&mut with, &pressed), expected, "{context}");
            assert_eq!(with.popup, without.popup, "{context}: popup");
            assert_eq!(selection(&with, queue), selected, "{context}: kept");
        }
    }
}
