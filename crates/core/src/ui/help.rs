//! The keys help (spec 0008 "The keys help", AC7 and AC8): the sections of
//! the keys that act where the user is, generated from the keymap, and the
//! help popup's keys (move, filter, run, close). Pure model; the rendering
//! reads [`visible`], [`Help`], [`locate`] and [`no_match`].
//!
//! Which section a command is listed in (spec 0008 "Commands and their
//! default keys", "Acts in"):
//!
//! - the **window's** section (`Queue`, `Library · Albums`, `Search ·
//!   Tracks`, …, or the open popup's): the commands that act on the rows of
//!   that window, with a text that names what they do there
//!   (`ChooseSelected`, `AddSelectedItemToQueue`, `RemoveFromQueue`,
//!   `ShowActionsOnSelectedItem`, `RoleFilter`, `Search`, `ClosePopup`);
//! - `Lists`: the six list-moving commands, on pages with a list and in
//!   the popups that have one;
//! - `Pages`: the page commands (`Queue`, `LibraryPage`, `LikedTrackPage`,
//!   `SearchPage`, `MixesPage`, `PreviousPage`), the focus commands where the page has
//!   more than one pane and the tab commands where a pane has tabs;
//! - `Playback`: playback, seek, volume, modes, the devices popup
//!   (`SwitchDevice`, spec 0014), the two prompts and
//!   `ShowActionsOnCurrentTrack`;
//! - `Actions`: the `[[actions]]` bindings;
//! - `App`: `OpenCommandHelp` and `Quit`.
//!
//! A key that does nothing where the help was opened is not listed (over a
//! popup only the popup's own keys act, so only its section, `Lists` and
//! `OpenCommandHelp` are listed; in a text input no keymap key acts, so
//! the section lists the input's fixed keys). A row whose binding would do
//! nothing right now is dim, not hidden.

use super::dispatch;
use super::keymap::{ActionKind, Binding, COMMANDS, KeySequence, Target, UiCommand};
use super::page::{Page, WindowKind};
use super::popup::Popup;
use super::search::SearchFocus;
use super::{Connection, Effect, Key, PageKind, State};

/// The help popup's state: the filter and the highlighted row. What it
/// lists is computed from the rest of the [`State`] ([`help`],
/// [`visible`]); the popup or page it was opened over stays in the state,
/// untouched.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Help {
    /// What the filter holds.
    pub filter: String,
    /// Whether typed characters go to the filter (after `/`, until
    /// `Enter` or `Esc`).
    pub typing: bool,
    /// The highlighted binding row, counted over the rows [`visible`]
    /// returns (headings are not rows).
    pub cursor: usize,
}

/// One row: the keys of a binding and what it does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelpRow {
    /// The binding's key sequences in the file's syntax, two spaces apart
    /// (`j  down  C-n`).
    pub keys: String,
    /// The command's or action's name; empty for a fixed key.
    pub command: String,
    /// The help text.
    pub text: String,
    /// Whether the binding would do nothing right now.
    pub dim: bool,
    /// What the keys are bound to; `None` for a fixed key (a text input's
    /// editing keys, a question's `y`/`n`), which `Enter` cannot run.
    pub binding: Option<Binding>,
    /// The sequences bound to it; `Enter` presses the first.
    pub sequences: Vec<KeySequence>,
}

/// A titled group of rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelpSection {
    pub title: String,
    pub rows: Vec<HelpRow>,
}

/// Where the help was opened, for the window section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Where {
    /// A popup is open: only its keys act.
    Popup,
    /// The queue page.
    Queue,
    /// A list window of a browse or search page.
    Window(WindowKind),
    /// The search page's top hit row.
    TopHit,
    /// A text input (the search input).
    Input,
}

fn context(state: &State) -> Where {
    if state.popup.is_some() {
        return Where::Popup;
    }
    let page = state.page();
    if page.kind == PageKind::Queue {
        return Where::Queue;
    }
    match page.search.as_ref().map(|s| s.focus) {
        Some(SearchFocus::Input) => Where::Input,
        Some(SearchFocus::TopHit) => Where::TopHit,
        _ => page
            .windows
            .get(page.focus)
            .map_or(Where::Input, |w| Where::Window(w.kind)),
    }
}

