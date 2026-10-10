//! The keymap (spec 0008 "`keymap.toml`", "Commands and their default
//! keys"): spotify-player's key syntax, the UI commands and the actions a
//! key can be bound to, the default bindings, and building the keymap from
//! the file's entries (already deserialised: no TOML here).
//!
//! Dispatch (which command acts where) lives in [`super`]; this module only
//! says which binding a sequence of keys names.

use std::collections::BTreeMap;
use std::fmt;

use serde::Deserialize;
use serde::de::{self, IgnoredAny, MapAccess, Visitor};

use super::Key;

/// A key without a modifier, as `M-` and `C-` (on a named key) take it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BaseKey {
    Char(char),
    Enter,
    Esc,
    Backspace,
    Up,
    Down,
    Left,
    Right,
    Tab,
    BackTab,
    PageUp,
    PageDown,
    Home,
    End,
    Insert,
    Delete,
    /// `f1`–`f12`.
    F(u8),
}

impl BaseKey {
    /// The key pressed alone.
    pub fn key(self) -> Key {
        match self {
            Self::Char(c) => Key::Char(c),
            Self::Enter => Key::Enter,
            Self::Esc => Key::Esc,
            Self::Backspace => Key::Backspace,
            Self::Up => Key::Up,
            Self::Down => Key::Down,
            Self::Left => Key::Left,
            Self::Right => Key::Right,
            Self::Tab => Key::Tab,
            Self::BackTab => Key::BackTab,
            Self::PageUp => Key::PageUp,
            Self::PageDown => Key::PageDown,
            Self::Home => Key::Home,
            Self::End => Key::End,
            Self::Insert => Key::Insert,
            Self::Delete => Key::Delete,
            Self::F(n) => Key::F(n),
        }
    }

    /// A key name of the file's syntax (`enter`, `f5`), or a single
    /// non-space character.
    fn parse(text: &str) -> Option<Self> {
        let mut chars = text.chars();
        if let (Some(c), None) = (chars.next(), chars.next()) {
            return (c != ' ').then_some(Self::Char(c));
        }
        Some(match text {
            "enter" => Self::Enter,
            "space" => Self::Char(' '),
            "tab" => Self::Tab,
            "backtab" => Self::BackTab,
            "backspace" => Self::Backspace,
            "esc" => Self::Esc,
            "left" => Self::Left,
            "right" => Self::Right,
            "up" => Self::Up,
            "down" => Self::Down,
            "insert" => Self::Insert,
            "delete" => Self::Delete,
            "home" => Self::Home,
            "end" => Self::End,
            "page_up" => Self::PageUp,
            "page_down" => Self::PageDown,
            _ => {
                let n: u8 = text.strip_prefix('f')?.parse().ok()?;
                // `f01` is not a name.
                if !(1..=12).contains(&n) || text != format!("f{n}") {
                    return None;
                }
                Self::F(n)
            }
        })
    }
}

impl fmt::Display for BaseKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Char(' ') => "space",
            Self::Char(c) => return write!(f, "{c}"),
            Self::Enter => "enter",
            Self::Esc => "esc",
            Self::Backspace => "backspace",
            Self::Up => "up",
            Self::Down => "down",
            Self::Left => "left",
            Self::Right => "right",
            Self::Tab => "tab",
            Self::BackTab => "backtab",
            Self::PageUp => "page_up",
            Self::PageDown => "page_down",
            Self::Home => "home",
            Self::End => "end",
            Self::Insert => "insert",
            Self::Delete => "delete",
            Self::F(n) => return write!(f, "f{n}"),
        };
        f.write_str(name)
    }
}

impl Key {
    /// The key pressed alone, if it has no modifier.
    pub fn base(self) -> Option<BaseKey> {
        Some(match self {
            Self::Char(c) => BaseKey::Char(c),
            Self::Enter => BaseKey::Enter,
            Self::Esc => BaseKey::Esc,
            Self::Backspace => BaseKey::Backspace,
            Self::Up => BaseKey::Up,
            Self::Down => BaseKey::Down,
            Self::Left => BaseKey::Left,
            Self::Right => BaseKey::Right,
            Self::Tab => BaseKey::Tab,
            Self::BackTab => BaseKey::BackTab,
            Self::PageUp => BaseKey::PageUp,
            Self::PageDown => BaseKey::PageDown,
            Self::Home => BaseKey::Home,
            Self::End => BaseKey::End,
            Self::Insert => BaseKey::Insert,
            Self::Delete => BaseKey::Delete,
            Self::F(n) => BaseKey::F(n),
            Self::Ctrl(_) | Self::Alt(_) | Self::CtrlKey(_) => return None,
        })
    }

