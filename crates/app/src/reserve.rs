//! The `org.freedesktop.ReserveDevice1` primitives over the session bus
//! (spec 0003 "Reservation"), with zbus's blocking API: the thin D-Bus
//! layer under `tidal_player_audio::output::reserve`, which holds the
//! decision table and the timing budgets (AC13).
//!
//! The D-Bus calls themselves are checked by hand at acceptance (spec 0003
//! "Not covered by automated tests"); the naming and the classification of
//! the owner's answer are pure and tested here.

use std::collections::HashMap;
use std::time::Duration;

use tidal_player_audio::output::reserve::REPLY_TIMEOUT;
use tidal_player_audio::{ANSWER_WITHIN, ReleaseReply, ReleaseRequests, Reserver};
use zbus::blocking::{Connection, Proxy, connection, proxy::Builder as ProxyBuilder};
use zbus::fdo::{RequestNameFlags, RequestNameReply};
use zbus::proxy::CacheProperties;

/// The ReserveDevice1 interface name.
pub const INTERFACE: &str = "org.freedesktop.ReserveDevice1";

/// The priority sent with `RequestRelease` and exported while holding a
/// card: the highest, as a user asking for this exact device.
pub const PRIORITY: i32 = i32::MAX;
/// What other applications see as the holder.
pub const APPLICATION_NAME: &str = "tidal-player";

/// `org.freedesktop.ReserveDevice1.Audio{card}`.
pub fn reservation_name(card: u32) -> String {
    format!("{INTERFACE}.Audio{card}")
}

/// `/org/freedesktop/ReserveDevice1/Audio{card}`.
pub fn reservation_path(card: u32) -> String {
    format!("/org/freedesktop/ReserveDevice1/Audio{card}")
}

/// Why a `RequestRelease` call got no `bool` back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallFailure {
    /// Nobody owns the name.
    NoOwner,
    /// The owner did not answer in time.
    Timeout,
    /// Any other error: the owner exists but did not agree.
    Other,
}

/// Classifies a D-Bus error by its name.
pub fn failure_for_error_name(name: &str) -> CallFailure {
    match name {
        "org.freedesktop.DBus.Error.ServiceUnknown"
        | "org.freedesktop.DBus.Error.NameHasNoOwner" => CallFailure::NoOwner,
        "org.freedesktop.DBus.Error.NoReply" | "org.freedesktop.DBus.Error.Timeout" => {
            CallFailure::Timeout
        }
        _ => CallFailure::Other,
    }
}

/// Classifies a zbus error (a timeout is an I/O `TimedOut`).
fn failure_of(error: &zbus::Error) -> CallFailure {
    match error {
        zbus::Error::MethodError(name, _, _) => failure_for_error_name(name.as_str()),
        zbus::Error::FDO(e) => match **e {
            zbus::fdo::Error::ServiceUnknown(_) | zbus::fdo::Error::NameHasNoOwner(_) => {
                CallFailure::NoOwner
            }
            zbus::fdo::Error::NoReply(_) | zbus::fdo::Error::Timeout(_) => CallFailure::Timeout,
            _ => CallFailure::Other,
        },
        zbus::Error::InputOutput(e) if e.kind() == std::io::ErrorKind::TimedOut => {
            CallFailure::Timeout
        }
        _ => CallFailure::Other,
    }
}

/// Maps the owner's answer to the decision table's input; `holder` is the
/// owner's `ApplicationName`, if it was read.
pub fn classify_release(answer: Result<bool, CallFailure>, holder: Option<String>) -> ReleaseReply {
    match answer {
        Ok(true) => ReleaseReply::Released,
        Ok(false) => ReleaseReply::Refused { holder },
        Err(CallFailure::NoOwner) => ReleaseReply::NoOwner,
        Err(CallFailure::Timeout | CallFailure::Other) => ReleaseReply::NoReply { holder },
    }
}

/// The object exported while holding a card: asks the engine, which
/// releases the card while paused and refuses otherwise (spec 0005). The
/// engine closes the PCM before it answers and gives the name back right
/// after; no answer within [`ANSWER_WITHIN`] is a refusal.
struct Device {
    card: u32,
    requests: ReleaseRequests,
}

#[zbus::interface(name = "org.freedesktop.ReserveDevice1")]
impl Device {
    fn request_release(&self, priority: i32) -> bool {
        let released = self.requests.request(ANSWER_WITHIN);
        tracing::info!(
            card = self.card,
            priority,
            released,
            "answered a request to release the card"
        );
        released
    }

    #[zbus(property)]
    fn priority(&self) -> i32 {
        PRIORITY
    }

    #[zbus(property)]
    fn application_name(&self) -> String {
        APPLICATION_NAME.into()
    }

    #[zbus(property)]
    fn application_device_name(&self) -> String {
        format!("hw:{}", self.card)
    }
}

