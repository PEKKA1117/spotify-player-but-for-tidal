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
/// Wait after the owner agreed to release, before claiming the name.
pub const SETTLE: Duration = Duration::from_millis(200);

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
mod tests {
    use super::*;
    use crate::output::fake::{Call, FakeBackend, FakeClock, FakeDevice, FakeReserver, Log};
    use crate::output::open::{FallbackMemo, PcmError};
    use crate::output::pcm_sink::PcmSink;
    use crate::sink::{Codec, SampleFormat, Sink, SourceFormat};
    use std::sync::Arc;

    const DEVICE: &str = "hw:1,0";

    fn busy(holder: Option<&str>) -> SinkError {
        SinkError::Busy {
            device: DEVICE.into(),
            holder: holder.map(Into::into),
        }
    }

    #[test]
    fn ac13_reservation() {
        let rr = Call::RequestRelease {
            card: 1,
            timeout: Duration::from_millis(500),
        };
        let pw = || Some("PipeWire".to_string());
        type Row = (
            &'static str,
            ReleaseReply,
            Result<(), Option<String>>,
            Result<Option<u32>, SinkError>,
            Vec<Call>,
        );
        let rows: Vec<Row> = vec![
            (
                "no bus",
                ReleaseReply::NoBus,
                Ok(()),
                Ok(None),
                vec![rr.clone()],
            ),
            (
                "no owner",
                ReleaseReply::NoOwner,
                Ok(()),
                Ok(Some(1)),
                vec![rr.clone(), Call::Claim(1)],
            ),
            (
                "owner replies true",
                ReleaseReply::Released,
                Ok(()),
                Ok(Some(1)),
                vec![
                    rr.clone(),
                    Call::Sleep(Duration::from_secs(1)),
                    Call::Claim(1),
                ],
            ),
            (
                "owner replies false",
                ReleaseReply::Refused { holder: pw() },
                Ok(()),
                Err(busy(Some("PipeWire"))),
                vec![rr.clone()],
            ),
            (
                "no reply within 500 ms",
                ReleaseReply::NoReply { holder: pw() },
                Ok(()),
                Err(busy(Some("PipeWire"))),
                vec![rr.clone()],
            ),
            (
                "claim lost to another app",
                ReleaseReply::NoOwner,
                Err(pw()),
                Err(busy(Some("PipeWire"))),
                vec![rr.clone(), Call::Claim(1)],
            ),
        ];
        for (name, reply, claim, want, calls) in rows {
            let log = Log::default();
            let mut reserver = FakeReserver::new(&log, reply, claim);
            let clock = FakeClock::new(&log);
            let got = reserve(&mut reserver, &clock, 1, DEVICE);
            assert_eq!(got, want, "{name}");
            assert_eq!(log.calls(), calls, "{name}");
        }
    }

    /// Bug (issue #6): a USB DAC opened within ~200-400 ms of PipeWire
    /// releasing it never locks its clock feedback and bips. The engine waits
    /// 1 s after the owner replies `true`, before claiming and opening.
    #[test]
    fn ac30_settle_after_release() {
        let log = Log::default();
        let mut reserver = FakeReserver::new(&log, ReleaseReply::Released, Ok(()));
        let clock = FakeClock::new(&log);
        assert_eq!(reserve(&mut reserver, &clock, 1, DEVICE), Ok(Some(1)));
        let calls = log.calls();
        let sleep = calls.iter().position(|c| matches!(c, Call::Sleep(_)));
        let claim = calls.iter().position(|c| matches!(c, Call::Claim(1)));
        assert!(sleep < claim, "settle before the claim: {calls:?}");
        assert_eq!(
            sleep.map(|i| &calls[i]),
            Some(&Call::Sleep(Duration::from_secs(1))),
            "settle duration: {calls:?}"
        );
    }

    const SOURCE: SourceFormat = SourceFormat {
        codec: Codec::Flac,
        sample_rate: 44_100,
        channels: 2,
        bits_per_sample: Some(16),
    };

    fn sink(log: &Log, backend: FakeBackend) -> PcmSink<FakeBackend> {
        PcmSink::new(
            DEVICE,
            backend,
            Box::new(FakeReserver::new(log, ReleaseReply::NoOwner, Ok(()))),
            Arc::new(FakeClock::new(log)),
            FallbackMemo::default(),
        )
    }

    fn working(log: &Log) -> FakeBackend {
        FakeBackend::new(log)
            .with_device(DEVICE, FakeDevice::new(&[SampleFormat::S32Le], &[44_100]))
    }

    /// Every claim is released once, after the device was closed.
    fn assert_released_after_close(name: &str, log: &Log) {
        let calls = log.calls();
        let claims = calls.iter().filter(|c| matches!(c, Call::Claim(_))).count();
        let releases: Vec<usize> = calls
            .iter()
            .enumerate()
            .filter(|(_, c)| matches!(c, Call::Release(1)))
            .map(|(i, _)| i)
            .collect();
        assert!(claims > 0, "{name}: never claimed: {calls:?}");
        assert_eq!(releases.len(), claims, "{name}: {calls:?}");
        for i in releases {
            let opens = calls[..i]
                .iter()
                .filter(|c| matches!(c, Call::Open(_)))
                .count();
            let closes = calls[..i]
                .iter()
                .filter(|c| matches!(c, Call::Close))
                .count();
            assert_eq!(
                opens, closes,
                "{name}: released while the device was open: {calls:?}"
            );
        }
    }

    #[test]
    fn ac13_released_on_every_close_path() {
        // End of track / stop / device switch: the engine closes the sink.
        let log = Log::default();
        let mut s = sink(&log, working(&log));
        s.open(&SOURCE).unwrap();
        s.close();
        s.close();
        assert_released_after_close("close", &log);

        // The sink is dropped without close (engine thread ends).
        let log = Log::default();
        let mut s = sink(&log, working(&log));
        s.open(&SOURCE).unwrap();
        drop(s);
        assert_released_after_close("drop", &log);

        // Re-open (format change between tracks) closes and releases first.
        let log = Log::default();
        let mut s = sink(&log, working(&log));
        s.open(&SOURCE).unwrap();
        s.open(&SOURCE).unwrap();
        s.close();
        assert_released_after_close("reopen", &log);

        // Output error while playing, then the engine closes.
        let log = Log::default();
        let backend = working(&log).with_write_results(vec![Err(PcmError::Lost)]);
        let mut s = sink(&log, backend);
        s.open(&SOURCE).unwrap();
        assert!(matches!(s.write(&[0, 0]), Err(SinkError::Lost(_))));
        s.close();
        assert_released_after_close("lost", &log);

        // Open errors after the claim: busy past the budget, no such device,
        // and a fallback that refuses too.
        let log = Log::default();
        let backend = working(&log).with_open_default(Err(PcmError::Busy));
        let mut s = sink(&log, backend);
        assert!(matches!(s.open(&SOURCE), Err(SinkError::Busy { .. })));
        assert_released_after_close("busy", &log);

        let log = Log::default();
        let mut s = sink(&log, FakeBackend::new(&log));
        assert!(matches!(s.open(&SOURCE), Err(SinkError::NotFound(_))));
        assert_released_after_close("not found", &log);

        let log = Log::default();
        let backend = FakeBackend::new(&log)
            .with_device(DEVICE, FakeDevice::new(&[], &[]))
            .with_device("plughw:1,0", FakeDevice::new(&[], &[]));
        let mut s = sink(&log, backend);
        assert!(s.open(&SOURCE).is_err());
        assert_released_after_close("fallback refused", &log);
    }
}
