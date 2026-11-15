//! Spec 0030 AC3–AC6: *Copy link*, `CopyLink`, `OpenSpotifyLinkFromClipboard`
//! and what the clipboard's text does.

use std::time::Duration;

use crate::item::Item;
use crate::library::{
    AlbumKind, AlbumSummary, LibraryRequest, LibraryResponse, ListPage, PageData, PlaylistSummary,
};
use crate::protocol::RepeatMode;
use crate::protocol::{self, Command, InsertAt, PlaybackState, PlayerSnapshot, QueueEntry};
use crate::track::{AlbumRef, ArtistRef, EntryId, Track, TrackId};

use super::super::dispatch::run_command;
use super::super::keymap::{ActionEntry, KeymapFile, Target, UiCommand, build};
use super::super::{
    Action, CopiedVia, Effect, Key, PageKind, State, WindowKind, apply_keymap, update,
};

const UUID: &str = "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d";
const MIX: &str = "0016bd04834072e9da423a4f4d911d";
const GP: [Key; 2] = [Key::Char('g'), Key::Char('p')];

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

fn list<T>(items: Vec<T>) -> ListPage<T> {
    let total = items.len() as u32;
    ListPage {
        items,
        offset: 0,
        total,
        hidden: 0,
        positions: Vec::new(),
    }
}

fn library_data() -> PageData {
    PageData::Library {
        playlists: list(vec![PlaylistSummary {
            uuid: UUID.into(),
            title: "Mine".into(),
            tracks: Some(2),
            duration: None,
            own: true,
            description: None,
            public: false,
        }]),
        albums: list(vec![AlbumSummary {
            id: 10,
            title: "Al10".into(),
            artists: vec![artist(20)],
            year: None,
            kind: AlbumKind::Album,
            tracks: Some(3),
            duration: None,
        }]),
        artists: list(vec![artist(20)]),
        collected_tracks: Some(3),
    }
}