    /// Control + `base`: a character as [`Key::Ctrl`] (lower case:
    /// terminals report Control-letters without case), a named key as
    /// [`Key::CtrlKey`].
    pub fn ctrl(base: BaseKey) -> Self {
        match base {
            BaseKey::Char(c) => Self::Ctrl(c.to_ascii_lowercase()),
            named => Self::CtrlKey(named),
        }
    }

    /// One key of the file's syntax: a name or a character, optionally
    /// after `C-` or `M-`; `None` if it is none of these.
    pub fn parse(text: &str) -> Option<Self> {
        if let Some(rest) = text.strip_prefix("C-") {
            return BaseKey::parse(rest).map(Self::ctrl);
        }
        if let Some(rest) = text.strip_prefix("M-") {
            return BaseKey::parse(rest).map(Self::Alt);
        }
        BaseKey::parse(text).map(BaseKey::key)
    }
}

impl fmt::Display for Key {
    /// The key in the file's syntax (`C-s`, `M-enter`, `space`, `G`).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ctrl(c) => write!(f, "C-{}", BaseKey::Char(*c)),
            Self::CtrlKey(base) => write!(f, "C-{base}"),
            Self::Alt(base) => write!(f, "M-{base}"),
            plain => match plain.base() {
                Some(base) => write!(f, "{base}"),
                None => Ok(()),
            },
        }
    }
}

/// Keys pressed one after another (`g g`); never empty once parsed.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KeySequence(pub Vec<Key>);

/// Why a key sequence does not parse.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KeyError {
    #[error("empty key sequence")]
    Empty,
    #[error("unknown key \"{key}\" in \"{sequence}\"")]
    UnknownKey { key: String, sequence: String },
}

impl KeySequence {
    /// Keys separated by single spaces (spotify-player's syntax).
    pub fn parse(text: &str) -> Result<Self, KeyError> {
        if text.is_empty() {
            return Err(KeyError::Empty);
        }
        text.split(' ')
            .map(|key| {
                Key::parse(key).ok_or_else(|| KeyError::UnknownKey {
                    key: key.to_owned(),
                    sequence: text.to_owned(),
                })
            })
            .collect::<Result<Vec<_>, _>>()
            .map(Self)
    }

    pub fn keys(&self) -> &[Key] {
        &self.0
    }
}

impl fmt::Display for KeySequence {
    /// The label the help and the docs use: the file's syntax.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, key) in self.0.iter().enumerate() {
            if i > 0 {
                f.write_str(" ")?;
            }
            write!(f, "{key}")?;
        }
        Ok(())
    }
}

/// A command a key can be bound to (spec 0008 "Commands and their default
/// keys"); where it acts is decided by dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UiCommand {
    ResumePause,
    NextTrack,
    PreviousTrack,
    /// Seconds, 1–600; `None`: the configured seek step.
    SeekForward {
        duration: Option<u16>,
    },
    SeekBackward {
        duration: Option<u16>,
    },
    SeekStart,
    Shuffle,
    Repeat,
    ToggleAutoplay,
    VolumeUp,
    VolumeDown,
    /// Percent, −25…25, not 0.
    VolumeChange {
        offset: i8,
    },
    Mute,
    AddToQueuePrompt,
    PlayNextPrompt,
    SelectNextOrScrollDown,
    SelectPreviousOrScrollUp,
    PageSelectNextOrScrollDown,
    PageSelectPreviousOrScrollUp,
    SelectFirstOrScrollToTop,
    SelectLastOrScrollToBottom,
    ChooseSelected,
    AddSelectedItemToQueue,
    RemoveFromQueue,
    ShowActionsOnSelectedItem,
    ShowActionsOnCurrentTrack,
    FocusNextWindow,
    FocusPreviousWindow,
    NextTab,
    PreviousTab,
    RoleFilter,
    Queue,
    LibraryPage,
    LikedTrackPage,
    SearchPage,
    MixesPage,
    Search,
    PreviousPage,
    ClosePopup,
    OpenCommandHelp,
    Quit,
    /// The devices popup (spec 0014).
    SwitchDevice,
}