/// The sections of the help for `state`, unfiltered: the open popup's, or
/// the focused window's; `Lists`, `Pages`, `Playback`, `Actions`, `App`
/// where they apply (AC7).
pub fn help(state: &State) -> Vec<HelpSection> {
    let at = context(state);
    let page = state.page();
    let mut sections = Vec::new();
    let mut add = |title: &str, rows: Vec<HelpRow>| {
        if !rows.is_empty() {
            sections.push(HelpSection {
                title: title.to_owned(),
                rows,
            });
        }
    };
    match (at, state.popup.as_ref()) {
        (Where::Popup, Some(popup)) => {
            let mut rows = popup_fixed_rows(popup);
            rows.extend(command_rows(state, at, |c| popup_acts(popup, c)));
            if matches!(popup, Popup::Devices { .. }) {
                for row in &mut rows {
                    if row.binding == Some(Binding::Command(UiCommand::ChooseSelected)) {
                        row.text = "switch to the selected device".to_owned();
                    }
                }
            }
            add(popup_title(popup), rows);
            if popup_has_list(popup) {
                add("Lists", command_rows(state, at, UiCommand::is_list_move));
            }
            if popup_has_help(popup) {
                add(
                    "App",
                    command_rows(state, at, |c| c == UiCommand::OpenCommandHelp),
                );
            }
        }
        (Where::Popup | Where::Input, _) => add("Search · Input", input_rows()),
        (Where::Queue | Where::Window(_) | Where::TopHit, _) => {
            add(
                &window_title(at),
                command_rows(state, at, |c| in_window(at, c)),
            );
            if matches!(at, Where::Queue | Where::Window(_)) {
                add("Lists", command_rows(state, at, UiCommand::is_list_move));
            }
            add("Pages", command_rows(state, at, |c| in_pages(page, c)));
            add("Playback", command_rows(state, at, is_playback));
            add("Actions", action_rows(state, at));
            add("App", command_rows(state, at, is_app));
        }
    }
    sections
}

/// The sections [`help`] gives, with the rows the open help's filter keeps
/// (keys, command name or help text contain it, ignoring case); empty
/// sections disappear. Without an open help, [`help`].
pub fn visible(state: &State) -> Vec<HelpSection> {
    let sections = help(state);
    let Some(open) = &state.help else {
        return sections;
    };
    let needle = open.filter.to_lowercase();
    if needle.is_empty() {
        return sections;
    }
    sections
        .into_iter()
        .filter_map(|mut section| {
            section.rows.retain(|row| {
                [&row.keys, &row.command, &row.text]
                    .iter()
                    .any(|field| field.to_lowercase().contains(&needle))
            });
            (!section.rows.is_empty()).then_some(section)
        })
        .collect()
}

/// Where the highlighted row `cursor` (counted over rows only) is:
/// (section index, row index in it).
pub fn locate(sections: &[HelpSection], cursor: usize) -> Option<(usize, usize)> {
    let mut left = cursor;
    for (s, section) in sections.iter().enumerate() {
        if left < section.rows.len() {
            return Some((s, left));
        }
        left -= section.rows.len();
    }
    None
}

/// The filter, when it is set and keeps no row (`No keys match "xyz"`).
pub fn no_match(state: &State) -> Option<&str> {
    let open = state.help.as_ref()?;
    (!open.filter.is_empty() && visible(state).is_empty()).then_some(open.filter.as_str())
}

// --- key-sequence hints (spec 0013) ------------------------------------------

/// The hint for the pending keys of a sequence (spec 0013 "What it
/// lists"): content only, the TUI decides when to draw it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hints {
    /// The pending keys in the file's syntax (`g`, `s l`).
    pub prefix: String,
    /// One per distinct next key, in the keys help's order.
    pub entries: Vec<HintEntry>,
}

