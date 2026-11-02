//! Themes (spec 0017 "`theme.toml`", "Components"): spotify-player's theme
//! format, built from the file's entries (already deserialised: no TOML
//! here) into the theme list, the built-in `default` (today's look) first.
//! The colours, modifiers and styles are the TUI model's own; the binary
//! maps them to the terminal's (no ratatui here).
//!
//! The Themes popup (`SwitchTheme`, spec 0017 "The Themes popup") lives
//! here too: moving its cursor previews the theme under it, `Enter` keeps
//! it, `Esc` and `q` go back to the theme in use when it opened.

use std::fmt;

use serde::Deserialize;
use serde::de::{self, IgnoredAny, MapAccess, Visitor};

use super::dispatch::step;
use super::keymap::UiCommand;
use super::popup::Popup;
use super::{Effect, State};

pub mod bombastine;

pub use bombastine::Envy;

// --- colours, modifiers, styles ----------------------------------------------

/// A colour: the terminal's own (`Reset`), one of its sixteen ANSI colours
/// (ratatui's names: `Gray` is ANSI 7, `White` the bright white), an
/// indexed one or `#rrggbb`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Color {
    Reset,
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    Gray,
    DarkGray,
    LightRed,
    LightGreen,
    LightYellow,
    LightBlue,
    LightMagenta,
    LightCyan,
    White,
    Indexed(u8),
    Rgb(u8, u8, u8),
}

/// Text attributes, a set.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Modifiers(u16);

impl Modifiers {
    pub const NONE: Self = Self(0);
    pub const BOLD: Self = Self(1);
    pub const DIM: Self = Self(1 << 1);
    pub const ITALIC: Self = Self(1 << 2);
    pub const UNDERLINED: Self = Self(1 << 3);
    pub const RAPID_BLINK: Self = Self(1 << 4);
    pub const REVERSED: Self = Self(1 << 5);
    pub const HIDDEN: Self = Self(1 << 6);
    pub const CROSSED_OUT: Self = Self(1 << 7);

    /// The eight, with their names in `theme.toml`.
    pub const ALL: [(&'static str, Self); 8] = [
        ("Bold", Self::BOLD),
        ("Dim", Self::DIM),
        ("Italic", Self::ITALIC),
        ("Underlined", Self::UNDERLINED),
        ("RapidBlink", Self::RAPID_BLINK),
        ("Reversed", Self::REVERSED),
        ("Hidden", Self::HIDDEN),
        ("CrossedOut", Self::CROSSED_OUT),
    ];

    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub const fn difference(self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

impl std::ops::BitOr for Modifiers {
    type Output = Self;

    fn bitor(self, other: Self) -> Self {
        self.union(other)
    }
}

/// Colours and modifiers; a colour left out is the one underneath.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Style {
    pub fg: Option<Color>,
    pub bg: Option<Color>,
    pub modifiers: Modifiers,
}

impl Style {
    pub const fn new() -> Self {
        Self {
            fg: None,
            bg: None,
            modifiers: Modifiers::NONE,
        }
    }

    pub const fn fg(self, fg: Color) -> Self {
        Self {
            fg: Some(fg),
            ..self
        }
    }

    pub const fn bg(self, bg: Color) -> Self {
        Self {
            bg: Some(bg),
            ..self
        }
    }

    /// With `modifiers` added.
    pub const fn add(self, modifiers: Modifiers) -> Self {
        Self {
            modifiers: self.modifiers.union(modifiers),
            ..self
        }
    }

    /// With `modifiers` taken out.
    pub const fn remove(self, modifiers: Modifiers) -> Self {
        Self {
            modifiers: self.modifiers.difference(modifiers),
            ..self
        }
    }

    /// `other` laid over this one (ratatui's `patch`): its colours replace
    /// these where set, its modifiers add.
    pub const fn patch(self, other: Self) -> Self {
        Self {
            fg: match other.fg {
                Some(fg) => Some(fg),
                None => self.fg,
            },
            bg: match other.bg {
                Some(bg) => Some(bg),
                None => self.bg,
            },
            modifiers: self.modifiers.union(other.modifiers),
        }
    }
}

// --- the palette -------------------------------------------------------------

/// The sixteen terminal colours a palette sets and a component style names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PaletteColor {
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    White,
    BrightBlack,
    BrightRed,
    BrightGreen,
    BrightYellow,
    BrightBlue,
    BrightMagenta,
    BrightCyan,
    BrightWhite,
}

/// Every palette colour, in ANSI order.
pub const PALETTE_COLORS: [PaletteColor; 16] = [
    PaletteColor::Black,
    PaletteColor::Red,
    PaletteColor::Green,
    PaletteColor::Yellow,
    PaletteColor::Blue,
    PaletteColor::Magenta,
    PaletteColor::Cyan,
    PaletteColor::White,
    PaletteColor::BrightBlack,
    PaletteColor::BrightRed,
    PaletteColor::BrightGreen,
    PaletteColor::BrightYellow,
    PaletteColor::BrightBlue,
    PaletteColor::BrightMagenta,
    PaletteColor::BrightCyan,
    PaletteColor::BrightWhite,
];

impl PaletteColor {
    /// The palette's key (`bright_black`).
    pub fn key(self) -> &'static str {
        match self {
            Self::Black => "black",
            Self::Red => "red",
            Self::Green => "green",
            Self::Yellow => "yellow",
            Self::Blue => "blue",
            Self::Magenta => "magenta",
            Self::Cyan => "cyan",
            Self::White => "white",
            Self::BrightBlack => "bright_black",
            Self::BrightRed => "bright_red",
            Self::BrightGreen => "bright_green",
            Self::BrightYellow => "bright_yellow",
            Self::BrightBlue => "bright_blue",
            Self::BrightMagenta => "bright_magenta",
            Self::BrightCyan => "bright_cyan",
            Self::BrightWhite => "bright_white",
        }
    }