/// The queue page with entries 1 and 2 (tracks 101, 102), 2 playing.
fn queue() -> State {
    let mut state = State::default();
    let snapshot = PlayerSnapshot {
        queue: [1, 2]
            .iter()
            .map(|id| QueueEntry {
                id: EntryId(*id),
                track: track(id + 100),
                suggested: false,
            })
            .collect(),
        current: Some(EntryId(2)),
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
    };
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

/// The library page, loaded, with the focus on `window`.
fn library(window: WindowKind) -> State {
    let mut state = queue();
    let effects = press(&mut state, &[Key::Char('g'), Key::Char('l')]);
    let [Effect::Library { id, .. }] = effects[..] else {
        panic!("one request expected: {effects:?}");
    };
    update(
        &mut state,
        Action::LibraryReply {
            id,
            result: Ok(LibraryResponse::Page(library_data())),
        },
    );
    for _ in 0..3 {
        if state.page().focused().map(|w| w.kind) == Some(window) {
            return state;
        }
        press(&mut state, &[Key::Tab]);
    }
    panic!("no {window:?} window");
}

fn link(path: &str) -> String {
    format!("https://tidal.com/browse/{path}")
}

// --- AC3 ---------------------------------------------------------------------

/// AC3: choosing *Copy link* (the last entry) closes the popup and asks
/// the TUI to copy the row's link.
#[test]
fn ac3_copy_link_from_popup() {
    // (state, the keys opening the popup, the link)
    let cases: Vec<(State, Vec<Key>, String)> = vec![
        (queue(), vec![Key::Char('a')], link("track/102")),
        (
            queue(),
            vec![Key::Char('g'), Key::Char('a')],
            link("track/101"),
        ),
        (
            library(WindowKind::Albums),
            vec![Key::Char('g'), Key::Char('a')],
            link("album/10"),
        ),
        (
            library(WindowKind::Artists),
            vec![Key::Char('g'), Key::Char('a')],
            link("artist/20"),
        ),
    ];
    for (mut state, keys, want) in cases {
        press(&mut state, &keys);
        assert!(state.popup.is_some(), "{want}: no popup");
        let effects = press(&mut state, &[Key::Char('G'), Key::Enter]);
        assert_eq!(effects, vec![Effect::Copy(want.clone())], "{want}");
        assert_eq!(state.popup, None, "{want}");
    }
}

/// AC3: the copy's result is the message row.
#[test]
fn ac3_copied_message() {
    let album = link("album/10");
    let cases = [
        (Ok(CopiedVia::Tool), format!("Link copied: {album}")),
        (
            Ok(CopiedVia::Osc52),
            format!("Link copied (OSC 52): {album}"),
        ),
        (
            Err("Copying the link needs wl-copy: install wl-clipboard".to_owned()),
            "Copying the link needs wl-copy: install wl-clipboard".to_owned(),
        ),
    ];
    for (result, want) in cases {
        let mut state = queue();
        let effects = update(
            &mut state,
            Action::Copied {
                link: album.clone(),
                result: result.clone(),
            },
        );
        assert_eq!(effects, vec![], "{result:?}");
        assert_eq!(state.message(), Some(want.as_str()), "{result:?}");
    }
}

// --- AC4 ---------------------------------------------------------------------

/// AC4: `CopyLink` bindings run what the popup would, on either target;
/// nothing where the popup has no *Copy link*.
#[test]
fn ac4_copy_link_binding() {
    let keymap = build(&KeymapFile {
        keymaps: vec![],
        actions: vec![
            ActionEntry {
                action: "CopyLink".into(),
                key_sequence: "x".into(),
                target: Target::SelectedItem,
            },
            ActionEntry {
                action: "CopyLink".into(),
                key_sequence: "y".into(),
                target: Target::PlayingTrack,
            },
        ],
    })
    .expect("a valid keymap");
    let with = |mut state: State| {
        apply_keymap(&mut state, keymap.clone());
        state
    };
    // (state, key, effects)
    let cases: Vec<(&str, State, Key, Vec<Effect>)> = vec![
        (
            "queue entry",
            with(queue()),
            Key::Char('x'),
            vec![Effect::Copy(link("track/101"))],
        ),
        (
            "playing track",
            with(queue()),
            Key::Char('y'),
            vec![Effect::Copy(link("track/102"))],
        ),
        (
            "playlist",
            {
                let mut state = with(library(WindowKind::Playlists));
                press(&mut state, &[Key::Char('j')]);
                state
            },
            Key::Char('x'),
            vec![Effect::Copy(link(&format!("playlist/{UUID}")))],
        ),
        (
            "Collected Tracks",
            with(library(WindowKind::Playlists)),
            Key::Char('x'),
            vec![],
        ),
    ];
    for (row, mut state, key, want) in cases {
        assert_eq!(press(&mut state, &[key]), want, "{row}");
        assert_eq!(state.popup, None, "{row}");
    }
}

// --- AC5 ---------------------------------------------------------------------

/// AC5: `g p` asks for the clipboard on a page, and does nothing over a
/// popup, a prompt or the keys help.
#[test]
fn ac5_reads_the_clipboard_on_a_page() {
    let over = |keys: &[Key]| {
        let mut state = queue();
        press(&mut state, keys);
        state
    };
    let cases: Vec<(&str, State, Vec<Effect>)> = vec![
        ("queue page", queue(), vec![Effect::ReadClipboard]),
        (
            "library page",
            library(WindowKind::Albums),
            vec![Effect::ReadClipboard],
        ),
        ("actions popup", over(&[Key::Char('a')]), vec![]),
        ("prompt", over(&[Key::Char('o')]), vec![]),
        ("keys help", over(&[Key::Char('?')]), vec![]),
    ];
    for (at, state, want) in cases {
        let mut by_keys = state.clone();
        assert_eq!(press(&mut by_keys, &GP), want, "{at}: keys");
        let mut direct = state.clone();
        let ran = run_command(&mut direct, UiCommand::OpenSpotifyLinkFromClipboard);
        assert_eq!(ran, want, "{at}: command");
    }
}

// --- AC6 ---------------------------------------------------------------------

/// What reading `text` did: the page shown, the requests and commands.
#[derive(Debug, PartialEq, Eq)]
enum Did {
    /// This page opened and was asked for.
    Opened(PageKind),
    Sent(Command),
    Message(String),
}

fn did(text: Result<&str, &str>) -> Did {
    let mut state = queue();
    let pages = state.history.len();
    let effects = update(
        &mut state,
        Action::ClipboardRead(text.map(str::to_owned).map_err(str::to_owned)),
    );
    match &effects[..] {
        [Effect::Send(command)] => {
            assert_eq!(state.history.len(), pages, "{text:?}: a page opened");
            Did::Sent(command.clone())
        }
        [Effect::Library { request, .. }] => {
            assert!(
                matches!(request, LibraryRequest::Page(_)),
                "{text:?}: {request:?}"
            );
            assert_eq!(state.history.len(), pages + 1, "{text:?}: no page pushed");
            Did::Opened(state.page().kind.clone())
        }
        [] => Did::Message(state.message().unwrap_or_default().to_owned()),
        _ => panic!("{text:?}: {effects:?}"),
    }
}

/// AC6: the clipboard's text, per the spec's table.
#[test]
fn ac6_clipboard_text() {
    let queued = |id| {
        Did::Sent(Command::Open {
            items: vec![Item::Track(TrackId(id))],
            at: Some(InsertAt::End),
        })
    };
    let long = "x".repeat(100);
    let cut = format!("Not a Tidal link: {}…", "x".repeat(60));
    let artist_link = link("artist/7804");
    let mix_link = link(&format!("mix/{MIX}"));
    let playlist_link = format!("https://listen.tidal.com/playlist/{UUID}/u\n");
    let cases: Vec<(Result<&str, &str>, Did)> = vec![
        (
            Ok("https://tidal.com/album/10"),
            Did::Opened(PageKind::Album(10)),
        ),
        (
            Ok(&playlist_link),
            Did::Opened(PageKind::Playlist(UUID.into())),
        ),
        (Ok(&artist_link), Did::Opened(PageKind::Artist(7804))),
        (Ok(&mix_link), Did::Opened(PageKind::Mix(MIX.into()))),
        (
            Ok("https://tidal.com/browse/track/77640617"),
            queued(77640617),
        ),
        (Ok("  77640617\n"), queued(77640617)),
        (Ok("hello"), Did::Message("Not a Tidal link: hello".into())),
        (
            Ok("line one\nline two"),
            Did::Message("Not a Tidal link: line one line two".into()),
        ),
        (Ok(&long), Did::Message(cut)),
        (Ok(""), Did::Message("The clipboard is empty".into())),
        (Ok(" \n\t"), Did::Message("The clipboard is empty".into())),
        (
            Err("The clipboard holds no text"),
            Did::Message("The clipboard holds no text".into()),
        ),
    ];
    for (text, want) in cases {
        assert_eq!(did(text), want, "{text:?}");
    }
}

/// AC6: an answer that comes while a popup, a prompt or the keys help is
/// open is dropped.
#[test]
fn ac6_late_answer_dropped() {
    for keys in [
        vec![Key::Char('a')],
        vec![Key::Char('o')],
        vec![Key::Char('?')],
    ] {
        let mut state = queue();
        press(&mut state, &keys);
        let before = state.clone();
        let effects = update(
            &mut state,
            Action::ClipboardRead(Ok("https://tidal.com/album/10".into())),
        );
        assert_eq!(effects, vec![], "{keys:?}");
        assert_eq!(state, before, "{keys:?}");
    }
}
