//! Pure UI model: key -> `Action` -> new `State` plus `Effect`s, no I/O.

/// The whole UI state. Empty until later specs add fields.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State {}

/// An input to the UI model (a decoded key press, a timer tick, ...).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// The user asked to quit.
    Quit,
    /// A periodic timer tick.
    Tick,
}

/// A side effect the caller must perform on behalf of the model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Leave the application.
    Quit,
}

/// Applies `action` to `state` and returns the effects the caller must run.
pub fn update(_state: &mut State, _action: Action) -> Vec<Effect> {
    vec![Effect::Quit]
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
        let others = [Action::Tick];
        for action in others {
            let mut state = State::default();
            assert_eq!(update(&mut state, action.clone()), vec![], "{action:?}");
            assert_eq!(state, State::default(), "{action:?}");
        }
    }
}
