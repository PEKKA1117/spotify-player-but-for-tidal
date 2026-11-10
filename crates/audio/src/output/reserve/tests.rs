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
    FakeBackend::new(log).with_device(DEVICE, FakeDevice::new(&[SampleFormat::S32Le], &[44_100]))
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