/// Every command, with its default parameters, in the table's order.
pub const COMMANDS: [UiCommand; 41] = [
    UiCommand::ResumePause,
    UiCommand::NextTrack,
    UiCommand::PreviousTrack,
    UiCommand::SeekForward { duration: None },
    UiCommand::SeekBackward { duration: None },
    UiCommand::SeekStart,
    UiCommand::Shuffle,
    UiCommand::Repeat,
    UiCommand::ToggleAutoplay,
    UiCommand::VolumeUp,
    UiCommand::VolumeDown,
    UiCommand::VolumeChange { offset: 1 },
    UiCommand::Mute,
    UiCommand::AddToQueuePrompt,
    UiCommand::PlayNextPrompt,
    UiCommand::SelectNextOrScrollDown,
    UiCommand::SelectPreviousOrScrollUp,
    UiCommand::PageSelectNextOrScrollDown,
    UiCommand::PageSelectPreviousOrScrollUp,
    UiCommand::SelectFirstOrScrollToTop,
    UiCommand::SelectLastOrScrollToBottom,
    UiCommand::ChooseSelected,
    UiCommand::AddSelectedItemToQueue,
    UiCommand::RemoveFromQueue,
    UiCommand::ShowActionsOnSelectedItem,
    UiCommand::ShowActionsOnCurrentTrack,
    UiCommand::FocusNextWindow,
    UiCommand::FocusPreviousWindow,
    UiCommand::NextTab,
    UiCommand::PreviousTab,
    UiCommand::RoleFilter,
    UiCommand::Queue,
    UiCommand::LibraryPage,
    UiCommand::LikedTrackPage,
    UiCommand::SearchPage,
    UiCommand::MixesPage,
    UiCommand::Search,
    UiCommand::PreviousPage,
    UiCommand::ClosePopup,
    UiCommand::OpenCommandHelp,
    UiCommand::Quit,
];

