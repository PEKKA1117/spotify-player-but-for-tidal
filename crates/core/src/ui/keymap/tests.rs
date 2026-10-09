//! Spec 0008 AC1–AC6 and the model half of AC12: key syntax, building the
//! keymap, dispatch through it, rebinding, `[[actions]]`, text inputs
//! before the keymap, and the notice of skipped spotify-player names.

use std::collections::BTreeMap;
use std::time::Duration;

use crate::item::Item;
use crate::library::{
    AlbumKind, AlbumSummary, LibraryRequest, LibraryResponse, ListPage, PageData, PageRequest,
    PlaylistSummary,
};
use crate::protocol::RepeatMode;
use crate::protocol::{self, Command, InsertAt, PlaybackState, PlayerSnapshot, QueueEntry};
use crate::track::{AlbumRef, ArtistRef, EntryId, Track, TrackId};

use super::super::dispatch::run_command;
use super::super::{
    Action, Confirmed, Effect, Key, Page, PageKind, Popup, SearchFocus, State, TrackSource,
    apply_keymap, help, update,
};
use super::{
    ACTIONS, ActionBinding, ActionEntry, ActionKind, BaseKey, Binding, CommandEntry, KeyError,
    KeySequence, Keymap, KeymapEntry, KeymapFile, Target, UNSUPPORTED_ACTIONS,
    UNSUPPORTED_COMMANDS, UiCommand, build,
};

// --- fixtures ----------------------------------------------------------------

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
        artists: vec![artist(20), artist(21)],
        album: Some(AlbumRef {
            id: 10,
            title: "Al10".into(),
            cover: None,
        }),
        duration: Some(Duration::from_secs(100)),
        streamable: true,
    }
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

fn list<T>(items: Vec<T>, total: u32) -> ListPage<T> {
    ListPage {
        items,
        offset: 0,
        total,
        hidden: 0,
    }
}