    /// The terminal's own colour, what a palette that leaves it out has
    /// (spotify-player's defaults: `white` is ANSI 7).
    pub fn ansi(self) -> Color {
        match self {
            Self::Black => Color::Black,
            Self::Red => Color::Red,
            Self::Green => Color::Green,
            Self::Yellow => Color::Yellow,
            Self::Blue => Color::Blue,
            Self::Magenta => Color::Magenta,
            Self::Cyan => Color::Cyan,
            Self::White => Color::Gray,
            Self::BrightBlack => Color::DarkGray,
            Self::BrightRed => Color::LightRed,
            Self::BrightGreen => Color::LightGreen,
            Self::BrightYellow => Color::LightYellow,
            Self::BrightBlue => Color::LightBlue,
            Self::BrightMagenta => Color::LightMagenta,
            Self::BrightCyan => Color::LightCyan,
            Self::BrightWhite => Color::White,
        }
    }
}

/// A theme's palette: the screen's colours (`None`: the terminal's) and
/// the sixteen colours the component styles name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    pub background: Option<Color>,
    pub foreground: Option<Color>,
    pub colors: [Color; 16],
}

impl Default for Palette {
    fn default() -> Self {
        Self {
            background: None,
            foreground: None,
            colors: PALETTE_COLORS.map(PaletteColor::ansi),
        }
    }
}

impl Palette {
    pub fn color(&self, color: PaletteColor) -> Color {
        self.colors[color as usize]
    }

    /// A component's colour: a palette name through this palette, hex as
    /// it is.
    pub fn resolve(&self, color: StyleColor) -> Color {
        match color {
            StyleColor::Palette(name) => self.color(name),
            StyleColor::Rgb(r, g, b) => Color::Rgb(r, g, b),
        }
    }
}

/// A component style's colour, as written: a palette name or `#rrggbb`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StyleColor {
    Palette(PaletteColor),
    Rgb(u8, u8, u8),
}

/// A component style before the palette resolves it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct StyleSpec {
    pub fg: Option<StyleColor>,
    pub bg: Option<StyleColor>,
    pub modifiers: Modifiers,
}

impl StyleSpec {
    pub fn resolve(&self, palette: &Palette) -> Style {
        Style {
            fg: self.fg.map(|c| palette.resolve(c)),
            bg: self.bg.map(|c| palette.resolve(c)),
            modifiers: self.modifiers,
        }
    }
}

// --- parsing -----------------------------------------------------------------

