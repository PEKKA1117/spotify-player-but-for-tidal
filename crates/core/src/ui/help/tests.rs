//! Spec 0008 AC7 and AC8: the contents of the keys help (sections, keys,
//! dim rows) and its keys (open, move, filter, run, close).

use std::time::Duration;

use crate::library::{ListRef, PlaylistSummary};
use crate::protocol::{self, Command, PlaybackState, PlayerSnapshot, QueueEntry, RepeatMode};
use crate::track::{AlbumRef, ArtistRef, EntryId, Track, TrackId};

use super::super::keymap::{ActionEntry, CommandEntry, KeymapEntry, KeymapFile, Target, build};
use super::super::{
    Action, Confirmed, Connection, Effect, Header, Key, Page, PageKind, Popup, SearchFocus, State,
    TrackSource, Window, WindowKind, apply_keymap, update,
};
use super::{HelpRow, HelpSection, help, visible};

// --- fixtures ----------------------------------------------------------------

fn track(id: u64) -> Track {
    Track {
        id: TrackId(id),
        title: format!("T{id}"),
        version: None,
        artists: vec![ArtistRef {
            id: 20,
            name: "Ar20".into(),
        }],
        album: Some(AlbumRef {
            id: 10,
            title: "Al10".into(),
            cover: None,
        }),
        duration: Some(Duration::from_secs(100)),
        streamable: true,
    }
}

fn snapshot(ids: &[u64]) -> PlayerSnapshot {
    PlayerSnapshot {
        queue: ids
            .iter()
            .map(|id| QueueEntry {
                id: EntryId(*id),
                track: track(id + 100),
                suggested: false,
            })
            .collect(),
        current: ids.first().copied().map(EntryId),
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

/// The queue page with entries 1 and 2.
fn with_queue() -> State {
    let mut state = State::default();
    update(
        &mut state,
        Action::Player(protocol::Event::Player(snapshot(&[1, 2]))),
    );
    state
}

/// `kind` on top of the queue page, tuned by `tune`.
fn on_page(kind: PageKind, tune: impl FnOnce(&mut Page)) -> State {
    let mut state = with_queue();
    let mut page = Page::new(kind);
    tune(&mut page);
    state.history.push(page);
    state
}

fn focused(kind: PageKind, window: usize) -> State {
    on_page(kind, |p| p.focus = window)
}

fn search_on(focus: SearchFocus, window: usize) -> State {
    on_page(PageKind::Search("q".into()), |p| {
        p.search.as_mut().unwrap().focus = focus;
        p.focus = window;
    })
}

fn playlist_page(own: bool) -> State {
    on_page(PageKind::Playlist("p1".into()), |p| {
        p.header = Some(Header::Playlist {
            playlist: PlaylistSummary {
                uuid: "p1".into(),
                title: "Mine".into(),
                tracks: Some(2),
                duration: None,
                own,
            },
            etag: None,
        });
    })
}

fn with_popup(popup: Popup) -> State {
    let mut state = with_queue();
    state.popup = Some(popup);
    state
}

fn popups() -> Vec<(&'static str, Popup)> {
    vec![
        (
            "actions",
            Popup::Actions {
                title: "T101".into(),
                actions: vec![],
                cursor: 0,
            },
        ),
        (
            "add to playlist",
            Popup::AddToPlaylist {
                tracks: TrackSource::Tracks(vec![]),
                playlists: Window::new(WindowKind::Playlists, ListRef::Playlists),
                cursor: 0,
            },
        ),
        (
            "roles",
            Popup::Roles {
                checked: [false; 4],
                cursor: 0,
            },
        ),
        (
            "confirm",
            Popup::Confirm {
                question: "Delete?".into(),
                on_yes: Confirmed::DeletePlaylist {
                    uuid: "p1".into(),
                    title: "Mine".into(),
                },
            },
        ),
    ]
}

fn custom_keymap() -> State {
    let key = |command: &str, sequence: &str| KeymapEntry {
        command: CommandEntry::name(command),
        key_sequence: sequence.into(),
    };
    let file = KeymapFile {
        keymaps: vec![key("Shuffle", "S"), key("None", "C-s"), key("None", "n")],
        actions: vec![ActionEntry {
            action: "GoToAlbum".into(),
            key_sequence: "g B".into(),
            target: Target::PlayingTrack,
        }],
    };
    let mut state = with_queue();
    apply_keymap(&mut state, build(&file).expect("a valid keymap"));
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

fn titles(sections: &[HelpSection]) -> Vec<&str> {
    sections.iter().map(|s| s.title.as_str()).collect()
}

fn names<'a>(sections: &'a [HelpSection], title: &str) -> Vec<&'a str> {
    sections
        .iter()
        .find(|s| s.title == title)
        .map(|s| s.rows.iter().map(|r| r.command.as_str()).collect())
        .unwrap_or_default()
}

fn row<'a>(sections: &'a [HelpSection], command: &str) -> Option<&'a HelpRow> {
    sections
        .iter()
        .flat_map(|s| &s.rows)
        .find(|r| r.command == command)
}

