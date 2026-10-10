//! Keys to commands (spec 0008 "Key sequences and where keys act"): the
//! fixed keys of text inputs and questions first, then the keymap, with
//! spotify-player's sequence rule; a command acts only where it does (the
//! "Acts in" column), popups keep "only its own keys act".

use std::time::Duration;

use crate::item::parse_item;
use crate::protocol::{Command, InsertAt};

use super::keymap::{Binding, UiCommand};
use super::{Effect, Key, PageKind, Prompt, State, browse, help, search};

/// A key press: what it does, wherever the UI is.
pub(super) fn key(state: &mut State, key: Key) -> Vec<Effect> {
    if state.help.is_some() {
        return help::key(state, key);
    }
    if let Some(effects) = fixed_key(state, key) {
        set_pending(state, Vec::new());
        return effects;
    }
    match resolve(state, key) {
        Some(binding) => run_binding(state, binding),
        None => Vec::new(),
    }
}

/// The keys the keymap never sees (spec 0008 decision 6): text inputs take
/// every key, a question its `y`/`n`, the role filter its `Space`. `None`:
/// the key goes to the keymap.
fn fixed_key(state: &mut State, key: Key) -> Option<Vec<Effect>> {
    if state.popup.is_some() {
        return browse::popup_fixed_key(state, key);
    }
    if let Some(at) = state.prompt.as_ref().map(|p| p.at) {
        return Some(prompt_key(state, at, key));
    }
    if state.whole_list.is_none() && search::input_focused(state) {
        return Some(search::input_key(state, key));
    }
    None
}

/// Sets the collected keys (and the flag 0004 reads).
pub(super) fn set_pending(state: &mut State, pending: Vec<Key>) {
    state.pending_g = !pending.is_empty();
    state.pending = pending;
}

/// Adds `key` to the collected keys: the binding they now name, if any.
/// Keys that start no binding restart with `key` alone; there is no
/// timeout (spotify-player's rule).
pub(super) fn resolve(state: &mut State, key: Key) -> Option<Binding> {
    let mut keys = std::mem::take(&mut state.pending);
    keys.push(key);
    let keymap = &state.keymap;
    if keymap.get(&keys).is_none() && !keymap.starts(&keys) {
        keys = vec![key];
    }
    let found = keymap.get(&keys);
    let pending = if found.is_none() && keymap.starts(&keys) {
        keys
    } else {
        Vec::new()
    };
    set_pending(state, pending);
    found
}

/// Runs a binding where the UI is.
fn run_binding(state: &mut State, binding: Binding) -> Vec<Effect> {
    match binding {
        Binding::Command(command) => run_command(state, command),
        // An action acts on a page, never over a popup, a prompt or a load.
        Binding::Action(action)
            if state.popup.is_none() && state.prompt.is_none() && state.whole_list.is_none() =>
        {
            browse::run_action(state, action)
        }
        Binding::Action(_) => Vec::new(),
    }
}

/// Runs `command` where the UI is: in the open popup, prompt or whole-list
/// load if there is one, else on the page; nothing where it does not act.
pub(crate) fn run_command(state: &mut State, command: UiCommand) -> Vec<Effect> {
    // The help opens over a page or a popup; a prompt and a load keep
    // their keys.
    if command == UiCommand::OpenCommandHelp && state.prompt.is_none() && state.whole_list.is_none()
    {
        return help::open(state);
    }
    if state.popup.is_some() {
        return browse::popup_command(state, command);
    }
    if state.prompt.is_some() {
        if command == UiCommand::ClosePopup {
            state.prompt = None;
        }
        return Vec::new();
    }
    if state.whole_list.is_some() {
        return browse::whole_list_command(state, command);
    }
    page_command(state, command)
}

/// Where a list command moves a cursor at `i` of `len` rows (`len` > 0);
/// any other command leaves it.
pub(super) fn step(command: UiCommand, i: usize, len: usize, height: usize) -> usize {
    let last = len.saturating_sub(1);
    match command {
        UiCommand::SelectNextOrScrollDown => (i + 1).min(last),
        UiCommand::SelectPreviousOrScrollUp => i.saturating_sub(1),
        UiCommand::PageSelectNextOrScrollDown => (i + height).min(last),
        UiCommand::PageSelectPreviousOrScrollUp => i.saturating_sub(height),
        UiCommand::SelectFirstOrScrollToTop => 0,
        UiCommand::SelectLastOrScrollToBottom => last,
        _ => i,
    }
}