/// A palette value, as spotify-player parses it (ratatui's
/// `Color::from_str`): a colour name ignoring case, spaces, `-` and `_`,
/// with `bright`/`light`, `grey`/`gray`/`silver` read as ratatui reads
/// them; a number 0–255; or `#rrggbb`.
pub fn parse_palette_color(text: &str) -> Option<Color> {
    // ratatui-core 0.1.2's `Color::from_str`, step by step (the binary's
    // `ac1_palette_matches_ratatui` checks the two agree).
    let name = text
        .to_lowercase()
        .replace([' ', '-', '_'], "")
        .replace("bright", "light")
        .replace("grey", "gray")
        .replace("silver", "gray")
        .replace("lightblack", "darkgray")
        .replace("lightwhite", "white")
        .replace("lightgray", "white");
    let color = match name.as_str() {
        "reset" => Color::Reset,
        "black" => Color::Black,
        "red" => Color::Red,
        "green" => Color::Green,
        "yellow" => Color::Yellow,
        "blue" => Color::Blue,
        "magenta" => Color::Magenta,
        "cyan" => Color::Cyan,
        "gray" => Color::Gray,
        "darkgray" => Color::DarkGray,
        "lightred" => Color::LightRed,
        "lightgreen" => Color::LightGreen,
        "lightyellow" => Color::LightYellow,
        "lightblue" => Color::LightBlue,
        "lightmagenta" => Color::LightMagenta,
        "lightcyan" => Color::LightCyan,
        "white" => Color::White,
        _ => match text.parse::<u8>() {
            Ok(index) => Color::Indexed(index),
            Err(_) => {
                let (r, g, b) = hex(text)?;
                Color::Rgb(r, g, b)
            }
        },
    };
    // Spec 0017 addendum 67055c40c1e2.
    match color {
        Color::Green | Color::LightGreen => None,
        Color::Indexed(index) if bombastine::is_envy_index(index) => None,
        color => Some(color),
    }
}

/// A component style's colour: a palette name (`Black` … `BrightWhite`,
/// ignoring case, `_`, `-` and spaces) or `#rrggbb`.
pub fn parse_style_color(text: &str) -> Option<StyleColor> {
    let name = text.to_lowercase().replace([' ', '-', '_'], "");
    PALETTE_COLORS
        .into_iter()
        // Spec 0017 addendum 67055c40c1e2.
        .filter(|color| !matches!(color, PaletteColor::Green | PaletteColor::BrightGreen))
        .find(|color| color.key().replace('_', "") == name)
        .map(StyleColor::Palette)
        .or_else(|| hex(text).map(|(r, g, b)| StyleColor::Rgb(r, g, b)))
}

/// A modifier's name, ignoring case.
pub fn parse_modifier(text: &str) -> Option<Modifiers> {
    Modifiers::ALL
        .into_iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(text))
        .map(|(_, modifier)| modifier)
}

/// `#rrggbb`, as ratatui and spotify-player read it.
fn hex(text: &str) -> Option<(u8, u8, u8)> {
    if !text.starts_with('#') || text.len() != 7 {
        return None;
    }
    let r = u8::from_str_radix(text.get(1..3)?, 16).ok()?;
    let g = u8::from_str_radix(text.get(3..5)?, 16).ok()?;
    let b = u8::from_str_radix(text.get(5..7)?, 16).ok()?;
    Some((r, g, b))
}

// --- components --------------------------------------------------------------

/// Where a theme's styles are drawn (spec 0017 "Components").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Component {
    BlockTitle,
    Border,
    PlaybackStatus,
    PlaybackTrack,
    PlaybackArtists,
    PlaybackAlbum,
    PlaybackMetadata,
    PlaybackProgressBar,
    PlaybackProgressBarUnfilled,
    CurrentPlaying,
    PageDesc,
    PlaylistDesc,
    TableHeader,
    Selection,
    SecondaryRow,
    Like,
    /// This player's own: the message row and failed loads.
    Error,
    /// This player's own: the open prompts and the search input.
    Prompt,
    /// This player's own (spec 0020): the rows of a selection.
    SelectedRow,
}

/// Every component, in the spec's table order.
pub const COMPONENTS: [Component; 19] = [
    Component::BlockTitle,
    Component::Border,
    Component::PlaybackStatus,
    Component::PlaybackTrack,
    Component::PlaybackArtists,
    Component::PlaybackAlbum,
    Component::PlaybackMetadata,
    Component::PlaybackProgressBar,
    Component::PlaybackProgressBarUnfilled,
    Component::CurrentPlaying,
    Component::PageDesc,
    Component::PlaylistDesc,
    Component::TableHeader,
    Component::Selection,
    Component::SecondaryRow,
    Component::Like,
    Component::Error,
    Component::Prompt,
    Component::SelectedRow,
];