/// One next key and what it does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HintEntry {
    /// The next key in the file's syntax (`l`, `C-x`, `enter`).
    pub key: String,
    /// The binding's help text; empty for a nested prefix.
    pub text: String,
    /// Whether the binding would do nothing right now (for a nested
    /// prefix: whether everything it leads to would).
    pub dim: bool,
    /// For a nested prefix, how many listed bindings the key leads to;
    /// `0` for a key that completes a binding.
    pub more: usize,
}

/// The hint for `state`'s pending keys (spec 0013 AC1–AC3): `None` when
/// nothing is pending, the keys help or a whole-list load is open, hints
/// are off, or no binding under the pending keys acts in this view.
pub fn hints(state: &State) -> Option<Hints> {
    let pending = state.pending.as_slice();
    if !state.key_hints || pending.is_empty() || state.help.is_some() || state.whole_list.is_some()
    {
        return None;
    }
    // Per next key, in the help's order: the entry, and for a nested
    // prefix the rows it leads to (each counted once).
    let mut found: Vec<(Key, HintEntry, Vec<usize>)> = Vec::new();
    let rows = help(state).into_iter().flat_map(|section| section.rows);
    for (index, row) in rows.enumerate() {
        if row.binding.is_none() {
            continue;
        }
        for sequence in &row.sequences {
            let keys = sequence.keys();
            if keys.len() <= pending.len() || !keys.starts_with(pending) {
                continue;
            }
            let next = keys[pending.len()];
            let deeper = keys.len() > pending.len() + 1;
            match found.iter_mut().find(|(key, _, _)| *key == next) {
                Some((_, entry, leads)) => {
                    if deeper && !leads.contains(&index) {
                        leads.push(index);
                        entry.more = leads.len();
                        entry.dim &= row.dim;
                    }
                }
                None => {
                    let entry = HintEntry {
                        key: next.to_string(),
                        text: if deeper {
                            String::new()
                        } else {
                            row.text.clone()
                        },
                        dim: row.dim,
                        more: usize::from(deeper),
                    };
                    let leads = if deeper { vec![index] } else { Vec::new() };
                    found.push((next, entry, leads));
                }
            }
        }
    }
    if found.is_empty() {
        return None;
    }
    Some(Hints {
        prefix: KeySequence(pending.to_vec()).to_string(),
        entries: found.into_iter().map(|(_, entry, _)| entry).collect(),
    })
}

// --- what is listed where ----------------------------------------------------

fn is_playback(command: UiCommand) -> bool {
    use UiCommand as C;
    matches!(
        command,
        C::ResumePause
            | C::NextTrack
            | C::PreviousTrack
            | C::SeekForward { .. }
            | C::SeekBackward { .. }
            | C::SeekStart
            | C::Shuffle
            | C::Repeat
            | C::ToggleAutoplay
            | C::SwitchDevice
            | C::VolumeUp
            | C::VolumeDown
            | C::VolumeChange { .. }
            | C::Mute
            | C::AddToQueuePrompt
            | C::PlayNextPrompt
            | C::ShowActionsOnCurrentTrack
    )
}

fn is_app(command: UiCommand) -> bool {
    matches!(command, UiCommand::OpenCommandHelp | UiCommand::Quit)
}

/// The page commands; focus and tabs only where the page has something to
/// cycle.
fn in_pages(page: &Page, command: UiCommand) -> bool {
    use UiCommand as C;
    match command {
        C::Queue
        | C::LibraryPage
        | C::LikedTrackPage
        | C::SearchPage
        | C::MixesPage
        | C::PreviousPage => true,
        C::FocusNextWindow | C::FocusPreviousWindow => {
            page.panes.len() > 1 || page.search.is_some()
        }
        C::NextTab | C::PreviousTab => page.panes.iter().any(|pane| pane.len() > 1),
        _ => false,
    }
}

