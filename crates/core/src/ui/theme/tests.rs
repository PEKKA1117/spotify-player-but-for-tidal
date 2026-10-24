//! Spec 0017 AC1–AC3, AC5 and AC6: colour and modifier syntax, building
//! the theme list, the default theme, the Themes popup and its key, and
//! the notices.

use std::time::Duration;

use crate::protocol::{self, PlaybackState, PlayerSnapshot, QueueEntry, RepeatMode};
use crate::track::{AlbumRef, ArtistRef, EntryId, Track, TrackId};

use super::super::keymap::{
    Binding, CommandEntry, KeymapEntry, KeymapFile, UNSUPPORTED_COMMANDS, UiCommand,
};
use super::super::{
    Action, Connection, Effect, Key, Page, PageKind, Popup, SearchFocus, State, apply_keymap,
    apply_themes, help, keymap, update,
};
use super::*;

// --- fixtures ----------------------------------------------------------------

fn style(fg: Option<&str>, bg: Option<&str>, modifiers: &[&str]) -> Option<StyleEntry> {
    Some(StyleEntry {
        fg: fg.map(str::to_owned),
        bg: bg.map(str::to_owned),
        modifiers: modifiers.iter().map(|m| (*m).to_owned()).collect(),
        unknown: Vec::new(),
    })
}

fn fg(color: &str) -> Option<StyleEntry> {
    style(Some(color), None, &[])
}

/// A `[[themes]]` entry called `name` with `palette` and `styles` (a
/// `None` style: a name that is not one of this player's components).
fn entry(
    name: &str,
    palette: &[(&str, &str)],
    styles: Vec<(&str, Option<StyleEntry>)>,
) -> ThemeEntry {
    ThemeEntry {
        name: Some(name.to_owned()),
        palette: palette
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect(),
        component_style: styles.into_iter().map(|(k, v)| (k.to_owned(), v)).collect(),
        unknown: Vec::new(),
    }
}

fn file(themes: Vec<ThemeEntry>) -> ThemeFile {
    ThemeFile { themes }
}

fn names(themes: &Themes) -> Vec<&str> {
    themes.list().iter().map(|t| t.name.as_str()).collect()
}

const BOLD: Modifiers = Modifiers::BOLD;
const DIM: Modifiers = Modifiers::DIM;
const REVERSED: Modifiers = Modifiers::REVERSED;