/// spotify-player components this player does not draw: accepted and
/// skipped with a notice.
pub const SKIPPED_COMPONENTS: [&str; 4] = [
    "playback_genres",
    "lyrics_played",
    "lyrics_playing",
    "visualization",
];

impl Component {
    /// The name in `component_style`.
    pub fn name(self) -> &'static str {
        match self {
            Self::BlockTitle => "block_title",
            Self::Border => "border",
            Self::PlaybackStatus => "playback_status",
            Self::PlaybackTrack => "playback_track",
            Self::PlaybackArtists => "playback_artists",
            Self::PlaybackAlbum => "playback_album",
            Self::PlaybackMetadata => "playback_metadata",
            Self::PlaybackProgressBar => "playback_progress_bar",
            Self::PlaybackProgressBarUnfilled => "playback_progress_bar_unfilled",
            Self::CurrentPlaying => "current_playing",
            Self::PageDesc => "page_desc",
            Self::PlaylistDesc => "playlist_desc",
            Self::TableHeader => "table_header",
            Self::Selection => "selection",
            Self::SecondaryRow => "secondary_row",
            Self::Like => "like",
            Self::Error => "error",
            Self::Prompt => "prompt",
            Self::SelectedRow => "selected_row",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        COMPONENTS.iter().copied().find(|c| c.name() == name)
    }

    /// The default: the built-in theme's style, today's look, and what a
    /// theme gets for a component it leaves out (spec 0017 decision 1).
    pub fn default_style(self) -> StyleSpec {
        let with = |modifiers| StyleSpec {
            modifiers,
            ..StyleSpec::default()
        };
        match self {
            Self::PlaybackStatus
            | Self::PlaybackTrack
            | Self::PlaybackArtists
            | Self::CurrentPlaying
            | Self::PageDesc
            | Self::TableHeader
            | Self::Prompt
            | Self::SelectedRow => with(Modifiers::BOLD),
            Self::PlaylistDesc => with(Modifiers::DIM),
            Self::Selection => with(Modifiers::REVERSED),
            Self::Error => StyleSpec {
                fg: Some(StyleColor::Palette(PaletteColor::Red)),
                ..StyleSpec::default()
            },
            Self::BlockTitle
            | Self::Border
            | Self::PlaybackAlbum
            | Self::PlaybackMetadata
            | Self::PlaybackProgressBar
            | Self::PlaybackProgressBarUnfilled
            | Self::SecondaryRow
            | Self::Like => StyleSpec::default(),
        }
    }
}

// --- themes ------------------------------------------------------------------

/// The built-in theme's name.
pub const DEFAULT_THEME: &str = "default";

/// A theme, resolved: the screen's style (the palette's `background` and
/// `foreground`) and one style per component.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    pub name: String,
    pub screen: Style,
    styles: [Style; 19],
    envy: Option<(u8, u8, u8)>,
}

impl Default for Theme {
    /// The built-in `default`: today's look.
    fn default() -> Self {
        Self::resolve(
            DEFAULT_THEME.to_owned(),
            &Palette::default(),
            &COMPONENTS.map(Component::default_style),
        )
    }
}

impl Theme {
    /// `name` with `palette`, and `specs` (one per component, in
    /// [`COMPONENTS`] order).
    pub fn resolve(name: String, palette: &Palette, specs: &[StyleSpec; 19]) -> Self {
        let mut screen = Style::new();
        if let Some(bg) = palette.background {
            screen = screen.bg(bg);
        }
        if let Some(fg) = palette.foreground {
            screen = screen.fg(fg);
        }
        let styles = specs.map(|spec| spec.resolve(palette));
        let envy = [palette.background, palette.foreground]
            .into_iter()
            .flatten()
            .chain(palette.colors)
            .chain(styles.iter().flat_map(|s| [s.fg, s.bg]).flatten())
            .find_map(|color| match color {
                Color::Rgb(r, g, b) if bombastine::is_envy(r, g, b) => Some((r, g, b)),
                _ => None,
            });
        Self {
            name,
            screen,
            styles,
            envy,
        }
    }

    /// The style `component` is drawn in.
    pub fn style(&self, component: Component) -> Style {
        self.styles[component as usize]
    }

    /// Spec 0017 addendum 67055c40c1e2.
    pub fn envy(&self) -> Option<Envy> {
        self.envy.map(|hex| Envy {
            theme: self.name.clone(),
            hex,
        })
    }
}