/// The commands of the focused window's section.
fn in_window(at: Where, command: UiCommand) -> bool {
    use UiCommand as C;
    match at {
        Where::Queue => matches!(
            command,
            C::ChooseSelected | C::RemoveFromQueue | C::ShowActionsOnSelectedItem
        ),
        Where::TopHit => matches!(
            command,
            C::ChooseSelected
                | C::AddSelectedItemToQueue
                | C::ShowActionsOnSelectedItem
                | C::Search
        ),
        Where::Window(kind) => match command {
            C::ChooseSelected | C::AddSelectedItemToQueue | C::ShowActionsOnSelectedItem => true,
            C::RoleFilter => kind == WindowKind::AllTracks,
            C::Search => is_search(kind),
            _ => false,
        },
        Where::Popup | Where::Input => false,
    }
}

fn is_search(kind: WindowKind) -> bool {
    matches!(
        kind,
        WindowKind::SearchTracks
            | WindowKind::SearchAlbums
            | WindowKind::SearchArtists
            | WindowKind::SearchPlaylists
    )
}

/// The commands that act in `popup` (spec 0008: the list commands,
/// `ChooseSelected`, `ClosePopup`, `OpenCommandHelp`; the questions and
/// prompts take their keys themselves).
fn popup_acts(popup: &Popup, command: UiCommand) -> bool {
    match popup {
        Popup::Actions { .. }
        | Popup::AddToPlaylist { .. }
        | Popup::Roles { .. }
        | Popup::Devices { .. } => {
            matches!(command, UiCommand::ChooseSelected | UiCommand::ClosePopup)
        }
        Popup::Confirm { .. } => command == UiCommand::ClosePopup,
        Popup::NewPlaylist { .. } => false,
    }
}

fn popup_has_list(popup: &Popup) -> bool {
    matches!(
        popup,
        Popup::Actions { .. }
            | Popup::AddToPlaylist { .. }
            | Popup::Roles { .. }
            | Popup::Devices { .. }
    )
}

/// Whether `?` opens the help over `popup` (the name prompt types it).
fn popup_has_help(popup: &Popup) -> bool {
    !matches!(popup, Popup::NewPlaylist { .. })
}

fn popup_title(popup: &Popup) -> &'static str {
    match popup {
        Popup::Actions { .. } => "Popup · Actions",
        Popup::AddToPlaylist { .. } => "Popup · Add to playlist",
        Popup::NewPlaylist { .. } => "Popup · New playlist",
        Popup::Confirm { .. } => "Popup · Confirm",
        Popup::Roles { .. } => "Popup · Role filter",
        Popup::Devices { .. } => "Popup · Devices",
    }
}

fn window_title(at: Where) -> String {
    match at {
        Where::Queue => "Queue".into(),
        Where::TopHit => "Search · Top hit".into(),
        Where::Window(kind) => match kind {
            WindowKind::Playlists | WindowKind::Albums | WindowKind::Artists => {
                format!("Library · {}", kind.name())
            }
            WindowKind::FavoriteTracks => "Favorite tracks".into(),
            WindowKind::AlbumTracks => "Album".into(),
            WindowKind::PlaylistTracks => "Playlist".into(),
            WindowKind::TopTracks
            | WindowKind::ArtistAlbums
            | WindowKind::AppearsOn
            | WindowKind::AllTracks => format!("Artist · {}", kind.name()),
            WindowKind::SearchTracks
            | WindowKind::SearchAlbums
            | WindowKind::SearchArtists
            | WindowKind::SearchPlaylists => format!("Search · {}", kind.name()),
            WindowKind::Mixes => "Mixes".into(),
            WindowKind::MixTracks => "Mix".into(),
            WindowKind::RadioTracks | WindowKind::ArtistRadioTracks => "Radio".into(),
        },
        Where::Popup | Where::Input => String::new(),
    }
}

// --- rows --------------------------------------------------------------------

/// The text input's own keys (spec 0007 "Input keys"), which the keymap
/// never sees.
fn input_rows() -> Vec<HelpRow> {
    [
        ("enter", "search"),
        ("tab  backtab", "next / previous pane"),
        ("esc", "to the results"),
        ("C-u", "clear the input"),
        ("C-q", "back"),
        ("C-c", "quit"),
    ]
    .into_iter()
    .map(|(keys, text)| fixed_row(keys, text))
    .collect()
}