impl UiCommand {
    /// The name in `keymap.toml` (spotify-player's where it has one).
    pub fn name(self) -> &'static str {
        match self {
            Self::ResumePause => "ResumePause",
            Self::NextTrack => "NextTrack",
            Self::PreviousTrack => "PreviousTrack",
            Self::SeekForward { .. } => "SeekForward",
            Self::SeekBackward { .. } => "SeekBackward",
            Self::SeekStart => "SeekStart",
            Self::Shuffle => "Shuffle",
            Self::Repeat => "Repeat",
            Self::ToggleAutoplay => "ToggleAutoplay",
            Self::VolumeUp => "VolumeUp",
            Self::VolumeDown => "VolumeDown",
            Self::VolumeChange { .. } => "VolumeChange",
            Self::Mute => "Mute",
            Self::AddToQueuePrompt => "AddToQueuePrompt",
            Self::PlayNextPrompt => "PlayNextPrompt",
            Self::SelectNextOrScrollDown => "SelectNextOrScrollDown",
            Self::SelectPreviousOrScrollUp => "SelectPreviousOrScrollUp",
            Self::PageSelectNextOrScrollDown => "PageSelectNextOrScrollDown",
            Self::PageSelectPreviousOrScrollUp => "PageSelectPreviousOrScrollUp",
            Self::SelectFirstOrScrollToTop => "SelectFirstOrScrollToTop",
            Self::SelectLastOrScrollToBottom => "SelectLastOrScrollToBottom",
            Self::ChooseSelected => "ChooseSelected",
            Self::AddSelectedItemToQueue => "AddSelectedItemToQueue",
            Self::RemoveFromQueue => "RemoveFromQueue",
            Self::ShowActionsOnSelectedItem => "ShowActionsOnSelectedItem",
            Self::ShowActionsOnCurrentTrack => "ShowActionsOnCurrentTrack",
            Self::FocusNextWindow => "FocusNextWindow",
            Self::FocusPreviousWindow => "FocusPreviousWindow",
            Self::NextTab => "NextTab",
            Self::PreviousTab => "PreviousTab",
            Self::RoleFilter => "RoleFilter",
            Self::Queue => "Queue",
            Self::LibraryPage => "LibraryPage",
            Self::LikedTrackPage => "LikedTrackPage",
            Self::SearchPage => "SearchPage",
            Self::MixesPage => "MixesPage",
            Self::Search => "Search",
            Self::PreviousPage => "PreviousPage",
            Self::ClosePopup => "ClosePopup",
            Self::OpenCommandHelp => "OpenCommandHelp",
            Self::Quit => "Quit",
            Self::SwitchDevice => "SwitchDevice",
        }
    }

    /// Whether this is one of the list commands (they act in lists and
    /// popups).
    pub fn is_list_move(self) -> bool {
        matches!(
            self,
            Self::SelectNextOrScrollDown
                | Self::SelectPreviousOrScrollUp
                | Self::PageSelectNextOrScrollDown
                | Self::PageSelectPreviousOrScrollUp
                | Self::SelectFirstOrScrollToTop
                | Self::SelectLastOrScrollToBottom
        )
    }

    /// The command named `name` with `params` (the inline table's, `None`
    /// for a plain name); `Ok(None)` when no command has that name.
    fn from_name(
        name: &str,
        params: Option<&BTreeMap<String, i64>>,
    ) -> Result<Option<Self>, String> {
        let Some(command) = COMMANDS.iter().find(|c| c.name() == name).copied() else {
            return Ok(None);
        };
        let empty = BTreeMap::new();
        let given = params.unwrap_or(&empty);
        let allowed: &[&str] = match command {
            Self::VolumeChange { .. } => &["offset"],
            Self::SeekForward { .. } | Self::SeekBackward { .. } => &["duration"],
            _ if params.is_some() => return Err(format!("{name} takes no parameters")),
            _ => &[],
        };
        if let Some(unknown) = given.keys().find(|k| !allowed.contains(&k.as_str())) {
            return Err(format!("unknown parameter \"{unknown}\" of {name}"));
        }
        Ok(Some(match command {
            Self::VolumeChange { .. } => {
                let offset = given
                    .get("offset")
                    .ok_or_else(|| format!("{name} needs an offset"))?;
                match i8::try_from(*offset) {
                    Ok(offset) if offset != 0 && (-25..=25).contains(&offset) => {
                        Self::VolumeChange { offset }
                    }
                    _ => {
                        return Err(format!(
                            "{name} offset must be from -25 to 25 and not 0, got {offset}"
                        ));
                    }
                }
            }
            Self::SeekForward { .. } | Self::SeekBackward { .. } => {
                let duration = match given.get("duration") {
                    None => None,
                    Some(d) => match u16::try_from(*d) {
                        Ok(d) if (1..=600).contains(&d) => Some(d),
                        _ => {
                            return Err(format!(
                                "{name} duration must be from 1 to 600 seconds, got {d}"
                            ));
                        }
                    },
                };
                if matches!(command, Self::SeekForward { .. }) {
                    Self::SeekForward { duration }
                } else {
                    Self::SeekBackward { duration }
                }
            }
            other => other,
        }))
    }
}

/// An entry of the actions popup a key can run directly (`[[actions]]`,
/// spec 0006 "Actions").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActionKind {
    GoToAlbum,
    /// The first artist (the popup lists each).
    GoToArtist,
    /// The radio of the selected track or artist (spec 0011).
    GoToRadio,
    AddToQueue,
    PlayNext,
    AddToLiked,
    DeleteFromLiked,
    AddToPlaylist,
    DeleteFromPlaylist,
    RemoveFromQueue,
    DeletePlaylist,
}

/// Every action, in the spec's order.
pub const ACTIONS: [ActionKind; 11] = [
    ActionKind::GoToAlbum,
    ActionKind::GoToArtist,
    ActionKind::GoToRadio,
    ActionKind::AddToQueue,
    ActionKind::PlayNext,
    ActionKind::AddToLiked,
    ActionKind::DeleteFromLiked,
    ActionKind::AddToPlaylist,
    ActionKind::DeleteFromPlaylist,
    ActionKind::RemoveFromQueue,
    ActionKind::DeletePlaylist,
];