/// The commands of every visible row, headings left out.
fn flat(state: &State) -> Vec<String> {
    visible(state)
        .into_iter()
        .flat_map(|s| s.rows)
        .map(|r| r.command)
        .collect()
}

/// The command of the highlighted row.
fn highlighted(state: &State) -> String {
    let cursor = state.help.as_ref().expect("help open").cursor;
    flat(state)[cursor].clone()
}

const LISTS: [&str; 6] = [
    "SelectNextOrScrollDown",
    "SelectPreviousOrScrollUp",
    "PageSelectNextOrScrollDown",
    "PageSelectPreviousOrScrollUp",
    "SelectFirstOrScrollToTop",
    "SelectLastOrScrollToBottom",
];
const PLAYBACK: [&str; 15] = [
    "ResumePause",
    "NextTrack",
    "PreviousTrack",
    "SeekForward",
    "SeekBackward",
    "SeekStart",
    "Shuffle",
    "Repeat",
    "ToggleAutoplay",
    "VolumeUp",
    "VolumeDown",
    "Mute",
    "AddToQueuePrompt",
    "PlayNextPrompt",
    "ShowActionsOnCurrentTrack",
];
const APP: [&str; 2] = ["OpenCommandHelp", "Quit"];
const ROWS: [&str; 3] = [
    "ChooseSelected",
    "AddSelectedItemToQueue",
    "ShowActionsOnSelectedItem",
];

fn pages(focus: bool, tabs: bool) -> Vec<&'static str> {
    let mut pages = Vec::new();
    if focus {
        pages.extend(["FocusNextWindow", "FocusPreviousWindow"]);
    }
    if tabs {
        pages.extend(["NextTab", "PreviousTab"]);
    }
    pages.extend([
        "Queue",
        "LibraryPage",
        "LikedTrackPage",
        "SearchPage",
        "PreviousPage",
    ]);
    pages
}

type Expected = Vec<(&'static str, Vec<&'static str>)>;

/// A page of windows: its window section, `Lists`, `Pages`, `Playback`,
/// `App`.
fn page_sections(window: (&'static str, Vec<&'static str>), focus: bool, tabs: bool) -> Expected {
    vec![
        window,
        ("Lists", LISTS.to_vec()),
        ("Pages", pages(focus, tabs)),
        ("Playback", PLAYBACK.to_vec()),
        ("App", APP.to_vec()),
    ]
}

// --- AC7 ---------------------------------------------------------------------

