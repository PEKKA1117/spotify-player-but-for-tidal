//! Card reservation with `org.freedesktop.ReserveDevice1` (spec 0003
//! "Output (ALSA)", "Reservation"; AC13).
//!
//! The D-Bus calls sit behind [`Reserver`], implemented with zbus in the
//! binary, so this crate gains no D-Bus dependency. [`reserve`] holds the
//! decision table and the timing budgets.

use std::time::Duration;

use crate::output::clock::Clock;
use crate::sink::SinkError;

/// How long the owner has to answer `RequestRelease` before the card is
/// reported busy (never stolen from a slow owner).
pub const REPLY_TIMEOUT: Duration = Duration::from_millis(500);
/// Wait after the owner agreed to release, before claiming the name and
/// opening the device. A USB DAC opened sooner after PipeWire let it go may
/// never lock its clock feedback, and bips (spec 0003 AC30: 200 ms failed,
/// 400 ms held, on a MOONRIVER 3).
pub const SETTLE: Duration = Duration::from_secs(1);

/// What the current owner of `org.freedesktop.ReserveDevice1.Audio{card}`
/// answered to `RequestRelease`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReleaseReply {
    /// There is no session bus: proceed without a reservation.
    NoBus,
    /// Nobody owns the name (the call failed with "no owner").
    NoOwner,
    /// The owner replied `true`: it released the card.
    Released,
    /// The owner replied `false`.
    Refused {
        /// The owner's `ApplicationName`, if known.
        holder: Option<String>,
    },
    /// No reply within the timeout.
    NoReply {
        /// The owner's `ApplicationName`, if known.
        holder: Option<String>,
    },
}

/// The ReserveDevice1 primitives for one session bus.
pub trait Reserver: Send {
    /// Call `RequestRelease` on the owner of
    /// `org.freedesktop.ReserveDevice1.Audio{card}`, waiting at most `timeout`
    /// for the reply.
    fn request_release(&mut self, card: u32, timeout: Duration) -> ReleaseReply;
    /// Own the name (`ReplaceExisting`, `AllowReplacement`) and refuse
    /// `RequestRelease` while holding it. `Err` carries the holder's
    /// application name, if known.
    fn claim(&mut self, card: u32) -> Result<(), Option<String>>;
    /// Give the name back. Called once per successful [`Reserver::claim`].
    fn release(&mut self, card: u32);
}

/// A [`Reserver`] for systems without a session bus: never reserves.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoReserver;

impl Reserver for NoReserver {
    fn request_release(&mut self, _card: u32, _timeout: Duration) -> ReleaseReply {
        ReleaseReply::NoBus
    }

    fn claim(&mut self, _card: u32) -> Result<(), Option<String>> {
        Ok(())
    }

    fn release(&mut self, _card: u32) {}
}

/// Reserve `card` for `device` per the decision table. `Ok(Some(card))`: the
/// name is held and must be released with [`Reserver::release`] when the
/// device is closed; `Ok(None)`: proceed without a reservation.
pub fn reserve(
    reserver: &mut dyn Reserver,
    clock: &dyn Clock,
    card: u32,
    device: &str,
) -> Result<Option<u32>, SinkError> {
    let busy = |holder| SinkError::Busy {
        device: device.to_owned(),
        holder,
    };
    match reserver.request_release(card, REPLY_TIMEOUT) {
        ReleaseReply::NoBus => return Ok(None),
        ReleaseReply::NoOwner => {}
        ReleaseReply::Released => clock.sleep(SETTLE),
        ReleaseReply::Refused { holder } | ReleaseReply::NoReply { holder } => {
            return Err(busy(holder));
        }
    }
    reserver.claim(card).map_err(busy)?;
    Ok(Some(card))
}

#[cfg(test)]
mod tests;