impl ActionKind {
    /// The name in `keymap.toml`.
    pub fn name(self) -> &'static str {
        match self {
            Self::GoToAlbum => "GoToAlbum",
            Self::GoToArtist => "GoToArtist",
            Self::GoToRadio => "GoToRadio",
            Self::AddToQueue => "AddToQueue",
            Self::PlayNext => "PlayNext",
            Self::AddToLiked => "AddToLiked",
            Self::DeleteFromLiked => "DeleteFromLiked",
            Self::AddToPlaylist => "AddToPlaylist",
            Self::DeleteFromPlaylist => "DeleteFromPlaylist",
            Self::RemoveFromQueue => "RemoveFromQueue",
            Self::DeletePlaylist => "DeletePlaylist",
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        ACTIONS.iter().find(|a| a.name() == name).copied()
    }
}

/// What an `[[actions]]` binding acts on.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Deserialize)]
pub enum Target {
    /// The selected row (the queue entry under the cursor on the queue).
    #[default]
    SelectedItem,
    /// The playing track (what `a` opens the popup on).
    PlayingTrack,
}

/// An `[[actions]]` binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ActionBinding {
    pub action: ActionKind,
    pub target: Target,
}

/// What a key sequence is bound to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Binding {
    Command(UiCommand),
    Action(ActionBinding),
}

impl Binding {
    /// The command's or the action's name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Command(command) => command.name(),
            Self::Action(binding) => binding.action.name(),
        }
    }
}

/// spotify-player commands without a counterpart here: skipped with a
/// notice (spec 0008 decision 3).
pub const UNSUPPORTED_COMMANDS: [&str; 29] = [
    "PlayRandom",
    "RefreshPlayback",
    "RestartIntegratedClient",
    "SwitchTheme",
    "SwitchDevice",
    "ShowActionsOnCurrentContext",
    "JumpToHighlightTrackInContext",
    "JumpToCurrentTrackInContext",
    "BrowseUserPlaylists",
    "BrowseUserFollowedArtists",
    "BrowseUserSavedAlbums",
    "CurrentlyPlayingContextPage",
    "TopTrackPage",
    "RecentlyPlayedTrackPage",
    "LyricsPage",
    "BrowsePage",
    "OpenSpotifyLinkFromClipboard",
    "SortTrackByTitle",
    "SortTrackByArtists",
    "SortTrackByAlbum",
    "SortTrackByDuration",
    "SortTrackByAddedDate",
    "ReverseTrackOrder",
    "SortLibraryAlphabetically",
    "SortLibraryByRecent",
    "MovePlaylistItemUp",
    "MovePlaylistItemDown",
    "CreatePlaylist",
    "OpenLogs",
];

/// spotify-player actions without a counterpart here: skipped with a
/// notice.
pub const UNSUPPORTED_ACTIONS: [&str; 10] = [
    "GoToShow",
    "AddToLibrary",
    "DeleteFromLibrary",
    "ShowActionsOnAlbum",
    "ShowActionsOnArtist",
    "ShowActionsOnShow",
    "ToggleLiked",
    "CopyLink",
    "Follow",
    "Unfollow",
];

/// `command = "None"`: removes whatever the sequence is bound to.
pub const NONE: &str = "None";

// --- the file's entries ------------------------------------------------------

/// `keymap.toml`, deserialised.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeymapFile {
    #[serde(default)]
    pub keymaps: Vec<KeymapEntry>,
    #[serde(default)]
    pub actions: Vec<ActionEntry>,
}

/// A `[[keymaps]]` entry.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeymapEntry {
    pub command: CommandEntry,
    pub key_sequence: String,
}

/// An `[[actions]]` entry.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionEntry {
    pub action: String,
    pub key_sequence: String,
    #[serde(default)]
    pub target: Target,
}

/// A `[[keymaps]]` entry's `command`, as written: a name, or a one-entry
/// table of a name and its parameters (`{ VolumeChange = { offset = 1 } }`).
/// Validated by [`build`], so an unknown name gets its entry's index and a
/// spotify-player name becomes a notice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandEntry {
    pub name: String,
    /// The parameters, when written as a table. A table's values are kept
    /// only for the commands that take parameters; any other name's are
    /// skipped (left empty), whatever they hold.
    pub params: Option<BTreeMap<String, i64>>,
}

impl CommandEntry {
    /// A plain name.
    pub fn name(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            params: None,
        }
    }
}

impl<'de> Deserialize<'de> for CommandEntry {
    fn deserialize<D: de::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct CommandVisitor;