/// AC7: per context, the section titles in order and each section's
/// commands (and nothing else).
#[test]
fn ac7_help_sections() {
    let browse = ROWS.to_vec();
    let search_rows = vec![
        "ChooseSelected",
        "AddSelectedItemToQueue",
        "ShowActionsOnSelectedItem",
        "Search",
    ];
    let mut cases: Vec<(String, State, Expected)> = vec![
        (
            "queue".into(),
            with_queue(),
            page_sections(
                (
                    "Queue",
                    vec![
                        "ChooseSelected",
                        "RemoveFromQueue",
                        "ShowActionsOnSelectedItem",
                    ],
                ),
                false,
                false,
            ),
        ),
        (
            "favorite tracks".into(),
            on_page(PageKind::FavoriteTracks, |_| {}),
            page_sections(("Favorite tracks", browse.clone()), false, false),
        ),
        (
            "album".into(),
            on_page(PageKind::Album(10), |_| {}),
            page_sections(("Album", browse.clone()), false, false),
        ),
        (
            "own playlist".into(),
            playlist_page(true),
            page_sections(("Playlist", browse.clone()), false, false),
        ),
        (
            "followed playlist".into(),
            playlist_page(false),
            page_sections(("Playlist", browse.clone()), false, false),
        ),
    ];
    for (window, title) in [
        (0, "Library · Playlists"),
        (1, "Library · Albums"),
        (2, "Library · Artists"),
    ] {
        cases.push((
            title.into(),
            focused(PageKind::Library, window),
            page_sections((title, browse.clone()), true, false),
        ));
    }
    for (window, title) in [
        (0, "Artist · Top tracks"),
        (1, "Artist · Albums"),
        (2, "Artist · Appears on"),
        (3, "Artist · All tracks"),
    ] {
        let mut rows = browse.clone();
        if window == 3 {
            rows.push("RoleFilter");
        }
        cases.push((
            title.into(),
            focused(PageKind::Artist(20), window),
            page_sections((title, rows), true, true),
        ));
    }
    for (window, title) in [
        (0, "Search · Tracks"),
        (1, "Search · Albums"),
        (2, "Search · Artists"),
        (3, "Search · Playlists"),
    ] {
        cases.push((
            title.into(),
            search_on(SearchFocus::Windows, window),
            page_sections((title, search_rows.clone()), true, false),
        ));
    }
    cases.push((
        "search top hit".into(),
        search_on(SearchFocus::TopHit, 0),
        vec![
            ("Search · Top hit", search_rows.clone()),
            ("Pages", pages(true, false)),
            ("Playback", PLAYBACK.to_vec()),
            ("App", APP.to_vec()),
        ],
    ));
    for (name, popup) in popups() {
        let (title, own): (&str, Vec<&str>) = match name {
            "actions" => ("Popup · Actions", vec!["ChooseSelected", "ClosePopup"]),
            "add to playlist" => (
                "Popup · Add to playlist",
                vec!["ChooseSelected", "ClosePopup"],
            ),
            "roles" => (
                "Popup · Role filter",
                vec!["", "ChooseSelected", "ClosePopup"],
            ),
            _ => ("Popup · Confirm", vec!["", "", "ClosePopup"]),
        };
        let mut expected: Expected = vec![(title, own)];
        if name != "confirm" {
            expected.push(("Lists", LISTS.to_vec()));
        }
        expected.push(("App", vec!["OpenCommandHelp"]));
        cases.push((format!("popup {name}"), with_popup(popup), expected));
    }
    cases.push((
        "search input".into(),
        on_page(PageKind::Search(String::new()), |_| {}),
        vec![("Search · Input", vec!["", "", "", "", "", ""])],
    ));
    for (name, state, expected) in cases {
        let sections = help(&state);
        let expected_titles: Vec<&str> = expected.iter().map(|(t, _)| *t).collect();
        assert_eq!(titles(&sections), expected_titles, "{name}: titles");
        for (title, commands) in expected {
            assert_eq!(names(&sections, title), commands, "{name}: {title}");
        }
        assert!(
            sections
                .iter()
                .flat_map(|s| &s.rows)
                .all(|r| !r.keys.is_empty() && !r.text.is_empty()),
            "{name}: every row has keys and a text"
        );
    }
}