/// The theme list (`default` first) and the skipped spotify-player
/// component names the file used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Themes {
    themes: Vec<Theme>,
    skipped: Vec<String>,
}

impl Default for Themes {
    /// `default` alone (no `theme.toml`).
    fn default() -> Self {
        Self {
            themes: vec![Theme::default()],
            skipped: Vec::new(),
        }
    }
}

impl Themes {
    /// Every theme, in order; never empty.
    pub fn list(&self) -> &[Theme] {
        &self.themes
    }

    /// The index of the theme called `name`.
    pub fn find(&self, name: &str) -> Option<usize> {
        self.themes.iter().position(|t| t.name == name)
    }

    /// The skipped spotify-player components, in file order, each once.
    pub fn skipped(&self) -> &[String] {
        &self.skipped
    }

    /// The message row's line about the skipped names, if there are any.
    pub fn notice(&self) -> Option<String> {
        let names = &self.skipped;
        match names.len() {
            0 => None,
            1 => Some(format!(
                "theme.toml: 1 spotify-player style not used here: {}",
                names[0]
            )),
            n => Some(format!(
                "theme.toml: {n} spotify-player styles not used here: {}",
                names.join(", ")
            )),
        }
    }

    /// `unknown theme "x" (themes: default, dracula)`: what the binary
    /// says, after the source, of a `theme` no theme is called.
    pub fn unknown(&self, name: &str) -> String {
        let names: Vec<&str> = self.themes.iter().map(|t| t.name.as_str()).collect();
        format!("unknown theme \"{name}\" (themes: {})", names.join(", "))
    }
}

// --- the file's entries ------------------------------------------------------

/// `theme.toml`, deserialised.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThemeFile {
    #[serde(default)]
    pub themes: Vec<ThemeEntry>,
}

/// A `[[themes]]` entry, as written: unknown fields are kept by name so
/// [`build`] can name them with the entry's index.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ThemeEntry {
    pub name: Option<String>,
    /// `[themes.palette]`, in file order: (key, value).
    pub palette: Vec<(String, String)>,
    /// `[themes.component_style]`, in file order: (name, style). The style
    /// is `None` for a name that is not one of this player's components
    /// (skipped or unknown: its value is not read).
    pub component_style: Vec<(String, Option<StyleEntry>)>,
    /// The fields other than these three, in file order.
    pub unknown: Vec<String>,
}

/// A component's inline table, as written.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StyleEntry {
    pub fg: Option<String>,
    pub bg: Option<String>,
    pub modifiers: Vec<String>,
    /// The fields other than `fg`, `bg` and `modifiers`, in file order.
    pub unknown: Vec<String>,
}

impl<'de> Deserialize<'de> for ThemeEntry {
    fn deserialize<D: de::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct EntryVisitor;

        impl<'de> Visitor<'de> for EntryVisitor {
            type Value = ThemeEntry;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a theme: name, palette and component_style")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<ThemeEntry, A::Error> {
                let mut entry = ThemeEntry::default();
                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "name" => entry.name = Some(map.next_value()?),
                        "palette" => entry.palette = map.next_value::<Ordered<String>>()?.0,
                        "component_style" => {
                            entry.component_style = map.next_value::<ComponentStyles>()?.0;
                        }
                        _ => {
                            map.next_value::<IgnoredAny>()?;
                            entry.unknown.push(key);
                        }
                    }
                }
                Ok(entry)
            }
        }

        deserializer.deserialize_map(EntryVisitor)
    }
}

impl<'de> Deserialize<'de> for StyleEntry {
    fn deserialize<D: de::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct StyleVisitor;

        impl<'de> Visitor<'de> for StyleVisitor {
            type Value = StyleEntry;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a style such as { fg = \"Cyan\", modifiers = [\"Bold\"] }")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<StyleEntry, A::Error> {
                let mut style = StyleEntry::default();
                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "fg" => style.fg = Some(map.next_value()?),
                        "bg" => style.bg = Some(map.next_value()?),
                        "modifiers" => style.modifiers = map.next_value()?,
                        _ => {
                            map.next_value::<IgnoredAny>()?;
                            style.unknown.push(key);
                        }
                    }
                }
                Ok(style)
            }
        }

        deserializer.deserialize_map(StyleVisitor)
    }
}

/// A table's entries in file order.
struct Ordered<V>(Vec<(String, V)>);