        impl<'de> Visitor<'de> for CommandVisitor {
            type Value = CommandEntry;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(
                    "a command name or a one-entry table such as { VolumeChange = { offset = 1 } }",
                )
            }

            fn visit_str<E: de::Error>(self, name: &str) -> Result<CommandEntry, E> {
                Ok(CommandEntry::name(name))
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<CommandEntry, A::Error> {
                let Some(name) = map.next_key::<String>()? else {
                    return Err(de::Error::invalid_length(0, &self));
                };
                let takes_params = COMMANDS
                    .iter()
                    .any(|c| c.name() == name && Self::takes_params(*c));
                let params = if takes_params {
                    map.next_value::<BTreeMap<String, i64>>()?
                } else {
                    map.next_value::<IgnoredAny>()?;
                    BTreeMap::new()
                };
                if map.next_key::<IgnoredAny>()?.is_some() {
                    return Err(de::Error::invalid_length(2, &self));
                }
                Ok(CommandEntry {
                    name,
                    params: Some(params),
                })
            }
        }

        impl CommandVisitor {
            fn takes_params(command: UiCommand) -> bool {
                matches!(
                    command,
                    UiCommand::VolumeChange { .. }
                        | UiCommand::SeekForward { .. }
                        | UiCommand::SeekBackward { .. }
                )
            }
        }

        deserializer.deserialize_any(CommandVisitor)
    }
}

// --- building ----------------------------------------------------------------

/// Which list of the file an entry is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Keymaps,
    Actions,
}

/// An entry of the file: `keymaps[3]` (indices from 0, as written in the
/// messages).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntryRef {
    pub section: Section,
    pub index: usize,
}

impl fmt::Display for EntryRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let section = match self.section {
            Section::Keymaps => "keymaps",
            Section::Actions => "actions",
        };
        write!(f, "{section}[{}]", self.index)
    }
}

/// Why `keymap.toml` is refused (the binary puts `<path>: ` in front).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KeymapError {
    #[error("{entry}: {error}")]
    Key { entry: EntryRef, error: KeyError },
    #[error("{entry}: unknown command \"{name}\"")]
    UnknownCommand { entry: EntryRef, name: String },
    #[error("{entry}: unknown action \"{name}\"")]
    UnknownAction { entry: EntryRef, name: String },
    /// A parameter out of range, missing, unknown or given to a command
    /// without parameters.
    #[error("{entry}: {message}")]
    Parameter { entry: EntryRef, message: String },
    /// `prefix` is bound and starts each of `longer` (sequence, binding
    /// name).
    #[error(
        "\"{prefix}\" is bound to {bound_to} and is the start of {}; unbind those with command = \"None\" first",
        longer.iter().map(|(s, b)| format!("\"{s}\" ({b})")).collect::<Vec<_>>().join(", ")
    )]
    PrefixConflict {
        prefix: String,
        bound_to: String,
        longer: Vec<(String, String)>,
    },
    #[error("Quit has no key left")]
    NoQuitKey,
}

/// The effective bindings: the defaults with the file's entries merged in,
/// and the spotify-player names the file used that this player does not
/// have.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keymap {
    bindings: Vec<(KeySequence, Binding)>,
    unsupported: Vec<String>,
}

impl Default for Keymap {
    /// The defaults of spec 0008 "Commands and their default keys".
    fn default() -> Self {
        Self {
            bindings: defaults(),
            unsupported: Vec::new(),
        }
    }
}

impl Keymap {
    /// Every binding, the defaults' order first, then the file's additions.
    pub fn bindings(&self) -> &[(KeySequence, Binding)] {
        &self.bindings
    }

    /// The skipped spotify-player commands and actions, in file order,
    /// each once.
    pub fn unsupported(&self) -> &[String] {
        &self.unsupported
    }

    /// The binding of exactly `keys`.
    pub fn get(&self, keys: &[Key]) -> Option<Binding> {
        self.bindings
            .iter()
            .find(|(s, _)| s.0 == keys)
            .map(|(_, b)| *b)
    }

    /// Whether `keys` is the start of a longer binding.
    pub fn starts(&self, keys: &[Key]) -> bool {
        self.bindings
            .iter()
            .any(|(s, _)| s.0.len() > keys.len() && s.0.starts_with(keys))
    }

