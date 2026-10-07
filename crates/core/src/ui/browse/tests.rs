//! Spec 0006 AC9–AC16: the client model's pages, windows, scrolling
//! loads, playing and queueing, popups, playlist editing, `Esc` and the
//! disconnected state.

use std::time::Duration;

use crate::item::Item;
use crate::library::{
    AlbumKind, AlbumSummary, CreditedTrack, FavoriteKind, LibraryRequest, LibraryResponse,
    ListItems, ListPage, ListRef, PageData, PageRequest, PlaylistSummary, RoleCategory,
};
use crate::protocol::RepeatMode;
use crate::protocol::{self, Command, InsertAt, PlaybackState, PlayerSnapshot, QueueEntry};
use crate::track::{AlbumRef, ArtistRef, EntryId, Track, TrackId};

use super::super::{
    Action, Confirmed, DISCONNECTED, Effect, Key, Load, MenuAction, PLAYLIST_CHANGED, PageKind,
    Popup, SHUT_DOWN, State, TrackSource, Window, update,
};

// --- fixtures ----------------------------------------------------------------

const GL: [Key; 2] = [Key::Char('g'), Key::Char('l')];
const GY: [Key; 2] = [Key::Char('g'), Key::Char('y')];
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
        artists: vec![artist(20), artist(21)],
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

