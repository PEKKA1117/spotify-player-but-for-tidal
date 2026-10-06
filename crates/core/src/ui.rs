//! Pure UI model: key -> `Action` -> new `State` plus `Effect`s, no I/O.

use crate::protocol;

/// The whole UI state. Empty until later specs add fields.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State {
    /// Whether the session has expired and login is required.
    pub login_required: bool,
}

/// An input to the UI model (a decoded key press, a timer tick, ...).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// The user asked to quit.
    Quit,
    /// A periodic timer tick.
    Tick,
    /// An event from the player.
    Player(protocol::Event),
}

/// A side effect the caller must perform on behalf of the model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Leave the application.
    Quit,
}

/// Applies `action` to `state` and returns the effects the caller must run.
pub fn update(_state: &mut State, action: Action) -> Vec<Effect> {
    match action {
        Action::Quit => vec![Effect::Quit],
        Action::Tick => Vec::new(),
        Action::Player(_) => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ac5_quit_emits_quit() {
        assert_eq!(
            update(&mut State::default(), Action::Quit),
            vec![Effect::Quit]
        );
    }

    #[test]
    fn ac5_other_actions_emit_nothing() {
        let others = [
            Action::Tick,
            Action::Player(protocol::Event::ShuttingDown),
            Action::Player(protocol::Event::LoginRestored),
        ];
        for action in others {
            let mut state = State::default();
            assert_eq!(update(&mut state, action.clone()), vec![], "{action:?}");
            assert_eq!(state, State::default(), "{action:?}");
        }
    }

    #[test]
    fn ac14_login_status() {
        // Table-driven test: (start_state, event, expected_login_required, expected_effects)
        let cases = vec![
            (
                State::default(),
                protocol::Event::LoginRequired,
                true,
                vec![],
            ),
            (
                State {
                    login_required: true,
                },
                protocol::Event::LoginRestored,
                false,
                vec![],
            ),
            (
                State::default(),
                protocol::Event::LoginRestored,
                false,
                vec![],
            ),
        ];

        for (mut state, event, expected_login_required, expected_effects) in cases {
            let effects = update(&mut state, Action::Player(event.clone()));
            assert_eq!(
                state.login_required, expected_login_required,
                "login_required mismatch for {:?}",
                event
            );
            assert_eq!(
                effects, expected_effects,
                "effects mismatch for {:?}",
                event
            );
        }
    }
}
