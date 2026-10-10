//! Spec 0012 AC1–AC7: filtering the focused window in the client model:
//! matching, the typing keys, the kept filter, the rows and cursor, acting
//! on the matches, loading for a filter and the search page.

use std::time::Duration;

use crate::item::Item;
use crate::library::{
    AlbumKind, AlbumSummary, CreditedTrack, LibraryRequest, LibraryResponse, ListItems, ListPage,
    ListRef, MixSummary, PageData, PageRequest, PlaylistSummary, RoleCategory, TopHit,
};
use crate::protocol::{
    self, Command, InsertAt, PlaybackState, PlayerSnapshot, QueueEntry, RepeatMode,
};
use crate::track::{AlbumRef, ArtistRef, EntryId, Track, TrackId};

use super::super::keymap::{CommandEntry, KeymapEntry, KeymapFile, build};
use super::super::{
    Action, Effect, Key, Load, MenuAction, Page, PageKind, Popup, Row, SearchFocus, State, Window,
    WindowKind, apply_keymap, update,
};
use super::{MAX_FILTER, matches, queue_shown, queue_title};

// --- fixtures ----------------------------------------------------------------

const GL: [Key; 2] = [Key::Char('g'), Key::Char('l')];
const GY: [Key; 2] = [Key::Char('g'), Key::Char('y')];
const GS: [Key; 2] = [Key::Char('g'), Key::Char('s')];
const GA: [Key; 2] = [Key::Char('g'), Key::Char('a')];

fn artist(id: u64, name: &str) -> ArtistRef {
    ArtistRef {
        id,
        name: name.into(),
    }
}

fn track(id: u64, title: &str, artist_name: &str, album: &str) -> Track {
    Track {
        id: TrackId(id),
        title: title.into(),
        version: None,
        artists: vec![artist(id + 1000, artist_name)],
        album: Some(AlbumRef {
            id: id + 2000,
            title: album.into(),
            cover: None,
        }),
        duration: Some(Duration::from_secs(100)),
        streamable: true,
    }
}

/// Love Song, Hell Above, Lovely, Other (IDs 1–4).
fn four() -> Vec<Track> {
    vec![
        track(1, "Love Song", "Ar A", "Al One"),
        track(2, "Hell Above", "Pierce The Veil", "Collide"),
        track(3, "Lovely", "Ar B", "Al Two"),
        track(4, "Other", "Ar C", "Al Three"),
    ]
}

fn list<T>(items: Vec<T>, offset: u32, total: u32) -> ListPage<T> {
    ListPage {
        items,
        offset,
        total,
        hidden: 0,
    }
}

fn press(state: &mut State, keys: &[Key]) -> Vec<Effect> {
    keys.iter()
        .flat_map(|k| update(state, Action::Key(*k)))
        .collect()
}

fn typed(text: &str) -> Vec<Key> {
    text.chars().map(Key::Char).collect()
}

/// `/`, then `text` typed.
fn slash(text: &str) -> Vec<Key> {
    [vec![Key::Char('/')], typed(text)].concat()
}

/// `/`, `text`, `Enter`: a kept filter.
fn kept(text: &str) -> Vec<Key> {
    [slash(text), vec![Key::Enter]].concat()
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

fn requests(effects: &[Effect]) -> Vec<(u64, LibraryRequest)> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::Library { id, request } => Some((*id, request.clone())),
            _ => None,
        })
        .collect()
}

/// Presses `keys`, which must ask for one page, and answers it.
fn open(state: &mut State, keys: &[Key], data: PageData) {
    let effects = press(state, keys);
    let asked = requests(&effects);
    assert_eq!(asked.len(), 1, "one request: {effects:?}");
    reply(state, asked[0].0, LibraryResponse::Page(data));
}

fn fav_data(tracks: Vec<Track>, total: u32) -> PageData {
    PageData::FavoriteTracks {
        tracks: list(tracks, 0, total),
    }
}

/// The favorite tracks page holding [`four`], complete.
fn favorites() -> State {
    let mut state = State::default();
    open(&mut state, &GY, fav_data(four(), 4));
    state
}