/// AC7: the keys are the effective ones, in the file's syntax, and the
/// help text says what the key does in this window.
#[test]
fn ac7_help_keys_and_texts() {
    let state = focused(PageKind::Library, 1);
    let sections = help(&state);
    let keys = |c: &str| row(&sections, c).map(|r| r.keys.clone());
    assert_eq!(keys("ResumePause").as_deref(), Some("space"));
    assert_eq!(keys("Shuffle").as_deref(), Some("C-s"));
    assert_eq!(keys("FocusPreviousWindow").as_deref(), Some("backtab"));
    assert_eq!(
        keys("SelectNextOrScrollDown").as_deref(),
        Some("j  down  C-n")
    );
    assert_eq!(keys("SelectFirstOrScrollToTop").as_deref(), Some("g g"));
    assert_eq!(keys("OpenCommandHelp").as_deref(), Some("?  C-h"));
    assert_eq!(keys("Quit").as_deref(), Some("q  C-c"));
    assert_eq!(keys("VolumeChange"), None, "no key: not listed");
    let text = |state: &State, c: &str| row(&help(state), c).map(|r| r.text.clone());
    assert_eq!(
        text(&state, "ChooseSelected").as_deref(),
        Some("open the album")
    );
    assert_eq!(
        text(&focused(PageKind::Library, 0), "ChooseSelected").as_deref(),
        Some("open the playlist")
    );
    assert_eq!(
        text(&focused(PageKind::Library, 2), "ChooseSelected").as_deref(),
        Some("open the artist")
    );
    assert_eq!(
        text(&on_page(PageKind::Album(10), |_| {}), "ChooseSelected").as_deref(),
        Some("play the track with its list")
    );
    assert_eq!(
        text(&state, "Quit").as_deref(),
        Some("quit (an attached TUI detaches)")
    );
}

/// AC7: a custom keymap: the moved key shown, the removed one gone, the
/// `Actions` section present (after `Playback`, before `App`).
#[test]
fn ac7_help_custom_keymap() {
    let state = custom_keymap();
    let sections = help(&state);
    assert_eq!(
        titles(&sections),
        ["Queue", "Lists", "Pages", "Playback", "Actions", "App"]
    );
    assert_eq!(row(&sections, "Shuffle").unwrap().keys, "S");
    assert!(row(&sections, "NextTrack").is_none(), "unbound: not listed");
    let action = row(&sections, "GoToAlbum").expect("the action");
    assert_eq!(action.keys, "g B");
    assert_eq!(names(&sections, "Actions"), ["GoToAlbum"]);
    assert!(
        !help(&with_queue()).iter().any(|s| s.title == "Actions"),
        "no [[actions]]: no section"
    );
}

/// AC7: rows that would do nothing now are dim: playback keys on an empty
/// queue, anything sent to the player while disconnected.
#[test]
fn ac7_help_dim_rows() {
    let dim = |state: &State, c: &str| row(&help(state), c).unwrap_or_else(|| panic!("{c}")).dim;
    let empty = State::default();
    let full = with_queue();
    let mut offline = with_queue();
    offline.connection = Connection::Disconnected { shut_down: false };
    // (command, empty queue, queue, disconnected)
    let table = [
        ("ResumePause", true, false, true),
        ("NextTrack", true, false, true),
        ("SeekStart", true, false, true),
        ("Shuffle", false, false, true),
        ("VolumeUp", false, false, true),
        ("Mute", false, false, true),
        ("LibraryPage", false, false, false),
        ("Queue", false, false, false),
        ("OpenCommandHelp", false, false, false),
        ("Quit", false, false, false),
        ("SelectNextOrScrollDown", false, false, false),
        ("ShowActionsOnCurrentTrack", true, false, false),
    ];
    for (command, on_empty, on_full, on_offline) in table {
        assert_eq!(dim(&empty, command), on_empty, "{command}: empty queue");
        assert_eq!(dim(&full, command), on_full, "{command}: queue");
        assert_eq!(
            dim(&offline, command),
            on_offline,
            "{command}: disconnected"
        );
    }
    // On the queue page the entry rows need an entry.
    assert!(dim(&empty, "ChooseSelected"));
    assert!(!dim(&full, "ChooseSelected"));
    assert!(dim(&offline, "ChooseSelected"));
}

// --- AC8 ---------------------------------------------------------------------