fn rgb(hex: u32) -> Color {
    Color::Rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

// --- AC1 ---------------------------------------------------------------------

/// AC1: palette values as ratatui's `Color::from_str` reads them (the
/// binary's `ac1_palette_matches_ratatui` checks every row against it).
#[test]
fn ac1_palette_colours() {
    use Color::*;
    let rows: &[(&str, Option<Color>)] = &[
        ("reset", Some(Reset)),
        ("Reset", Some(Reset)),
        ("black", Some(Black)),
        ("red", Some(Red)),
        ("green", Some(Green)),
        ("yellow", Some(Yellow)),
        ("blue", Some(Blue)),
        ("magenta", Some(Magenta)),
        ("cyan", Some(Cyan)),
        ("gray", Some(Gray)),
        ("darkgray", Some(DarkGray)),
        ("lightred", Some(LightRed)),
        ("lightgreen", Some(LightGreen)),
        ("lightyellow", Some(LightYellow)),
        ("lightblue", Some(LightBlue)),
        ("lightmagenta", Some(LightMagenta)),
        ("lightcyan", Some(LightCyan)),
        ("white", Some(White)),
        // Case, spaces, `-` and `_`.
        ("Red", Some(Red)),
        ("RED", Some(Red)),
        ("Bright Black", Some(DarkGray)),
        ("bright_black", Some(DarkGray)),
        ("BrightBlack", Some(DarkGray)),
        ("light-red", Some(LightRed)),
        ("Light Red", Some(LightRed)),
        ("bright_red", Some(LightRed)),
        ("bright-magenta", Some(LightMagenta)),
        ("dark_gray", Some(DarkGray)),
        ("Dark Gray", Some(DarkGray)),
        // `grey` and `silver` are `gray`; ratatui's `white` is the bright
        // one, so `light gray` and `bright white` are `White`.
        ("grey", Some(Gray)),
        ("dark-grey", Some(DarkGray)),
        ("silver", Some(Gray)),
        ("light gray", Some(White)),
        ("light grey", Some(White)),
        ("bright white", Some(White)),
        ("bright_white", Some(White)),
        ("light white", Some(White)),
        ("light black", Some(DarkGray)),
        // Indexed colours.
        ("0", Some(Indexed(0))),
        ("7", Some(Indexed(7))),
        ("255", Some(Indexed(255))),
        // Hex, either case.
        ("#ff5555", Some(rgb(0xff5555))),
        ("#FF5555", Some(rgb(0xff5555))),
        ("#1e1F29", Some(rgb(0x1e1f29))),
        ("#000000", Some(rgb(0x000000))),
        // Refused.
        ("", None),
        ("#fff", None),
        ("#ggg000", None),
        ("#ff55", None),
        ("256", None),
        ("-1", None),
        ("orange", None),
        ("Purpel", None),
    ];
    for (text, want) in rows {
        assert_eq!(parse_palette_color(text), *want, "{text:?}");
    }
}

/// The palette name `color` in the spellings a style may use.
fn spellings(color: PaletteColor) -> Vec<String> {
    let key = color.key();
    let words: Vec<String> = key
        .split('_')
        .map(|w| {
            let mut chars = w.chars();
            chars
                .next()
                .map(|c| c.to_uppercase().chain(chars).collect())
                .unwrap_or_default()
        })
        .collect();
    vec![
        words.concat(),
        key.to_owned(),
        key.to_uppercase(),
        key.replace('_', "-"),
        words.join(" "),
        words.concat().to_lowercase(),
    ]
}

/// AC1: a style's colours: the sixteen palette names in every accepted
/// spelling, and `#rrggbb`; nothing else.
#[test]
fn ac1_style_colours() {
    for color in PALETTE_COLORS {
        for text in spellings(color) {
            assert_eq!(
                parse_style_color(&text),
                Some(StyleColor::Palette(color)),
                "{text:?}"
            );
        }
    }
    assert_eq!(
        parse_style_color("#50fa7b"),
        Some(StyleColor::Rgb(0x50, 0xfa, 0x7b))
    );
    assert_eq!(
        parse_style_color("#50FA7B"),
        Some(StyleColor::Rgb(0x50, 0xfa, 0x7b))
    );
    for refused in [
        "Purpel", "Reset", "7", "", "#fff", "#ggg000", "Gray", "DarkGray", "LightRed", "orange",
    ] {
        assert_eq!(parse_style_color(refused), None, "{refused:?}");
    }
}

/// AC1: the eight modifiers, ignoring case; nothing else.
#[test]
fn ac1_modifiers() {
    let rows: &[(&str, Modifiers)] = &[
        ("Bold", Modifiers::BOLD),
        ("bold", Modifiers::BOLD),
        ("BOLD", Modifiers::BOLD),
        ("Dim", Modifiers::DIM),
        ("Italic", Modifiers::ITALIC),
        ("Underlined", Modifiers::UNDERLINED),
        ("RapidBlink", Modifiers::RAPID_BLINK),
        ("rapidblink", Modifiers::RAPID_BLINK),
        ("Reversed", Modifiers::REVERSED),
        ("Hidden", Modifiers::HIDDEN),
        ("CrossedOut", Modifiers::CROSSED_OUT),
        ("crossedOUT", Modifiers::CROSSED_OUT),
    ];
    for (text, want) in rows {
        assert_eq!(parse_modifier(text), Some(*want), "{text:?}");
    }
    for refused in ["Blink", "", "Underline", "Strikethrough"] {
        assert_eq!(parse_modifier(refused), None, "{refused:?}");
    }
}

// --- AC2 ---------------------------------------------------------------------

/// AC2: the list's order, `default` replaced in place, fallbacks, the
/// palette, whole-style replacement and the screen style.
#[test]
fn ac2_build_themes() {
    // No entries: `default` alone.
    let themes = build(&ThemeFile::default()).unwrap();
    assert_eq!(names(&themes), ["default"]);
    assert_eq!(themes.list()[0], Theme::default());
    assert!(themes.skipped().is_empty());

    // Entries follow `default`, in file order.
    let themes = build(&file(vec![
        entry("dracula", &[], vec![]),
        entry("gruvbox_dark", &[], vec![]),
        entry("ayu", &[], vec![]),
    ]))
    .unwrap();
    assert_eq!(
        names(&themes),
        ["default", "dracula", "gruvbox_dark", "ayu"]
    );
    assert_eq!(themes.find("gruvbox_dark"), Some(2));
    assert_eq!(themes.find("nord"), None);

    // An entry named `default` replaces the built-in one, in first place.
    let themes = build(&file(vec![
        entry("x", &[], vec![]),
        entry("default", &[("background", "#101010")], vec![]),
    ]))
    .unwrap();
    assert_eq!(names(&themes), ["default", "x"]);
    assert_eq!(themes.list()[0].screen, Style::new().bg(rgb(0x101010)));

    // `background` and `foreground` give the screen style; none leaves
    // the terminal's.
    let themes = build(&file(vec![
        entry(
            "both",
            &[("background", "#1e1f29"), ("foreground", "white")],
            vec![],
        ),
        entry("fg", &[("foreground", "7")], vec![]),
        entry("none", &[("red", "#ff5555")], vec![]),
    ]))
    .unwrap();
    let screen = |i: usize| themes.list()[i].screen;
    assert_eq!(screen(1), Style::new().bg(rgb(0x1e1f29)).fg(Color::White));
    assert_eq!(screen(2), Style::new().fg(Color::Indexed(7)));
    assert_eq!(screen(3), Style::new());

    // A component colour resolves through the theme's palette; hex as it
    // is; a palette entry left out keeps its ANSI colour (`white` is ANSI
    // 7). `error`'s default `Red` goes through the palette too.
    let themes = build(&file(vec![entry(
        "p",
        &[("magenta", "#bd93f9"), ("red", "#ff5555")],
        vec![
            ("block_title", style(Some("Magenta"), None, &["Bold"])),
            ("border", fg("#44475a")),
            ("like", style(Some("White"), Some("bright_black"), &[])),
            ("secondary_row", fg("Green")),
        ],
    )]))
    .unwrap();
    let theme = &themes.list()[1];
    assert_eq!(
        theme.style(Component::BlockTitle),
        Style::new().fg(rgb(0xbd93f9)).add(BOLD)
    );
    assert_eq!(
        theme.style(Component::Border),
        Style::new().fg(rgb(0x44475a))
    );
    assert_eq!(
        theme.style(Component::Like),
        Style::new().fg(Color::Gray).bg(Color::DarkGray)
    );
    assert_eq!(
        theme.style(Component::SecondaryRow),
        Style::new().fg(Color::Green)
    );
    assert_eq!(
        theme.style(Component::Error),
        Style::new().fg(rgb(0xff5555))
    );
    // Left out: the "Default" column (today's look, decision 1).
    assert_eq!(
        theme.style(Component::Selection),
        Style::new().add(REVERSED)
    );
    assert_eq!(
        theme.style(Component::PlaybackTrack),
        Style::new().add(BOLD)
    );
    assert_eq!(theme.style(Component::PlaylistDesc), Style::new().add(DIM));
    assert_eq!(theme.style(Component::PlaybackAlbum), Style::new());

    // A style replaces the default whole: cyan text without `Reversed`;
    // an empty one clears it.
    let themes = build(&file(vec![entry(
        "s",
        &[],
        vec![
            ("selection", fg("Cyan")),
            ("current_playing", style(None, None, &[])),
            ("error", style(None, Some("Red"), &["Italic", "underlined"])),
        ],
    )]))
    .unwrap();
    let theme = &themes.list()[1];
    assert_eq!(
        theme.style(Component::Selection),
        Style::new().fg(Color::Cyan)
    );
    assert_eq!(theme.style(Component::CurrentPlaying), Style::new());
    assert_eq!(
        theme.style(Component::Error),
        Style::new()
            .bg(Color::Red)
            .add(Modifiers::ITALIC | Modifiers::UNDERLINED)
    );

    // spotify-player's four components this player does not draw:
    // skipped, each once, in file order.
    let themes = build(&file(vec![
        entry(
            "a",
            &[],
            vec![
                ("lyrics_played", None),
                ("visualization", None),
                ("border", fg("Red")),
            ],
        ),
        entry(
            "b",
            &[],
            vec![("lyrics_played", None), ("playback_genres", None)],
        ),
    ]))
    .unwrap();
    assert_eq!(
        themes.skipped(),
        ["lyrics_played", "visualization", "playback_genres"]
    );
    assert_eq!(
        themes.list()[1].style(Component::Border),
        Style::new().fg(Color::Red)
    );
}

/// AC2: every refusal under "Errors" for `theme.toml`, with the entry's
/// index and the path inside it.
#[test]
fn ac2_build_errors() {
    let ok = || entry("ok", &[], vec![]);
    let unknown_field = ThemeEntry {
        unknown: vec!["colour".into()],
        ..entry("x", &[], vec![])
    };
    let unknown_style_field = |style: &'static str| {
        let mut entry = StyleEntry::default();
        entry.unknown.push("colour".into());
        vec![(style, Some(entry))]
    };
    let cases: Vec<(&str, Vec<ThemeEntry>, &str)> = vec![
        (
            "unknown field",
            vec![ok(), unknown_field],
            "themes[1]: unknown field \"colour\"",
        ),
        (
            "unknown palette colour",
            vec![
                ok(),
                entry("x", &[("red", "#ff5555"), ("orange", "#ffaa00")], vec![]),
            ],
            "themes[1].palette: unknown colour \"orange\"",
        ),
        (
            "invalid palette colour",
            vec![ok(), entry("x", &[("red", "#ff55")], vec![])],
            "themes[1].palette.red: invalid colour \"#ff55\"",
        ),
        (
            "invalid background",
            vec![entry("x", &[("background", "Purpel")], vec![])],
            "themes[0].palette.background: invalid colour \"Purpel\"",
        ),
        (
            "unknown style",
            vec![ok(), entry("x", &[], vec![("blok_title", None)])],
            "themes[1].component_style: unknown style \"blok_title\"",
        ),
        (
            "unknown field in a style",
            vec![ok(), entry("x", &[], unknown_style_field("border"))],
            "themes[1].component_style.border: unknown field \"colour\", expected fg, bg or modifiers",
        ),
        (
            "bad fg",
            vec![ok(), entry("x", &[], vec![("border", fg("Purpel"))])],
            "themes[1].component_style.border.fg: invalid colour \"Purpel\" (a palette name such as BrightBlack, or #rrggbb)",
        ),
        (
            "bad bg",
            vec![entry(
                "x",
                &[],
                vec![("like", style(None, Some("Reset"), &[]))],
            )],
            "themes[0].component_style.like.bg: invalid colour \"Reset\" (a palette name such as BrightBlack, or #rrggbb)",
        ),
        (
            "bad modifier",
            vec![
                ok(),
                entry(
                    "x",
                    &[],
                    vec![("selection", style(None, None, &["Bold", "Blink"]))],
                ),
            ],
            "themes[1].component_style.selection: unknown modifier \"Blink\"",
        ),
        (
            "missing name",
            vec![
                ok(),
                entry("a", &[], vec![]),
                entry("b", &[], vec![]),
                ThemeEntry::default(),
            ],
            "themes[3]: missing name",
        ),
        (
            "empty name",
            vec![
                ok(),
                entry("a", &[], vec![]),
                entry("b", &[], vec![]),
                entry("", &[], vec![]),
            ],
            "themes[3]: empty name",
        ),
        (
            "name taken",
            vec![
                ok(),
                entry("dracula", &[], vec![]),
                entry("b", &[], vec![]),
                entry("dracula", &[], vec![]),
            ],
            "themes[3]: \"dracula\" is already themes[1]",
        ),
        (
            "default twice",
            vec![entry("default", &[], vec![]), entry("default", &[], vec![])],
            "themes[1]: \"default\" is already themes[0]",
        ),
    ];
    for (name, entries, message) in cases {
        match build(&file(entries)) {
            Ok(themes) => panic!("{name}: built {:?}", names(&themes)),
            Err(e) => assert_eq!(e.to_string(), message, "{name}"),
        }
    }
}