/// [`Reserver`] over the session bus. Each held card has its own
/// connection, which owns the name and serves the [`Device`] object.
#[derive(Default)]
pub struct ZbusReserver {
    held: HashMap<u32, Connection>,
    requests: ReleaseRequests,
}

impl std::fmt::Debug for ZbusReserver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ZbusReserver")
            .field("held", &self.held.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl ZbusReserver {
    /// `requests` reaches the engine that owns this reserver's sink.
    pub fn new(requests: ReleaseRequests) -> Self {
        Self {
            held: HashMap::new(),
            requests,
        }
    }

    /// A proxy on the current owner of the card's name.
    fn owner<'c>(connection: &'c Connection, card: u32) -> zbus::Result<Proxy<'c>> {
        ProxyBuilder::new(connection)
            .destination(reservation_name(card))?
            .path(reservation_path(card))?
            .interface(INTERFACE)?
            .cache_properties(CacheProperties::No)
            .build()
    }

    /// The owner's `ApplicationName`, if it answers in time.
    fn holder(connection: &Connection, card: u32) -> Option<String> {
        Self::owner(connection, card)
            .and_then(|p| p.get_property::<String>("ApplicationName"))
            .ok()
    }
}

impl Reserver for ZbusReserver {
    fn request_release(&mut self, card: u32, timeout: Duration) -> ReleaseReply {
        // A connection of its own, so the timeout bounds this call only.
        let Ok(connection) = connection::Builder::session()
            .map(|b| b.method_timeout(timeout))
            .and_then(connection::Builder::build)
        else {
            return ReleaseReply::NoBus;
        };
        let answer = Self::owner(&connection, card)
            .and_then(|owner| owner.call::<_, _, bool>("RequestRelease", &(PRIORITY,)))
            .map_err(|e| failure_of(&e));
        // Asked only of an owner that answered (no second wait on a slow one).
        let holder = match answer {
            Ok(false) => Self::holder(&connection, card),
            _ => None,
        };
        classify_release(answer, holder)
    }

    fn claim(&mut self, card: u32) -> Result<(), Option<String>> {
        let name = reservation_name(card);
        let connection = connection::Builder::session()
            .map(|b| b.method_timeout(REPLY_TIMEOUT))
            .and_then(|b| {
                b.serve_at(
                    reservation_path(card),
                    Device {
                        card,
                        requests: self.requests.clone(),
                    },
                )
            })
            .and_then(connection::Builder::build)
            .map_err(|e| {
                tracing::warn!("cannot reserve card {card}: {e}");
                None
            })?;
        let flags = RequestNameFlags::ReplaceExisting
            | RequestNameFlags::AllowReplacement
            | RequestNameFlags::DoNotQueue;
        match connection.request_name_with_flags(name.as_str(), flags) {
            Ok(RequestNameReply::PrimaryOwner | RequestNameReply::AlreadyOwner) => {
                self.held.insert(card, connection);
                Ok(())
            }
            Ok(_) => Err(Self::holder(&connection, card)),
            Err(e) => {
                tracing::warn!("cannot reserve card {card}: {e}");
                Err(None)
            }
        }
    }

    fn release(&mut self, card: u32) {
        if let Some(connection) = self.held.remove(&card) {
            let _ = connection.release_name(reservation_name(card).as_str());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reservation_names() {
        assert_eq!(reservation_name(1), "org.freedesktop.ReserveDevice1.Audio1");
        assert_eq!(
            reservation_path(12),
            "/org/freedesktop/ReserveDevice1/Audio12"
        );
    }

    #[test]
    fn release_answers() {
        let pw = || Some("PipeWire".to_string());
        let rows = [
            (Ok(true), pw(), ReleaseReply::Released),
            (Ok(false), pw(), ReleaseReply::Refused { holder: pw() }),
            (Ok(false), None, ReleaseReply::Refused { holder: None }),
            (Err(CallFailure::NoOwner), None, ReleaseReply::NoOwner),
            (
                Err(CallFailure::Timeout),
                pw(),
                ReleaseReply::NoReply { holder: pw() },
            ),
            // Never steal the card from an owner that answered oddly.
            (
                Err(CallFailure::Other),
                None,
                ReleaseReply::NoReply { holder: None },
            ),
        ];
        for (answer, holder, want) in rows {
            assert_eq!(classify_release(answer, holder), want, "{answer:?}");
        }

        let names = [
            (
                "org.freedesktop.DBus.Error.ServiceUnknown",
                CallFailure::NoOwner,
            ),
            (
                "org.freedesktop.DBus.Error.NameHasNoOwner",
                CallFailure::NoOwner,
            ),
            ("org.freedesktop.DBus.Error.NoReply", CallFailure::Timeout),
            ("org.freedesktop.DBus.Error.Timeout", CallFailure::Timeout),
            (
                "org.freedesktop.DBus.Error.UnknownMethod",
                CallFailure::Other,
            ),
        ];
        for (name, want) in names {
            assert_eq!(failure_for_error_name(name), want, "{name}");
        }
    }
}