/// AC8: `?` and `C-h` open the help from every page and popup, and close
/// it again with no effect; it leaves what was open as it was.
#[test]
fn ac8_help_keys() {
    let mut origins: Vec<(String, State)> = vec![
        ("queue".into(), with_queue()),
        ("library".into(), focused(PageKind::Library, 1)),
        ("artist".into(), focused(PageKind::Artist(20), 3)),
        ("search windows".into(), search_on(SearchFocus::Windows, 0)),
        ("search top hit".into(), search_on(SearchFocus::TopHit, 0)),
    ];
    origins.extend(
        popups()
            .into_iter()
            .map(|(n, p)| (format!("popup {n}"), with_popup(p))),
    );
    for (name, origin) in origins {
        for open in [Key::Char('?'), Key::Ctrl('h')] {
            for close in [Key::Char('?'), Key::Ctrl('h'), Key::Char('q'), Key::Esc] {
                let mut state = origin.clone();
                assert_eq!(press(&mut state, &[open]), vec![], "{name}: open");
                assert!(state.help.is_some(), "{name}: {open:?} opens the help");
                assert_eq!(state.popup, origin.popup, "{name}: popup kept");
                assert_eq!(press(&mut state, &[close]), vec![], "{name}: close");
                assert_eq!(state, origin, "{name}: {open:?} then {close:?}");
            }
        }
    }
}

/// AC8: `?` types into every text input instead of opening the help.
#[test]
fn ac8_help_text_inputs() {
    let mut prompt = with_queue();
    press(&mut prompt, &[Key::Char('o'), Key::Char('?')]);
    assert!(prompt.help.is_none());
    assert_eq!(prompt.prompt.as_ref().map(|p| p.text.as_str()), Some("?"));

    let mut search = on_page(PageKind::Search(String::new()), |_| {});
    press(&mut search, &[Key::Char('?')]);
    assert!(search.help.is_none());
    assert_eq!(search.page().search.as_ref().unwrap().input, "?");

    let mut name = with_popup(Popup::NewPlaylist {
        tracks: TrackSource::Tracks(vec![]),
        name: String::new(),
    });
    press(&mut name, &[Key::Char('?')]);
    assert!(name.help.is_none());
    assert!(matches!(name.popup, Some(Popup::NewPlaylist { name: ref n, .. }) if n == "?"));
}

/// AC8: list commands move the highlight over binding rows only
/// (headings skipped), clamped.
#[test]
fn ac8_help_moves() {
    let mut state = with_queue();
    press(&mut state, &[Key::Char('?')]);
    let rows = flat(&state);
    let last = rows.len() - 1;
    let cursor = |s: &State| s.help.as_ref().unwrap().cursor;
    assert_eq!(cursor(&state), 0);
    assert_eq!(highlighted(&state), "ChooseSelected");
    press(&mut state, &[Key::Char('k')]);
    assert_eq!(cursor(&state), 0, "clamped at the top");
    press(&mut state, &[Key::Char('j'), Key::Down, Key::Ctrl('n')]);
    assert_eq!(cursor(&state), 3);
    assert_eq!(
        highlighted(&state),
        "SelectNextOrScrollDown",
        "the heading `Lists` is skipped: the next row of the next section"
    );
    press(&mut state, &[Key::Char('G')]);
    assert_eq!(cursor(&state), last);
    press(&mut state, &[Key::Char('j')]);
    assert_eq!(cursor(&state), last, "clamped at the bottom");
    press(&mut state, &[Key::Char('g'), Key::Char('g')]);
    assert_eq!(cursor(&state), 0);
    press(&mut state, &[Key::Ctrl('f')]);
    assert_eq!(cursor(&state), 20.min(last));
    press(&mut state, &[Key::Ctrl('b')]);
    assert_eq!(cursor(&state), 0);
    press(&mut state, &[Key::End]);
    assert_eq!(cursor(&state), last);
}

