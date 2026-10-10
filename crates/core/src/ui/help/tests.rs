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
use super::{HelpRow, HelpSection, HintEntry, Hints, help, hints, visible};

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
        device: "default".into(),
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
const PLAYBACK: [&str; 16] = [
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
    // Spec 0014.
    "SwitchDevice",
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
        "MixesPage",
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
        // The default `[[actions]]` binding (spec 0011).
        ("Actions", vec!["GoToRadio"]),
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
            ("Actions", vec!["GoToRadio"]),
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
    assert_eq!(names(&sections, "Actions"), ["GoToRadio", "GoToAlbum"]);
    // Without a file's `[[actions]]` the section holds the default one.
    assert_eq!(names(&help(&with_queue()), "Actions"), ["GoToRadio"]);
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
        // Spec 0014: the devices popup asks the player.
        ("SwitchDevice", false, false, true),
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

// --- spec 0013: key-sequence hints ---------------------------------------------

use super::super::page::Rows;
use super::super::{Purpose, WholeList, WholeListSource};

/// A keymap of `keymaps` (sequence, command) and `actions` (sequence,
/// action) over the defaults, on `state`.
fn keyed(mut state: State, keymaps: &[(&str, &str)], actions: &[(&str, &str)]) -> State {
    let file = KeymapFile {
        keymaps: keymaps
            .iter()
            .map(|(sequence, command)| KeymapEntry {
                command: CommandEntry::name(command),
                key_sequence: (*sequence).into(),
            })
            .collect(),
        actions: actions
            .iter()
            .map(|(sequence, action)| ActionEntry {
                action: (*action).into(),
                key_sequence: (*sequence).into(),
                target: Target::PlayingTrack,
            })
            .collect(),
    };
    apply_keymap(&mut state, build(&file).expect("a valid keymap"));
    state
}

/// The library, *Playlists* focused, with one playlist row (selected).
fn library_with_row() -> State {
    on_page(PageKind::Library, |p| {
        p.focus = 0;
        p.windows[0].rows = Rows::Playlists(vec![PlaylistSummary {
            uuid: "p1".into(),
            title: "Mine".into(),
            tracks: Some(2),
            duration: None,
            own: true,
        }]);
        p.windows[0].total = Some(1);
    })
}

/// The queue page with entries 1 and 2, the cursor on entry 1.
fn queue_with_cursor() -> State {
    let mut state = with_queue();
    state.cursor = Some(EntryId(1));
    state
}

fn entry(key: &str, text: &str, dim: bool) -> HintEntry {
    HintEntry {
        key: key.into(),
        text: text.into(),
        dim,
        more: 0,
    }
}

fn nested(key: &str, more: usize, dim: bool) -> HintEntry {
    HintEntry {
        key: key.into(),
        text: String::new(),
        dim,
        more,
    }
}

fn pressed(mut state: State, keys: &[Key]) -> State {
    press(&mut state, keys);
    state
}

/// AC1: after `g`, one entry per next key with the keys help's text, dim
/// and order, only the bindings the help lists in this view; a custom
/// keymap's bindings appear and an unbound one is gone.
#[test]
fn ac1_hints_entries() {
    let g = [Key::Char('g')];
    let pages = |a: HintEntry| {
        vec![
            a,
            entry("g", "move to the top", false),
            entry("l", "the library", false),
            entry("y", "favorite tracks", false),
            entry("s", "the search page (on one: its input)", false),
            entry("m", "your mixes", false),
        ]
    };
    let rows: Vec<(&str, State, Vec<HintEntry>)> = vec![
        (
            "library window, a row selected",
            pressed(library_with_row(), &g),
            pages(entry("a", "actions on the selected row", false)),
        ),
        (
            "library window, no row",
            pressed(focused(PageKind::Library, 0), &g),
            pages(entry("a", "actions on the selected row", true)),
        ),
        (
            "queue page",
            pressed(queue_with_cursor(), &g),
            pages(entry("a", "actions on the entry", false)),
        ),
        (
            "actions popup",
            pressed(with_popup(popups().remove(0).1), &g),
            vec![entry("g", "move to the top", false)],
        ),
        (
            "custom keymap",
            pressed(
                keyed(
                    queue_with_cursor(),
                    &[("g n", "NextTrack"), ("g l", "None")],
                    &[("g B", "GoToAlbum")],
                ),
                &g,
            ),
            vec![
                entry("a", "actions on the entry", false),
                entry("g", "move to the top", false),
                entry("y", "favorite tracks", false),
                entry("s", "the search page (on one: its input)", false),
                entry("m", "your mixes", false),
                entry("n", "next track", false),
                entry("B", "go to the album (playing track)", false),
            ],
        ),
        (
            "custom keymap, disconnected",
            {
                let mut state = keyed(queue_with_cursor(), &[("g n", "NextTrack")], &[]);
                state.connection = Connection::Disconnected { shut_down: false };
                pressed(state, &g)
            },
            {
                let mut entries = pages(entry("a", "actions on the entry", false));
                entries.push(entry("n", "next track", true));
                entries
            },
        ),
    ];
    for (name, state, entries) in rows {
        assert_eq!(
            hints(&state),
            Some(Hints {
                prefix: "g".into(),
                entries,
            }),
            "{name}"
        );
    }
}

/// AC2: a next key that starts longer sequences is one `+N` entry with no
/// text, dim only when everything under it is; pressing it lists the next
/// level.
#[test]
fn ac2_hints_nested_prefix() {
    let s = Key::Char('s');
    let state = keyed(
        State::default(),
        &[
            ("s q", "Queue"),
            ("s l a", "LikedTrackPage"),
            ("s l b", "NextTrack"),
        ],
        &[],
    );
    let after_s = pressed(state.clone(), &[s]);
    assert_eq!(
        hints(&after_s),
        Some(Hints {
            prefix: "s".into(),
            entries: vec![entry("q", "the queue page", false), nested("l", 2, false),],
        }),
        "after s"
    );
    let after_sl = pressed(state, &[s, Key::Char('l')]);
    assert_eq!(
        hints(&after_sl),
        Some(Hints {
            prefix: "s l".into(),
            entries: vec![
                entry("a", "favorite tracks", false),
                // Empty queue: next track does nothing.
                entry("b", "next track", true),
            ],
        }),
        "after s l"
    );
    // Everything under `n` is dim on an empty queue.
    let all_dim = keyed(
        State::default(),
        &[("s n a", "NextTrack"), ("s n b", "PreviousTrack")],
        &[],
    );
    assert_eq!(
        hints(&pressed(all_dim, &[s])),
        Some(Hints {
            prefix: "s".into(),
            entries: vec![nested("n", 2, true)],
        }),
        "all dim"
    );
}

/// AC3: no hints with nothing pending, over the keys help, during a
/// whole-list load, with `key_hints` off, or when nothing under the
/// pending keys acts here.
#[test]
fn ac3_no_hints() {
    let g = Key::Char('g');
    let rows: Vec<(&str, State)> = vec![
        ("nothing pending", library_with_row()),
        ("nothing pending, empty queue", State::default()),
        (
            "keys help open",
            pressed(library_with_row(), &[Key::Char('?'), g]),
        ),
        ("whole-list load", {
            let mut state = library_with_row();
            state.whole_list = Some(WholeList {
                source: WholeListSource::Window { page: 1, window: 0 },
                purpose: Purpose::Play { start: 0 },
            });
            pressed(state, &[g])
        }),
        ("key_hints off", {
            let mut state = library_with_row();
            state.key_hints = false;
            pressed(state, &[g])
        }),
        (
            "nothing under the prefix acts here",
            pressed(
                keyed(library_with_row(), &[("x y", "RemoveFromQueue")], &[]),
                &[Key::Char('x')],
            ),
        ),
    ];
    for (name, state) in rows {
        assert_eq!(hints(&state), None, "{name}");
    }
    // The pending keys themselves are untouched (0008: no timeout).
    let state = pressed(
        keyed(library_with_row(), &[("x y", "RemoveFromQueue")], &[]),
        &[Key::Char('x')],
    );
    assert_eq!(state.pending, vec![Key::Char('x')]);
}

/// AC4: the same keys give the same effects and state with `key_hints` on
/// and off; the hints are gone once the sequence completes, mismatches or
/// is cancelled.
#[test]
fn ac4_hints_do_not_change_keys() {
    let k = Key::Char;
    let nested_keymap = keyed(
        queue_with_cursor(),
        &[("s l a", "LibraryPage"), ("s l b", "NextTrack")],
        &[],
    );
    let rows: Vec<(&str, State, Vec<Key>)> = vec![
        ("complete g l", queue_with_cursor(), vec![k('g'), k('l')]),
        ("mismatch g x", queue_with_cursor(), vec![k('g'), k('x')]),
        ("mismatch g n", queue_with_cursor(), vec![k('g'), k('n')]),
        ("cancel g esc", queue_with_cursor(), vec![k('g'), Key::Esc]),
        ("nested s l a", nested_keymap, vec![k('s'), k('l'), k('a')]),
        (
            "over a popup g g",
            pressed(with_popup(popups().remove(0).1), &[k('j')]),
            vec![k('g'), k('g')],
        ),
        (
            "over a popup g esc",
            with_popup(popups().remove(0).1),
            vec![k('g'), Key::Esc],
        ),
    ];
    for (name, origin, keys) in rows {
        let mut on = origin.clone();
        on.key_hints = true;
        let mut off = origin;
        off.key_hints = false;
        for (i, key) in keys.iter().enumerate() {
            let effects_on = press(&mut on, &[*key]);
            let effects_off = press(&mut off, &[*key]);
            assert_eq!(effects_on, effects_off, "{name}: key {i}");
            let mut same = off.clone();
            same.key_hints = true;
            assert_eq!(on, same, "{name}: key {i}");
            if i + 1 < keys.len() {
                assert!(hints(&on).is_some(), "{name}: no hints after key {i}");
            }
        }
        assert!(on.pending.is_empty(), "{name}: still pending");
        assert_eq!(hints(&on), None, "{name}: hints after the sequence");
        assert_eq!(hints(&off), None, "{name}: hints with key_hints off");
    }
}

// --- spec 0014 AC8 ------------------------------------------------------------

/// Spec 0014 AC8: the devices popup's section lists its keys (`r`, `q`,
/// `Enter`, `esc`) with `Lists` and `?`; `D` is in `Playback` everywhere,
/// dim while disconnected, and runs from the help like any row.
#[test]
fn ac8_devices_popup_help() {
    use super::super::DeviceList;
    use crate::protocol::DeviceEntry;

    let lists = [
        DeviceList::Loading { id: 1 },
        DeviceList::Loaded(vec![DeviceEntry {
            name: "default".into(),
            description: "shared, through the system mixer".into(),
        }]),
        DeviceList::Failed("permission denied".into()),
    ];
    for list in lists {
        let state = with_popup(Popup::Devices {
            list: list.clone(),
            cursor: 0,
        });
        let sections = help(&state);
        assert_eq!(
            titles(&sections),
            vec!["Popup · Devices", "Lists", "App"],
            "{list:?}"
        );
        assert_eq!(
            names(&sections, "Popup · Devices"),
            vec!["", "", "ChooseSelected", "ClosePopup"],
            "{list:?}"
        );
        let own: Vec<(&str, &str)> = sections[0]
            .rows
            .iter()
            .map(|r| (r.keys.as_str(), r.text.as_str()))
            .collect();
        assert_eq!(
            own,
            vec![
                ("r", "read the list again"),
                ("q", "close"),
                ("enter", "switch to the selected device"),
                ("esc", "close / cancel"),
            ],
            "{list:?}"
        );
        assert_eq!(names(&sections, "Lists"), LISTS.to_vec(), "{list:?}");
        assert_eq!(names(&sections, "App"), vec!["OpenCommandHelp"]);
    }

    // `D` in `Playback`, with its key and text; dim while disconnected.
    let state = with_queue();
    let switch = row(&help(&state), "SwitchDevice").cloned();
    let switch = switch.expect("SwitchDevice is listed");
    assert_eq!(
        (switch.keys.as_str(), switch.text.as_str(), switch.dim),
        ("D", "choose the output device", false)
    );
    assert!(names(&help(&state), "Playback").contains(&"SwitchDevice"));
    let mut offline = with_queue();
    offline.connection = Connection::Disconnected { shut_down: false };
    assert!(
        row(&help(&offline), "SwitchDevice").is_some_and(|r| r.dim),
        "dim while disconnected"
    );

    // `Enter` on its row opens the popup, as `D` does.
    let mut via = with_queue();
    press(&mut via, &[Key::Char('?')]);
    let index = flat(&via)
        .iter()
        .position(|c| c == "SwitchDevice")
        .expect("listed");
    via.help.as_mut().expect("open").cursor = index;
    let effects = press(&mut via, &[Key::Enter]);
    assert!(
        matches!(effects.as_slice(), [Effect::Devices { .. }]),
        "{effects:?}"
    );
    assert!(matches!(via.popup, Some(Popup::Devices { .. })));
}
