//! Pure mapping from key presses to UI actions.

use crossterm::event::{KeyCode, KeyEvent};
use tidal_player_core::ui::Action;

/// Maps a key press to the `Action` it triggers, if any.
pub fn key_to_action(key: KeyEvent) -> Option<Action> {
    match key.code {
        KeyCode::Char('q') | KeyCode::Esc => Some(Action::Quit),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;

    #[test]
    fn maps_q_and_esc_to_quit() {
        for code in [KeyCode::Char('q'), KeyCode::Esc] {
            let key = KeyEvent::new(code, KeyModifiers::NONE);
            assert_eq!(key_to_action(key), Some(Action::Quit), "{code:?}");
        }
        let other = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE);
        assert_eq!(key_to_action(other), None);
    }
}