/// The keys a popup takes before the keymap.
fn popup_fixed_rows(popup: &Popup) -> Vec<HelpRow> {
    let rows: &[(&str, &str)] = match popup {
        Popup::Roles { .. } => &[("space", "check / uncheck")],
        Popup::Confirm { .. } => &[("y", "yes"), ("n", "no")],
        Popup::NewPlaylist { .. } => &[("enter", "create the playlist"), ("esc", "cancel")],
        Popup::Devices { .. } => &[("r", "read the list again"), ("q", "close")],
        _ => &[],
    };
    rows.iter().map(|(k, t)| fixed_row(k, t)).collect()
}

fn fixed_row(keys: &str, text: &str) -> HelpRow {
    HelpRow {
        keys: keys.to_owned(),
        command: String::new(),
        text: text.to_owned(),
        dim: false,
        binding: None,
        sequences: Vec::new(),
    }
}

/// The keymap's bindings, grouped: each distinct binding once with its
/// sequences.
fn grouped(state: &State) -> Vec<(Binding, Vec<KeySequence>)> {
    let mut groups: Vec<(Binding, Vec<KeySequence>)> = Vec::new();
    for (sequence, binding) in state.keymap.bindings() {
        match groups.iter_mut().find(|(b, _)| b == binding) {
            Some((_, sequences)) => sequences.push(sequence.clone()),
            None => groups.push((*binding, vec![sequence.clone()])),
        }
    }
    groups
}

/// The command's place in the spec's table.
fn table_position(command: UiCommand) -> usize {
    COMMANDS
        .iter()
        .position(|c| c.name() == command.name())
        .unwrap_or(usize::MAX)
}

fn row(binding: Binding, sequences: Vec<KeySequence>, text: String, dim: bool) -> HelpRow {
    HelpRow {
        keys: sequences
            .iter()
            .map(KeySequence::to_string)
            .collect::<Vec<_>>()
            .join("  "),
        command: binding.name().to_owned(),
        text,
        dim,
        binding: Some(binding),
        sequences,
    }
}

/// The rows of the commands `wanted` selects, with their effective keys,
/// in the spec's table order; a command without a key has no row.
fn command_rows(state: &State, at: Where, wanted: impl Fn(UiCommand) -> bool) -> Vec<HelpRow> {
    let mut groups: Vec<(UiCommand, Vec<KeySequence>)> = grouped(state)
        .into_iter()
        .filter_map(|(binding, sequences)| match binding {
            Binding::Command(command) if wanted(command) => Some((command, sequences)),
            _ => None,
        })
        .collect();
    groups.sort_by_key(|(command, _)| table_position(*command));
    groups
        .into_iter()
        .map(|(command, sequences)| {
            row(
                Binding::Command(command),
                sequences,
                text(at, command),
                dim(state, at, command),
            )
        })
        .collect()
}

/// The `[[actions]]` bindings, in the file's order.
fn action_rows(state: &State, at: Where) -> Vec<HelpRow> {
    grouped(state)
        .into_iter()
        .filter_map(|(binding, sequences)| match binding {
            Binding::Action(action) => {
                let playing = action.target == Target::PlayingTrack;
                let mut text = action_text(action.action).to_owned();
                if playing {
                    text.push_str(" (playing track)");
                }
                let dim = if playing {
                    state.current().is_none()
                } else {
                    no_selection(state, at)
                };
                Some(row(binding, sequences, text, dim))
            }
            Binding::Command(_) => None,
        })
        .collect()
}

fn action_text(action: ActionKind) -> &'static str {
    match action {
        ActionKind::GoToAlbum => "go to the album",
        ActionKind::GoToArtist => "go to the artist",
        ActionKind::GoToRadio => "the radio of the selected track or artist",
        ActionKind::AddToQueue => "add to the end of the queue",
        ActionKind::PlayNext => "play next",
        ActionKind::AddToLiked => "add to favorites",
        ActionKind::DeleteFromLiked => "remove from favorites",
        ActionKind::AddToPlaylist => "add to a playlist…",
        ActionKind::DeleteFromPlaylist => "remove from this playlist",
        ActionKind::RemoveFromQueue => "remove from the queue",
        ActionKind::DeletePlaylist => "delete the playlist",
    }
}

