//! Pure mapping from terminal key presses to UI actions. Which key does
//! what (and key sequences such as `g g`) is decided by
//! `tidal_player_core::ui`; this only decodes crossterm's events.

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use tidal_player_core::ui::{Action, Key};

/// Maps a key press to the `Action` it triggers, if any: characters as
/// typed (Shift already gives the upper-case one), Control + a letter, and
/// the keys the TUI uses. Alt combinations and other keys are dropped.
pub fn key_to_action(key: KeyEvent) -> Option<Action> {
    if key.modifiers.contains(KeyModifiers::ALT) {
        return None;
    }
    let decoded = match key.code {
        KeyCode::Char(c) if key.modifiers.contains(KeyModifiers::CONTROL) => {
            Key::Ctrl(c.to_ascii_lowercase())
        }
        KeyCode::Char(c) => Key::Char(c),
        KeyCode::Enter => Key::Enter,
        KeyCode::Esc => Key::Esc,
        KeyCode::Backspace => Key::Backspace,
        KeyCode::Up => Key::Up,
        KeyCode::Down => Key::Down,
        KeyCode::Tab if key.modifiers.contains(KeyModifiers::SHIFT) => Key::BackTab,
        KeyCode::Tab => Key::Tab,
        // Shift-Tab: crossterm reports `BackTab` (with or without Shift).
        KeyCode::BackTab => Key::BackTab,
        KeyCode::PageUp => Key::PageUp,
        KeyCode::PageDown => Key::PageDown,
        _ => return None,
    };
    Some(Action::Key(decoded))
}