/// A command on the page shown (no popup, prompt or load).
fn page_command(state: &mut State, command: UiCommand) -> Vec<Effect> {
    use UiCommand as C;
    let volume = i8::try_from(state.steps.volume).unwrap_or(i8::MAX);
    let seek = |duration: Option<u16>| match duration {
        Some(secs) => i64::from(secs) * 1000,
        None => i64::try_from(state.steps.seek.as_millis()).unwrap_or(i64::MAX),
    };
    // Pages, focus and popups; volume and modes (they apply to the next
    // load too); the lists; playback (it needs a queue).
    let send = match command {
        C::Quit => return vec![Effect::Quit],
        C::ClosePopup | C::OpenCommandHelp => return Vec::new(),
        C::SwitchDevice => return browse::open_devices(state),
        C::Queue => return browse::open(state, PageKind::Queue),
        C::LibraryPage => return browse::open(state, PageKind::Library),
        C::LikedTrackPage => return browse::open(state, PageKind::FavoriteTracks),
        C::SearchPage => return search::open(state),
        C::MixesPage => return browse::open(state, PageKind::Mixes),
        C::Search => return search::focus_input(state),
        C::PreviousPage => return browse::back(state),
        C::ShowActionsOnSelectedItem => return browse::actions_on_selected(state),
        C::ShowActionsOnCurrentTrack => return browse::actions_on_playing(state),
        C::FocusNextWindow | C::FocusPreviousWindow => {
            let forward = command == C::FocusNextWindow;
            if state.page().search.is_some() {
                return search::cycle(state, forward);
            }
            return browse::cycle_focus(state, forward);
        }
        C::NextTab => return browse::cycle_tab(state, true),
        C::PreviousTab => return browse::cycle_tab(state, false),
        C::AddToQueuePrompt | C::PlayNextPrompt => {
            state.prompt = Some(Prompt {
                at: if command == C::AddToQueuePrompt {
                    InsertAt::End
                } else {
                    InsertAt::Next
                },
                text: String::new(),
            });
            return Vec::new();
        }
        C::Shuffle => Command::ToggleShuffle,
        C::Repeat => Command::CycleRepeat,
        C::ToggleAutoplay => Command::ToggleAutoplay,
        C::VolumeUp => Command::ChangeVolume(volume),
        C::VolumeDown => Command::ChangeVolume(-volume),
        C::VolumeChange { offset } => Command::ChangeVolume(offset),
        C::Mute => Command::ToggleMute,
        C::ChooseSelected
        | C::AddSelectedItemToQueue
        | C::RemoveFromQueue
        | C::RoleFilter
        | C::SelectNextOrScrollDown
        | C::SelectPreviousOrScrollUp
        | C::PageSelectNextOrScrollDown
        | C::PageSelectPreviousOrScrollUp
        | C::SelectFirstOrScrollToTop
        | C::SelectLastOrScrollToBottom => {
            return if state.page().kind == PageKind::Queue {
                queue_command(state, command)
            } else if command == C::ChooseSelected && state.page().search.is_some() {
                search::enter(state)
            } else {
                browse::list_command(state, command)
            };
        }
        C::ResumePause
        | C::NextTrack
        | C::PreviousTrack
        | C::SeekForward { .. }
        | C::SeekBackward { .. }
        | C::SeekStart => {
            if state.queue().is_empty() {
                return Vec::new();
            }
            match command {
                C::ResumePause => Command::TogglePause,
                C::NextTrack => Command::Next,
                C::PreviousTrack => Command::Previous,
                C::SeekForward { duration } => Command::SeekBy(seek(duration)),
                C::SeekBackward { duration } => Command::SeekBy(-seek(duration)),
                _ => Command::SeekTo(Duration::ZERO),
            }
        }
    };
    vec![Effect::Send(send)]
}

/// A list command on the queue page: the cursor (by entry ID), `Enter`
/// plays the entry, `d` removes it.
fn queue_command(state: &mut State, command: UiCommand) -> Vec<Effect> {
    let height = state.list_height;
    let send = match command {
        UiCommand::ChooseSelected => state.cursor.map(Command::PlayEntry),
        UiCommand::RemoveFromQueue => state.cursor.map(Command::RemoveFromQueue),
        _ if command.is_list_move() => {
            move_cursor(state, |i, len| step(command, i, len, height));
            None
        }
        // `Z` and `f` act on browse pages only.
        _ => None,
    };
    match send {
        Some(send) if !state.queue().is_empty() => vec![Effect::Send(send)],
        _ => Vec::new(),
    }
}

/// Moves the cursor to `to(index, len)` (a non-empty queue), and keeps it
/// in view.
fn move_cursor(state: &mut State, to: impl Fn(usize, usize) -> usize) {
    let queue = state.queue();
    if queue.is_empty() {
        return;
    }
    let index = state
        .cursor
        .and_then(|id| queue.iter().position(|e| e.id == id))
        .unwrap_or(0);
    let id = queue[to(index, queue.len()).min(queue.len() - 1)].id;
    state.cursor = Some(id);
    state.anchor = Some(id);
}

/// A key while the prompt is open: it edits the prompt, nothing else.
fn prompt_key(state: &mut State, at: InsertAt, key: Key) -> Vec<Effect> {
    let Some(prompt) = state.prompt.as_mut() else {
        return Vec::new();
    };
    match key {
        Key::Char(c) => prompt.text.push(c),
        Key::Backspace => {
            prompt.text.pop();
        }
        Key::Esc => state.prompt = None,
        Key::Enter => {
            let text = std::mem::take(&mut prompt.text);
            state.prompt = None;
            if text.trim().is_empty() {
                return Vec::new();
            }
            return match parse_item(&text) {
                Ok(item) => vec![Effect::Send(Command::Open {
                    items: vec![item],
                    at: Some(at),
                })],
                Err(e) => {
                    state.message = Some(e.to_string());
                    Vec::new()
                }
            };
        }
        _ => {}
    }
    Vec::new()
}