    /// The sequences bound to `binding`, in order.
    pub fn sequences(&self, binding: Binding) -> Vec<&KeySequence> {
        self.bindings
            .iter()
            .filter(|(_, b)| *b == binding)
            .map(|(s, _)| s)
            .collect()
    }

    /// The message row's line about the skipped names (spec 0008
    /// decision 3), if there are any.
    pub fn notice(&self) -> Option<String> {
        let names = &self.unsupported;
        match names.len() {
            0 => None,
            1 => Some(format!(
                "keymap.toml: 1 spotify-player command not supported here: {}",
                names[0]
            )),
            n => Some(format!(
                "keymap.toml: {n} spotify-player commands not supported here: {}",
                names.join(", ")
            )),
        }
    }

    /// Binds `sequence`, replacing its binding (in place) if it has one.
    fn bind(&mut self, sequence: KeySequence, binding: Binding) {
        match self.bindings.iter_mut().find(|(s, _)| *s == sequence) {
            Some(slot) => slot.1 = binding,
            None => self.bindings.push((sequence, binding)),
        }
    }

    fn unbind(&mut self, sequence: &KeySequence) {
        self.bindings.retain(|(s, _)| s != sequence);
    }

    fn skip(&mut self, name: &str) {
        if !self.unsupported.iter().any(|n| n == name) {
            self.unsupported.push(name.to_owned());
        }
    }
}

/// The default bindings, in the order of the spec's table.
pub fn defaults() -> Vec<(KeySequence, Binding)> {
    use BaseKey as B;
    use Key::{Char, Ctrl};
    use UiCommand as C;
    let one = |key: Key| KeySequence(vec![key]);
    let g = |c: char| KeySequence(vec![Char('g'), Char(c)]);
    let table: Vec<(Vec<KeySequence>, UiCommand)> = vec![
        (vec![one(Char(' '))], C::ResumePause),
        (vec![one(Char('n'))], C::NextTrack),
        (vec![one(Char('p'))], C::PreviousTrack),
        (vec![one(Char('>'))], C::SeekForward { duration: None }),
        (vec![one(Char('<'))], C::SeekBackward { duration: None }),
        (vec![one(Char('^'))], C::SeekStart),
        (vec![one(Ctrl('s'))], C::Shuffle),
        (vec![one(Ctrl('r'))], C::Repeat),
        (vec![one(Char('A'))], C::ToggleAutoplay),
        (vec![one(Char('+'))], C::VolumeUp),
        (vec![one(Char('-'))], C::VolumeDown),
        (vec![one(Char('_'))], C::Mute),
        (vec![one(Char('o'))], C::AddToQueuePrompt),
        (vec![one(Char('O'))], C::PlayNextPrompt),
        (
            vec![one(Char('j')), one(Key::Down), one(Ctrl('n'))],
            C::SelectNextOrScrollDown,
        ),
        (
            vec![one(Char('k')), one(Key::Up), one(Ctrl('p'))],
            C::SelectPreviousOrScrollUp,
        ),
        (
            vec![one(Ctrl('f')), one(Key::PageDown)],
            C::PageSelectNextOrScrollDown,
        ),
        (
            vec![one(Ctrl('b')), one(Key::PageUp)],
            C::PageSelectPreviousOrScrollUp,
        ),
        (vec![g('g')], C::SelectFirstOrScrollToTop),
        (
            vec![one(Char('G')), one(B::End.key())],
            C::SelectLastOrScrollToBottom,
        ),
        (vec![one(Key::Enter)], C::ChooseSelected),
        (
            vec![one(Char('Z')), one(Ctrl('z'))],
            C::AddSelectedItemToQueue,
        ),
        (vec![one(Char('d'))], C::RemoveFromQueue),
        (vec![g('a'), one(Ctrl(' '))], C::ShowActionsOnSelectedItem),
        (vec![one(Char('a'))], C::ShowActionsOnCurrentTrack),
        (vec![one(Key::Tab)], C::FocusNextWindow),
        (vec![one(Key::BackTab)], C::FocusPreviousWindow),
        (vec![one(Char(']'))], C::NextTab),
        (vec![one(Char('['))], C::PreviousTab),
        (vec![one(Char('f'))], C::RoleFilter),
        (vec![one(Char('z'))], C::Queue),
        (vec![g('l')], C::LibraryPage),
        (vec![g('y')], C::LikedTrackPage),
        (vec![g('s')], C::SearchPage),
        (vec![g('m')], C::MixesPage),
        (vec![one(Char('/'))], C::Search),
        (vec![one(Key::Backspace), one(Ctrl('q'))], C::PreviousPage),
        (vec![one(Key::Esc)], C::ClosePopup),
        (vec![one(Char('?')), one(Ctrl('h'))], C::OpenCommandHelp),
        (vec![one(Char('q')), one(Ctrl('c'))], C::Quit),
    ];
    // The default `[[actions]]` bindings (spec 0011): `r`, the radio of
    // the selected track or artist.
    let actions = [(
        vec![one(Char('r'))],
        ActionBinding {
            action: ActionKind::GoToRadio,
            target: Target::SelectedItem,
        },
    )];
    table
        .into_iter()
        .flat_map(|(sequences, command)| {
            sequences
                .into_iter()
                .map(move |s| (s, Binding::Command(command)))
        })
        .chain(actions.into_iter().flat_map(|(sequences, action)| {
            sequences
                .into_iter()
                .map(move |s| (s, Binding::Action(action)))
        }))
        .collect()
}