/// AC8: `/` filters on keys, command names and help text ignoring case;
/// `Backspace` edits, `Enter` keeps, `Esc` clears and a second `Esc`
/// closes.
#[test]
fn ac8_help_filter() {
    let seek = vec!["SeekForward", "SeekBackward", "SeekStart"];
    let slash = || vec![Key::Char('/')];
    // (name, keys after `?`, rows still shown ("*all": every row), filter, typing)
    type Case<'a> = (&'a str, Vec<Key>, Vec<&'a str>, &'a str, bool);
    let cases: Vec<Case> = vec![
        (
            "by name",
            [slash(), typed("seek")].concat(),
            seek.clone(),
            "seek",
            true,
        ),
        (
            "ignoring case",
            [slash(), typed("SEEK")].concat(),
            seek.clone(),
            "SEEK",
            true,
        ),
        (
            "by text",
            [slash(), typed("repeat: off")].concat(),
            vec!["Repeat"],
            "repeat: off",
            true,
        ),
        (
            "by keys",
            [slash(), typed("C-r")].concat(),
            vec!["Repeat"],
            "C-r",
            true,
        ),
        (
            "backspace edits",
            [slash(), typed("seekx"), vec![Key::Backspace]].concat(),
            seek.clone(),
            "seek",
            true,
        ),
        (
            "enter keeps",
            [slash(), typed("seek"), vec![Key::Enter]].concat(),
            seek.clone(),
            "seek",
            false,
        ),
        (
            "nothing matches",
            [slash(), typed("xyz")].concat(),
            vec![],
            "xyz",
            true,
        ),
        (
            "q and ? type while typing",
            [slash(), typed("q?")].concat(),
            vec![],
            "q?",
            true,
        ),
        (
            "esc clears",
            [slash(), typed("seek"), vec![Key::Esc]].concat(),
            vec!["*all"],
            "",
            false,
        ),
        (
            "esc clears a kept filter",
            [slash(), typed("seek"), vec![Key::Enter, Key::Esc]].concat(),
            vec!["*all"],
            "",
            false,
        ),
    ];
    let all = flat(&{
        let mut s = with_queue();
        press(&mut s, &[Key::Char('?')]);
        s
    });
    for (name, keys, expected, filter, typing) in cases {
        let mut state = with_queue();
        press(&mut state, &[Key::Char('?')]);
        assert_eq!(press(&mut state, &keys), vec![], "{name}");
        let help_state = state
            .help
            .as_ref()
            .unwrap_or_else(|| panic!("{name}: open"));
        assert_eq!(help_state.filter, filter, "{name}: filter");
        assert_eq!(help_state.typing, typing, "{name}: typing");
        let shown = flat(&state);
        if expected == ["*all"] {
            assert_eq!(shown, all, "{name}");
        } else {
            assert_eq!(shown, expected, "{name}");
        }
        assert!(
            visible(&state).iter().all(|s| !s.rows.is_empty()),
            "{name}: empty sections disappear"
        );
    }
    // A kept filter: movement works again, and `Esc`, `Esc` closes.
    let mut state = with_queue();
    press(&mut state, &[Key::Char('?'), Key::Char('/')]);
    press(&mut state, &typed("seek"));
    press(&mut state, &[Key::Enter, Key::Char('j')]);
    assert_eq!(highlighted(&state), "SeekBackward");
    press(&mut state, &[Key::Esc]);
    assert!(state.help.is_some(), "the first Esc only clears");
    press(&mut state, &[Key::Esc]);
    assert!(state.help.is_none(), "the second closes");
    // Esc while typing an empty filter ends the typing.
    let mut state = with_queue();
    press(&mut state, &[Key::Char('?'), Key::Char('/'), Key::Esc]);
    assert!(!state.help.as_ref().unwrap().typing);
}