fn snapshot(tracks: Vec<Track>, current: Option<u64>) -> PlayerSnapshot {
    PlayerSnapshot {
        queue: tracks
            .into_iter()
            .enumerate()
            .map(|(i, track)| QueueEntry {
                id: EntryId(i as u64 + 1),
                suggested: false,
                track,
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

/// The queue page with [`four`] as entries 1–4, entry 2 playing.
fn queue() -> State {
    let mut state = State::default();
    update(
        &mut state,
        Action::Player(protocol::Event::Player(snapshot(four(), Some(2)))),
    );
    state
}

fn window(state: &State) -> &Window {
    &state.page().windows[state.page().focus]
}

/// The focused window's filter: (text, typing).
fn filter_of(state: &State) -> (String, bool) {
    let page = state.page();
    let filter = if page.kind == PageKind::Queue {
        &page.filter
    } else {
        &page.windows[page.focus].filter
    };
    (filter.text.clone(), filter.typing)
}

/// Whether any filter of the history is being typed into.
fn any_typing(state: &State) -> bool {
    state
        .history
        .iter()
        .any(|p| p.filter.typing || p.windows.iter().any(|w| w.filter.typing))
}

/// The titles of the rows the focused window shows.
fn shown(state: &State) -> Vec<String> {
    let w = window(state);
    (0..w.len())
        .filter_map(|i| w.row(i)?.track().map(|t| t.title.clone()))
        .collect()
}

fn selected_title(state: &State) -> Option<String> {
    state.page().selected()?.track().map(|t| t.title.clone())
}

fn album(id: u64, title: &str, artist_name: &str, year: u16) -> AlbumSummary {
    AlbumSummary {
        id,
        title: title.into(),
        artists: vec![artist(7, artist_name)],
        year: Some(year),
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

fn library_data() -> PageData {
    PageData::Library {
        playlists: list(
            vec![
                playlist("p1", "Running", true),
                playlist("p2", "Late night", false),
            ],
            0,
            2,
        ),
        albums: list(
            vec![
                album(10, "Collide With The Sky", "Pierce The Veil", 2012),
                album(11, "Misadventures", "Pierce The Veil", 2016),
            ],
            0,
            2,
        ),
        artists: list(vec![artist(7, "Pierce The Veil")], 0, 1),
    }
}

/// The page of `kind` on top of the queue, loaded with nothing, its
/// window `focus` focused.
fn on_page(kind: PageKind, focus: usize) -> State {
    let mut state = queue();
    let mut page = Page::new(kind);
    page.focus = focus;
    if let Some(search) = page.search.as_mut() {
        search.focus = SearchFocus::Windows;
    }
    state.history.push(page);
    state
}

fn search_data() -> PageData {
    PageData::Search {
        top_hit: Some(Box::new(TopHit::Artist(artist(7, "Pierce The Veil")))),
        tracks: list(four(), 0, 4),
        albums: list(vec![album(10, "Collide With The Sky", "PTV", 2012)], 0, 1),
        artists: list(vec![artist(7, "Pierce The Veil")], 0, 1),
        playlists: list(vec![playlist("s1", "This Is PTV", false)], 0, 1),
    }
}

/// A search page with results, the top hit focused.
fn searched() -> State {
    let mut state = queue();
    press(&mut state, &GS);
    press(&mut state, &typed("ptv"));
    let effects = press(&mut state, &[Key::Enter]);
    let asked = requests(&effects);
    reply(&mut state, asked[0].0, LibraryResponse::Page(search_data()));
    state
}

fn search_focus(state: &State) -> (SearchFocus, usize) {
    let page = state.page();
    (page.search.as_ref().unwrap().focus, page.focus)
}

/// (name, keys after the page opened, paste after them, text, typing).
type TypingCase<'a> = (&'a str, Vec<Key>, Option<&'a str>, &'a str, bool);
/// (name, keys after the page opened, rows shown, selected row).
type RowsCase = (
    &'static str,
    Vec<Key>,
    Vec<&'static str>,
    Option<&'static str>,
);

// --- AC1 ---------------------------------------------------------------------

/// AC1: every row kind with each of its fields; case folded; the filter is
/// one substring of one field; an empty and an all-space filter match
/// everything; accents are not folded.
#[test]
fn ac1_matches() {
    let song = Track {
        version: Some("Live".into()),
        artists: vec![artist(1, "Pierce The Veil"), artist(2, "Kellin Quinn")],
        ..track(1, "Hell Above", "x", "Collide With The Sky")
    };
    let credit = CreditedTrack {
        track: song.clone(),
        roles: vec![RoleCategory::Performer],
    };
    let rós = album(10, "Ágætis byrjun", "Sigur Rós", 1999);
    let night = playlist("p", "Late Night", true);
    let björk = artist(3, "Björk");
    let mix = MixSummary {
        id: "m".into(),
        title: "My Daily Discovery".into(),
        subtitle: Some("Fresh picks".into()),
    };
    let love = track(5, "Love Song", "A", "B");
    let t = Row::Track(&song);
    let c = Row::Credit(&credit);
    let a = Row::Album(&rós);
    let p = Row::Playlist(&night);
    let r = Row::Artist(&björk);
    let m = Row::Mix(&mix);
    let cases: Vec<(&str, Row<'_>, &str, bool)> = vec![
        // A track: title, version, every artist, album.
        ("title", t, "hell ab", true),
        ("version", t, "live", true),
        ("first artist", t, "the veil", true),
        ("second artist", t, "kellin", true),
        ("album", t, "collide", true),
        ("across two fields", t, "pierce hell", false),
        ("nowhere", t, "xyz", false),
        ("case", Row::Track(&love), "LOVE", true),
        ("empty", t, "", true),
        ("all spaces", t, "   ", true),
        ("spaces kept", t, "hell  above", false),
        // A credit: its track's fields.
        ("credit title", c, "HELL", true),
        ("credit artist", c, "quinn", true),
        ("credit nowhere", c, "xyz", false),
        // An album: title, every artist, year.
        ("album title", a, "byrjun", true),
        ("album artist", a, "sigur", true),
        ("album year", a, "1999", true),
        ("album other year", a, "2012", false),
        ("unicode case", a, "RÓS", true),
        ("accents not folded", a, "ros", false),
        ("accents not folded, title", a, "agaetis", false),
        // A playlist: title.
        ("playlist", p, "NIGHT", true),
        ("playlist nowhere", p, "running", false),
        // An artist: name.
        ("artist", r, "BJÖRK", true),
        ("artist accents", r, "bjork", false),
        // A mix: title, subtitle.
        ("mix title", m, "daily", true),
        ("mix subtitle", m, "fresh", true),
        ("mix nowhere", m, "radio", false),
        ("mix empty", m, "", true),
    ];
    for (name, row, filter, expected) in cases {
        assert_eq!(matches(row, filter), expected, "{name}: {filter:?}");
    }
}

// --- AC2 ---------------------------------------------------------------------

/// AC2: the typing keys on favorite tracks: printable characters, paste,
/// `backspace`, `C-u`, the 100-character limit; `enter` keeps, `esc`
/// clears, both stop typing; `up`/`down`/`page_up`/`page_down` move over
/// the matches; `C-c` quits; `/` on a kept filter resumes it. Then `/`
/// starts typing on every window kind and on the queue.
#[test]
fn ac2_typing_keys() {
    use Key::{Backspace, Ctrl, Down, End, Enter, Esc, PageDown, PageUp, Tab, Up};
    let long = "x".repeat(MAX_FILTER + 5);
    // (name, keys after the page opened, paste after them, text, typing)
    let cases: Vec<TypingCase<'_>> = vec![
        ("slash", slash(""), None, "", true),
        ("typed", slash("lov"), None, "lov", true),
        ("space", slash("a b"), None, "a b", true),
        (
            "backspace",
            [slash("lov"), vec![Backspace]].concat(),
            None,
            "lo",
            true,
        ),
        (
            "backspace on empty",
            [slash(""), vec![Backspace]].concat(),
            None,
            "",
            true,
        ),
        (
            "C-u",
            [slash("lov"), vec![Ctrl('u')]].concat(),
            None,
            "",
            true,
        ),
        (
            "C-u, typing on",
            [slash("lov"), vec![Ctrl('u')], typed("he")].concat(),
            None,
            "he",
            true,
        ),
        ("enter keeps", kept("lov"), None, "lov", false),
        (
            "esc clears",
            [slash("lov"), vec![Esc]].concat(),
            None,
            "",
            false,
        ),
        ("limit", slash(&long), None, &long[..MAX_FILTER], true),
        ("paste", slash("lo"), Some("ve\nly\t"), "lovely", true),
        (
            "paste over the limit",
            slash(""),
            Some(&long),
            &long[..MAX_FILTER],
            true,
        ),
        ("paste after enter", kept("lo"), Some("x"), "lo", false),
        (
            "resume",
            [kept("lo"), slash("v")].concat(),
            None,
            "lov",
            true,
        ),
        (
            "other keys ignored",
            [slash("lo"), vec![Tab, End, Key::F(1)]].concat(),
            None,
            "lo",
            true,
        ),
    ];
    for (name, keys, paste, text, typing) in cases {
        let mut state = favorites();
        let effects = press(&mut state, &keys);
        assert_eq!(effects, vec![], "{name}: effects");
        if let Some(paste) = paste {
            update(&mut state, Action::Paste(paste.into()));
        }
        assert_eq!(filter_of(&state), (text.to_owned(), typing), "{name}");
    }

    // Moving over the matches while typing: Love Song, Lovely.
    let mut state = favorites();
    state.list_height = 1;
    press(&mut state, &slash("love"));
    assert_eq!(shown(&state), ["Love Song", "Lovely"]);
    let moves: Vec<(Key, &str)> = vec![
        (Down, "Lovely"),
        (Down, "Lovely"),
        (Up, "Love Song"),
        (PageDown, "Lovely"),
        (PageUp, "Love Song"),
    ];
    for (key, expected) in moves {
        assert_eq!(press(&mut state, &[key]), vec![], "{key:?}");
        assert_eq!(selected_title(&state).as_deref(), Some(expected), "{key:?}");
        assert_eq!(
            filter_of(&state),
            ("love".into(), true),
            "{key:?}: typing on"
        );
    }
    // `C-c` quits.
    assert_eq!(press(&mut state, &[Ctrl('c')]), vec![Effect::Quit]);

    // On the queue page.
    let mut state = queue();
    assert_eq!(press(&mut state, &slash("love")), vec![]);
    assert_eq!(filter_of(&state), ("love".into(), true));
    assert_eq!(press(&mut state, &[Down]), vec![]);
    assert_eq!(state.cursor, Some(EntryId(3)));
    assert_eq!(press(&mut state, &[Enter]), vec![]);
    assert_eq!(filter_of(&state), ("love".into(), false));

    // `/` starts typing on every window kind.
    let mut pages: Vec<(PageKind, usize)> = vec![
        (PageKind::FavoriteTracks, 0),
        (PageKind::Album(10), 0),
        (PageKind::Playlist("p".into()), 0),
        (PageKind::Mixes, 0),
        (PageKind::Mix("m".into()), 0),
        (PageKind::TrackRadio(1), 0),
        (PageKind::ArtistRadio(7), 0),
    ];
    pages.extend((0..3).map(|w| (PageKind::Library, w)));
    pages.extend((0..4).map(|w| (PageKind::Artist(7), w)));
    pages.extend((0..4).map(|w| (PageKind::Search("q".into()), w)));
    for (kind, focus) in pages {
        let mut state = on_page(kind.clone(), focus);
        assert_eq!(press(&mut state, &slash("ab")), vec![], "{kind:?} {focus}");
        assert_eq!(
            filter_of(&state),
            ("ab".into(), true),
            "{kind:?} window {focus}"
        );
        assert_eq!(state.page().focus, focus, "{kind:?}: focus kept");
    }
}

/// AC2: while typing, keys bound in `keymap.toml` type: with `q` →
/// `NextTrack`, `j` → `Quit` and `?` bound, `qj?` is typed and nothing is
/// emitted.
#[test]
fn ac2_typing_ignores_keymap() {
    let entry = |sequence: &str, command: &str| KeymapEntry {
        command: CommandEntry::name(command),
        key_sequence: sequence.into(),
    };
    let file = KeymapFile {
        keymaps: vec![
            entry("q", "NextTrack"),
            entry("j", "Quit"),
            entry("n", "Quit"),
            entry("tab", "Quit"),
        ],
        actions: vec![],
    };
    for start in [favorites(), queue()] {
        let mut state = start;
        apply_keymap(&mut state, build(&file).expect("a valid keymap"));
        let origin_page = state.page().kind.clone();
        let effects = press(
            &mut state,
            &[slash("qj?"), vec![Key::Tab], typed("n g")].concat(),
        );
        assert_eq!(effects, vec![], "{origin_page:?}");
        assert!(state.help.is_none(), "{origin_page:?}: no help");
        assert_eq!(state.page().kind, origin_page);
        assert_eq!(
            filter_of(&state),
            ("qj?n g".into(), true),
            "{origin_page:?}"
        );
        assert!(state.pending.is_empty(), "{origin_page:?}: no pending keys");
    }
}

/// AC2: `/` does nothing on a loading or failed page, in the search input
/// (it types `/`), on *Top hit*, over a popup, the prompt or a whole-list
/// load.
#[test]
fn ac2_slash_where_it_does_nothing() {
    let mut loading = queue();
    press(&mut loading, &GY);
    let mut failed = loading.clone();
    let id = match &failed.page().load {
        Load::Loading { id } => *id,
        other => panic!("{other:?}"),
    };
    update(
        &mut failed,
        Action::LibraryReply {
            id,
            result: Err("timed out".into()),
        },
    );
    let mut input = queue();
    press(&mut input, &GS);
    let top_hit = searched();
    assert_eq!(search_focus(&top_hit).0, SearchFocus::TopHit);
    let mut popup = favorites();
    press(&mut popup, &GA);
    assert!(popup.popup.is_some());
    let mut prompt = favorites();
    press(&mut prompt, &[Key::Char('o')]);
    let mut whole = State {
        page_size: 2,
        ..State::default()
    };
    open(&mut whole, &GY, fav_data(four()[..2].to_vec(), 4));
    press(&mut whole, &[Key::Enter]);
    assert!(whole.whole_list.is_some());
    let cases: Vec<(&str, State)> = vec![
        ("loading", loading),
        ("failed", failed),
        ("search input", input),
        ("top hit", top_hit),
        ("popup", popup),
        ("prompt", prompt),
        ("whole-list load", whole),
    ];
    for (name, origin) in cases {
        let mut state = origin.clone();
        let effects = press(&mut state, &[Key::Char('/')]);
        assert_eq!(effects, vec![], "{name}");
        assert!(!any_typing(&state), "{name}: no filter typing");
        match name {
            "search input" => {
                let search = state.page().search.as_ref().unwrap();
                assert_eq!(
                    (search.input.as_str(), search.focus),
                    ("/", SearchFocus::Input)
                );
            }
            "prompt" => assert_eq!(state.prompt.as_ref().unwrap().text, "/"),
            _ => assert_eq!(state, origin, "{name}: nothing changed"),
        }
    }
}

// --- AC3 ---------------------------------------------------------------------

/// AC3: a kept filter: keys act on the rows shown; `esc` clears it (and
/// nothing else); `/` edits it; each window keeps its own; the history,
/// a refetch keep it; a new page has none; a new search clears the
/// search windows' filters.
#[test]
fn ac3_kept_filter() {
    use Key::{BackTab, Backspace, Char, Enter, Esc, Tab};
    // Keys act on the rows shown; `esc` clears the filter, the cursor
    // stays on its row, nothing is emitted.
    let mut state = favorites();
    press(&mut state, &kept("love"));
    assert_eq!(press(&mut state, &[Char('j')]), vec![]);
    assert_eq!(selected_title(&state).as_deref(), Some("Lovely"));
    let before = state.clone();
    assert_eq!(press(&mut state, &[Esc]), vec![]);
    assert_eq!(filter_of(&state), (String::new(), false));
    assert_eq!(selected_title(&state).as_deref(), Some("Lovely"));
    assert_eq!(state.page().kind, before.page().kind);
    assert_eq!(state.history.len(), before.history.len());
    // A second `esc` does nothing.
    let cleared = state.clone();
    assert_eq!(press(&mut state, &[Esc]), vec![]);
    assert_eq!(state, cleared);

    // `/` edits it again, from its text.
    let mut state = favorites();
    press(&mut state, &kept("lo"));
    press(&mut state, &slash("ve"));
    assert_eq!(filter_of(&state), ("love".into(), true));

    // Each window its own filter (Tab/BackTab, `[`/`]`).
    let mut state = State::default();
    open(&mut state, &GL, library_data());
    press(&mut state, &kept("run"));
    press(&mut state, &[Tab]);
    assert_eq!(filter_of(&state), (String::new(), false), "Albums: none");
    press(&mut state, &kept("mis"));
    press(&mut state, &[BackTab]);
    assert_eq!(filter_of(&state), ("run".into(), false), "Playlists: kept");
    press(&mut state, &[Tab]);
    assert_eq!(filter_of(&state), ("mis".into(), false), "Albums: kept");
    let mut artist_page = on_page(PageKind::Artist(7), 0);
    artist_page.history.last_mut().unwrap().load = Load::Idle;
    press(&mut artist_page, &kept("top"));
    press(&mut artist_page, &[Char(']')]);
    assert_eq!(window(&artist_page).kind, WindowKind::AllTracks);
    assert_eq!(filter_of(&artist_page), (String::new(), false));
    press(&mut artist_page, &[Char('[')]);
    assert_eq!(filter_of(&artist_page), ("top".into(), false));

    // The history keeps it; a newly opened page has none.
    let mut state = favorites();
    press(&mut state, &kept("love"));
    open(&mut state, &GL, library_data());
    assert_eq!(filter_of(&state), (String::new(), false), "new page");
    press(&mut state, &[Backspace]);
    assert_eq!(state.page().kind, PageKind::FavoriteTracks);
    assert_eq!(filter_of(&state), ("love".into(), false), "back: kept");
    assert_eq!(shown(&state), ["Love Song", "Lovely"]);

    // A refetch (after a favorite edit) keeps it, over the new rows.
    let mut state = favorites();
    press(&mut state, &kept("love"));
    press(&mut state, &GA);
    let Some(Popup::Actions { actions, .. }) = &state.popup else {
        panic!("no popup");
    };
    let at = actions
        .iter()
        .position(|a| matches!(a, MenuAction::RemoveFavorite(..)))
        .expect("Remove from favorites");
    press(&mut state, &vec![Char('j'); at]);
    let effects = press(&mut state, &[Enter]);
    let (write, _) = requests(&effects)[0].clone();
    let effects = reply(&mut state, write, LibraryResponse::Done);
    let (refetch, request) = requests(&effects)[0].clone();
    assert_eq!(request, LibraryRequest::Page(PageRequest::FavoriteTracks));
    let mut rows = four()[1..].to_vec();
    rows.push(track(9, "Love Again", "Ar D", "Al Four"));
    reply(
        &mut state,
        refetch,
        LibraryResponse::Page(fav_data(rows, 4)),
    );
    assert_eq!(filter_of(&state), ("love".into(), false), "refetch: kept");
    assert_eq!(shown(&state), ["Lovely", "Love Again"]);

    // A new search clears the search windows' filters.
    let mut state = searched();
    press(&mut state, &[Tab]);
    press(&mut state, &kept("love"));
    assert_eq!(filter_of(&state), ("love".into(), false));
    press(&mut state, &GS);
    press(&mut state, &typed("x"));
    let effects = press(&mut state, &[Enter]);
    let (id, _) = requests(&effects)[0].clone();
    reply(&mut state, id, LibraryResponse::Page(search_data()));
    assert!(
        state
            .page()
            .windows
            .iter()
            .all(|w| w.filter.text.is_empty()),
        "a new search: no filter"
    );
}

// --- AC4 ---------------------------------------------------------------------

/// AC4: the rows shown are the matches in list order (with the role
/// filter, both apply); the cursor stays on a still-matching row after an
/// edit, else goes to the first match; no match → no selected row and the
/// row keys emit nothing; on the queue the cursor stays an entry ID and
/// moves over matching entries only.
#[test]
fn ac4_rows_and_cursor() {
    use Key::{Backspace, Char, Down, Enter};
    // (keys after the page opened, rows shown, selected)
    let cases: Vec<RowsCase> = vec![
        (
            "matches in order",
            slash("lo"),
            vec!["Love Song", "Lovely"],
            Some("Love Song"),
        ),
        (
            "cursor not matching: first match",
            [vec![Char('j')], slash("lov")].concat(),
            vec!["Love Song", "Lovely"],
            Some("Love Song"),
        ),
        (
            "cursor still matching: stays",
            [slash("lo"), vec![Down], typed("v")].concat(),
            vec!["Love Song", "Lovely"],
            Some("Lovely"),
        ),
        (
            "cleared: stays on its row",
            [slash("lo"), vec![Down, Backspace, Backspace]].concat(),
            vec!["Love Song", "Hell Above", "Lovely", "Other"],
            Some("Lovely"),
        ),
        (
            "an artist matches",
            slash("veil"),
            vec!["Hell Above"],
            Some("Hell Above"),
        ),
        ("no match", slash("xyz"), vec![], None),
    ];
    for (name, keys, rows, selected) in cases {
        let mut state = favorites();
        press(&mut state, &keys);
        assert_eq!(shown(&state), rows, "{name}");
        assert_eq!(selected_title(&state).as_deref(), selected, "{name}");
    }

    // No match: the row keys emit nothing.
    let mut state = favorites();
    press(&mut state, &kept("xyz"));
    for keys in [
        vec![Enter],
        vec![Char('Z')],
        GA.to_vec(),
        vec![Char('r')],
        vec![Char('d')],
    ] {
        assert_eq!(press(&mut state, &keys), vec![], "{keys:?}");
        assert!(state.popup.is_none(), "{keys:?}");
        assert_eq!(state.history.len(), 2, "{keys:?}");
    }

    // The role filter and the text filter both apply (*All tracks*).
    let mut state = State::default();
    let mut page = Page::new(PageKind::Artist(7));
    let credits = vec![
        (track(1, "Love Song", "A", "X"), RoleCategory::Performer),
        (track(2, "Lovely", "A", "X"), RoleCategory::Producer),
        (track(3, "Other", "A", "X"), RoleCategory::Performer),
        (track(4, "Love Again", "A", "X"), RoleCategory::Performer),
    ];
    page.windows[3].append(
        ListItems::Credits(list(
            credits
                .into_iter()
                .map(|(track, role)| CreditedTrack {
                    track,
                    roles: vec![role],
                })
                .collect(),
            0,
            4,
        )),
        100,
    );
    page.focus = 3;
    page.tabs[0] = 1;
    state.history.push(page);
    // Only Performer: Love Song, Other, Love Again; then `love`.
    press(
        &mut state,
        &[
            Char('f'),
            Char('j'),
            Char(' '),
            Char('j'),
            Char(' '),
            Char('j'),
            Char(' '),
        ],
    );
    press(&mut state, &[Enter]);
    assert_eq!(shown(&state), ["Love Song", "Other", "Love Again"]);
    press(&mut state, &kept("love"));
    assert_eq!(shown(&state), ["Love Song", "Love Again"]);
    assert_eq!(window(&state).visible(), vec![0, 3]);

    // The queue: entries matching, cursor by entry ID over them only.
    let mut state = queue();
    assert_eq!(state.cursor, Some(EntryId(2)));
    press(&mut state, &kept("lo"));
    assert_eq!(queue_shown(&state), vec![0, 2]);
    assert_eq!(state.cursor, Some(EntryId(1)), "first match");
    press(&mut state, &[Char('j')]);
    assert_eq!(state.cursor, Some(EntryId(3)));
    press(&mut state, &[Char('j')]);
    assert_eq!(state.cursor, Some(EntryId(3)), "the last match");
    press(&mut state, &[Char('k')]);
    assert_eq!(state.cursor, Some(EntryId(1)));
    // No match on the queue: nothing selected.
    press(&mut state, &kept("xyz"));
    assert_eq!(queue_shown(&state), Vec::<usize>::new());
    for keys in [vec![Enter], vec![Char('d')], GA.to_vec()] {
        assert_eq!(press(&mut state, &keys), vec![], "{keys:?}");
        assert!(state.popup.is_none(), "{keys:?}");
    }
    // The queue changes while filtered: the cursor keeps its entry if it
    // matches, else the first match.
    let mut state = queue();
    press(&mut state, &kept("lo"));
    press(&mut state, &[Char('j')]);
    assert_eq!(state.cursor, Some(EntryId(3)));
    let mut changed = snapshot(four(), Some(2));
    changed.queue.remove(2);
    update(&mut state, Action::Player(protocol::Event::Player(changed)));
    assert_eq!(queue_shown(&state), vec![0]);
    assert_eq!(state.cursor, Some(EntryId(1)));
}

// --- AC5 ---------------------------------------------------------------------

/// AC5: `enter` on a match queues the matches from it (complete, and
/// through the whole-list load); `Z`, the actions popup and `r` act on the
/// selected match; a playlist removal sends its position; on the queue
/// `enter` and `d` send the selected match's entry.
#[test]
fn ac5_acting_on_matches() {
    use Key::{Char, Enter};
    let titles =
        |tracks: &[Track]| -> Vec<String> { tracks.iter().map(|t| t.title.clone()).collect() };
    let load_queue = |effects: &[Effect]| match effects {
        [Effect::Send(Command::LoadQueue { tracks, start })] => (titles(tracks), *start),
        other => panic!("not one LoadQueue: {other:?}"),
    };

    // A complete list: the matches, from the selected one.
    let mut state = favorites();
    press(&mut state, &kept("love"));
    press(&mut state, &[Char('j')]);
    let (tracks, start) = load_queue(&press(&mut state, &[Enter]));
    assert_eq!(
        (tracks, start),
        (vec!["Love Song".to_owned(), "Lovely".into()], 1)
    );

    // An incomplete list: the whole list is loaded, then its matches.
    let mut state = State {
        page_size: 4,
        ..State::default()
    };
    open(&mut state, &GY, fav_data(four(), 6));
    let mut effects = press(&mut state, &kept("love"));
    press(&mut state, &[Char('j')]);
    effects.extend(press(&mut state, &[Enter]));
    assert!(state.whole_list.is_some());
    let asked = requests(&effects);
    assert_eq!(asked.len(), 1, "{effects:?}");
    assert_eq!(
        asked[0].1,
        LibraryRequest::More {
            list: ListRef::FavoriteTracks,
            offset: 4,
            limit: 4
        }
    );
    let rest = vec![
        track(5, "Love Again", "Ar D", "Al Four"),
        track(6, "Nope", "Ar E", "Al Five"),
    ];
    let effects = reply(
        &mut state,
        asked[0].0,
        LibraryResponse::Items(ListItems::Tracks(list(rest, 4, 6))),
    );
    let (tracks, start) = load_queue(&effects);
    assert_eq!(
        (tracks, start),
        (
            vec!["Love Song".to_owned(), "Lovely".into(), "Love Again".into()],
            1
        )
    );

    // `Z`, the actions popup, `r`: the selected match (Lovely, ID 3).
    let mut state = favorites();
    press(&mut state, &kept("love"));
    press(&mut state, &[Char('j')]);
    assert_eq!(
        press(&mut state.clone(), &[Char('Z')]),
        vec![Effect::Send(Command::Open {
            items: vec![Item::Track(TrackId(3))],
            at: Some(InsertAt::End),
        })]
    );
    let mut popup = state.clone();
    press(&mut popup, &GA);
    assert!(
        matches!(&popup.popup, Some(Popup::Actions { title, .. }) if title == "Lovely"),
        "{:?}",
        popup.popup
    );
    let effects = press(&mut state, &[Char('r')]);
    assert_eq!(state.page().kind, PageKind::TrackRadio(3));
    assert_eq!(
        requests(&effects)[0].1,
        LibraryRequest::Page(PageRequest::TrackRadio(3))
    );

    // *Remove from this playlist*: the selected match's position.
    let mut state = State::default();
    open(&mut state, &GL, library_data());
    open(
        &mut state,
        &[Enter],
        PageData::Playlist {
            playlist: playlist("p1", "Running", true),
            etag: Some("e1".into()),
            tracks: list(four(), 0, 4),
        },
    );
    press(&mut state, &kept("love"));
    press(&mut state, &[Char('j')]);
    press(&mut state, &GA);
    let Some(Popup::Actions { actions, .. }) = &state.popup else {
        panic!("no popup");
    };
    let removal = actions
        .iter()
        .find_map(|a| match a {
            MenuAction::RemoveFromPlaylist { index, .. } => Some(*index),
            _ => None,
        })
        .expect("Remove from this playlist");
    assert_eq!(removal, 2, "Lovely is the playlist's third track");

    // The queue: `enter` and `d` on the selected match's entry.
    let mut state = queue();
    press(&mut state, &kept("love"));
    press(&mut state, &[Char('j')]);
    assert_eq!(
        press(&mut state.clone(), &[Enter]),
        vec![Effect::Send(Command::PlayEntry(EntryId(3)))]
    );
    assert_eq!(
        press(&mut state, &[Char('d')]),
        vec![Effect::Send(Command::RemoveFromQueue(EntryId(3)))]
    );
}

// --- AC6 ---------------------------------------------------------------------

/// AC6: a filter on an incomplete list with too few matches below the
/// cursor asks for one `More`, another after each reply until enough
/// match or the list is complete; never two at once; none for whole lists
/// and the queue; after `esc`, none beyond the unfiltered rule.
#[test]
fn ac6_loads_for_filter() {
    use Key::{Char, Esc};
    let small = || State {
        page_size: 2,
        list_height: 1,
        ..State::default()
    };
    let more = |offset| LibraryRequest::More {
        list: ListRef::FavoriteTracks,
        offset,
        limit: 2,
    };
    let page = |ids: [u64; 2], offset: u32| {
        LibraryResponse::Items(ListItems::Tracks(list(
            ids.iter()
                .map(|&id| track(id, &format!("Song {id}"), "Ar", "Al"))
                .collect(),
            offset,
            6,
        )))
    };
    // A filter matching nothing: page by page to the end.
    let mut state = small();
    open(&mut state, &GY, fav_data(four()[..2].to_vec(), 6));
    assert_eq!(
        press(&mut state, &[Char('/')]),
        vec![],
        "empty filter: no More"
    );
    let effects = press(&mut state, &[Char('z')]);
    let asked = requests(&effects);
    assert_eq!(
        asked.iter().map(|a| a.1.clone()).collect::<Vec<_>>(),
        vec![more(2)]
    );
    assert_eq!(press(&mut state, &[Char('z')]), vec![], "one at a time");
    let effects = reply(&mut state, asked[0].0, page([3, 4], 2));
    let asked = requests(&effects);
    assert_eq!(
        asked.iter().map(|a| a.1.clone()).collect::<Vec<_>>(),
        vec![more(4)]
    );
    let effects = reply(&mut state, asked[0].0, page([5, 6], 4));
    assert_eq!(effects, vec![], "complete: no more");
    assert!(window(&state).complete());
    assert_eq!(window(&state).len(), 0);

    // Enough matches below the cursor: nothing asked.
    let mut state = State {
        page_size: 4,
        list_height: 1,
        ..State::default()
    };
    let songs: Vec<Track> = (1..=4)
        .map(|id| track(id, &format!("Song {id}"), "Ar", "Al"))
        .collect();
    open(&mut state, &GY, fav_data(songs, 10));
    assert_eq!(press(&mut state, &slash("song")), vec![]);
    // Moving near the end of the matches asks, as scrolling does.
    let effects = press(&mut state, &[Key::Down, Key::Down]);
    assert_eq!(requests(&effects).len(), 1, "{effects:?}");

    // `esc` stops it: the reply in flight asks for nothing more.
    let mut state = small();
    open(&mut state, &GY, fav_data(four()[..2].to_vec(), 6));
    let effects = press(&mut state, &slash("z"));
    let asked = requests(&effects);
    assert_eq!(asked.len(), 1);
    assert_eq!(press(&mut state, &[Esc]), vec![]);
    assert_eq!(reply(&mut state, asked[0].0, page([3, 4], 2)), vec![]);
    // A kept filter cleared with `esc` too.
    let mut state = small();
    open(&mut state, &GY, fav_data(four()[..2].to_vec(), 6));
    let effects = press(&mut state, &kept("z"));
    let asked = requests(&effects);
    assert_eq!(asked.len(), 1);
    assert_eq!(press(&mut state, &[Esc]), vec![]);
    assert_eq!(reply(&mut state, asked[0].0, page([3, 4], 2)), vec![]);

    // Whole lists and the queue never load more.
    let mut state = small();
    open(
        &mut state,
        &[Key::Char('g'), Key::Char('m')],
        PageData::Mixes {
            mixes: list(
                vec![MixSummary {
                    id: "m1".into(),
                    title: "Daily".into(),
                    subtitle: None,
                }],
                0,
                1,
            ),
        },
    );
    assert_eq!(press(&mut state, &slash("zz")), vec![]);
    assert_eq!(window(&state).len(), 0);
    let mut state = queue();
    state.list_height = 1;
    assert_eq!(press(&mut state, &slash("zz")), vec![]);
}

// --- AC7 ---------------------------------------------------------------------

/// AC7: `/` on a search result window filters it and leaves the focus
/// there; `g s` and `tab` still reach the input; the input types `/`; `/`
/// on *Top hit* does nothing.
#[test]
fn ac7_search_page() {
    use Key::{Char, Enter, Tab};
    let mut state = searched();
    // Top hit: nothing.
    assert_eq!(press(&mut state, &[Char('/')]), vec![]);
    assert_eq!(search_focus(&state), (SearchFocus::TopHit, 0));
    assert!(!any_typing(&state));
    // Tracks: `/` filters it, the focus stays.
    press(&mut state, &[Tab]);
    assert_eq!(search_focus(&state), (SearchFocus::Windows, 0));
    assert_eq!(press(&mut state, &slash("love")), vec![]);
    assert_eq!(search_focus(&state), (SearchFocus::Windows, 0));
    assert_eq!(filter_of(&state), ("love".into(), true));
    assert_eq!(state.page().search.as_ref().unwrap().input, "ptv");
    press(&mut state, &[Enter]);
    assert_eq!(shown(&state), ["Love Song", "Lovely"]);
    // `enter` on a filtered search track queues the matches.
    let effects = press(&mut state.clone(), &[Key::Down, Enter]);
    assert!(
        matches!(&effects[..], [Effect::Send(Command::LoadQueue { tracks, start: 1 })] if tracks.len() == 2),
        "{effects:?}"
    );
    // `g s` reaches the input, which types `/`.
    let mut input = state.clone();
    press(&mut input, &GS);
    assert_eq!(search_focus(&input).0, SearchFocus::Input);
    press(&mut input, &[Char('/')]);
    assert_eq!(input.page().search.as_ref().unwrap().input, "ptv/");
    // `tab` from the last window reaches it too.
    let mut tabbed = state.clone();
    press(&mut tabbed, &[Tab, Tab, Tab, Tab]);
    assert_eq!(search_focus(&tabbed).0, SearchFocus::Input);
}

// --- titles ------------------------------------------------------------------

/// AC9 (the model's part): the window titles and the queue's, with a
/// filter kept and while typing.
#[test]
fn ac9_titles_in_model() {
    let mut state = favorites();
    assert_eq!(window(&state).title(), "Favorite tracks (4)");
    press(&mut state, &[Key::Char('/')]);
    assert_eq!(window(&state).title(), "Favorite tracks (4 · /▏)");
    press(&mut state, &typed("love"));
    assert_eq!(
        window(&state).title(),
        "Favorite tracks (4 · /love▏ · 2 matches)"
    );
    press(
        &mut state,
        &[Key::Backspace, Key::Backspace, Key::Backspace],
    );
    press(&mut state, &typed("ther"));
    assert_eq!(
        window(&state).title(),
        "Favorite tracks (4 · /lther▏ · 0 matches)"
    );
    press(&mut state, &[Key::Ctrl('u')]);
    press(&mut state, &[typed("other"), vec![Key::Enter]].concat());
    assert_eq!(
        window(&state).title(),
        "Favorite tracks (4 · /other · 1 match)"
    );
    let mut empty = Window::new(WindowKind::AlbumTracks, ListRef::AlbumTracks(1));
    empty.filter.text = "love".into();
    empty.refilter();
    assert_eq!(empty.title(), "Tracks (/love · 0 matches)");

    let mut state = queue();
    assert_eq!(queue_title(&state), "Queue (4)");
    press(&mut state, &kept("love"));
    assert_eq!(queue_title(&state), "Queue (4 · /love · 2 matches)");
}