/// The keymap: the defaults with `file`'s entries merged in (spec 0008
/// "`keymap.toml`"): an entry replaces the binding of its sequence only,
/// `None` removes it, later entries beat earlier ones (`[[keymaps]]`, then
/// `[[actions]]`); spotify-player names without a counterpart are skipped
/// and listed in [`Keymap::unsupported`].
pub fn build(file: &KeymapFile) -> Result<Keymap, KeymapError> {
    let mut keymap = Keymap::default();
    for (index, entry) in file.keymaps.iter().enumerate() {
        let at = EntryRef {
            section: Section::Keymaps,
            index,
        };
        let sequence = KeySequence::parse(&entry.key_sequence)
            .map_err(|error| KeymapError::Key { entry: at, error })?;
        let name = entry.command.name.as_str();
        if name == NONE {
            keymap.unbind(&sequence);
            continue;
        }
        if UNSUPPORTED_COMMANDS.contains(&name) {
            keymap.skip(name);
            continue;
        }
        let command = UiCommand::from_name(name, entry.command.params.as_ref())
            .map_err(|message| KeymapError::Parameter { entry: at, message })?
            .ok_or_else(|| KeymapError::UnknownCommand {
                entry: at,
                name: name.to_owned(),
            })?;
        keymap.bind(sequence, Binding::Command(command));
    }
    for (index, entry) in file.actions.iter().enumerate() {
        let at = EntryRef {
            section: Section::Actions,
            index,
        };
        let sequence = KeySequence::parse(&entry.key_sequence)
            .map_err(|error| KeymapError::Key { entry: at, error })?;
        let name = entry.action.as_str();
        if UNSUPPORTED_ACTIONS.contains(&name) {
            keymap.skip(name);
            continue;
        }
        let action = ActionKind::from_name(name).ok_or_else(|| KeymapError::UnknownAction {
            entry: at,
            name: name.to_owned(),
        })?;
        keymap.bind(
            sequence,
            Binding::Action(ActionBinding {
                action,
                target: entry.target,
            }),
        );
    }
    check_prefixes(&keymap)?;
    if keymap
        .sequences(Binding::Command(UiCommand::Quit))
        .is_empty()
    {
        return Err(KeymapError::NoQuitKey);
    }
    Ok(keymap)
}

/// A bound sequence that starts a longer bound one could never be finished
/// (spec 0008 decision 4): the first such, with every sequence it starts.
fn check_prefixes(keymap: &Keymap) -> Result<(), KeymapError> {
    for (prefix, binding) in &keymap.bindings {
        let longer: Vec<(String, String)> = keymap
            .bindings
            .iter()
            .filter(|(s, _)| s.0.len() > prefix.0.len() && s.0.starts_with(&prefix.0))
            .map(|(s, b)| (s.to_string(), b.name().to_owned()))
            .collect();
        if !longer.is_empty() {
            return Err(KeymapError::PrefixConflict {
                prefix: prefix.to_string(),
                bound_to: binding.name().to_owned(),
                longer,
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