/// The help text of `command` where the help was opened.
fn text(at: Where, command: UiCommand) -> String {
    use UiCommand as C;
    let owned = |s: &str| s.to_owned();
    match command {
        C::ChooseSelected => owned(match at {
            Where::Queue => "play the entry",
            Where::TopHit => "open or play the top hit",
            Where::Popup => "run the selected entry",
            Where::Input => "search",
            Where::Window(kind) => match kind {
                WindowKind::Playlists | WindowKind::SearchPlaylists => "open the playlist",
                WindowKind::Albums
                | WindowKind::ArtistAlbums
                | WindowKind::AppearsOn
                | WindowKind::SearchAlbums => "open the album",
                WindowKind::Artists | WindowKind::SearchArtists => "open the artist",
                WindowKind::Mixes => "open the mix",
                _ => "play the track with its list",
            },
        }),
        C::ShowActionsOnSelectedItem if at == Where::Queue => owned("actions on the entry"),
        C::ShowActionsOnSelectedItem => owned("actions on the selected row"),
        C::AddSelectedItemToQueue => owned("add to the end of the queue"),
        C::RemoveFromQueue => owned("remove from the queue"),
        C::RoleFilter => owned("the role filter"),
        C::Search => owned("back to the search input"),
        C::ResumePause => owned("play / pause"),
        C::NextTrack => owned("next track"),
        C::PreviousTrack => owned("previous track"),
        C::SeekForward { duration: None } => owned("seek forward"),
        C::SeekForward {
            duration: Some(secs),
        } => format!("seek forward {secs} s"),
        C::SeekBackward { duration: None } => owned("seek backward"),
        C::SeekBackward {
            duration: Some(secs),
        } => format!("seek backward {secs} s"),
        C::SeekStart => owned("back to the start of the track"),
        C::Shuffle => owned("shuffle on / off"),
        C::Repeat => owned("repeat: off → queue → track"),
        C::ToggleAutoplay => owned("autoplay on / off"),
        C::VolumeUp => owned("volume up"),
        C::VolumeDown => owned("volume down"),
        C::VolumeChange { offset } => format!("volume by {offset:+} %"),
        C::Mute => owned("mute / unmute"),
        C::AddToQueuePrompt => owned("add a link or ID to the end of the queue"),
        C::PlayNextPrompt => owned("add a link or ID to play next"),
        C::SelectNextOrScrollDown => owned("move down"),
        C::SelectPreviousOrScrollUp => owned("move up"),
        C::PageSelectNextOrScrollDown => owned("move down a window"),
        C::PageSelectPreviousOrScrollUp => owned("move up a window"),
        C::SelectFirstOrScrollToTop => owned("move to the top"),
        C::SelectLastOrScrollToBottom => owned("move to the last loaded row"),
        C::ShowActionsOnCurrentTrack => owned("actions on the playing track"),
        C::FocusNextWindow => owned("next pane"),
        C::FocusPreviousWindow => owned("previous pane"),
        C::NextTab => owned("next tab"),
        C::PreviousTab => owned("previous tab"),
        C::Queue => owned("the queue page"),
        C::LibraryPage => owned("the library"),
        C::LikedTrackPage => owned("favorite tracks"),
        C::SearchPage => owned("the search page (on one: its input)"),
        C::MixesPage => owned("your mixes"),
        C::PreviousPage => owned("back"),
        C::ClosePopup => owned("close / cancel"),
        C::OpenCommandHelp => owned("this help"),
        C::Quit => owned("quit (an attached TUI detaches)"),
        C::SwitchDevice => owned("choose the output device"),
    }
}

/// Whether nothing is selected where the row-bound commands act.
fn no_selection(state: &State, at: Where) -> bool {
    match at {
        Where::Queue => state
            .cursor
            .is_none_or(|id| !state.queue().iter().any(|e| e.id == id)),
        Where::Popup => false,
        Where::Window(_) | Where::TopHit | Where::Input => state.page().selected().is_none(),
    }
}