/// Maps a terminal event to the `Action` it triggers, if any: a key press
/// as [`key_to_action`] decodes it, and pasted text (bracketed paste) as
/// one `Action::Paste` (spec 0007 AC11). Key releases, resizes and the
/// rest are dropped.
pub fn event_to_action(event: Event) -> Option<Action> {
    match event {
        Event::Key(key) if key.kind == KeyEventKind::Press => key_to_action(key),
        Event::Paste(text) => Some(Action::Paste(text)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;
    use tidal_player_core::ui::{Effect, Key, State, update};

    /// 0001 AC5 through the key map: `q` and `Esc` quit (with the prompt
    /// closed; with it open they edit it, spec 0004 AC28).
    #[test]
    fn maps_q_and_ctrl_c_to_quit_and_esc_to_nothing() {
        // Spec 0006 AC15: `Esc` no longer quits; `q` and `C-c` do.
        for (code, modifiers) in [
            (KeyCode::Char('q'), KeyModifiers::NONE),
            (KeyCode::Char('c'), KeyModifiers::CONTROL),
        ] {
            let key = KeyEvent::new(code, modifiers);
            let action = key_to_action(key).expect("an action");
            assert_eq!(
                update(&mut State::default(), action),
                vec![Effect::Quit],
                "{code:?}"
            );
        }
        let esc = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
        let effects = key_to_action(esc).map(|a| update(&mut State::default(), a));
        assert!(effects.as_ref().is_none_or(Vec::is_empty), "{effects:?}");
        let other = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE);
        let effects = key_to_action(other).map(|a| update(&mut State::default(), a));
        assert!(effects.as_ref().is_none_or(Vec::is_empty), "{effects:?}");
    }

    /// 0004 AC20 and 0006 AC18: crossterm key events decode into the
    /// core's keys: characters as typed (Shift gives the upper-case
    /// character), Control + letter, and the named keys (`Tab`, `BackTab`
    /// as crossterm reports Shift-Tab or as Shift + `Tab`, `Backspace`,
    /// `PageUp`, `PageDown`); anything else is dropped.
    #[test]
    fn ac18_key_events() {
        let none = KeyModifiers::NONE;
        let shift = KeyModifiers::SHIFT;
        let ctrl = KeyModifiers::CONTROL;
        let cases = [
            (KeyCode::Char(' '), none, Some(Key::Char(' '))),
            (KeyCode::Char('n'), none, Some(Key::Char('n'))),
            (KeyCode::Char('q'), none, Some(Key::Char('q'))),
            (KeyCode::Char('>'), shift, Some(Key::Char('>'))),
            (KeyCode::Char('^'), shift, Some(Key::Char('^'))),
            (KeyCode::Char('+'), shift, Some(Key::Char('+'))),
            (KeyCode::Char('_'), shift, Some(Key::Char('_'))),
            (KeyCode::Char('G'), shift, Some(Key::Char('G'))),
            (KeyCode::Char('O'), shift, Some(Key::Char('O'))),
            (KeyCode::Char('A'), shift, Some(Key::Char('A'))),
            (KeyCode::Char('s'), ctrl, Some(Key::Ctrl('s'))),
            (KeyCode::Char('r'), ctrl, Some(Key::Ctrl('r'))),
            (KeyCode::Char('S'), ctrl | shift, Some(Key::Ctrl('s'))),
            (KeyCode::Enter, none, Some(Key::Enter)),
            (KeyCode::Esc, none, Some(Key::Esc)),
            (KeyCode::Backspace, none, Some(Key::Backspace)),
            (KeyCode::Up, none, Some(Key::Up)),
            (KeyCode::Down, none, Some(Key::Down)),
            (KeyCode::F(1), none, None),
            (KeyCode::Tab, none, Some(Key::Tab)),
            (KeyCode::BackTab, none, Some(Key::BackTab)),
            (KeyCode::BackTab, shift, Some(Key::BackTab)),
            (KeyCode::Tab, shift, Some(Key::BackTab)),
            (KeyCode::PageUp, none, Some(Key::PageUp)),
            (KeyCode::PageDown, none, Some(Key::PageDown)),
            (KeyCode::Char(' '), ctrl, Some(Key::Ctrl(' '))),
            (KeyCode::Char('f'), ctrl, Some(Key::Ctrl('f'))),
            (KeyCode::Char('b'), ctrl, Some(Key::Ctrl('b'))),
            (KeyCode::Char('q'), ctrl, Some(Key::Ctrl('q'))),
            (KeyCode::Char('z'), ctrl, Some(Key::Ctrl('z'))),
            (KeyCode::Char('c'), ctrl, Some(Key::Ctrl('c'))),
            // 0007 AC11: `C-u` clears the search input.
            (KeyCode::Char('u'), ctrl, Some(Key::Ctrl('u'))),
            (KeyCode::Home, none, None),
            (KeyCode::Char('x'), KeyModifiers::ALT, None),
        ];
        for (code, modifiers, key) in cases {
            assert_eq!(
                key_to_action(KeyEvent::new(code, modifiers)),
                key.map(Action::Key),
                "{code:?} {modifiers:?}"
            );
        }
    }

    /// 0007 AC11: pasted text (bracketed paste) becomes one
    /// `Action::Paste` and reaches a focused search input as characters;
    /// a key press decodes as before, a key release and a resize do
    /// nothing.
    #[test]
    fn ac11_paste_reaches_the_search_input() {
        use crossterm::event::KeyEventState;
        let paste = Event::Paste("pierce the veil".into());
        assert_eq!(
            event_to_action(paste.clone()),
            Some(Action::Paste("pierce the veil".into()))
        );
        let press = Event::Key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));
        assert_eq!(event_to_action(press), Some(Action::Key(Key::Char('q'))));
        let release = Event::Key(KeyEvent {
            code: KeyCode::Char('q'),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Release,
            state: KeyEventState::NONE,
        });
        assert_eq!(event_to_action(release), None);
        assert_eq!(event_to_action(Event::Resize(80, 24)), None);

        // Through the model: `g s`, then the paste, then `C-u`.
        let mut state = State::default();
        for key in [Key::Char('g'), Key::Char('s')] {
            update(&mut state, Action::Key(key));
        }
        let effects: Vec<Effect> = event_to_action(paste)
            .into_iter()
            .flat_map(|action| update(&mut state, action))
            .collect();
        assert!(effects.is_empty(), "{effects:?}");
        let input = |state: &State| state.page().search.as_ref().map(|s| s.input.clone());
        assert_eq!(input(&state).as_deref(), Some("pierce the veil"));
        let ctrl_u = KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL);
        let action = key_to_action(ctrl_u).expect("C-u decodes");
        update(&mut state, action);
        assert_eq!(input(&state).as_deref(), Some(""));
    }
}
