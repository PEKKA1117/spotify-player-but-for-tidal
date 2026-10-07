//! Pure mapping from terminal key presses to UI actions. Which key does
//! what (and key sequences such as `g g`) is decided by
//! `tidal_player_core::ui`; this only decodes crossterm's events.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
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
        _ => return None,
    };
    Some(Action::Key(decoded))
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

    /// AC20: crossterm key events decode into the core's keys: characters
    /// as typed (Shift gives the upper-case character), Control + letter,
    /// and the named keys; anything else is dropped.
    #[test]
    fn ac20_key_events() {
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
            (KeyCode::Tab, none, None),
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
}