impl<'de, V: Deserialize<'de>> Deserialize<'de> for Ordered<V> {
    fn deserialize<D: de::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct OrderedVisitor<V>(std::marker::PhantomData<V>);

        impl<'de, V: Deserialize<'de>> Visitor<'de> for OrderedVisitor<V> {
            type Value = Ordered<V>;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a table")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Ordered<V>, A::Error> {
                let mut entries = Vec::new();
                while let Some((key, value)) = map.next_entry()? {
                    entries.push((key, value));
                }
                Ok(Ordered(entries))
            }
        }

        deserializer.deserialize_map(OrderedVisitor(std::marker::PhantomData))
    }
}

/// `component_style`: this player's components read as styles, any other
/// name kept with its value unread.
struct ComponentStyles(Vec<(String, Option<StyleEntry>)>);

impl<'de> Deserialize<'de> for ComponentStyles {
    fn deserialize<D: de::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct StylesVisitor;

        impl<'de> Visitor<'de> for StylesVisitor {
            type Value = ComponentStyles;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a table of component styles")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<ComponentStyles, A::Error> {
                let mut styles = Vec::new();
                while let Some(name) = map.next_key::<String>()? {
                    let style = if Component::from_name(&name).is_some() {
                        Some(map.next_value::<StyleEntry>()?)
                    } else {
                        map.next_value::<IgnoredAny>()?;
                        None
                    };
                    styles.push((name, style));
                }
                Ok(ComponentStyles(styles))
            }
        }

        deserializer.deserialize_map(StylesVisitor)
    }
}

// --- building ----------------------------------------------------------------

/// Why `theme.toml` is refused (the binary puts `<path>: ` in front);
/// `index` is the entry's, from 0.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ThemeError {
    #[error("themes[{index}]: unknown field \"{field}\"")]
    UnknownField { index: usize, field: String },
    #[error("themes[{index}]: missing name")]
    MissingName { index: usize },
    #[error("themes[{index}]: empty name")]
    EmptyName { index: usize },
    #[error("themes[{index}]: \"{name}\" is already themes[{first}]")]
    DuplicateName {
        index: usize,
        name: String,
        first: usize,
    },
    #[error("themes[{index}].palette: unknown colour \"{key}\"")]
    UnknownPaletteColor { index: usize, key: String },
    #[error("themes[{index}].palette.{key}: invalid colour \"{value}\"")]
    InvalidPaletteColor {
        index: usize,
        key: String,
        value: String,
    },
    #[error("themes[{index}].component_style: unknown style \"{name}\"")]
    UnknownStyle { index: usize, name: String },
    #[error(
        "themes[{index}].component_style.{style}: unknown field \"{field}\", expected fg, bg or modifiers"
    )]
    UnknownStyleField {
        index: usize,
        style: String,
        field: String,
    },
    #[error(
        "themes[{index}].component_style.{style}.{field}: invalid colour \"{value}\" (a palette name such as BrightBlack, or #rrggbb)"
    )]
    InvalidStyleColor {
        index: usize,
        style: String,
        /// `fg` or `bg`.
        field: &'static str,
        value: String,
    },
    #[error("themes[{index}].component_style.{style}: unknown modifier \"{name}\"")]
    UnknownModifier {
        index: usize,
        style: String,
        name: String,
    },
}

/// The theme list from `file`'s entries (spec 0017 "`theme.toml`"): the
/// built-in `default` first (replaced in place by an entry of that name),
/// then the entries in file order; what an entry leaves out keeps the
/// default (decision 1).
pub fn build(file: &ThemeFile) -> Result<Themes, ThemeError> {
    let mut themes = Themes::default();
    let mut names: Vec<&str> = Vec::new();
    for (index, entry) in file.themes.iter().enumerate() {
        if let Some(field) = entry.unknown.first() {
            return Err(ThemeError::UnknownField {
                index,
                field: field.clone(),
            });
        }
        let name = entry
            .name
            .as_deref()
            .ok_or(ThemeError::MissingName { index })?;
        if name.is_empty() {
            return Err(ThemeError::EmptyName { index });
        }
        if let Some(first) = names.iter().position(|n| *n == name) {
            return Err(ThemeError::DuplicateName {
                index,
                name: name.to_owned(),
                first,
            });
        }
        names.push(name);
        let palette = palette(index, &entry.palette)?;
        let mut specs = COMPONENTS.map(Component::default_style);
        for (style, value) in &entry.component_style {
            if let Some(component) = Component::from_name(style) {
                let value = value.clone().unwrap_or_default();
                specs[component as usize] = spec(index, style, &value)?;
            } else if SKIPPED_COMPONENTS.contains(&style.as_str()) {
                if !themes.skipped.contains(style) {
                    themes.skipped.push(style.clone());
                }
            } else {
                return Err(ThemeError::UnknownStyle {
                    index,
                    name: style.clone(),
                });
            }
        }
        let theme = Theme::resolve(name.to_owned(), &palette, &specs);
        if name == DEFAULT_THEME {
            themes.themes[0] = theme;
        } else {
            themes.themes.push(theme);
        }
    }
    Ok(themes)
}