// --- AC3 ---------------------------------------------------------------------

/// AC3: the built-in `default` resolves to the "Default" column: today's
/// look (bold, dim, reversed, red), and no screen colours.
#[test]
fn ac3_default_is_todays_look() {
    use Component::*;
    let none = Style::new();
    let bold = Style::new().add(BOLD);
    let rows: [(Component, Style); 18] = [
        (BlockTitle, none),
        (Border, none),
        (PlaybackStatus, bold),
        (PlaybackTrack, bold),
        (PlaybackArtists, bold),
        (PlaybackAlbum, none),
        (PlaybackMetadata, none),
        (PlaybackProgressBar, none),
        (PlaybackProgressBarUnfilled, none),
        (CurrentPlaying, bold),
        (PageDesc, bold),
        (PlaylistDesc, Style::new().add(DIM)),
        (TableHeader, bold),
        (Selection, Style::new().add(REVERSED)),
        (SecondaryRow, none),
        (Like, none),
        (Error, Style::new().fg(Color::Red)),
        (Prompt, bold),
    ];
    let theme = Theme::default();
    assert_eq!(theme.name, "default");
    assert_eq!(theme.screen, Style::new());
    for (component, want) in rows {
        assert_eq!(theme.style(component), want, "{}", component.name());
    }
    assert_eq!(State::default().theme(), &theme);
}

