//! Wire protocol between a daemon and its clients. Transport-agnostic: these
//! types only describe messages, they never send them.

use serde::{Deserialize, Serialize};

/// A request a client sends to the daemon.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Command {
    /// Ask the daemon to shut down.
    Shutdown,
}

/// A notification the daemon sends to its clients.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Event {
    /// The daemon is shutting down.
    ShuttingDown,
    /// The session has expired; login is required.
    LoginRequired,
    /// The session has been restored after expiry.
    LoginRestored,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::de::DeserializeOwned;

    fn round_trip<T>(v: &T)
    where
        T: Serialize + DeserializeOwned + PartialEq + Clone + std::fmt::Debug,
    {
        let json = serde_json::to_string(v).expect("serialize");
        assert_eq!(
            serde_json::from_str::<T>(&json).ok(),
            Some(v.clone()),
            "{v:?} did not survive JSON {json}"
        );
    }

    /// Every variant of `Command`; extend when a variant is added.
    fn all_commands() -> Vec<Command> {
        vec![Command::Shutdown]
    }

    /// Every variant of `Event`; extend when a variant is added.
    fn all_events() -> Vec<Event> {
        vec![
            Event::ShuttingDown,
            Event::LoginRequired,
            Event::LoginRestored,
        ]
    }

    #[test]
    fn ac11_round_trip() {
        all_commands().iter().for_each(round_trip);
        all_events().iter().for_each(round_trip);
    }
}