/// Entry `index`'s palette: the terminal's colours with `entries` set.
fn palette(index: usize, entries: &[(String, String)]) -> Result<Palette, ThemeError> {
    let mut palette = Palette::default();
    for (key, value) in entries {
        let color = || {
            parse_palette_color(value).ok_or_else(|| ThemeError::InvalidPaletteColor {
                index,
                key: key.clone(),
                value: value.clone(),
            })
        };
        match key.as_str() {
            "background" => palette.background = Some(color()?),
            "foreground" => palette.foreground = Some(color()?),
            _ => {
                let name = PALETTE_COLORS
                    .into_iter()
                    .find(|c| c.key() == key)
                    .ok_or_else(|| ThemeError::UnknownPaletteColor {
                        index,
                        key: key.clone(),
                    })?;
                palette.colors[name as usize] = color()?;
            }
        }
    }
    Ok(palette)
}

/// Entry `index`'s style of `style`, as written.
fn spec(index: usize, style: &str, entry: &StyleEntry) -> Result<StyleSpec, ThemeError> {
    if let Some(field) = entry.unknown.first() {
        return Err(ThemeError::UnknownStyleField {
            index,
            style: style.to_owned(),
            field: field.clone(),
        });
    }
    let color = |field: &'static str, value: &Option<String>| {
        value
            .as_deref()
            .map(|value| {
                parse_style_color(value).ok_or_else(|| ThemeError::InvalidStyleColor {
                    index,
                    style: style.to_owned(),
                    field,
                    value: value.to_owned(),
                })
            })
            .transpose()
    };
    let mut modifiers = Modifiers::NONE;
    for name in &entry.modifiers {
        modifiers = modifiers
            | parse_modifier(name).ok_or_else(|| ThemeError::UnknownModifier {
                index,
                style: style.to_owned(),
                name: name.clone(),
            })?;
    }
    Ok(StyleSpec {
        fg: color("fg", &entry.fg)?,
        bg: color("bg", &entry.bg)?,
        modifiers,
    })
}

// --- the Themes popup (`T`) --------------------------------------------------

/// Opens the Themes popup with the cursor on the theme in use.
pub(super) fn open(state: &mut State) -> Vec<Effect> {
    state.popup = Some(Popup::Themes {
        cursor: state.theme,
        active: state.theme,
    });
    Vec::new()
}

/// A command while the Themes popup is open: the list commands move the
/// cursor and preview the theme under it; `ChooseSelected` keeps it,
/// `ClosePopup` goes back to the one in use before; nothing else acts.
pub(super) fn popup_command(state: &mut State, command: UiCommand) -> Vec<Effect> {
    let Some(Popup::Themes { cursor, active }) = state.popup else {
        return Vec::new();
    };
    match command {
        UiCommand::ChooseSelected => state.popup = None,
        UiCommand::ClosePopup => cancel(state),
        _ if command.is_list_move() => {
            let cursor = step(
                command,
                cursor,
                state.themes.list().len(),
                state.list_height,
            );
            // Spec 0017 addendum 67055c40c1e2.
            if let Some(envy) = state.themes.list()[cursor].envy() {
                return vec![Effect::Envy(envy)];
            }
            state.theme = cursor;
            state.popup = Some(Popup::Themes { cursor, active });
        }
        _ => {}
    }
    Vec::new()
}

/// `q` in the Themes popup: back to the theme in use before, closed.
pub(super) fn cancel(state: &mut State) {
    if let Some(Popup::Themes { active, .. }) = state.popup {
        state.theme = active;
    }
    state.popup = None;
}

#[cfg(test)]
mod tests;
