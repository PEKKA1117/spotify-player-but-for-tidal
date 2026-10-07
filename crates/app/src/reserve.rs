//! The `org.freedesktop.ReserveDevice1` primitives over the session bus
//! (spec 0003 "Reservation"), with zbus's blocking API: the thin D-Bus
//! layer under `tidal_player_audio::output::reserve`, which holds the
//! decision table and the timing budgets (AC13).
//!
//! The D-Bus calls themselves are checked by hand at acceptance (spec 0003
//! "Not covered by automated tests"); the naming and the classification of
//! the owner's answer are pure and tested here.

use tidal_player_audio::ReleaseReply;

/// The priority sent with `RequestRelease` and exported while holding a
/// card: the highest, as a user asking for this exact device.
pub const PRIORITY: i32 = i32::MAX;
/// What other applications see as the holder.
pub const APPLICATION_NAME: &str = "tidal-player";

/// `org.freedesktop.ReserveDevice1.Audio{card}`.
pub fn reservation_name(card: u32) -> String {
    let _ = card;
    String::new()
}

/// `/org/freedesktop/ReserveDevice1/Audio{card}`.
pub fn reservation_path(card: u32) -> String {
    let _ = card;
    String::new()
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
    let _ = name;
    CallFailure::Other
}

/// Maps the owner's answer to the decision table's input; `holder` is the
/// owner's `ApplicationName`, if it was read.
pub fn classify_release(answer: Result<bool, CallFailure>, holder: Option<String>) -> ReleaseReply {
    let _ = (answer, holder);
    ReleaseReply::NoOwner
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