/// AC8: `Enter` on a row closes the help and emits what the binding's
/// keys emit in the original view.
#[test]
fn ac8_help_runs_row() {
    // space -> TogglePause
    let mut state = with_queue();
    press(&mut state, &[Key::Char('?'), Key::Char('/')]);
    press(&mut state, &typed("play / pause"));
    press(&mut state, &[Key::Enter]);
    assert_eq!(
        press(&mut state, &[Key::Enter]),
        vec![Effect::Send(Command::TogglePause)]
    );
    assert!(state.help.is_none());

    // g l -> the library pushed and fetched
    let mut state = with_queue();
    press(&mut state, &[Key::Char('?'), Key::Char('/')]);
    press(&mut state, &typed("the library"));
    press(&mut state, &[Key::Enter]);
    let effects = press(&mut state, &[Key::Enter]);
    assert!(state.help.is_none());
    assert_eq!(state.page().kind, PageKind::Library);
    assert!(
        matches!(effects.as_slice(), [Effect::Library { .. }]),
        "{effects:?}"
    );

    // Disconnected: nothing leaves.
    let mut state = with_queue();
    state.connection = Connection::Disconnected { shut_down: false };
    press(&mut state, &[Key::Char('?'), Key::Char('/')]);
    press(&mut state, &typed("play / pause"));
    press(&mut state, &[Key::Enter]);
    assert_eq!(press(&mut state, &[Key::Enter]), vec![]);

    // Enter with no row shown does nothing and keeps the help.
    let mut state = with_queue();
    press(&mut state, &[Key::Char('?'), Key::Char('/')]);
    press(&mut state, &typed("xyz"));
    press(&mut state, &[Key::Enter, Key::Enter]);
    assert!(state.help.is_some());
}

/// AC8: for every binding row, `Enter` leaves the state and the effects
/// exactly as pressing the binding's keys does in the original view,
/// from pages and from popups, `[[actions]]` rows included.
#[test]
fn ac8_help_runs_every_row() {
    let mut origins: Vec<(String, State)> = vec![
        ("queue".into(), with_queue()),
        ("custom".into(), custom_keymap()),
        ("library".into(), focused(PageKind::Library, 0)),
        ("artist".into(), focused(PageKind::Artist(20), 3)),
        ("search".into(), search_on(SearchFocus::Windows, 1)),
        ("empty queue".into(), State::default()),
    ];
    origins.extend(
        popups()
            .into_iter()
            .map(|(n, p)| (format!("popup {n}"), with_popup(p))),
    );
    for (name, origin) in origins {
        let sections = help(&origin);
        let rows: Vec<_> = sections.iter().flat_map(|s| &s.rows).collect();
        assert!(!rows.is_empty(), "{name}");
        for (index, row) in rows.iter().enumerate() {
            let Some(first) = row.sequences.first() else {
                continue;
            };
            let mut direct = origin.clone();
            let expected = press(&mut direct, first.keys());

            let mut via = origin.clone();
            press(&mut via, &[Key::Char('?')]);
            via.help.as_mut().expect("open").cursor = index;
            let effects = press(&mut via, &[Key::Enter]);
            assert_eq!(effects, expected, "{name}: {} ({})", row.keys, row.command);
            assert_eq!(via, direct, "{name}: {} ({})", row.keys, row.command);
        }
    }
}

/// AC8: while it is open no other key acts.
#[test]
fn ac8_help_no_other_key_acts() {
    let keys = [
        Key::Char(' '),
        Key::Char('n'),
        Key::Char('z'),
        Key::Char('a'),
        Key::Char('o'),
        Key::Char('d'),
        Key::Char('Z'),
        Key::Char('+'),
        Key::Char('x'),
        Key::Tab,
        Key::BackTab,
        Key::Backspace,
        Key::Ctrl('s'),
        Key::Ctrl('x'),
    ];
    for key in keys {
        let mut state = with_queue();
        press(&mut state, &[Key::Char('?')]);
        let before = state.clone();
        assert_eq!(press(&mut state, &[key]), vec![], "{key:?}");
        assert_eq!(state, before, "{key:?}");
    }
    // `g l` completes a binding the help does not run.
    let mut state = with_queue();
    press(&mut state, &[Key::Char('?')]);
    let before = state.clone();
    assert_eq!(press(&mut state, &[Key::Char('g'), Key::Char('l')]), vec![]);
    assert_eq!(state, before);
}