/// Whether `command` would do nothing right now: playback keys on an empty
/// queue, row keys with no row, anything sent to the player while
/// disconnected.
fn dim(state: &State, at: Where, command: UiCommand) -> bool {
    use UiCommand as C;
    let offline = state.connection != Connection::Connected;
    let empty = state.queue().is_empty();
    match command {
        C::ResumePause
        | C::NextTrack
        | C::PreviousTrack
        | C::SeekForward { .. }
        | C::SeekBackward { .. }
        | C::SeekStart => empty || offline,
        C::Shuffle
        | C::Repeat
        | C::ToggleAutoplay
        | C::VolumeUp
        | C::VolumeDown
        | C::VolumeChange { .. }
        | C::Mute
        | C::SwitchDevice => offline,
        C::ShowActionsOnCurrentTrack => state.current().is_none(),
        C::ChooseSelected | C::AddSelectedItemToQueue | C::RemoveFromQueue => {
            offline || no_selection(state, at)
        }
        C::ShowActionsOnSelectedItem => no_selection(state, at),
        _ => false,
    }
}

// --- keys --------------------------------------------------------------------

/// Opens the help over whatever is shown (a text input, a prompt and a
/// whole-list load never reach here: they keep their keys).
pub(super) fn open(state: &mut State) -> Vec<Effect> {
    state.help = Some(Help::default());
    Vec::new()
}

/// A key while the help is open (AC8): the filter's when typing, else
/// `/`, a list command, `ChooseSelected` (runs the row), `ClosePopup`
/// (clears the filter, then closes), `OpenCommandHelp` and `Quit` (close);
/// nothing else acts.
pub(super) fn key(state: &mut State, key: Key) -> Vec<Effect> {
    let Some(open) = state.help.as_mut() else {
        return Vec::new();
    };
    if open.typing {
        match key {
            Key::Char(c) => open.filter.push(c),
            Key::Backspace => {
                open.filter.pop();
            }
            Key::Enter => open.typing = false,
            Key::Esc => {
                open.filter.clear();
                open.typing = false;
            }
            _ => return Vec::new(),
        }
        open.cursor = 0;
        return Vec::new();
    }
    if key == Key::Char('/') {
        open.typing = true;
        dispatch::set_pending(state, Vec::new());
        return Vec::new();
    }
    let Some(Binding::Command(command)) = dispatch::resolve(state, key) else {
        return Vec::new();
    };
    match command {
        UiCommand::OpenCommandHelp | UiCommand::Quit => state.help = None,
        UiCommand::ClosePopup => {
            if let Some(open) = state.help.as_mut() {
                if open.filter.is_empty() {
                    state.help = None;
                } else {
                    open.filter.clear();
                    open.cursor = 0;
                }
            }
        }
        UiCommand::ChooseSelected => return run_selected(state),
        _ if command.is_list_move() => {
            let rows: usize = visible(state).iter().map(|s| s.rows.len()).sum();
            let height = state.list_height;
            if let Some(open) = state.help.as_mut()
                && rows > 0
            {
                open.cursor = dispatch::step(command, open.cursor.min(rows - 1), rows, height);
            }
        }
        _ => {}
    }
    Vec::new()
}

/// `Enter` on a row: closes the help and presses the binding's first key
/// sequence in the view the help was opened from. A fixed key (no binding)
/// does nothing.
fn run_selected(state: &mut State) -> Vec<Effect> {
    let sections = visible(state);
    let cursor = state.help.as_ref().map_or(0, |h| h.cursor);
    let Some((s, r)) = locate(&sections, cursor) else {
        return Vec::new();
    };
    let Some(sequence) = sections[s].rows[r].sequences.first().cloned() else {
        return Vec::new();
    };
    state.help = None;
    dispatch::set_pending(state, Vec::new());
    sequence
        .keys()
        .iter()
        .flat_map(|k| dispatch::key(state, *k))
        .collect()
}

#[cfg(test)]
mod tests;
