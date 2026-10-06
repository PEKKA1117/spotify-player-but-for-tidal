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

    #[test]
    fn ac11_round_trip() {
        for c in [Command::Shutdown] {
            round_trip(&c);
        }
        for e in [Event::ShuttingDown] {
            round_trip(&e);
        }
    }
}