fn playlists(range: std::ops::Range<u32>) -> Vec<PlaylistSummary> {
    range
        .map(|i| playlist(&format!("p{i}"), &format!("P{i}"), true))
        .collect()
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

fn fav_data(tracks: Vec<Track>, total: u32) -> PageData {
    PageData::FavoriteTracks {
        tracks: list(tracks, 0, total),
    }
}

fn playlist_data(uuid: &str, title: &str, own: bool, ids: &[u64]) -> PageData {
    PageData::Playlist {
        playlist: playlist(uuid, title, own),
        etag: Some("e1".into()),
        tracks: list(tracks(ids.iter().copied()), 0, ids.len() as u32),
    }
}

fn artist_data(id: u64) -> PageData {
    PageData::Artist {
        artist: artist(id),
        top_tracks: list(tracks(1..4), 0, 3),
        albums: list(vec![album(10)], 0, 1),
        appears_on: list(vec![album(11)], 0, 1),
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

fn with_queue(ids: &[u64], current: Option<u64>) -> State {
    let mut state = State::default();
    update(
        &mut state,
        Action::Player(protocol::Event::Player(snapshot(ids, current))),
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

/// Presses `keys`, which must ask for one page, and answers it.
fn open(state: &mut State, keys: &[Key], data: PageData) -> Vec<Effect> {
    let effects = press(state, keys);
    let (id, request) = one_request(&effects);
    assert!(matches!(request, LibraryRequest::Page(_)), "{request:?}");
    reply(state, id, LibraryResponse::Page(data))
}

fn more(list: ListRef, offset: u32, limit: u32) -> LibraryRequest {
    LibraryRequest::More {
        list,
        offset,
        limit,
    }
}

fn items_tracks(tracks: Vec<Track>, offset: u32, total: u32) -> LibraryResponse {
    LibraryResponse::Items(ListItems::Tracks(list(tracks, offset, total)))
}

fn window(state: &State, index: usize) -> &Window {
    &state.page().windows[index]
}

fn kinds(state: &State) -> Vec<PageKind> {
    state.history.iter().map(|p| p.kind.clone()).collect()
}

fn send(command: Command) -> Vec<Out> {
    vec![Out::Send(command)]
}

fn open_items(item: Item, at: InsertAt) -> Command {
    Command::Open {
        items: vec![item],
        at: Some(at),
    }
}

/// The library page, loaded, on top of the queue.
fn library() -> State {
    let mut state = State::default();
    open(&mut state, &GL, library_data());
    state
}

/// The labels of the open actions popup.
fn labels(state: &State) -> Vec<String> {
    match &state.popup {
        Some(Popup::Actions { actions, .. }) => actions.iter().map(MenuAction::label).collect(),
        other => panic!("no actions popup: {other:?}"),
    }
}

/// Opens the actions popup with `keys` and runs the action labelled
/// `label`.
fn run_action(state: &mut State, keys: &[Key], label: &str) -> Vec<Effect> {
    assert_eq!(press(state, keys), vec![], "opening the popup");
    let index = labels(state)
        .iter()
        .position(|l| l == label)
        .unwrap_or_else(|| panic!("no {label:?} in {:?}", labels(state)));
    let moves = vec![Key::Char('j'); index];
    assert_eq!(press(state, &moves), vec![]);
    let effects = press(state, &[Key::Enter]);
    assert!(
        !matches!(state.popup, Some(Popup::Actions { .. })),
        "{label}: the actions popup stayed open"
    );
    effects
}

// --- AC9 ------------------------------------------------------------------------

/// AC9: `z`, `g l`, `g y` and `Enter` on album/playlist/artist rows push
/// the page and ask for it with a fresh ID (the queue asks nothing);
/// opening the top page again does nothing; `Backspace`/`C-q` pop to the
/// page under, kept as it was, with no request; the queue is never popped;
/// the 51st push drops the oldest page above the queue.
#[test]
fn ac9_history() {
    use Key::{BackTab, Backspace, Char, Ctrl, Enter, Tab};
    // The queue is the page at start; `z` on it does nothing.
    let mut state = State::default();
    assert_eq!(kinds(&state), vec![PageKind::Queue]);
    assert_eq!(press(&mut state, &[Char('z')]), vec![]);
    assert_eq!(kinds(&state), vec![PageKind::Queue]);

    // (keys from a loaded library page, the page pushed, its request, the
    // keys that open it again)
    let cases: Vec<(Vec<Key>, PageKind, Option<PageRequest>, Vec<Key>)> = vec![
        (
            GY.to_vec(),
            PageKind::FavoriteTracks,
            Some(PageRequest::FavoriteTracks),
            GY.to_vec(),
        ),
        (vec![Char('z')], PageKind::Queue, None, vec![Char('z')]),
        (
            vec![Enter],
            PageKind::Playlist("p1".into()),
            Some(PageRequest::Playlist("p1".into())),
            vec![],
        ),
        (
            vec![Tab, Enter],
            PageKind::Album(10),
            Some(PageRequest::Album(10)),
            vec![],
        ),
        (
            vec![BackTab, Enter],
            PageKind::Artist(20),
            Some(PageRequest::Artist(20)),
            vec![],
        ),
        (
            vec![Tab, Tab, Char('j'), Enter],
            PageKind::Artist(21),
            Some(PageRequest::Artist(21)),
            vec![],
        ),
    ];
    for (keys, kind, request, again) in cases {
        let mut state = library();
        let effects = press(&mut state, &keys);
        assert_eq!(state.page().kind, kind, "{keys:?}");
        assert_eq!(state.history.len(), 3, "{keys:?}");
        match request {
            Some(request) => {
                let (id, got) = one_request(&effects);
                assert_eq!(got, LibraryRequest::Page(request), "{keys:?}");
                assert_eq!(state.page().load, Load::Loading { id }, "{keys:?}");
            }
            None => assert_eq!(effects, vec![], "{keys:?}"),
        }
        if !again.is_empty() {
            assert_eq!(press(&mut state, &again), vec![], "{keys:?} again");
            assert_eq!(state.history.len(), 3, "{keys:?} again");
        }
    }

    // Each request has a fresh ID.
    let mut state = State::default();
    let ids: Vec<u64> = [GL, GY, GL]
        .iter()
        .map(|keys| one_request(&press(&mut state, keys)).0)
        .collect();
    assert!(ids[0] < ids[1] && ids[1] < ids[2], "{ids:?}");

    // Back: the page under shows its rows, cursors and focus as they were,
    // without a request.
    let mut state = library();
    press(&mut state, &[Tab, Char('j'), Char('j')]);
    let kept = state.page().clone();
    assert_eq!((kept.focus, kept.windows[1].cursor), (1, 2));
    assert_eq!(requests(&press(&mut state, &[Enter])).len(), 1);
    assert_eq!(state.page().kind, PageKind::Album(12));
    assert_eq!(press(&mut state, &[Backspace]), vec![]);
    assert_eq!(state.page(), &kept);
    press(&mut state, &GY);
    assert_eq!(press(&mut state, &[Ctrl('q')]), vec![]);
    assert_eq!(state.page(), &kept);
    // The queue at the bottom is never popped.
    assert_eq!(
        press(&mut state, &[Backspace, Backspace, Ctrl('q')]),
        vec![]
    );
    assert_eq!(kinds(&state), vec![PageKind::Queue]);

    // At most 50 pages above the queue: the 51st push drops the oldest.
    let mut state = State::default();
    for i in 0..51 {
        press(&mut state, if i % 2 == 0 { &GL } else { &GY });
    }
    assert_eq!(state.history.len(), 51);
    assert_eq!(state.history[0].kind, PageKind::Queue);
    assert_eq!(state.history[1].kind, PageKind::FavoriteTracks);
    assert_eq!(state.page().kind, PageKind::Library);
}

/// AC9: a reply applies to the page or window that asked with its ID,
/// also below the top; one with an unknown or answered ID is dropped.
#[test]
fn ac9_reply_by_id() {
    // The library asked (a), then the favorite tracks on top of it (b).
    let setup = || {
        let mut state = State::default();
        let a = one_request(&press(&mut state, &GL)).0;
        let b = one_request(&press(&mut state, &GY)).0;
        (state, a, b)
    };
    enum Who {
        Library,
        Top,
        Unknown,
    }
    type Check = fn(&State);
    let cases: Vec<(&str, Who, Result<LibraryResponse, String>, Check)> = vec![
        (
            "the library's page lands below the top",
            Who::Library,
            Ok(LibraryResponse::Page(library_data())),
            |s| {
                assert_eq!(s.history[1].load, Load::Idle);
                assert_eq!(s.history[1].windows[0].rows.len(), 3);
                assert!(matches!(s.page().load, Load::Loading { .. }));
                assert!(s.page().windows[0].rows.is_empty());
            },
        ),
        (
            "the top's page lands on the top",
            Who::Top,
            Ok(LibraryResponse::Page(fav_data(tracks(1..4), 3))),
            |s| {
                assert_eq!(s.page().load, Load::Idle);
                assert_eq!(s.page().windows[0].rows.len(), 3);
                assert!(matches!(s.history[1].load, Load::Loading { .. }));
            },
        ),
        (
            "an error fails that page only",
            Who::Library,
            Err("Could not reach Tidal: timed out".into()),
            |s| {
                assert_eq!(
                    s.history[1].load,
                    Load::Failed(
                        "Could not load the library: Could not reach Tidal: timed out".into()
                    )
                );
                assert!(matches!(s.page().load, Load::Loading { .. }));
            },
        ),
        (
            "an album's error is shown as the player words it",
            Who::Top,
            Err("Album 1 was not found".into()),
            |s| assert_eq!(s.page().load, Load::Failed("Album 1 was not found".into())),
        ),
        (
            "an unknown ID is dropped",
            Who::Unknown,
            Ok(LibraryResponse::Page(library_data())),
            |s| {
                assert!(matches!(s.history[1].load, Load::Loading { .. }));
                assert!(matches!(s.page().load, Load::Loading { .. }));
                assert!(s.history[1].windows[0].rows.is_empty());
            },
        ),
    ];
    for (name, who, result, check) in cases {
        let (mut state, a, b) = setup();
        let id = match who {
            Who::Library => a,
            Who::Top => b,
            Who::Unknown => a + b + 100,
        };
        assert_eq!(
            update(&mut state, Action::LibraryReply { id, result }),
            vec![],
            "{name}"
        );
        check(&state);
    }

    // A second reply with an answered ID is dropped.
    let (mut state, a, _) = setup();
    reply(&mut state, a, LibraryResponse::Page(library_data()));
    let before = state.clone();
    reply(
        &mut state,
        a,
        LibraryResponse::Page(PageData::Library {
            playlists: list(vec![], 0, 0),
            albums: list(vec![], 0, 0),
            artists: list(vec![], 0, 0),
        }),
    );
    assert_eq!(state, before);

    // A window's `More` reply lands on that window only, below the top.
    let mut state = State::default();
    open(
        &mut state,
        &GL,
        PageData::Library {
            playlists: list(playlists(0..50), 0, 120),
            albums: list(vec![album(10)], 0, 1),
            artists: list(vec![artist(20)], 0, 1),
        },
    );
    let (id, _) = one_request(&press(&mut state, &[Key::Char('G')]));
    press(&mut state, &GY);
    reply(
        &mut state,
        id,
        LibraryResponse::Items(ListItems::Playlists(list(playlists(50..100), 50, 120))),
    );
    let library = &state.history[1];
    assert_eq!(library.windows[0].rows.len(), 100);
    assert_eq!(library.windows[0].load, Load::Idle);
    assert_eq!(library.windows[1].rows.len(), 1);
    assert!(state.page().windows[0].rows.is_empty());
}

// --- AC10 -----------------------------------------------------------------------

/// AC10: a cursor move within one window height of the last loaded row
/// asks for the next page once; rows are appended skipping known IDs; a
/// short page does not stop later loads; a failed page shows its message
/// and the next move asks again; `G` goes to the last loaded row; *All
/// tracks* asks when first focused; the role filter.
#[test]
fn ac10_scroll_loads() {
    use Key::{Char, Enter, Esc, Tab};
    let more_tracks = |offset| more(ListRef::FavoriteTracks, offset, 100);
    let mut state = State {
        list_height: 5,
        ..State::default()
    };
    open(&mut state, &GY, fav_data(tracks(0..100), 362));
    // Down to row 93: more than a window height above row 99.
    for _ in 0..93 {
        assert_eq!(press(&mut state, &[Char('j')]), vec![]);
    }
    // Row 94 is within 5 rows of row 99: the next page, once.
    let (id, request) = one_request(&press(&mut state, &[Char('j')]));
    assert_eq!(request, more_tracks(100));
    assert_eq!(window(&state, 0).load, Load::Loading { id });
    assert_eq!(press(&mut state, &[Char('j')]), vec![], "asked twice");
    // Appended, skipping the IDs already loaded (98 and 99 again).
    assert_eq!(
        reply(&mut state, id, items_tracks(tracks(98..200), 100, 362)),
        vec![]
    );
    let ids: Vec<u64> = window(&state, 0).tracks().iter().map(|t| t.id.0).collect();
    assert_eq!(ids, (0..200).collect::<Vec<_>>());
    // `G` goes to the last loaded row, and so asks for the next page.
    let (id, request) = one_request(&press(&mut state, &[Char('G')]));
    assert_eq!(window(&state, 0).cursor, 199);
    assert_eq!(request, more_tracks(200));
    // A short page (90 of 100) before the total does not stop the loads.
    reply(&mut state, id, items_tracks(tracks(200..290), 200, 362));
    let (id, request) = one_request(&press(&mut state, &[Char('G')]));
    assert_eq!(request, more_tracks(300));
    // A failed page keeps the rows and shows its message; the next move
    // near the end asks again.
    let message = "Tidal answered 429: try again in a moment";
    assert_eq!(fail(&mut state, id, message), vec![]);
    assert_eq!(window(&state, 0).load, Load::Failed(message.into()));
    assert_eq!(window(&state, 0).rows.len(), 290);
    let (id, request) = one_request(&press(&mut state, &[Char('k')]));
    assert_eq!(request, more_tracks(300));
    // The last page: nothing more to ask.
    reply(&mut state, id, items_tracks(tracks(300..362), 300, 362));
    assert!(window(&state, 0).complete());
    assert_eq!(
        press(&mut state, &[Char('G'), Char('k'), Char('j')]),
        vec![]
    );
    // 290..300 never came (the short page): 352 rows.
    assert_eq!(window(&state, 0).cursor, 351);

    // The page size and each endpoint's largest page:
    // (page size, keys, first page, the request `G` makes)
    let fav = |n: u64, total| fav_data(tracks(0..n), total);
    let lib = |n: u32, total| PageData::Library {
        playlists: list(playlists(0..n), 0, total),
        albums: list(vec![], 0, 0),
        artists: list(vec![], 0, 0),
    };
    let cases: Vec<(u32, [Key; 2], PageData, Option<LibraryRequest>)> = vec![
        (100, GY, fav(100, 362), Some(more_tracks(100))),
        (
            30,
            GY,
            fav(30, 362),
            Some(more(ListRef::FavoriteTracks, 30, 30)),
        ),
        (100, GY, fav(100, 100), None),
        (
            100,
            GL,
            lib(50, 120),
            Some(more(ListRef::Playlists, 50, 50)),
        ),
        (20, GL, lib(20, 120), Some(more(ListRef::Playlists, 20, 20))),
    ];
    for (page_size, keys, data, want) in cases {
        let mut state = State {
            page_size,
            ..State::default()
        };
        open(&mut state, &keys, data);
        let got = requests(&press(&mut state, &[Char('G')]));
        assert_eq!(
            got.into_iter().map(|(_, r)| r).collect::<Vec<_>>(),
            want.into_iter().collect::<Vec<_>>(),
            "page size {page_size}, {keys:?}"
        );
    }

    // *All tracks* asks for its first page when first focused, once.
    let mut state = library();
    press(&mut state, &[Tab, Tab]);
    let effects = open(&mut state, &[Enter], artist_data(20));
    assert_eq!(effects, vec![], "credits asked with the page");
    assert_eq!(press(&mut state, &[Tab, Tab]), vec![]);
    let (id, request) = one_request(&press(&mut state, &[Tab]));
    assert_eq!(request, more(ListRef::Credits(20), 0, 50));
    // Rows: even IDs performer, odd songwriter, 7 producer.
    let credits: Vec<CreditedTrack> = (0..50)
        .map(|i| CreditedTrack {
            track: track(i),
            roles: vec![match i {
                7 => RoleCategory::Producer,
                i if i % 2 == 0 => RoleCategory::Performer,
                _ => RoleCategory::Songwriter,
            }],
        })
        .collect();
    let page = ListPage {
        items: credits,
        offset: 0,
        total: 548,
        hidden: 37,
    };
    reply(
        &mut state,
        id,
        LibraryResponse::Items(ListItems::Credits(page)),
    );
    assert_eq!(press(&mut state, &[Tab, Tab, Tab, Tab]), vec![]);
    assert_eq!(window(&state, 3).title(), "All tracks (548 · 37 hidden)");
    // `f` opens the filter, all checked; `Esc` cancels it.
    assert_eq!(press(&mut state, &[Char('f')]), vec![]);
    assert_eq!(
        state.popup,
        Some(Popup::Roles {
            checked: [true; 4],
            cursor: 0
        })
    );
    assert_eq!(press(&mut state, &[Char(' '), Esc]), vec![]);
    assert_eq!(state.popup, None);
    assert_eq!(window(&state, 3).roles, [true; 4]);
    assert_eq!(window(&state, 3).len(), 50);
    // Performer and Songwriter off: only the producer row is left, near
    // the cursor, so the next page is asked for.
    let effects = press(
        &mut state,
        &[Char('f'), Char(' '), Char('j'), Char(' '), Enter],
    );
    assert_eq!(state.popup, None);
    assert_eq!(window(&state, 3).roles, [false, false, true, true]);
    assert_eq!(window(&state, 3).len(), 1);
    assert_eq!(
        window(&state, 3)
            .selected()
            .and_then(|r| r.track())
            .map(|t| t.id),
        Some(TrackId(7))
    );
    assert_eq!(
        window(&state, 3).title(),
        "All tracks (548 · Producer, Engineer · 37 hidden)"
    );
    let (_, request) = one_request(&effects);
    assert_eq!(request, more(ListRef::Credits(20), 50, 50));
    // `f` is the filter's key in *All tracks* only.
    assert_eq!(press(&mut state, &[Tab, Char('f')]), vec![]);
    assert_eq!(state.popup, None);
}

// --- AC11 -----------------------------------------------------------------------

/// AC11: `Tab`/`BackTab` cycle the focus, wrapping; cursor keys move the
/// focused window's cursor only, clamped; cursors survive leaving the
/// page; a loading, failed or empty window ignores them.
#[test]
fn ac11_windows_and_cursors() {
    use Key::{BackTab, Backspace, Char, Ctrl, Down, Enter, PageDown, PageUp, Tab, Up};
    let mut state = library();
    for (key, focus) in [
        (Tab, 1),
        (Tab, 2),
        (Tab, 0),
        (BackTab, 2),
        (BackTab, 1),
        (BackTab, 0),
    ] {
        assert_eq!(press(&mut state, &[key]), vec![], "{key:?}");
        assert_eq!(state.page().focus, focus, "{key:?}");
    }

    state.list_height = 2;
    let cursors = |s: &State| {
        s.page()
            .windows
            .iter()
            .map(|w| w.cursor)
            .collect::<Vec<_>>()
    };
    // (keys, every window's cursor after them), in sequence
    let steps: Vec<(Vec<Key>, [usize; 3])> = vec![
        (vec![Char('j')], [1, 0, 0]),
        (vec![Down], [2, 0, 0]),
        (vec![Char('j')], [2, 0, 0]),
        (vec![Char('k')], [1, 0, 0]),
        (vec![Up], [0, 0, 0]),
        (vec![Up], [0, 0, 0]),
        (vec![Char('G')], [2, 0, 0]),
        (vec![Char('g'), Char('g')], [0, 0, 0]),
        (vec![PageDown], [2, 0, 0]),
        (vec![PageUp], [0, 0, 0]),
        (vec![Ctrl('f')], [2, 0, 0]),
        (vec![Ctrl('b')], [0, 0, 0]),
        (vec![Tab, Char('j')], [0, 1, 0]),
        (vec![Tab, Char('G')], [0, 1, 2]),
    ];
    for (keys, want) in steps {
        assert_eq!(press(&mut state, &keys), vec![], "{keys:?}");
        assert_eq!(cursors(&state), want, "{keys:?}");
    }
    // Leaving the page and coming back keeps cursors and focus.
    press(&mut state, &[Enter]);
    assert_eq!(state.page().kind, PageKind::Artist(22));
    press(&mut state, &[Backspace]);
    assert_eq!(cursors(&state), [0, 1, 2]);
    assert_eq!(state.page().focus, 2);

    // A loading, failed or empty window ignores the cursor keys; a page
    // with one window keeps its focus.
    let keys = [Char('j'), Char('G'), PageDown, Down, Tab];
    let mut loading = State::default();
    let (id, _) = one_request(&press(&mut loading, &GY));
    assert_eq!(press(&mut loading, &keys), vec![]);
    assert_eq!((window(&loading, 0).cursor, loading.page().focus), (0, 0));
    fail(&mut loading, id, "Could not reach Tidal: down");
    assert_eq!(press(&mut loading, &keys), vec![]);
    assert_eq!(window(&loading, 0).cursor, 0);
    let mut empty = State::default();
    open(&mut empty, &GY, fav_data(vec![], 0));
    assert_eq!(press(&mut empty, &keys), vec![]);
    assert_eq!(window(&empty, 0).cursor, 0);
    assert_eq!(window(&empty, 0).empty_message(), "No favorite tracks yet");
}

// --- AC12 -----------------------------------------------------------------------

/// AC12: every row of "Playing and queueing from a page".
#[test]
fn ac12_play_and_queue() {
    use Key::{Char, Ctrl, Enter, Esc, Tab};
    // `Enter` on track i of a whole list: the list in page order, never
    // sorted, from i.
    let order = [5, 3, 9, 1, 7];
    for start in 0..order.len() {
        let mut state = State::default();
        open(&mut state, &GY, fav_data(tracks(order), 5));
        press(&mut state, &vec![Char('j'); start]);
        assert_eq!(
            outs(&press(&mut state, &[Enter])),
            send(Command::LoadQueue {
                tracks: tracks(order),
                start
            }),
            "start {start}"
        );
    }

    // A partly loaded list: the rest first, then one `LoadQueue`.
    let mut state = State::default();
    open(&mut state, &GY, fav_data(tracks(0..100), 250));
    press(&mut state, &[Char('j'), Char('j'), Char('j')]);
    let (id, request) = one_request(&press(&mut state, &[Enter]));
    assert_eq!(request, more(ListRef::FavoriteTracks, 100, 100));
    assert_eq!(state.message(), Some("Loading 100 of 250…"));
    let effects = reply(&mut state, id, items_tracks(tracks(100..200), 100, 250));
    let (id, request) = one_request(&effects);
    assert_eq!(request, more(ListRef::FavoriteTracks, 200, 100));
    assert_eq!(state.message(), Some("Loading 200 of 250…"));
    let effects = reply(&mut state, id, items_tracks(tracks(200..250), 200, 250));
    assert_eq!(
        outs(&effects),
        send(Command::LoadQueue {
            tracks: tracks(0..250),
            start: 3
        })
    );
    assert_eq!(state.whole_list, None);
    assert_eq!(state.message(), None);
    assert_eq!(press(&mut state, &[Char('k')]), vec![], "sent twice");

    // `Esc` while loading cancels: nothing is sent.
    let mut state = State::default();
    open(&mut state, &GY, fav_data(tracks(0..100), 250));
    let (id, _) = one_request(&press(&mut state, &[Enter]));
    assert_eq!(press(&mut state, &[Esc]), vec![]);
    assert_eq!((state.whole_list.clone(), state.message()), (None, None));
    let effects = reply(&mut state, id, items_tracks(tracks(100..200), 100, 250));
    assert!(
        !outs(&effects).iter().any(|o| matches!(o, Out::Send(_))),
        "{effects:?}"
    );

    // Above 40 000 tracks: refused, nothing asked or sent.
    let mut state = State::default();
    open(&mut state, &GY, fav_data(tracks(0..100), 40_001));
    assert_eq!(press(&mut state, &[Enter]), vec![]);
    assert_eq!(
        state.message(),
        Some("Too many tracks to queue at once (40 001)")
    );
    assert_eq!(state.whole_list, None);

    // `Z`/`C-z`: (page, keys, what is sent)
    let fav = || {
        let mut state = State::default();
        open(&mut state, &GY, fav_data(tracks([4, 6]), 2));
        state
    };
    let cases: Vec<(fn() -> State, Vec<Key>, Vec<Out>)> = vec![
        (
            fav,
            vec![Char('Z')],
            send(open_items(Item::Track(TrackId(4)), InsertAt::End)),
        ),
        (
            fav,
            vec![Char('j'), Ctrl('z')],
            send(open_items(Item::Track(TrackId(6)), InsertAt::End)),
        ),
        (
            library,
            vec![Char('Z')],
            send(open_items(Item::Playlist("p1".into()), InsertAt::End)),
        ),
        (
            library,
            vec![Tab, Ctrl('z')],
            send(open_items(Item::Album(10), InsertAt::End)),
        ),
        (library, vec![Tab, Tab, Char('Z')], vec![]),
        (library, vec![Tab, Tab, Ctrl('z')], vec![]),
        (fav, vec![Char('d')], vec![]),
    ];
    for (setup, keys, want) in cases {
        let mut state = setup();
        assert_eq!(outs(&press(&mut state, &keys)), want, "{keys:?}");
    }

    // The queue: `d` removes the entry under the cursor, by ID; `Enter`
    // plays it (0004).
    let mut state = with_queue(&[7, 3, 9], Some(3));
    assert_eq!(
        outs(&press(&mut state, &[Char('d')])),
        send(Command::RemoveFromQueue(EntryId(3)))
    );
    assert_eq!(
        outs(&press(&mut state, &[Char('j'), Char('d')])),
        send(Command::RemoveFromQueue(EntryId(9)))
    );
    assert_eq!(
        outs(&press(&mut state, &[Enter])),
        send(Command::PlayEntry(EntryId(9)))
    );
    // `d` on an empty queue does nothing.
    assert_eq!(press(&mut State::default(), &[Char('d')]), vec![]);
}

// --- AC13 -----------------------------------------------------------------------

/// Favorite tracks: track 1 with two artists, track 2 without an album.
fn fav_page() -> State {
    let mut one = track(1);
    one.artists.push(artist(21));
    let mut two = track(2);
    two.album = None;
    let mut state = State::default();
    open(&mut state, &GY, fav_data(vec![one, two], 2));
    state
}

/// The playlist page of p1 (`own`), tracks 4 and 6, ETag `e1`.
fn playlist_page(own: bool) -> State {
    let mut state = library();
    open(
        &mut state,
        &[Key::Enter],
        playlist_data("p1", "Mine", own, &[4, 6]),
    );
    state
}

/// AC13: the actions per item kind and order; `Enter` runs one and
/// closes the popup; `Esc` closes it; while open no other key acts.
#[test]
fn ac13_actions_popup() {
    use Key::{Char, Ctrl, Esc, Tab};
    use MenuAction as M;
    let fav_track = |id| M::AddFavorite(FavoriteKind::Track, id);
    let unfav_track = |id| M::RemoveFavorite(FavoriteKind::Track, id);
    let mut one = track(1);
    one.artists.push(artist(21));
    let mut two = track(2);
    two.album = None;
    let browse_track = |t: &Track| {
        let id = t.id.to_string();
        let mut actions: Vec<M> = t.album.iter().map(|a| M::GoToAlbum(a.id)).collect();
        actions.extend(t.artists.iter().cloned().map(M::GoToArtist));
        actions.extend([
            M::AddToQueue(Item::Track(t.id)),
            M::PlayNext(Item::Track(t.id)),
            fav_track(id.clone()),
            unfav_track(id),
            M::AddToPlaylist(TrackSource::Tracks(vec![t.clone()])),
        ]);
        actions
    };
    let mut own_playlist_track = browse_track(&track(6));
    own_playlist_track.push(M::RemoveFromPlaylist {
        uuid: "p1".into(),
        title: "Mine".into(),
        index: 1,
        etag: Some("e1".into()),
    });
    let entry = track(103);
    let queue_entry = vec![
        M::GoToAlbum(10),
        M::GoToArtist(artist(20)),
        M::PlayNext(Item::Track(TrackId(103))),
        M::RemoveFromQueue(EntryId(3)),
        fav_track("103".into()),
        unfav_track("103".into()),
        M::AddToPlaylist(TrackSource::Tracks(vec![entry.clone()])),
    ];
    let mut playing = queue_entry.clone();
    playing.remove(2);
    let album_row = vec![
        M::Open(PageKind::Album(10)),
        M::GoToArtist(artist(20)),
        M::GoToArtist(artist(21)),
        M::AddToQueue(Item::Album(10)),
        M::PlayNext(Item::Album(10)),
        M::AddFavorite(FavoriteKind::Album, "10".into()),
        M::RemoveFavorite(FavoriteKind::Album, "10".into()),
        M::AddToPlaylist(TrackSource::Album(10)),
    ];
    let p1 = Item::Playlist("p1".into());
    let p2 = Item::Playlist("p2".into());
    let queue = || with_queue(&[7, 3, 9], Some(3));
    let fav_playing = || {
        let mut state = fav_page();
        update(
            &mut state,
            Action::Player(protocol::Event::Player(snapshot(&[7, 3, 9], Some(3)))),
        );
        state
    };

    // (name, page, keys, the popup's title and actions)
    type Case = (
        &'static str,
        fn() -> State,
        Vec<Key>,
        Option<(String, Vec<M>)>,
    );
    let cases: Vec<Case> = vec![
        (
            "track with two artists",
            fav_page,
            GA.to_vec(),
            Some(("T1".into(), browse_track(&one))),
        ),
        (
            "C-Space",
            fav_page,
            vec![Ctrl(' ')],
            Some(("T1".into(), browse_track(&one))),
        ),
        (
            "track without an album",
            fav_page,
            vec![Char('j'), Char('g'), Char('a')],
            Some(("T2".into(), browse_track(&two))),
        ),
        (
            "own playlist's track",
            || playlist_page(true),
            vec![Char('j'), Ctrl(' ')],
            Some(("T6".into(), own_playlist_track)),
        ),
        (
            "followed playlist's track",
            || playlist_page(false),
            vec![Char('j'), Ctrl(' ')],
            Some(("T6".into(), browse_track(&track(6)))),
        ),
        (
            "album",
            library,
            vec![Tab, Ctrl(' ')],
            Some(("Al10".into(), album_row)),
        ),
        (
            "own playlist",
            library,
            vec![Ctrl(' ')],
            Some((
                "Mine".into(),
                vec![
                    M::Open(PageKind::Playlist("p1".into())),
                    M::AddToQueue(p1.clone()),
                    M::PlayNext(p1),
                    M::DeletePlaylist {
                        uuid: "p1".into(),
                        title: "Mine".into(),
                    },
                ],
            )),
        ),
        (
            "followed playlist",
            library,
            vec![Char('j'), Ctrl(' ')],
            Some((
                "Theirs".into(),
                vec![
                    M::Open(PageKind::Playlist("p2".into())),
                    M::AddToQueue(p2.clone()),
                    M::PlayNext(p2),
                    M::AddFavorite(FavoriteKind::Playlist, "p2".into()),
                    M::RemoveFavorite(FavoriteKind::Playlist, "p2".into()),
                ],
            )),
        ),
        (
            "artist",
            library,
            vec![Tab, Tab, Ctrl(' ')],
            Some((
                "Ar20".into(),
                vec![
                    M::Open(PageKind::Artist(20)),
                    M::AddFavorite(FavoriteKind::Artist, "20".into()),
                    M::RemoveFavorite(FavoriteKind::Artist, "20".into()),
                ],
            )),
        ),
        (
            "queue entry",
            queue,
            GA.to_vec(),
            Some(("T103".into(), queue_entry)),
        ),
        (
            "the playing track, from a page",
            fav_playing,
            vec![Char('a')],
            Some(("T103".into(), playing)),
        ),
        ("nothing playing", fav_page, vec![Char('a')], None),
    ];
    for (name, setup, keys, want) in cases {
        let mut state = setup();
        assert_eq!(press(&mut state, &keys), vec![], "{name}");
        let got = state.popup.clone().map(|popup| match popup {
            Popup::Actions {
                title,
                actions,
                cursor,
            } => {
                assert_eq!(cursor, 0, "{name}");
                (title, actions)
            }
            other => panic!("{name}: {other:?}"),
        });
        assert_eq!(got, want, "{name}");
    }

    // `Enter` runs the action and closes the popup:
    // (page, keys that open the popup, label, what is asked, the page on top)
    type Run = (fn() -> State, Vec<Key>, &'static str, Vec<Out>, PageKind);
    let runs: Vec<Run> = vec![
        (
            fav_page,
            GA.to_vec(),
            "Go to album",
            vec![Out::Library(LibraryRequest::Page(PageRequest::Album(10)))],
            PageKind::Album(10),
        ),
        (
            fav_page,
            GA.to_vec(),
            "Go to artist: Ar21",
            vec![Out::Library(LibraryRequest::Page(PageRequest::Artist(21)))],
            PageKind::Artist(21),
        ),
        (
            fav_page,
            GA.to_vec(),
            "Add to queue",
            send(open_items(Item::Track(TrackId(1)), InsertAt::End)),
            PageKind::FavoriteTracks,
        ),
        (
            fav_page,
            GA.to_vec(),
            "Play next",
            send(open_items(Item::Track(TrackId(1)), InsertAt::Next)),
            PageKind::FavoriteTracks,
        ),
        (
            fav_page,
            GA.to_vec(),
            "Add to favorites",
            vec![Out::Library(LibraryRequest::AddFavorite(
                FavoriteKind::Track,
                "1".into(),
            ))],
            PageKind::FavoriteTracks,
        ),
        (
            fav_page,
            GA.to_vec(),
            "Remove from favorites",
            vec![Out::Library(LibraryRequest::RemoveFavorite(
                FavoriteKind::Track,
                "1".into(),
            ))],
            PageKind::FavoriteTracks,
        ),
        (
            library,
            vec![Tab, Ctrl(' ')],
            "Add to favorites",
            vec![Out::Library(LibraryRequest::AddFavorite(
                FavoriteKind::Album,
                "10".into(),
            ))],
            PageKind::Library,
        ),
        (
            library,
            vec![Tab, Tab, Ctrl(' ')],
            "Open",
            vec![Out::Library(LibraryRequest::Page(PageRequest::Artist(20)))],
            PageKind::Artist(20),
        ),
        (
            library,
            vec![Ctrl(' ')],
            "Play next",
            send(open_items(Item::Playlist("p1".into()), InsertAt::Next)),
            PageKind::Library,
        ),
        (
            queue,
            GA.to_vec(),
            "Remove from queue",
            send(Command::RemoveFromQueue(EntryId(3))),
            PageKind::Queue,
        ),
    ];
    for (setup, keys, label, want, top) in runs {
        let mut state = setup();
        let got = run_action(&mut state, &keys, label);
        assert_eq!(outs(&got), want, "{label}");
        assert_eq!(state.popup, None, "{label}");
        assert_eq!(state.page().kind, top, "{label}");
    }

    // `j`/`k` move, clamped; `Esc` closes with no effect; while open no
    // other key acts.
    let mut state = fav_page();
    press(&mut state, &GA);
    let cursor = |s: &State| match &s.popup {
        Some(Popup::Actions { cursor, .. }) => *cursor,
        other => panic!("{other:?}"),
    };
    assert_eq!(press(&mut state, &[Char('k')]), vec![]);
    assert_eq!(cursor(&state), 0);
    press(&mut state, &vec![Char('j'); 20]);
    assert_eq!(cursor(&state), 7);
    let before = state.clone();
    assert_eq!(
        press(
            &mut state,
            &[Char(' '), Char('n'), Char('q'), Char('z'), Tab, Ctrl('c')]
        ),
        vec![]
    );
    assert_eq!(state, before);
    assert_eq!(press(&mut state, &[Esc]), vec![]);
    assert_eq!(state.popup, None);
    assert_eq!(state.page().kind, PageKind::FavoriteTracks);
}

// --- AC14 -----------------------------------------------------------------------

/// Queue, Library, playlist Mine (p1, own, tracks 4 and 6, ETag e1),
/// favorite tracks 1 and 6 on top.
fn editing() -> State {
    let mut state = playlist_page(true);
    open(&mut state, &GY, fav_data(tracks([1, 6]), 2));
    state
}

/// Runs *Add to playlist…* from `keys`, answers the playlists with Mine
/// (own), Theirs (followed) and Other (own), and checks the list.
fn add_to_playlist(state: &mut State, keys: &[Key]) {
    let effects = run_action(state, keys, "Add to playlist…");
    let (id, request) = one_request(&effects);
    assert_eq!(request, more(ListRef::Playlists, 0, 50));
    let answer = list(
        vec![
            playlist("p1", "Mine", true),
            playlist("p2", "Theirs", false),
            playlist("p3", "Other", true),
        ],
        0,
        3,
    );
    assert_eq!(
        reply(
            state,
            id,
            LibraryResponse::Items(ListItems::Playlists(answer))
        ),
        vec![]
    );
    let popup = state.popup.as_ref().expect("the playlists popup");
    let titles: Vec<&str> = popup
        .own_playlists()
        .iter()
        .map(|p| p.title.as_str())
        .collect();
    assert_eq!(titles, ["Mine", "Other"]);
}

/// Answers the one request among `effects` with `response`.
fn answer(state: &mut State, effects: &[Effect], response: LibraryResponse) -> Vec<Effect> {
    let (id, _) = one_request(effects);
    reply(state, id, response)
}

fn page(request: PageRequest) -> Out {
    Out::Library(LibraryRequest::Page(request))
}

fn add(uuid: &str, ids: &[u64], allow_duplicates: bool) -> Out {
    Out::Library(LibraryRequest::AddToPlaylist {
        uuid: uuid.into(),
        tracks: ids.iter().copied().map(TrackId).collect(),
        allow_duplicates,
    })
}

/// AC14: playlist editing and favorites in the model: requests, questions,
/// messages and re-fetches.
#[test]
fn ac14_playlist_editing() {
    use Key::{Backspace, Char, Ctrl, Enter, Esc, Tab};
    type Scenario = (&'static str, fn());
    let scenarios: Vec<Scenario> = vec![
        ("add a track to an own playlist", || {
            let mut state = editing();
            add_to_playlist(&mut state, &GA);
            let effects = press(&mut state, &[Char('j'), Enter]);
            assert_eq!(outs(&effects), vec![add("p1", &[1], false)]);
            assert_eq!(state.popup, None);
            // Success: the message, and the playlist's page fetched again.
            let effects = answer(&mut state, &effects, LibraryResponse::Done);
            assert_eq!(state.message(), Some("Added 1 track to Mine"));
            assert_eq!(
                outs(&effects),
                vec![page(PageRequest::Playlist("p1".into()))]
            );
            assert!(matches!(state.history[2].load, Load::Loading { .. }));
        }),
        ("a duplicate asks first", || {
            let mut state = editing();
            add_to_playlist(&mut state, &[Char('j'), Char('g'), Char('a')]);
            assert_eq!(press(&mut state, &[Char('j'), Enter]), vec![]);
            assert_eq!(
                state.popup,
                Some(Popup::Confirm {
                    question: "Already in Mine: add again? (y/n)".into(),
                    on_yes: Confirmed::AddToPlaylist {
                        uuid: "p1".into(),
                        title: "Mine".into(),
                        tracks: tracks([6]),
                    },
                })
            );
            // Other keys do nothing; `n` sends nothing.
            assert_eq!(
                press(&mut state, &[Char('x'), Char('q'), Char('n')]),
                vec![]
            );
            assert_eq!(state.popup, None);
            // `y` adds it again.
            add_to_playlist(&mut state, &[Char('g'), Char('a')]);
            press(&mut state, &[Char('j'), Enter]);
            assert_eq!(
                outs(&press(&mut state, &[Char('y')])),
                vec![add("p1", &[6], true)]
            );
            assert_eq!(state.popup, None);
        }),
        ("a new playlist", || {
            let mut state = editing();
            add_to_playlist(&mut state, &GA);
            assert_eq!(press(&mut state, &[Enter]), vec![]);
            assert!(matches!(state.popup, Some(Popup::NewPlaylist { .. })));
            press(&mut state, &typed("Roadd"));
            press(&mut state, &[Backspace]);
            let effects = press(&mut state, &[Enter]);
            assert_eq!(
                outs(&effects),
                vec![Out::Library(LibraryRequest::CreatePlaylist {
                    title: "Road".into()
                })]
            );
            assert_eq!(state.popup, None);
            let created = LibraryResponse::Created(playlist("p9", "Road", true));
            let effects = answer(&mut state, &effects, created);
            assert_eq!(state.message(), Some("Created Road"));
            let got = outs(&effects);
            assert_eq!(got.len(), 2, "{got:?}");
            assert!(got.contains(&add("p9", &[1], false)), "{got:?}");
            assert!(got.contains(&page(PageRequest::Library)), "{got:?}");
            // An empty name: nothing.
            add_to_playlist(&mut state, &GA);
            assert_eq!(press(&mut state, &[Enter, Char(' '), Enter]), vec![]);
            assert_eq!(state.popup, None);
            // `Esc` closes the name prompt.
            add_to_playlist(&mut state, &GA);
            assert_eq!(press(&mut state, &[Enter, Char('x'), Esc]), vec![]);
            assert_eq!(state.popup, None);
        }),
        ("an album's tracks, fetched whole first", || {
            let mut state = library();
            add_to_playlist(&mut state, &[Tab, Ctrl(' ')]);
            let effects = press(&mut state, &[Char('j'), Enter]);
            let (id, request) = one_request(&effects);
            assert_eq!(request, more(ListRef::AlbumTracks(10), 0, 100));
            let effects = reply(&mut state, id, items_tracks(tracks(1..4), 0, 3));
            assert_eq!(outs(&effects), vec![add("p1", &[1, 2, 3], false)]);
            answer(&mut state, &effects, LibraryResponse::Done);
            assert_eq!(state.message(), Some("Added 3 tracks to Mine"));
        }),
        ("remove from an own playlist", || {
            let mut state = playlist_page(true);
            let effects = run_action(
                &mut state,
                &[Char('j'), Ctrl(' ')],
                "Remove from this playlist",
            );
            assert_eq!(
                outs(&effects),
                vec![Out::Library(LibraryRequest::RemoveFromPlaylist {
                    uuid: "p1".into(),
                    index: 1,
                    etag: "e1".into(),
                })]
            );
            let effects = answer(&mut state, &effects, LibraryResponse::Done);
            assert_eq!(state.message(), Some("Removed from Mine"));
            assert_eq!(
                outs(&effects),
                vec![page(PageRequest::Playlist("p1".into()))]
            );
        }),
        ("a removal refused because the playlist changed", || {
            let mut state = playlist_page(true);
            let effects = run_action(&mut state, &[Ctrl(' ')], "Remove from this playlist");
            let (id, _) = one_request(&effects);
            let effects = fail(&mut state, id, PLAYLIST_CHANGED);
            assert_eq!(state.message(), Some(PLAYLIST_CHANGED));
            assert_eq!(
                outs(&effects),
                vec![page(PageRequest::Playlist("p1".into()))]
            );
        }),
        ("delete an own playlist", || {
            let mut state = library();
            assert_eq!(
                run_action(&mut state, &[Ctrl(' ')], "Delete playlist"),
                vec![]
            );
            assert_eq!(
                state.popup,
                Some(Popup::Confirm {
                    question: "Delete Mine? (y/n)".into(),
                    on_yes: Confirmed::DeletePlaylist {
                        uuid: "p1".into(),
                        title: "Mine".into(),
                    },
                })
            );
            let effects = press(&mut state, &[Char('y')]);
            assert_eq!(
                outs(&effects),
                vec![Out::Library(LibraryRequest::DeletePlaylist {
                    uuid: "p1".into()
                })]
            );
            let effects = answer(&mut state, &effects, LibraryResponse::Done);
            assert_eq!(state.message(), Some("Deleted Mine"));
            assert_eq!(outs(&effects), vec![page(PageRequest::Library)]);
        }),
        ("favorites: messages and re-fetches", || {
            let mut state = editing();
            let effects = run_action(&mut state, &GA, "Add to favorites");
            let effects = answer(&mut state, &effects, LibraryResponse::Done);
            assert_eq!(state.message(), Some("Added to favorites"));
            assert_eq!(outs(&effects), vec![page(PageRequest::FavoriteTracks)]);
            let mut state = library();
            let effects = run_action(&mut state, &[Tab, Ctrl(' ')], "Remove from favorites");
            let effects = answer(&mut state, &effects, LibraryResponse::Done);
            assert_eq!(state.message(), Some("Removed from favorites"));
            assert_eq!(outs(&effects), vec![page(PageRequest::Library)]);
        }),
        ("an error sets the message and changes nothing", || {
            let mut state = editing();
            let effects = run_action(&mut state, &GA, "Add to favorites");
            let (id, _) = one_request(&effects);
            let before = state.history.clone();
            assert_eq!(fail(&mut state, id, "Could not reach Tidal: down"), vec![]);
            assert_eq!(state.message(), Some("Could not reach Tidal: down"));
            assert_eq!(state.history, before);
        }),
    ];
    for (name, scenario) in scenarios {
        eprintln!("ac14: {name}");
        scenario();
    }
}

// --- AC15 -----------------------------------------------------------------------

/// AC15: `Esc` with nothing open emits nothing and changes nothing; `q`
/// and `C-c` quit; `Esc` closes a popup or the prompt and cancels a
/// whole-list load.
#[test]
fn ac15_esc_and_quit() {
    use Key::{Char, Ctrl, Enter, Esc};
    let pages: Vec<fn() -> State> = vec![State::default, || with_queue(&[1, 2], Some(1)), library];
    for setup in pages {
        let mut state = setup();
        let before = state.clone();
        assert_eq!(press(&mut state, &[Esc]), vec![]);
        assert_eq!(state, before);
        assert_eq!(press(&mut state.clone(), &[Char('q')]), vec![Effect::Quit]);
        assert_eq!(press(&mut state.clone(), &[Ctrl('c')]), vec![Effect::Quit]);
    }
    // (state, keys that open something, what is open)
    let mut state = library();
    press(&mut state, &[Ctrl(' ')]);
    assert!(state.popup.is_some());
    assert_eq!(press(&mut state, &[Esc]), vec![]);
    assert_eq!(state.popup, None);
    press(&mut state, &[Char('o')]);
    assert!(state.prompt.is_some());
    assert_eq!(press(&mut state, &[Esc]), vec![]);
    assert_eq!(state.prompt, None);
    let mut state = State::default();
    open(&mut state, &GY, fav_data(tracks(0..100), 250));
    assert_eq!(requests(&press(&mut state, &[Enter])).len(), 1);
    assert!(state.whole_list.is_some());
    assert_eq!(press(&mut state, &[Esc]), vec![]);
    assert_eq!(state.whole_list, None);
}

// --- AC16 -----------------------------------------------------------------------

/// AC16: while disconnected nothing is asked or sent, pages open failed,
/// pending loads fail, history and cursors work; the first `Welcome` after
/// re-fetches the top page if it failed that way; a session-expired error
/// shows in the page and the page can be opened again once restored.
#[test]
fn ac16_disconnected_and_login() {
    use Key::{Backspace, Char, Ctrl, Enter};
    let welcome = || Action::Welcome {
        snapshot: snapshot(&[1], Some(1)),
        login_required: false,
    };
    for (shut_down, message) in [(false, DISCONNECTED), (true, SHUT_DOWN)] {
        // A pending page load and a pending `More` fail with the message.
        let mut state = State {
            list_height: 1,
            ..State::default()
        };
        open(&mut state, &GY, fav_data(tracks(0..3), 300));
        let (more_id, _) = one_request(&press(&mut state, &[Char('j')]));
        let (page_id, _) = one_request(&press(&mut state, &GL));
        assert_eq!(
            update(&mut state, Action::Disconnected { shut_down }),
            vec![]
        );
        assert_eq!(state.page().load, Load::Failed(message.into()), "{message}");
        assert_eq!(
            state.history[1].windows[0].load,
            Load::Failed(message.into())
        );
        // Their late replies are dropped.
        reply(&mut state, page_id, LibraryResponse::Page(library_data()));
        reply(&mut state, more_id, items_tracks(tracks(3..6), 3, 300));
        assert_eq!(state.page().load, Load::Failed(message.into()));
        assert_eq!(state.history[1].windows[0].rows.len(), 3);

        // Opening a new page: failed, no request.
        assert_eq!(press(&mut state, &GY), vec![]);
        assert_eq!(state.page().kind, PageKind::FavoriteTracks);
        assert_eq!(state.page().load, Load::Failed(message.into()));
        // History and cursors work.
        assert_eq!(press(&mut state, &[Backspace, Backspace]), vec![]);
        assert_eq!(state.page().kind, PageKind::FavoriteTracks);
        assert_eq!(state.page().load, Load::Idle);
        assert_eq!(
            press(&mut state, &[Char('j'), Char('k'), Char('j')]),
            vec![]
        );
        assert_eq!(window(&state, 0).cursor, 2);
        // `Enter`, `Z`, `d` and the popup's actions emit nothing.
        assert_eq!(
            press(&mut state, &[Enter, Char('Z'), Ctrl('z'), Char('d')]),
            vec![]
        );
        for label in ["Add to queue", "Add to favorites", "Go to album"] {
            assert_eq!(run_action(&mut state, &GA, label), vec![], "{label}");
            if label == "Go to album" {
                assert_eq!(state.page().load, Load::Failed(message.into()));
                press(&mut state, &[Backspace]);
            }
        }

        // The first `Welcome` re-fetches the top page that failed that way.
        press(&mut state, &GL);
        let effects = update(&mut state, welcome());
        let (_, request) = one_request(&effects);
        assert_eq!(request, LibraryRequest::Page(PageRequest::Library));
        assert!(matches!(state.page().load, Load::Loading { .. }));
        // A page that did not fail is not fetched again.
        let mut state = library();
        update(&mut state, Action::Disconnected { shut_down });
        assert_eq!(update(&mut state, welcome()), vec![]);
        // Only the top: a failed page below it waits.
        let mut state = State::default();
        update(&mut state, Action::Disconnected { shut_down });
        press(&mut state, &GL);
        state.connection = super::super::Connection::Connected;
        open(&mut state, &GY, fav_data(tracks(0..1), 1));
        update(&mut state, Action::Disconnected { shut_down });
        assert_eq!(update(&mut state, welcome()), vec![]);
    }

    // Session expired: the player's error shows in the page; once
    // restored, opening the page again fetches it.
    let expired = "Session expired: run \"tidal-player login\"";
    let mut state = State::default();
    let (id, _) = one_request(&press(&mut state, &GY));
    update(&mut state, Action::Player(protocol::Event::LoginRequired));
    fail(&mut state, id, expired);
    match &state.page().load {
        Load::Failed(m) => assert!(m.contains(expired), "{m}"),
        other => panic!("{other:?}"),
    }
    update(&mut state, Action::Player(protocol::Event::LoginRestored));
    let (_, request) = one_request(&press(&mut state, &GY));
    assert_eq!(request, LibraryRequest::Page(PageRequest::FavoriteTracks));
    assert_eq!(state.history.len(), 2);
}