/// Playlists Mine (own) and Theirs (followed); albums 10, 11; artists 20,
/// 21.
fn library_data() -> PageData {
    PageData::Library {
        playlists: list(
            vec![
                playlist("p1", "Mine", true),
                playlist("p2", "Theirs", false),
            ],
            2,
        ),
        albums: list(vec![album(10), album(11)], 2),
        artists: list(vec![artist(20), artist(21)], 2),
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

/// The queue page over entries 7, 3, 9, entry 3 playing and under the
/// cursor (the middle row, so every cursor move moves it).
fn queue() -> State {
    let mut state = State::default();
    update(
        &mut state,
        Action::Player(protocol::Event::Player(snapshot(&[7, 3, 9], Some(3)))),
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

const GN: [Key; 2] = [Key::Char('g'), Key::Char('n')];
const MX: Key = Key::Alt(BaseKey::Char('x'));

/// What `keymap` binds the sequence labelled `label` to (found by its
/// label, so that only AC1 depends on the parser).
fn bound(keymap: &Keymap, label: &str) -> Option<Binding> {
    keymap
        .bindings()
        .iter()
        .find(|(s, _)| s.to_string() == label)
        .map(|(_, b)| *b)
}

fn send(command: Command) -> Vec<Effect> {
    vec![Effect::Send(command)]
}

/// Presses `keys`, which must ask for one page, and answers it with `data`.
fn open(state: &mut State, keys: &[Key], data: PageData) {
    let effects = press(state, keys);
    let [Effect::Library { id, .. }] = effects.as_slice() else {
        panic!("one request expected: {effects:?}");
    };
    update(
        state,
        Action::LibraryReply {
            id: *id,
            result: Ok(LibraryResponse::Page(data)),
        },
    );
}

/// The queue state under the library page, with the window `tabs` presses
/// of `Tab` from *Playlists* focused and `down` rows down.
fn library(tabs: usize, down: usize) -> State {
    let mut state = queue();
    open(
        &mut state,
        &[Key::Char('g'), Key::Char('l')],
        library_data(),
    );
    press(&mut state, &vec![Key::Tab; tabs]);
    press(&mut state, &vec![Key::Down; down]);
    state
}

/// The queue state under a playlist page (own or followed) on its first
/// track.
fn playlist_page(own: bool) -> State {
    let mut state = library(0, usize::from(!own));
    open(
        &mut state,
        &[Key::Enter],
        PageData::Playlist {
            playlist: playlist(if own { "p1" } else { "p2" }, "P", own),
            etag: Some("e1".into()),
            tracks: list(vec![track(1), track(2)], 2),
        },
    );
    state
}

fn entry(sequence: &str, command: &str) -> KeymapEntry {
    KeymapEntry {
        command: CommandEntry::name(command),
        key_sequence: sequence.into(),
    }
}

fn entry_with(sequence: &str, command: &str, params: &[(&str, i64)]) -> KeymapEntry {
    KeymapEntry {
        command: CommandEntry {
            name: command.into(),
            params: Some(params.iter().map(|(k, v)| (k.to_string(), *v)).collect()),
        },
        key_sequence: sequence.into(),
    }
}

fn action(sequence: &str, name: &str, target: Target) -> ActionEntry {
    ActionEntry {
        action: name.into(),
        key_sequence: sequence.into(),
        target,
    }
}

fn file(keymaps: Vec<KeymapEntry>, actions: Vec<ActionEntry>) -> KeymapFile {
    KeymapFile { keymaps, actions }
}

fn keymap(keymaps: Vec<KeymapEntry>, actions: Vec<ActionEntry>) -> Keymap {
    build(&file(keymaps, actions)).unwrap_or_else(|e| panic!("{e}"))
}

/// `state` with `keymap` in force.
fn with_keymap(mut state: State, keymap: Keymap) -> State {
    apply_keymap(&mut state, keymap);
    state
}

// --- AC1 ---------------------------------------------------------------------

/// AC1: every key name, single characters, `C-`/`M-` with characters and
/// names, `C-S` = `C-s` and multi-key sequences parse and print back as
/// written; malformed sequences are errors naming the key.
#[test]
fn ac1_key_syntax() {
    use BaseKey as B;
    use Key::{Alt, Char, Ctrl, CtrlKey};
    // (text, keys, label when it differs from the text)
    let ok: Vec<(&str, Vec<Key>, Option<&str>)> = vec![
        ("a", vec![Char('a')], None),
        ("A", vec![Char('A')], None),
        (">", vec![Char('>')], None),
        ("?", vec![Char('?')], None),
        ("-", vec![Char('-')], None),
        ("C", vec![Char('C')], None),
        ("enter", vec![Key::Enter], None),
        ("space", vec![Char(' ')], None),
        ("tab", vec![Key::Tab], None),
        ("backtab", vec![Key::BackTab], None),
        ("backspace", vec![Key::Backspace], None),
        ("esc", vec![Key::Esc], None),
        ("left", vec![Key::Left], None),
        ("right", vec![Key::Right], None),
        ("up", vec![Key::Up], None),
        ("down", vec![Key::Down], None),
        ("insert", vec![Key::Insert], None),
        ("delete", vec![Key::Delete], None),
        ("home", vec![Key::Home], None),
        ("end", vec![Key::End], None),
        ("page_up", vec![Key::PageUp], None),
        ("page_down", vec![Key::PageDown], None),
        ("f1", vec![Key::F(1)], None),
        ("f12", vec![Key::F(12)], None),
        ("C-s", vec![Ctrl('s')], None),
        ("C-S", vec![Ctrl('s')], Some("C-s")),
        ("C-space", vec![Ctrl(' ')], None),
        ("C--", vec![Ctrl('-')], None),
        ("C-enter", vec![CtrlKey(B::Enter)], None),
        ("C-f5", vec![CtrlKey(B::F(5))], None),
        ("M-a", vec![Alt(B::Char('a'))], None),
        ("M-A", vec![Alt(B::Char('A'))], None),
        ("M-enter", vec![Alt(B::Enter)], None),
        ("M-space", vec![Alt(B::Char(' '))], None),
        ("M-page_down", vec![Alt(B::PageDown)], None),
        ("g g", vec![Char('g'), Char('g')], None),
        ("s l a", vec![Char('s'), Char('l'), Char('a')], None),
        (
            "C-x M-y enter",
            vec![Ctrl('x'), Alt(B::Char('y')), Key::Enter],
            None,
        ),
    ];
    for (text, keys, label) in ok {
        let parsed = KeySequence::parse(text);
        assert_eq!(parsed, Ok(KeySequence(keys)), "{text:?}");
        let parsed = parsed.expect("parsed");
        assert_eq!(parsed.to_string(), label.unwrap_or(text), "{text:?}: label");
    }

    let unknown = |key: &str, sequence: &str| KeyError::UnknownKey {
        key: key.into(),
        sequence: sequence.into(),
    };
    // (text, the error naming the key)
    let errors = [
        ("", KeyError::Empty),
        ("ctrl+s", unknown("ctrl+s", "ctrl+s")),
        ("C-", unknown("C-", "C-")),
        ("M-", unknown("M-", "M-")),
        ("X-a", unknown("X-a", "X-a")),
        ("a  b", unknown("", "a  b")),
        ("a ", unknown("", "a ")),
        (" a", unknown("", " a")),
        ("f0", unknown("f0", "f0")),
        ("f13", unknown("f13", "f13")),
        ("F1", unknown("F1", "F1")),
        ("Enter", unknown("Enter", "Enter")),
        ("C-M-a", unknown("C-M-a", "C-M-a")),
        ("g gg", unknown("gg", "g gg")),
    ];
    for (text, error) in errors {
        assert_eq!(KeySequence::parse(text), Err(error), "{text:?}");
    }
    assert_eq!(
        KeySequence::parse("ctrl+s")
            .map_err(|e| e.to_string())
            .err()
            .as_deref(),
        Some(r#"unknown key "ctrl+s" in "ctrl+s""#)
    );
}

// --- AC2 ---------------------------------------------------------------------

/// The table of spec 0008 "Commands and their default keys", in order.
const DEFAULTS: [(&[&str], &str); 41] = [
    (&["space"], "ResumePause"),
    (&["n"], "NextTrack"),
    (&["p"], "PreviousTrack"),
    (&[">"], "SeekForward"),
    (&["<"], "SeekBackward"),
    (&["^"], "SeekStart"),
    (&["C-s"], "Shuffle"),
    (&["C-r"], "Repeat"),
    (&["A"], "ToggleAutoplay"),
    (&["+"], "VolumeUp"),
    (&["-"], "VolumeDown"),
    (&["_"], "Mute"),
    (&["o"], "AddToQueuePrompt"),
    (&["O"], "PlayNextPrompt"),
    (&["j", "down", "C-n"], "SelectNextOrScrollDown"),
    (&["k", "up", "C-p"], "SelectPreviousOrScrollUp"),
    (&["C-f", "page_down"], "PageSelectNextOrScrollDown"),
    (&["C-b", "page_up"], "PageSelectPreviousOrScrollUp"),
    (&["g g"], "SelectFirstOrScrollToTop"),
    (&["G", "end"], "SelectLastOrScrollToBottom"),
    (&["enter"], "ChooseSelected"),
    (&["Z", "C-z"], "AddSelectedItemToQueue"),
    (&["d"], "RemoveFromQueue"),
    (&["g a", "C-space"], "ShowActionsOnSelectedItem"),
    (&["a"], "ShowActionsOnCurrentTrack"),
    (&["tab"], "FocusNextWindow"),
    (&["backtab"], "FocusPreviousWindow"),
    (&["]"], "NextTab"),
    (&["["], "PreviousTab"),
    (&["f"], "RoleFilter"),
    (&["z"], "Queue"),
    (&["g l"], "LibraryPage"),
    (&["g y"], "LikedTrackPage"),
    (&["g s"], "SearchPage"),
    (&["g m"], "MixesPage"),
    (&["/"], "Search"),
    (&["backspace", "C-q"], "PreviousPage"),
    (&["esc"], "ClosePopup"),
    (&["?", "C-h"], "OpenCommandHelp"),
    (&["q", "C-c"], "Quit"),
    // The first default `[[actions]]` binding (spec 0011).
    (&["r"], "GoToRadio"),
];

/// What a built keymap must hold.
enum Expect {
    /// These sequences bound to these bindings (`None`: unbound), and these
    /// skipped names.
    Ok(Vec<(&'static str, Option<Binding>)>, Vec<&'static str>),
    /// This error message.
    Err(&'static str),
}

/// AC2: the defaults equal the table; entries add, replace and remove
/// bindings, later beats earlier; parameters are checked; `[[actions]]`
/// with each target; unknown names, bad keys and prefix conflicts are
/// errors naming the entry or both sequences; spotify-player names are
/// skipped and listed; unbinding every `Quit` key is refused.
#[test]
fn ac2_build_keymap() {
    use UiCommand as C;
    // The defaults: exactly the table, in its order, the parameterised
    // commands with their defaults.
    let defaults = Keymap::default();
    let got: Vec<(String, &str)> = defaults
        .bindings()
        .iter()
        .map(|(s, b)| (s.to_string(), b.name()))
        .collect();
    let expected: Vec<(String, &str)> = DEFAULTS
        .iter()
        .flat_map(|(keys, name)| keys.iter().map(move |k| (k.to_string(), *name)))
        .collect();
    assert_eq!(got, expected);
    assert_eq!(
        bound(&defaults, ">"),
        Some(Binding::Command(C::SeekForward { duration: None }))
    );
    assert!(defaults.unsupported().is_empty());
    assert_eq!(build(&KeymapFile::default()), Ok(Keymap::default()));

    let cmd = |c: UiCommand| Some(Binding::Command(c));
    let act = |action: ActionKind, target: Target| {
        Some(Binding::Action(ActionBinding { action, target }))
    };
    let none_on = |keys: &[&str]| keys.iter().map(|k| entry(k, "None")).collect::<Vec<_>>();
    let cases: Vec<(&str, KeymapFile, Expect)> = vec![
        (
            "an entry adds a sequence; the command keeps its own",
            file(vec![entry("g n", "NextTrack")], vec![]),
            Expect::Ok(
                vec![("g n", cmd(C::NextTrack)), ("n", cmd(C::NextTrack))],
                vec![],
            ),
        ),
        (
            "a default sequence is replaced",
            file(vec![entry("n", "PreviousTrack")], vec![]),
            Expect::Ok(
                vec![("n", cmd(C::PreviousTrack)), ("p", cmd(C::PreviousTrack))],
                vec![],
            ),
        ),
        (
            "None removes a binding",
            file(vec![entry("q", "None")], vec![]),
            Expect::Ok(vec![("q", None), ("C-c", cmd(C::Quit))], vec![]),
        ),
        (
            "a later entry beats an earlier one",
            file(
                vec![entry("x", "NextTrack"), entry("x", "PreviousTrack")],
                vec![],
            ),
            Expect::Ok(vec![("x", cmd(C::PreviousTrack))], vec![]),
        ),
        (
            "VolumeChange in range",
            file(
                vec![
                    entry_with("=", "VolumeChange", &[("offset", 1)]),
                    entry_with("M-=", "VolumeChange", &[("offset", -25)]),
                    entry_with("M-+", "VolumeChange", &[("offset", 25)]),
                ],
                vec![],
            ),
            Expect::Ok(
                vec![
                    ("=", cmd(C::VolumeChange { offset: 1 })),
                    ("M-=", cmd(C::VolumeChange { offset: -25 })),
                    ("M-+", cmd(C::VolumeChange { offset: 25 })),
                ],
                vec![],
            ),
        ),
        (
            "VolumeChange out of range",
            file(
                vec![
                    entry("x", "NextTrack"),
                    entry_with("=", "VolumeChange", &[("offset", 40)]),
                ],
                vec![],
            ),
            Expect::Err("keymaps[1]: VolumeChange offset must be from -25 to 25 and not 0, got 40"),
        ),
        (
            "VolumeChange below the range",
            file(
                vec![entry_with("=", "VolumeChange", &[("offset", -26)])],
                vec![],
            ),
            Expect::Err(
                "keymaps[0]: VolumeChange offset must be from -25 to 25 and not 0, got -26",
            ),
        ),
        (
            "VolumeChange 0",
            file(
                vec![entry_with("=", "VolumeChange", &[("offset", 0)])],
                vec![],
            ),
            Expect::Err("keymaps[0]: VolumeChange offset must be from -25 to 25 and not 0, got 0"),
        ),
        (
            "VolumeChange without its offset",
            file(vec![entry("=", "VolumeChange")], vec![]),
            Expect::Err("keymaps[0]: VolumeChange needs an offset"),
        ),
        (
            "SeekForward with and without duration",
            file(
                vec![
                    entry_with("L", "SeekForward", &[("duration", 30)]),
                    entry_with("H", "SeekBackward", &[]),
                    entry("M-l", "SeekForward"),
                    entry_with("M-L", "SeekForward", &[("duration", 600)]),
                    entry_with("M-H", "SeekBackward", &[("duration", 1)]),
                ],
                vec![],
            ),
            Expect::Ok(
                vec![
                    ("L", cmd(C::SeekForward { duration: Some(30) })),
                    ("H", cmd(C::SeekBackward { duration: None })),
                    ("M-l", cmd(C::SeekForward { duration: None })),
                    (
                        "M-L",
                        cmd(C::SeekForward {
                            duration: Some(600),
                        }),
                    ),
                    ("M-H", cmd(C::SeekBackward { duration: Some(1) })),
                ],
                vec![],
            ),
        ),
        (
            "SeekForward duration 0",
            file(
                vec![entry_with("L", "SeekForward", &[("duration", 0)])],
                vec![],
            ),
            Expect::Err("keymaps[0]: SeekForward duration must be from 1 to 600 seconds, got 0"),
        ),
        (
            "SeekBackward duration 601",
            file(
                vec![entry_with("H", "SeekBackward", &[("duration", 601)])],
                vec![],
            ),
            Expect::Err("keymaps[0]: SeekBackward duration must be from 1 to 600 seconds, got 601"),
        ),
        (
            "an unknown parameter",
            file(vec![entry_with("L", "SeekForward", &[("secs", 3)])], vec![]),
            Expect::Err(r#"keymaps[0]: unknown parameter "secs" of SeekForward"#),
        ),
        (
            "parameters on a command without any",
            file(vec![entry_with("x", "NextTrack", &[])], vec![]),
            Expect::Err("keymaps[0]: NextTrack takes no parameters"),
        ),
        (
            "actions with both targets and the default",
            file(
                vec![],
                vec![
                    action("g B", "GoToAlbum", Target::PlayingTrack),
                    action("g b", "GoToAlbum", Target::SelectedItem),
                    action("x", "AddToQueue", Target::default()),
                    action("C-x", "DeletePlaylist", Target::default()),
                ],
            ),
            Expect::Ok(
                vec![
                    ("g B", act(ActionKind::GoToAlbum, Target::PlayingTrack)),
                    ("g b", act(ActionKind::GoToAlbum, Target::SelectedItem)),
                    ("x", act(ActionKind::AddToQueue, Target::SelectedItem)),
                    ("C-x", act(ActionKind::DeletePlaylist, Target::SelectedItem)),
                ],
                vec![],
            ),
        ),
        (
            "an action replaces a command's sequence",
            file(vec![], vec![action("n", "PlayNext", Target::SelectedItem)]),
            Expect::Ok(
                vec![("n", act(ActionKind::PlayNext, Target::SelectedItem))],
                vec![],
            ),
        ),
        (
            "an unknown command, with its index",
            file(
                vec![
                    entry("x", "NextTrack"),
                    entry("y", "Quit"),
                    entry("z", "NxtTrack"),
                ],
                vec![],
            ),
            Expect::Err(r#"keymaps[2]: unknown command "NxtTrack""#),
        ),
        (
            "an unknown action, with its index",
            file(
                vec![],
                vec![
                    action("x", "GoToAlbum", Target::SelectedItem),
                    action("y", "GoToAlbm", Target::SelectedItem),
                ],
            ),
            Expect::Err(r#"actions[1]: unknown action "GoToAlbm""#),
        ),
        (
            "a command name is not an action",
            file(vec![], vec![action("y", "NextTrack", Target::SelectedItem)]),
            Expect::Err(r#"actions[0]: unknown action "NextTrack""#),
        ),
        (
            "an unknown key, with its index",
            file(
                vec![entry("x", "NextTrack"), entry("ctrl+s", "Shuffle")],
                vec![],
            ),
            Expect::Err(r#"keymaps[1]: unknown key "ctrl+s" in "ctrl+s""#),
        ),
        (
            "an empty sequence",
            file(vec![entry("", "NextTrack")], vec![]),
            Expect::Err("keymaps[0]: empty key sequence"),
        ),
        (
            "an empty action sequence",
            file(vec![], vec![action("", "GoToAlbum", Target::SelectedItem)]),
            Expect::Err("actions[0]: empty key sequence"),
        ),
        (
            "spotify-player names without a counterpart are skipped",
            file(
                vec![
                    entry("x", "PlayRandom"),
                    entry("n", "LyricsPage"),
                    entry("y", "PlayRandom"),
                ],
                vec![action("w", "GoToShow", Target::SelectedItem)],
            ),
            Expect::Ok(
                vec![
                    ("x", None),
                    ("y", None),
                    ("w", None),
                    ("n", cmd(C::NextTrack)),
                ],
                vec!["PlayRandom", "LyricsPage", "GoToShow"],
            ),
        ),
        (
            "a bound g hides the g sequences",
            file(vec![entry("g", "LibraryPage")], vec![]),
            Expect::Err(
                r#""g" is bound to LibraryPage and is the start of "g g" (SelectFirstOrScrollToTop), "g a" (ShowActionsOnSelectedItem), "g l" (LibraryPage), "g y" (LikedTrackPage), "g s" (SearchPage), "g m" (MixesPage); unbind those with command = "None" first"#,
            ),
        ),
        (
            "s and s l a",
            file(
                vec![entry("s", "NextTrack"), entry("s l a", "LibraryPage")],
                vec![],
            ),
            Expect::Err(
                r#""s" is bound to NextTrack and is the start of "s l a" (LibraryPage); unbind those with command = "None" first"#,
            ),
        ),
        (
            "an action's sequence conflicts too",
            file(
                vec![entry("s", "NextTrack")],
                vec![action("s b", "GoToAlbum", Target::PlayingTrack)],
            ),
            Expect::Err(
                r#""s" is bound to NextTrack and is the start of "s b" (GoToAlbum); unbind those with command = "None" first"#,
            ),
        ),
        (
            "removing the conflicting defaults first makes g valid",
            file(
                [
                    none_on(&["g g", "g a", "g l", "g y", "g s", "g m"]),
                    vec![entry("g", "LibraryPage")],
                ]
                .concat(),
                vec![],
            ),
            Expect::Ok(vec![("g", cmd(C::LibraryPage)), ("g g", None)], vec![]),
        ),
        (
            "unbinding every Quit key",
            file(none_on(&["q", "C-c"]), vec![]),
            Expect::Err("Quit has no key left"),
        ),
        (
            "Quit moved, then its defaults unbound",
            file(
                [vec![entry("M-q", "Quit")], none_on(&["q", "C-c"])].concat(),
                vec![],
            ),
            Expect::Ok(vec![("M-q", cmd(C::Quit)), ("q", None)], vec![]),
        ),
        (
            "help unbound, ? bound to something else",
            file(
                [vec![entry("?", "NextTrack")], none_on(&["C-h"])].concat(),
                vec![],
            ),
            Expect::Ok(vec![("?", cmd(C::NextTrack)), ("C-h", None)], vec![]),
        ),
    ];
    for (name, file, expect) in cases {
        let built = build(&file);
        match expect {
            Expect::Ok(bindings, unsupported) => {
                let keymap = built.unwrap_or_else(|e| panic!("{name}: {e}"));
                for (sequence, binding) in bindings {
                    assert_eq!(bound(&keymap, sequence), binding, "{name}: {sequence}");
                }
                assert_eq!(keymap.unsupported(), unsupported, "{name}");
            }
            Expect::Err(message) => {
                let error = built.err().map(|e| e.to_string());
                assert_eq!(error.as_deref(), Some(message), "{name}");
            }
        }
    }

    // The entries deserialise from the file's shape (slice D reads TOML
    // into them); command names are checked by `build`, not here.
    let json = r#"{
        "keymaps": [
            { "command": "NextTrack", "key_sequence": "g n" },
            { "command": { "VolumeChange": { "offset": 1 } }, "key_sequence": "=" },
            { "command": { "SeekBackward": { } }, "key_sequence": "H" },
            { "command": { "SortTrackByTitle": { "any": "thing" } }, "key_sequence": "s t" },
            { "command": "NxtTrack", "key_sequence": "x" }
        ],
        "actions": [
            { "action": "GoToAlbum", "key_sequence": "g B", "target": "PlayingTrack" },
            { "action": "AddToQueue", "key_sequence": "x" }
        ]
    }"#;
    let parsed: KeymapFile = serde_json::from_str(json).expect("deserialises");
    let params = |pairs: &[(&str, i64)]| -> Option<BTreeMap<String, i64>> {
        Some(pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect())
    };
    assert_eq!(
        parsed,
        file(
            vec![
                entry("g n", "NextTrack"),
                KeymapEntry {
                    command: CommandEntry {
                        name: "VolumeChange".into(),
                        params: params(&[("offset", 1)]),
                    },
                    key_sequence: "=".into(),
                },
                entry_with("H", "SeekBackward", &[]),
                entry_with("s t", "SortTrackByTitle", &[]),
                entry("x", "NxtTrack"),
            ],
            vec![
                action("g B", "GoToAlbum", Target::PlayingTrack),
                action("x", "AddToQueue", Target::SelectedItem),
            ],
        )
    );
    assert_eq!(
        serde_json::from_str::<KeymapFile>("{}").expect("empty"),
        KeymapFile::default()
    );
    for bad in [
        r#"{ "keymap": [] }"#,
        r#"{ "keymaps": [ { "command": "Quit", "key_sequence": "q", "mode": 1 } ] }"#,
        r#"{ "keymaps": [ { "command": "Quit" } ] }"#,
        r#"{ "keymaps": [ { "command": { "Quit": {}, "NextTrack": {} }, "key_sequence": "q" } ] }"#,
        r#"{ "keymaps": [ { "command": { "VolumeChange": { "offset": "1" } }, "key_sequence": "q" } ] }"#,
        r#"{ "actions": [ { "action": "GoToAlbum", "key_sequence": "x", "target": "Playing" } ] }"#,
        r#"{ "actions": [ { "action": "GoToAlbum", "key_sequence": "x", "mode": "y" } ] }"#,
    ] {
        assert!(serde_json::from_str::<KeymapFile>(bad).is_err(), "{bad}");
    }
}

// --- AC3 ---------------------------------------------------------------------

/// A state in which `command` acts.
fn acting(command: UiCommand) -> State {
    use UiCommand as C;
    match command {
        C::ClosePopup => {
            let mut state = queue();
            press(&mut state, &[Key::Char('a')]);
            assert!(state.popup.is_some());
            state
        }
        C::FocusNextWindow | C::FocusPreviousWindow | C::PreviousPage | C::Queue => library(0, 0),
        C::AddSelectedItemToQueue => library(1, 0),
        C::NextTab | C::PreviousTab | C::RoleFilter => {
            let mut state = queue();
            state.history.push(Page::new(PageKind::Artist(1)));
            // *All tracks*, the second tab of its pane.
            state.history[1].focus = 3;
            state
        }
        C::Search => {
            let mut state = queue();
            let mut page = Page::new(PageKind::Search("x".into()));
            page.search.as_mut().expect("a search page").focus = SearchFocus::Windows;
            state.history.push(page);
            state
        }
        _ => queue(),
    }
}

/// AC3: every default binding, pressed in a state where its command acts,
/// does exactly what running its command does, and does something
/// (`OpenCommandHelp`'s popup is AC8's).
#[test]
fn ac3_every_binding_dispatches() {
    let keymap = Keymap::default();
    assert!(!keymap.bindings().is_empty());
    for (sequence, binding) in keymap.bindings() {
        // The default `[[actions]]` binding (`r`) is AC9's (spec 0011).
        let Binding::Command(command) = *binding else {
            continue;
        };
        let state = acting(command);
        let mut by_keys = state.clone();
        let pressed = press(&mut by_keys, sequence.keys());
        let mut direct = state.clone();
        let ran = run_command(&mut direct, command);
        assert_eq!(pressed, ran, "{sequence}: effects");
        assert_eq!(by_keys, direct, "{sequence}: state");
        if command != UiCommand::OpenCommandHelp {
            assert!(
                !ran.is_empty() || direct != state,
                "{sequence} ({}) does nothing",
                command.name()
            );
        }
    }
}

// --- AC4 ---------------------------------------------------------------------

/// AC4: with a custom keymap, a moved key acts and a removed one does
/// nothing; sequences of any length; parameters reach the commands.
#[test]
fn ac4_rebinding() {
    use Key::{Alt, Char, Ctrl};
    let library = || {
        vec![Effect::Library {
            id: 1,
            request: LibraryRequest::Page(PageRequest::Library),
        }]
    };
    let sla = || vec![entry("s l a", "LibraryPage")];
    let (s, l, a) = (Char('s'), Char('l'), Char('a'));
    // (entries, keys, effects, the keys still being collected after)
    type Case = (Vec<KeymapEntry>, Vec<Key>, Vec<Effect>, Vec<Key>);
    let cases: Vec<Case> = vec![
        (
            vec![entry("g n", "NextTrack")],
            GN.to_vec(),
            send(Command::Next),
            vec![],
        ),
        (
            vec![entry("g n", "NextTrack")],
            vec![Char('n')],
            send(Command::Next),
            vec![],
        ),
        (vec![entry("q", "None")], vec![Char('q')], vec![], vec![]),
        (
            vec![entry("q", "None")],
            vec![Ctrl('c')],
            vec![Effect::Quit],
            vec![],
        ),
        (
            vec![entry("M-p", "ResumePause"), entry("space", "None")],
            vec![Alt(BaseKey::Char('p'))],
            send(Command::TogglePause),
            vec![],
        ),
        (
            vec![entry("M-p", "ResumePause"), entry("space", "None")],
            vec![Char(' ')],
            vec![],
            vec![],
        ),
        (sla(), vec![s], vec![], vec![s]),
        (sla(), vec![s, l], vec![], vec![s, l]),
        (sla(), vec![s, l, a], library(), vec![]),
        (sla(), vec![s, Char('n')], send(Command::Next), vec![]),
        (sla(), vec![s, l, Char('n')], send(Command::Next), vec![]),
        (sla(), vec![s, l, s, l, a], library(), vec![]),
        (sla(), vec![s, l, Char('g')], vec![], vec![Char('g')]),
        (
            vec![entry_with("=", "VolumeChange", &[("offset", 1)])],
            vec![Char('=')],
            send(Command::ChangeVolume(1)),
            vec![],
        ),
        (
            vec![entry_with("=", "VolumeChange", &[("offset", -3)])],
            vec![Char('=')],
            send(Command::ChangeVolume(-3)),
            vec![],
        ),
        (
            vec![entry_with("L", "SeekForward", &[("duration", 30)])],
            vec![Char('L')],
            send(Command::SeekBy(30_000)),
            vec![],
        ),
        (
            vec![entry_with("H", "SeekBackward", &[])],
            vec![Char('H')],
            send(Command::SeekBy(-5_000)),
            vec![],
        ),
        (
            vec![entry("x", "NextTrack")],
            vec![Char('g'), Char('x')],
            send(Command::Next),
            vec![],
        ),
    ];
    for (entries, keys, effects, pending) in cases {
        let mut state = with_keymap(queue(), keymap(entries.clone(), vec![]));
        assert_eq!(press(&mut state, &keys), effects, "{entries:?}: {keys:?}");
        assert_eq!(state.pending, pending, "{entries:?}: {keys:?}: pending");
        assert_eq!(
            state.pending_g,
            !pending.is_empty(),
            "{entries:?}: {keys:?}"
        );
    }
}

// --- AC5 ---------------------------------------------------------------------

/// The start of the popup row an action runs.
fn label_of(action: ActionKind) -> &'static str {
    match action {
        ActionKind::GoToAlbum => "Go to album",
        ActionKind::GoToArtist => "Go to artist",
        ActionKind::GoToRadio => "Go to radio",
        ActionKind::AddToQueue => "Add to queue",
        ActionKind::PlayNext => "Play next",
        ActionKind::AddToLiked => "Add to favorites",
        ActionKind::DeleteFromLiked => "Remove from favorites",
        ActionKind::AddToPlaylist => "Add to playlist…",
        ActionKind::DeleteFromPlaylist => "Remove from this playlist",
        ActionKind::RemoveFromQueue => "Remove from queue",
        ActionKind::DeletePlaylist => "Delete playlist",
    }
}

/// AC5: an `[[actions]]` binding emits exactly what choosing its entry in
/// the actions popup emits (on the selected row with `SelectedItem`, on the
/// playing track with `PlayingTrack`), and nothing where the popup would
/// not list it.
#[test]
fn ac5_action_bindings() {
    // (row kind, state with that row selected)
    let rows: Vec<(&str, State)> = vec![
        ("queue entry", queue()),
        ("own playlist row", library(0, 0)),
        ("followed playlist row", library(0, 1)),
        ("album row", library(1, 0)),
        ("artist row", library(2, 0)),
        ("track on a followed playlist", playlist_page(false)),
        ("track on an own playlist", playlist_page(true)),
    ];
    let mut listed = 0;
    for (target, opener) in [
        (Target::SelectedItem, vec![Key::Char('g'), Key::Char('a')]),
        (Target::PlayingTrack, vec![Key::Char('a')]),
    ] {
        for (row, state) in &rows {
            for kind in ACTIONS {
                let binding = action("M-x", kind.name(), target);
                let mut by_binding = with_keymap(state.clone(), keymap(vec![], vec![binding]));
                let got = press(&mut by_binding, &[MX]);

                // The same through the popup.
                let mut by_popup = with_keymap(state.clone(), by_binding.keymap.clone());
                press(&mut by_popup, &opener);
                let Some(Popup::Actions { actions: menu, .. }) = &by_popup.popup else {
                    panic!("{row}: no popup");
                };
                let index = menu
                    .iter()
                    .position(|a| a.label().starts_with(label_of(kind)));
                let context = format!("{row}, {target:?}, {kind:?}");
                match index {
                    Some(index) => {
                        listed += 1;
                        let mut keys = vec![Key::Down; index];
                        keys.push(Key::Enter);
                        let expected = press(&mut by_popup, &keys);
                        assert_eq!(got, expected, "{context}");
                        assert_eq!(by_binding, by_popup, "{context}: state");
                        assert!(
                            !got.is_empty()
                                || by_binding.history != state.history
                                || by_binding.popup.is_some(),
                            "{context}: did nothing"
                        );
                    }
                    None => {
                        assert_eq!(got, vec![], "{context}");
                        let unchanged = with_keymap(state.clone(), by_binding.keymap.clone());
                        assert_eq!(by_binding, unchanged, "{context}: state");
                    }
                }
            }
        }
    }
    assert!(listed > 30, "too few listed rows: {listed}");

    // A few by hand.
    let bound = |state: State, kind: ActionKind, target: Target| {
        let entries = vec![action("M-x", kind.name(), target)];
        let mut state = with_keymap(state, keymap(vec![], entries));
        let effects = press(&mut state, &[MX]);
        (effects, state)
    };
    let (effects, _) = bound(library(1, 0), ActionKind::AddToQueue, Target::SelectedItem);
    assert_eq!(
        effects,
        send(Command::Open {
            items: vec![Item::Album(10)],
            at: Some(InsertAt::End),
        })
    );
    let (effects, state) = bound(queue(), ActionKind::GoToArtist, Target::PlayingTrack);
    assert_eq!(state.page().kind, PageKind::Artist(20), "the first artist");
    assert_eq!(effects.len(), 1);
    let (effects, state) = bound(library(2, 0), ActionKind::GoToAlbum, Target::SelectedItem);
    assert_eq!((effects, state.history.len()), (vec![], 2), "no album");
    let (effects, _) = bound(queue(), ActionKind::PlayNext, Target::PlayingTrack);
    assert_eq!(effects, vec![], "no Play next on the playing track");
    let (effects, state) = bound(
        library(0, 0),
        ActionKind::DeletePlaylist,
        Target::SelectedItem,
    );
    assert_eq!(effects, vec![]);
    assert!(matches!(
        state.popup,
        Some(Popup::Confirm {
            on_yes: Confirmed::DeletePlaylist { .. },
            ..
        })
    ));
    // Nothing playing: `PlayingTrack` does nothing.
    let mut idle = State::default();
    update(
        &mut idle,
        Action::Player(protocol::Event::Player(snapshot(&[7, 3], None))),
    );
    for kind in ACTIONS {
        let (effects, state) = bound(idle.clone(), kind, Target::PlayingTrack);
        assert_eq!(effects, vec![], "{kind:?}");
        assert!(state.popup.is_none(), "{kind:?}");
    }
}

// --- AC6 ---------------------------------------------------------------------

/// AC6: text inputs take printable keys and their editing keys before the
/// keymap; the role filter's `Space` and a question's `y`/`n` act whatever
/// the keymap binds them to; outside them, the keymap is in force.
#[test]
fn ac6_text_inputs_first() {
    let to_quit = ["backspace", "enter", "esc", "space", "y", "n", "C-u", "tab"];
    let custom = || {
        keymap(
            [
                vec![entry("q", "NextTrack"), entry("j", "Quit")],
                to_quit.iter().map(|k| entry(k, "Quit")).collect(),
            ]
            .concat(),
            vec![],
        )
    };
    let prompt = |state: &State| state.prompt.as_ref().map(|p| p.text.clone());

    // The keymap is in force outside the inputs.
    let mut state = with_keymap(queue(), custom());
    assert_eq!(press(&mut state, &[Key::Char('q')]), send(Command::Next));
    assert_eq!(press(&mut state, &[Key::Char('j')]), vec![Effect::Quit]);
    assert_eq!(press(&mut state, &[Key::Enter]), vec![Effect::Quit]);

    // The `o` prompt.
    let mut state = with_keymap(queue(), custom());
    let mut effects = press(&mut state, &[Key::Char('o')]);
    effects.extend(press(&mut state, &typed("qj? 1")));
    assert_eq!((effects, prompt(&state)), (vec![], Some("qj? 1".into())));
    assert_eq!(press(&mut state, &[Key::Backspace, Key::Backspace]), vec![]);
    assert_eq!(prompt(&state), Some("qj?".into()));
    assert_eq!(press(&mut state, &[Key::Esc]), vec![]);
    assert_eq!(prompt(&state), None);
    let effects = press(
        &mut state,
        &[vec![Key::Char('O')], typed("123"), vec![Key::Enter]].concat(),
    );
    assert_eq!(
        effects,
        send(Command::Open {
            items: vec![Item::Track(TrackId(123))],
            at: Some(InsertAt::Next),
        })
    );

    // The search input.
    let input = |state: &State| {
        state
            .page()
            .search
            .as_ref()
            .map(|s| (s.input.clone(), s.focus))
    };
    let mut state = with_keymap(queue(), custom());
    let mut effects = press(&mut state, &[Key::Char('g'), Key::Char('s')]);
    effects.extend(press(&mut state, &typed("qj? n")));
    assert_eq!(effects, vec![]);
    assert_eq!(input(&state), Some(("qj? n".into(), SearchFocus::Input)));
    assert_eq!(press(&mut state, &[Key::Backspace]), vec![]);
    assert_eq!(input(&state), Some(("qj? ".into(), SearchFocus::Input)));
    assert_eq!(press(&mut state, &[Key::Ctrl('u')]), vec![]);
    assert_eq!(input(&state), Some((String::new(), SearchFocus::Input)));
    assert_eq!(press(&mut state, &[Key::Tab]), vec![]);
    assert_eq!(input(&state), Some((String::new(), SearchFocus::Windows)));
    let effects = press(
        &mut state,
        &[vec![Key::Char('/')], typed("qj"), vec![Key::Enter]].concat(),
    );
    assert_eq!(
        effects,
        vec![Effect::Library {
            id: 1,
            request: LibraryRequest::Page(PageRequest::Search("qj".into())),
        }]
    );

    // The playlist-name prompt.
    let name = |state: &State| match &state.popup {
        Some(Popup::NewPlaylist { name, .. }) => Some(name.clone()),
        _ => None,
    };
    let new_playlist = Popup::NewPlaylist {
        tracks: TrackSource::Tracks(vec![track(1)]),
        name: String::new(),
    };
    let mut state = with_keymap(queue(), custom());
    state.popup = Some(new_playlist.clone());
    assert_eq!(press(&mut state, &typed("qj? n")), vec![]);
    assert_eq!(name(&state), Some("qj? n".into()));
    assert_eq!(press(&mut state, &[Key::Backspace, Key::Backspace]), vec![]);
    assert_eq!(name(&state), Some("qj?".into()));
    assert_eq!(
        press(&mut state, &[Key::Enter]),
        vec![Effect::Library {
            id: 1,
            request: LibraryRequest::CreatePlaylist {
                title: "qj?".into()
            },
        }]
    );
    assert_eq!(state.popup, None);
    state.popup = Some(new_playlist);
    assert_eq!(press(&mut state, &[Key::Esc]), vec![]);
    assert_eq!(state.popup, None);

    // The role filter's `Space`.
    let mut state = with_keymap(queue(), custom());
    state.popup = Some(Popup::Roles {
        checked: [true; 4],
        cursor: 0,
    });
    assert_eq!(press(&mut state, &[Key::Char(' ')]), vec![]);
    assert_eq!(
        state.popup,
        Some(Popup::Roles {
            checked: [false, true, true, true],
            cursor: 0,
        })
    );

    // A question's `y` and `n`.
    let question = Popup::Confirm {
        question: "Delete P? (y/n)".into(),
        on_yes: Confirmed::DeletePlaylist {
            uuid: "p1".into(),
            title: "P".into(),
        },
    };
    let mut state = with_keymap(queue(), custom());
    state.popup = Some(question.clone());
    assert_eq!(press(&mut state, &[Key::Char('n')]), vec![]);
    assert_eq!(state.popup, None);
    state.popup = Some(question);
    assert_eq!(
        press(&mut state, &[Key::Char('y')]),
        vec![Effect::Library {
            id: 1,
            request: LibraryRequest::DeletePlaylist { uuid: "p1".into() },
        }]
    );
}

// --- AC12 (model) ------------------------------------------------------------

/// AC12: spotify-player names without a counterpart are listed once, in
/// file order, and applying the keymap puts the one-line notice in the
/// message row; the supported entries are in force.
#[test]
fn ac12_unsupported_notice() {
    // (entries, actions, the message after applying)
    type Case = (Vec<KeymapEntry>, Vec<ActionEntry>, Option<&'static str>);
    let cases: Vec<Case> = vec![
        (
            vec![
                entry("x", "PlayRandom"),
                entry("g n", "NextTrack"),
                entry("y", "LyricsPage"),
                entry("w", "PlayRandom"),
                entry("v", "SwitchTheme"),
            ],
            vec![],
            Some(
                "keymap.toml: 3 spotify-player commands not supported here: PlayRandom, LyricsPage, SwitchTheme",
            ),
        ),
        (
            vec![entry("g n", "NextTrack")],
            vec![action("x", "CopyLink", Target::SelectedItem)],
            Some("keymap.toml: 1 spotify-player command not supported here: CopyLink"),
        ),
        (vec![entry("g n", "NextTrack")], vec![], None),
    ];
    for (keymaps, actions, message) in cases {
        let built = keymap(keymaps.clone(), actions);
        let mut state = queue();
        apply_keymap(&mut state, built);
        assert_eq!(state.message(), message, "{keymaps:?}");
        assert_eq!(press(&mut state, &GN), send(Command::Next), "{keymaps:?}");
    }

    // Every listed spotify-player name is skipped, never an error.
    let keymaps = UNSUPPORTED_COMMANDS
        .iter()
        .zip('a'..)
        .map(|(name, c)| entry(&format!("M-{c}"), name))
        .collect();
    let actions = UNSUPPORTED_ACTIONS
        .iter()
        .map(|name| action("x", name, Target::SelectedItem))
        .collect();
    let built = keymap(keymaps, actions);
    let names: Vec<&str> = UNSUPPORTED_COMMANDS
        .iter()
        .chain(UNSUPPORTED_ACTIONS.iter())
        .copied()
        .collect();
    assert_eq!(built.unsupported(), names);
    assert_eq!(built.bindings(), Keymap::default().bindings());
}

/// AC12: the notice is applied before the player's first `Welcome` (the
/// TUI builds its state before it connects); that `Welcome` shows it in
/// the message row instead of clearing it. From then on it is a client
/// message like any other: the next `Welcome` clears it. Without a notice,
/// `Welcome` clears the message as before.
#[test]
fn ac12_notice_after_welcome() {
    let notice = "keymap.toml: 1 spotify-player command not supported here: PlayRandom";
    let welcome = || Action::Welcome {
        snapshot: snapshot(&[7], Some(7)),
        login_required: false,
    };
    let mut state = State::default();
    apply_keymap(&mut state, keymap(vec![entry("x", "PlayRandom")], vec![]));
    assert_eq!(state.message(), Some(notice));
    update(&mut state, welcome());
    assert_eq!(state.message(), Some(notice), "lost on the first Welcome");
    update(&mut state, welcome());
    assert_eq!(state.message(), None, "kept past the second Welcome");

    let mut state = State::default();
    apply_keymap(&mut state, keymap(vec![entry("g n", "NextTrack")], vec![]));
    state.message = Some("an earlier message".into());
    update(&mut state, welcome());
    assert_eq!(state.message(), None);
}

// --- spec 0011 AC9 ------------------------------------------------------------

/// Spec 0011 AC9: the defaults hold `g m` and `r`, both in the keys help;
/// `GoToRadio` parses with both targets, `r` can be unbound, `r x` conflicts
/// with the default `r` until it is, `GoToRadio` is no longer skipped, and
/// the defaults have no prefix conflict.
#[test]
fn ac9_mixes_and_radio_keys() {
    use UiCommand as C;
    let radio = |target| {
        Some(Binding::Action(ActionBinding {
            action: ActionKind::GoToRadio,
            target,
        }))
    };
    let defaults = Keymap::default();
    assert_eq!(
        bound(&defaults, "g m"),
        Some(Binding::Command(C::MixesPage))
    );
    assert_eq!(bound(&defaults, "r"), radio(Target::SelectedItem));
    assert_eq!(build(&KeymapFile::default()), Ok(Keymap::default()));

    // Both are in the keys help, with their texts.
    let sections = help(&queue());
    let text_of = |name: &str| {
        sections
            .iter()
            .flat_map(|s| &s.rows)
            .find(|r| r.command == name)
            .map(|r| (r.keys.clone(), r.text.clone()))
    };
    assert_eq!(
        text_of("MixesPage"),
        Some(("g m".into(), "your mixes".into()))
    );
    assert_eq!(
        text_of("GoToRadio"),
        Some((
            "r".into(),
            "the radio of the selected track or artist".into()
        ))
    );

    // A user `[[actions]]` entry parses with both targets, and is not
    // skipped as a spotify-player action.
    assert!(!UNSUPPORTED_ACTIONS.contains(&"GoToRadio"));
    assert!(ACTIONS.contains(&ActionKind::GoToRadio));
    let json = r#"{ "actions": [
        { "action": "GoToRadio", "key_sequence": "x" },
        { "action": "GoToRadio", "key_sequence": "y", "target": "PlayingTrack" }
    ] }"#;
    let parsed: KeymapFile = serde_json::from_str(json).expect("deserialises");
    let built = build(&parsed).expect("valid");
    assert_eq!(bound(&built, "x"), radio(Target::SelectedItem));
    assert_eq!(bound(&built, "y"), radio(Target::PlayingTrack));
    assert_eq!(built.unsupported(), Vec::<String>::new());
    assert_eq!(bound(&built, "r"), radio(Target::SelectedItem), "kept");

    // `command = "None"` removes the default.
    let unbound = keymap(vec![entry("r", "None")], vec![]);
    assert_eq!(bound(&unbound, "r"), None);
    let mut state = with_keymap(
        {
            let mut state = State::default();
            let data = PageData::FavoriteTracks {
                tracks: list(vec![track(1)], 1),
            };
            open(&mut state, &[Key::Char('g'), Key::Char('y')], data);
            state
        },
        unbound,
    );
    assert_eq!(press(&mut state, &[Key::Char('r')]), vec![], "unbound");
    assert_eq!(state.history.len(), 2);

    // `r x` conflicts with the default `r` until it is unbound.
    assert_eq!(
        build(&file(vec![entry("r x", "NextTrack")], vec![]))
            .err()
            .map(|e| e.to_string())
            .as_deref(),
        Some(
            r#""r" is bound to GoToRadio and is the start of "r x" (NextTrack); unbind those with command = "None" first"#
        )
    );
    let moved = keymap(vec![entry("r", "None"), entry("r x", "NextTrack")], vec![]);
    assert_eq!(bound(&moved, "r x"), Some(Binding::Command(C::NextTrack)));
}