// --- AC5 ---------------------------------------------------------------------

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

fn snapshot() -> PlayerSnapshot {
    PlayerSnapshot {
        queue: [1u64, 2]
            .iter()
            .map(|id| QueueEntry {
                id: EntryId(*id),
                track: track(id + 100),
                suggested: false,
            })
            .collect(),
        current: Some(EntryId(1)),
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

/// The three themes `default`, `dracula`, `gruvbox_dark`.
fn three() -> Themes {
    build(&file(vec![
        entry("dracula", &[("background", "#1e1f29")], vec![]),
        entry("gruvbox_dark", &[("background", "#282828")], vec![]),
    ]))
    .unwrap()
}

/// The queue page with two entries and the three themes, `active` in use.
fn themed(active: usize) -> State {
    let mut state = State::default();
    update(
        &mut state,
        Action::Player(protocol::Event::Player(snapshot())),
    );
    apply_themes(&mut state, three(), active);
    state
}

fn press(state: &mut State, keys: &[Key]) -> Vec<Effect> {
    keys.iter()
        .flat_map(|k| update(state, Action::Key(*k)))
        .collect()
}

fn themes_popup(state: &State) -> Option<(usize, usize)> {
    match state.popup {
        Some(Popup::Themes { cursor, active }) => Some((cursor, active)),
        _ => None,
    }
}

/// AC5: `T` opens the popup with the cursor and `●` on the theme in use;
/// the list commands move and preview; `Enter` keeps; `Esc` and `q` go
/// back; no other key acts inside it.
#[test]
fn ac5_themes_popup() {
    use Key::{Char, Down, Enter, Esc, Up};
    // (keys after `T`, popup after (cursor, active), the theme drawn)
    type Case = (Vec<Key>, Option<(usize, usize)>, usize);
    let cases: Vec<Case> = vec![
        (vec![], Some((1, 1)), 1),
        (vec![Char('j')], Some((2, 1)), 2),
        (vec![Down], Some((2, 1)), 2),
        (vec![Char('j'), Char('j')], Some((2, 1)), 2),
        (vec![Char('k')], Some((0, 1)), 0),
        (vec![Up, Up], Some((0, 1)), 0),
        (vec![Char('G')], Some((2, 1)), 2),
        (vec![Char('g'), Char('g')], Some((0, 1)), 0),
        (vec![Key::Ctrl('f')], Some((2, 1)), 2),
        (vec![Key::Ctrl('b')], Some((0, 1)), 0),
        // `Enter` keeps the theme under the cursor and closes.
        (vec![Char('j'), Enter], None, 2),
        (vec![Char('k'), Enter], None, 0),
        (vec![Enter], None, 1),
        // `Esc` and `q` go back to the theme marked `●` and close.
        (vec![Char('j'), Esc], None, 1),
        (vec![Char('k'), Char('q')], None, 1),
        (vec![Esc], None, 1),
    ];
    for (keys, popup, theme) in cases {
        let mut state = themed(1);
        assert_eq!(press(&mut state, &[Char('T')]), vec![], "T");
        assert_eq!(press(&mut state, &keys), vec![], "{keys:?}");
        assert_eq!(themes_popup(&state), popup, "{keys:?}");
        assert_eq!(state.theme, theme, "{keys:?}");
        assert_eq!(state.theme().name, three().list()[theme].name, "{keys:?}");
    }
    // A theme kept with `Enter` is the one `●` marks next time.
    let mut state = themed(0);
    press(&mut state, &[Char('T'), Char('G'), Enter, Char('T')]);
    assert_eq!(themes_popup(&state), Some((2, 2)));

    // No other key acts inside it: nothing sent, nothing changed.
    for key in [
        Char('n'),
        Char(' '),
        Char('D'),
        Char('T'),
        Char('/'),
        Char('z'),
        Char('o'),
        Char('a'),
        Char('+'),
        Key::Tab,
        Key::Backspace,
        Char('r'),
    ] {
        let mut state = themed(1);
        press(&mut state, &[Char('T'), Char('j')]);
        let before = state.clone();
        assert_eq!(press(&mut state, &[key]), vec![], "{key:?}");
        assert_eq!(state, before, "{key:?}");
    }

    // `T` opens it on every page, and while disconnected (themes are the
    // TUI's own).
    let pages = [
        PageKind::Queue,
        PageKind::Library,
        PageKind::FavoriteTracks,
        PageKind::Mixes,
        PageKind::Album(1),
        PageKind::Artist(1),
        PageKind::Playlist("p".into()),
        PageKind::Mix("m".into()),
        PageKind::TrackRadio(1),
        PageKind::Search("q".into()),
    ];
    for kind in pages {
        let mut state = themed(2);
        let mut page = Page::new(kind.clone());
        if let Some(search) = page.search.as_mut() {
            search.focus = SearchFocus::Windows;
        }
        state.history.push(page);
        assert_eq!(press(&mut state, &[Char('T')]), vec![], "{kind:?}");
        assert_eq!(themes_popup(&state), Some((2, 2)), "{kind:?}");
    }
    let mut state = themed(1);
    update(&mut state, Action::Disconnected { shut_down: false });
    assert_ne!(state.connection, Connection::Connected);
    press(&mut state, &[Char('T'), Char('j')]);
    assert_eq!(themes_popup(&state), Some((2, 1)));
    assert_eq!(state.theme, 2);

    // With no `theme.toml`, `default` alone: the cursor stays on it.
    let mut state = State::default();
    press(&mut state, &[Char('T'), Char('j')]);
    assert_eq!(themes_popup(&state), Some((0, 0)));
}

/// AC5: `T` is `SwitchTheme`, no longer a skipped spotify-player name; it
/// types `T` in every text input and does nothing with another popup open;
/// rebinding it works.
#[test]
fn ac5_switch_theme_key() {
    use Key::{Char, Ctrl};
    assert!(
        !UNSUPPORTED_COMMANDS.contains(&"SwitchTheme"),
        "still skipped"
    );
    assert_eq!(
        keymap::Keymap::default().get(&[Char('T')]),
        Some(Binding::Command(UiCommand::SwitchTheme))
    );

    // Text inputs take `T`.
    let mut prompt = themed(1);
    press(&mut prompt, &[Char('o'), Char('T')]);
    assert_eq!(prompt.prompt.as_ref().map(|p| p.text.as_str()), Some("T"));
    assert_eq!(themes_popup(&prompt), None);

    let mut search = themed(1);
    press(&mut search, &[Char('g'), Char('s'), Char('T')]);
    assert_eq!(
        search.page().search.as_ref().map(|s| s.input.as_str()),
        Some("T")
    );
    assert_eq!(themes_popup(&search), None);

    let mut filter = themed(1);
    press(&mut filter, &[Char('/'), Char('T')]);
    assert_eq!(super::super::filter::queue_filter(&filter).text, "T");
    assert_eq!(themes_popup(&filter), None);

    let mut help_filter = themed(1);
    press(&mut help_filter, &[Char('?'), Char('/'), Char('T')]);
    assert_eq!(
        help_filter.help.as_ref().map(|h| h.filter.as_str()),
        Some("T")
    );
    assert_eq!(themes_popup(&help_filter), None);

    let mut name = themed(1);
    name.popup = Some(Popup::NewPlaylist {
        tracks: super::super::TrackSource::Album(1),
        name: String::new(),
    });
    press(&mut name, &[Char('T')]);
    assert!(
        matches!(&name.popup, Some(Popup::NewPlaylist { name, .. }) if name == "T"),
        "{:?}",
        name.popup
    );

    // Another popup open: `T` does nothing.
    let mut actions = themed(1);
    press(&mut actions, &[Char('a')]);
    assert!(matches!(actions.popup, Some(Popup::Actions { .. })));
    let before = actions.clone();
    assert_eq!(press(&mut actions, &[Char('T')]), vec![]);
    assert_eq!(actions, before);

    // Rebinding in `keymap.toml`: no notice, and the new key opens it.
    let entries = vec![
        KeymapEntry {
            command: CommandEntry::name("SwitchTheme"),
            key_sequence: "C-t".into(),
        },
        KeymapEntry {
            command: CommandEntry::name("None"),
            key_sequence: "T".into(),
        },
    ];
    let built = keymap::build(&KeymapFile {
        keymaps: entries,
        actions: vec![],
    })
    .unwrap();
    assert_eq!(built.notice(), None);
    let mut state = themed(1);
    apply_keymap(&mut state, built);
    press(&mut state, &[Char('T')]);
    assert_eq!(themes_popup(&state), None, "T still bound");
    press(&mut state, &[Ctrl('t')]);
    assert_eq!(themes_popup(&state), Some((1, 1)));
}

/// AC5: the keys help lists `T  choose a theme` under `App`, and a
/// `Popup · Themes` section when opened from the popup.
#[test]
fn ac5_help_lists_switch_theme() {
    use Key::Char;
    let state = themed(1);
    let sections = help(&state);
    let app = sections
        .iter()
        .find(|s| s.title == "App")
        .expect("an App section");
    let row = app
        .rows
        .iter()
        .find(|r| r.command == "SwitchTheme")
        .unwrap_or_else(|| panic!("no SwitchTheme row: {:?}", app.rows));
    assert_eq!(
        (row.keys.as_str(), row.text.as_str()),
        ("T", "choose a theme")
    );
    assert!(!row.dim);

    let mut state = themed(1);
    press(&mut state, &[Char('T')]);
    let sections = help(&state);
    let titles: Vec<&str> = sections.iter().map(|s| s.title.as_str()).collect();
    assert_eq!(titles, ["Popup · Themes", "Lists", "App"]);
    let rows: Vec<(&str, &str)> = sections[0]
        .rows
        .iter()
        .map(|r| (r.keys.as_str(), r.text.as_str()))
        .collect();
    assert_eq!(
        rows,
        [
            ("q", "back to the theme in use"),
            ("enter", "keep the selected theme"),
            ("esc", "close / cancel"),
        ]
    );
    // `Enter` on the help's `T` row opens the popup.
    let mut state = themed(1);
    press(&mut state, &[Char('?'), Char('/')]);
    press(
        &mut state,
        &"choose a theme".chars().map(Char).collect::<Vec<_>>(),
    );
    press(&mut state, &[Key::Enter, Key::Enter]);
    assert_eq!(themes_popup(&state), Some((1, 1)));
}

// --- AC6 ---------------------------------------------------------------------

fn skipping(names: &[&str]) -> Themes {
    build(&file(vec![entry(
        "s",
        &[],
        names.iter().map(|n| (*n, None)).collect(),
    )]))
    .unwrap()
}

/// AC6: skipped components give the TUI's start notice, kept through the
/// player's first `Welcome`.
#[test]
fn ac6_theme_notice() {
    let cases: [(&[&str], Option<&str>); 3] = [
        (
            &["lyrics_played", "lyrics_playing", "lyrics_played"],
            Some(
                "theme.toml: 2 spotify-player styles not used here: lyrics_played, lyrics_playing",
            ),
        ),
        (
            &["visualization"],
            Some("theme.toml: 1 spotify-player style not used here: visualization"),
        ),
        (&[], None),
    ];
    for (skipped, notice) in cases {
        let themes = skipping(skipped);
        assert_eq!(themes.notice().as_deref(), notice, "{skipped:?}");
        let mut state = State::default();
        apply_themes(&mut state, themes, 1);
        assert_eq!(state.message(), notice, "{skipped:?}");
        update(
            &mut state,
            Action::Welcome {
                snapshot: snapshot(),
                login_required: false,
            },
        );
        assert_eq!(state.message(), notice, "{skipped:?}: after Welcome");
        update(
            &mut state,
            Action::Welcome {
                snapshot: snapshot(),
                login_required: false,
            },
        );
        assert_eq!(state.message(), None, "{skipped:?}: the second Welcome");
    }
}

/// AC6: with a keymap notice too, both are shown, the keymap's first,
/// joined by ` · `, whichever is applied first.
#[test]
fn ac6_both_notices() {
    let keymap = || {
        keymap::build(&KeymapFile {
            keymaps: vec![KeymapEntry {
                command: CommandEntry::name("PlayRandom"),
                key_sequence: "x".into(),
            }],
            actions: vec![],
        })
        .unwrap()
    };
    let want = "keymap.toml: 1 spotify-player command not supported here: PlayRandom \
                · theme.toml: 1 spotify-player style not used here: lyrics_played";
    let mut first = State::default();
    apply_keymap(&mut first, keymap());
    apply_themes(&mut first, skipping(&["lyrics_played"]), 0);
    let mut second = State::default();
    apply_themes(&mut second, skipping(&["lyrics_played"]), 0);
    apply_keymap(&mut second, keymap());
    for state in [&mut first, &mut second] {
        assert_eq!(state.message(), Some(want));
        update(
            state,
            Action::Welcome {
                snapshot: snapshot(),
                login_required: false,
            },
        );
        assert_eq!(state.message(), Some(want), "after Welcome");
    }
}
