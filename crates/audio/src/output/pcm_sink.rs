//! The output [`Sink`] over a [`PcmBackend`] and a [`Reserver`] (spec 0003
//! "Output (ALSA)"). The ALSA build plugs in `AlsaBackend`; tests a fake.

use std::sync::Arc;
use std::time::Duration;

use crate::output::clock::Clock;
use crate::output::format::pack_into;
use crate::output::open::{FallbackMemo, Opened, PcmBackend, PcmError, open_output};
use crate::output::reserve::Reserver;
use crate::sink::{
    OutputInfo, SampleFormat, Sink, SinkError, SinkFactory, SourceFormat, WriteOutcome,
};

/// The longest stretch of audio a drain waits for (0003 AC31).
const DRAIN_AUDIO_CAP: Duration = Duration::from_secs(2);
/// How long a drain waits beyond the audio's own duration before it drops
/// what is left.
const DRAIN_MARGIN: Duration = Duration::from_millis(500);
/// Between two looks at the delay while draining.
const DRAIN_POLL: Duration = Duration::from_millis(10);

#[derive(Debug)]
struct OpenState {
    format: SampleFormat,
    rate: u32,
    channels: usize,
    period_frames: usize,
    reserved: Option<u32>,
    pcm: Pcm,
}

/// Where the PCM is, as far as pausing goes (0003 AC29): ALSA accepts
/// `snd_pcm_pause(1)` only on a running PCM and `snd_pcm_pause(0)` only on a
/// paused one, and answers `EBADFD` otherwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pcm {
    /// Opened, discarded, drained or recovered: holds nothing, plays nothing
    /// until written to.
    Prepared,
    Running,
    Paused,
}

/// A [`Sink`] for one device name, generic over the PCM backend.
pub struct PcmSink<B: PcmBackend> {
    requested: String,
    backend: B,
    reserver: Box<dyn Reserver>,
    clock: Arc<dyn Clock>,
    memo: FallbackMemo,
    open: Option<OpenState>,
    bytes: Vec<u8>,
}

impl<B: PcmBackend> PcmSink<B> {
    /// A sink for `device`; nothing is opened until [`Sink::open`].
    pub fn new(
        device: &str,
        backend: B,
        reserver: Box<dyn Reserver>,
        clock: Arc<dyn Clock>,
        memo: FallbackMemo,
    ) -> Self {
        Self {
            requested: device.to_owned(),
            backend,
            reserver,
            clock,
            memo,
            open: None,
            bytes: Vec::new(),
        }
    }

    fn state(&self) -> Result<&OpenState, SinkError> {
        self.open.as_ref().ok_or(SinkError::NotOpen)
    }

    fn set_pcm(&mut self, pcm: Pcm) {
        if let Some(state) = self.open.as_mut() {
            state.pcm = pcm;
        }
    }

    fn error(&self, error: PcmError) -> SinkError {
        match error {
            PcmError::Lost => SinkError::Lost(self.requested.clone()),
            PcmError::NotFound => SinkError::NotFound(self.requested.clone()),
            PcmError::Busy => SinkError::Busy {
                device: self.requested.clone(),
                holder: None,
            },
            PcmError::Underrun => SinkError::Backend("underrun".into()),
            PcmError::Suspended => SinkError::Backend("suspended".into()),
            PcmError::Refused(e) | PcmError::Other(e) => SinkError::Backend(e),
        }
    }
}

impl<B: PcmBackend> Sink for PcmSink<B> {
    fn open(&mut self, source: &SourceFormat) -> Result<OutputInfo, SinkError> {
        self.close();
        let Opened {
            info,
            config,
            reserved,
        } = open_output(
            &mut self.backend,
            self.reserver.as_mut(),
            self.clock.as_ref(),
            &self.memo,
            &self.requested,
            source,
        )?;
        self.open = Some(OpenState {
            format: config.format,
            rate: config.rate,
            channels: usize::from(config.channels),
            period_frames: config.period_frames as usize,
            reserved,
            pcm: Pcm::Prepared,
        });
        Ok(info)
    }

    fn write(&mut self, samples: &[i32]) -> Result<WriteOutcome, SinkError> {
        let state = self.state()?;
        let (format, channels) = (state.format, state.channels);
        // At most one period, so a write never blocks much longer than that.
        let frames = (samples.len() / channels).min(state.period_frames);
        self.bytes.clear();
        pack_into(&samples[..frames * channels], format, &mut self.bytes);
        match self.backend.write(&self.bytes) {
            Ok(frames) => {
                if frames > 0 {
                    self.set_pcm(Pcm::Running);
                }
                Ok(WriteOutcome {
                    frames,
                    underrun: false,
                })
            }
            Err(error @ (PcmError::Underrun | PcmError::Suspended)) => {
                self.backend.recover(&error).map_err(|e| self.error(e))?;
                self.set_pcm(Pcm::Prepared);
                Ok(WriteOutcome {
                    frames: 0,
                    underrun: true,
                })
            }
            Err(error) => Err(self.error(error)),
        }
    }

    fn delay_frames(&self) -> Result<u64, SinkError> {
        self.state()?;
        match self.backend.delay() {
            Ok(frames) => Ok(u64::try_from(frames).unwrap_or(0)),
            Err(PcmError::Underrun | PcmError::Suspended) => Ok(0),
            Err(e) => Err(self.error(e)),
        }
    }

    /// Pauses or resumes the device where ALSA allows it (0003 AC29). A
    /// prepared PCM holds nothing and plays nothing until written to, which
    /// the engine does not do while paused, so there is nothing to pause.
    fn set_paused(&mut self, paused: bool) -> Result<(), SinkError> {
        let target = match (paused, self.state()?.pcm) {
            (true, Pcm::Running) => Pcm::Paused,
            (false, Pcm::Paused) => Pcm::Running,
            _ => return Ok(()),
        };
        self.backend.pause(paused).map_err(|e| self.error(e))?;
        self.set_pcm(target);
        Ok(())
    }

    fn discard(&mut self) -> Result<(), SinkError> {
        self.state()?;
        self.backend.drop_frames().map_err(|e| self.error(e))?;
        self.backend.prepare().map_err(|e| self.error(e))?;
        self.set_pcm(Pcm::Prepared);
        Ok(())
    }

    /// Waits for the device to play what it holds, then drops the rest and
    /// prepares it (0003 AC31). The wait polls the delay and is bounded by
    /// the held audio's duration (at most [`DRAIN_AUDIO_CAP`]) plus
    /// [`DRAIN_MARGIN`]: `snd_pcm_drain` has no bound, and PipeWire's ALSA
    /// plugin (1.6.10) never returns from it.
    fn drain(&mut self) -> Result<(), SinkError> {
        let state = self.state()?;
        if state.pcm == Pcm::Prepared {
            // Nothing written since the last prepare: nothing to play out.
            return Ok(());
        }
        let rate = state.rate;
        let start = self.clock.now();
        let mut budget = None;
        loop {
            let frames = match self.backend.delay() {
                Ok(frames) if frames > 0 => frames,
                Ok(_) | Err(PcmError::Underrun | PcmError::Suspended) => break,
                Err(e) => return Err(self.error(e)),
            };
            let budget = *budget.get_or_insert_with(|| {
                frames_to_duration(frames, rate).min(DRAIN_AUDIO_CAP) + DRAIN_MARGIN
            });
            if self.clock.now().saturating_sub(start) >= budget {
                break;
            }
            self.clock.sleep(DRAIN_POLL);
        }
        self.backend.drop_frames().map_err(|e| self.error(e))?;
        self.backend.prepare().map_err(|e| self.error(e))?;
        self.set_pcm(Pcm::Prepared);
        Ok(())
    }

    fn close(&mut self) {
        self.close_with(&mut || {});
    }

    /// Closes the PCM, calls `between` (the answer to `RequestRelease`,
    /// spec 0005), then gives the card's name back.
    fn close_with(&mut self, between: &mut dyn FnMut()) {
        let Some(state) = self.open.take() else {
            between();
            return;
        };
        self.backend.close();
        between();
        if let Some(card) = state.reserved {
            self.reserver.release(card);
        }
    }
}

impl<B: PcmBackend> Drop for PcmSink<B> {
    fn drop(&mut self) {
        self.close();
    }
}

/// How long `frames` last at `rate`.
fn frames_to_duration(frames: i64, rate: u32) -> Duration {
    let frames = u64::try_from(frames).unwrap_or(0);
    Duration::from_nanos(frames.saturating_mul(1_000_000_000) / u64::from(rate.max(1)))
}

type MakeBackend<B> = Box<dyn FnMut() -> B + Send>;
type MakeReserver = Box<dyn FnMut() -> Box<dyn Reserver> + Send>;

/// Creates [`PcmSink`]s that share one clock and one fallback memo, so a
/// device whose negotiation was refused goes straight to `plughw:` for the
/// rest of the process.
pub struct PcmSinkFactory<B: PcmBackend> {
    make_backend: MakeBackend<B>,
    make_reserver: MakeReserver,
    clock: Arc<dyn Clock>,
    memo: FallbackMemo,
}

impl<B: PcmBackend> PcmSinkFactory<B> {
    pub fn new(
        make_backend: impl FnMut() -> B + Send + 'static,
        make_reserver: impl FnMut() -> Box<dyn Reserver> + Send + 'static,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            make_backend: Box::new(make_backend),
            make_reserver: Box::new(make_reserver),
            clock,
            memo: FallbackMemo::default(),
        }
    }
}

impl<B: PcmBackend + 'static> SinkFactory for PcmSinkFactory<B> {
    fn create(&mut self, device: &str) -> Box<dyn Sink> {
        Box::new(PcmSink::new(
            device,
            (self.make_backend)(),
            (self.make_reserver)(),
            self.clock.clone(),
            self.memo.clone(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::fake::{Call, FakeBackend, FakeClock, FakeDevice, FakeReserver, Log};
    use crate::output::format::pack;
    use crate::output::reserve::ReleaseReply;
    use crate::sink::Codec;
    use std::time::Duration;

    const DEVICE: &str = "hw:1,0";

    const HIRES: SourceFormat = SourceFormat {
        codec: Codec::Flac,
        sample_rate: 96_000,
        channels: 2,
        bits_per_sample: Some(24),
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

    fn device(log: &Log) -> FakeBackend {
        FakeBackend::new(log)
            .with_device(DEVICE, FakeDevice::new(&[SampleFormat::S24_3Le], &[96_000]))
    }

    #[test]
    fn write_packs_the_negotiated_format() {
        let log = Log::default();
        let backend = device(&log);
        let mut s = sink(&log, backend.clone());
        s.open(&HIRES).unwrap();
        let samples = [0x12_3456 << 8, -2 << 8, 1 << 8, -1 << 8];
        let out = s.write(&samples).unwrap();
        assert_eq!(
            out,
            WriteOutcome {
                frames: 2,
                underrun: false
            }
        );
        assert_eq!(backend.written(), pack(&samples, SampleFormat::S24_3Le));
    }

    #[test]
    fn write_takes_at_most_one_period() {
        let log = Log::default();
        let backend = device(&log);
        let mut s = sink(&log, backend.clone());
        s.open(&HIRES).unwrap();
        let out = s.write(&vec![0; 2 * 3000]).unwrap();
        assert_eq!(out.frames, 1024);
        assert_eq!(backend.written().len(), 1024 * 2 * 3);
    }

    #[test]
    fn write_partial_reports_frames_taken() {
        let log = Log::default();
        let backend = device(&log).with_write_results(vec![Ok(3)]);
        let mut s = sink(&log, backend.clone());
        s.open(&HIRES).unwrap();
        let out = s.write(&[7 << 8; 2 * 10]).unwrap();
        assert_eq!(
            out,
            WriteOutcome {
                frames: 3,
                underrun: false
            }
        );
        assert_eq!(backend.written().len(), 3 * 2 * 3);
    }

    #[test]
    fn write_underrun_and_suspend_are_recovered() {
        for error in [PcmError::Underrun, PcmError::Suspended] {
            let log = Log::default();
            let backend = device(&log).with_write_results(vec![Err(error.clone())]);
            let mut s = sink(&log, backend.clone());
            s.open(&HIRES).unwrap();
            let out = s.write(&[1 << 8, 2 << 8]).unwrap();
            assert_eq!(
                out,
                WriteOutcome {
                    frames: 0,
                    underrun: true
                },
                "{error:?}"
            );
            assert!(log.calls().contains(&Call::Recover(error.clone())));
            // The caller writes the unconsumed frame again; it arrives once.
            let out = s.write(&[1 << 8, 2 << 8]).unwrap();
            assert_eq!(
                out,
                WriteOutcome {
                    frames: 1,
                    underrun: false
                }
            );
            assert_eq!(
                backend.written(),
                pack(&[1 << 8, 2 << 8], SampleFormat::S24_3Le)
            );
        }
    }

    #[test]
    fn write_device_lost() {
        let log = Log::default();
        let backend = device(&log).with_write_results(vec![Err(PcmError::Lost)]);
        let mut s = sink(&log, backend);
        s.open(&HIRES).unwrap();
        assert_eq!(s.write(&[0, 0]), Err(SinkError::Lost(DEVICE.into())));
    }

    #[test]
    fn use_before_open_fails() {
        let log = Log::default();
        let mut s = sink(&log, device(&log));
        assert_eq!(s.write(&[0, 0]), Err(SinkError::NotOpen));
        assert_eq!(s.delay_frames(), Err(SinkError::NotOpen));
        assert_eq!(s.set_paused(true), Err(SinkError::NotOpen));
        assert_eq!(s.discard(), Err(SinkError::NotOpen));
        assert_eq!(s.drain(), Err(SinkError::NotOpen));
    }

    #[test]
    fn pause_discard_drain_delay() {
        let log = Log::default();
        let backend = device(&log);
        let mut s = sink(&log, backend.clone());
        s.open(&HIRES).unwrap();
        log.clear();
        backend.set_delay(4096);
        assert_eq!(s.delay_frames(), Ok(4096));
        // Pausing needs a running PCM (0003 AC29).
        s.write(&[0, 0]).unwrap();
        log.clear();
        s.set_paused(true).unwrap();
        s.set_paused(false).unwrap();
        s.discard().unwrap();
        // Nothing is held after a discard: the drain has nothing to wait
        // for and calls nothing (0003 AC31).
        s.drain().unwrap();
        assert_eq!(
            log.calls(),
            vec![
                Call::Pause(true),
                Call::Pause(false),
                Call::Drop,
                Call::Prepare,
            ]
        );
    }

    /// 0003 AC31: draining waits for the device through its delay, within a
    /// budget, and never calls `snd_pcm_drain`, which never returns on
    /// PipeWire 1.6.10's ALSA plugin.
    #[test]
    fn ac31_drain_is_bounded() {
        // At HIRES's 96 kHz, 9,600 frames last 100 ms.
        const TENTH: i64 = 9_600;
        let ms = Duration::from_millis;
        struct Row {
            name: &'static str,
            written: bool,
            delays: Vec<Result<i64, PcmError>>,
            result: Result<(), SinkError>,
            /// Least and most time spent waiting (the fake clock's sleeps).
            waited: (Duration, Duration),
            /// The device is dropped and prepared once the wait is over.
            dropped: bool,
        }
        let rows = [
            Row {
                name: "plays out to 0",
                written: true,
                delays: vec![Ok(TENTH), Ok(TENTH / 2), Ok(0)],
                result: Ok(()),
                waited: (ms(1), ms(600)),
                dropped: true,
            },
            Row {
                name: "underrun at the end",
                written: true,
                delays: vec![Ok(TENTH), Err(PcmError::Underrun)],
                result: Ok(()),
                waited: (ms(1), ms(600)),
                dropped: true,
            },
            Row {
                name: "stuck: the delay never falls",
                written: true,
                delays: vec![Ok(TENTH)],
                result: Ok(()),
                // The audio's 100 ms plus 500 ms, and not a bit more.
                waited: (ms(600), ms(600)),
                dropped: true,
            },
            Row {
                name: "stuck with a delay beyond 2 s: capped",
                written: true,
                delays: vec![Ok(TENTH * 100)],
                result: Ok(()),
                waited: (ms(2_500), ms(2_500)),
                dropped: true,
            },
            Row {
                name: "stuck on one frame: a budget that is no multiple of the poll",
                written: true,
                delays: vec![Ok(1)],
                result: Ok(()),
                // One frame at 96 kHz is 10,416 ns.
                waited: (
                    ms(500) + Duration::from_nanos(10_416),
                    ms(500) + Duration::from_nanos(10_416),
                ),
                dropped: true,
            },
            Row {
                name: "nothing written",
                written: false,
                delays: vec![Ok(0)],
                result: Ok(()),
                waited: (Duration::ZERO, Duration::ZERO),
                dropped: false,
            },
            Row {
                name: "device lost while waiting",
                written: true,
                delays: vec![Ok(TENTH), Err(PcmError::Lost)],
                result: Err(SinkError::Lost(DEVICE.into())),
                waited: (Duration::ZERO, ms(600)),
                dropped: false,
            },
        ];
        for row in rows {
            let log = Log::default();
            let backend = device(&log).with_delays(row.delays);
            let mut s = sink(&log, backend);
            s.open(&HIRES).unwrap();
            if row.written {
                s.write(&[0; 2 * 64]).unwrap();
            }
            log.clear();
            assert_eq!(s.drain(), row.result, "{}", row.name);
            let calls = log.calls();
            assert!(
                !calls.contains(&Call::Drain),
                "{}: snd_pcm_drain was called: {calls:?}",
                row.name
            );
            let waited: Duration = calls
                .iter()
                .filter_map(|c| match c {
                    Call::Sleep(d) => Some(*d),
                    _ => None,
                })
                .sum();
            assert!(
                (row.waited.0..=row.waited.1).contains(&waited),
                "{}: waited {waited:?}, expected {:?}..={:?}",
                row.name,
                row.waited.0,
                row.waited.1
            );
            let rest: Vec<_> = calls
                .into_iter()
                .filter(|c| !matches!(c, Call::Sleep(_)))
                .collect();
            let expected = if row.dropped {
                vec![Call::Drop, Call::Prepare]
            } else {
                Vec::new()
            };
            assert_eq!(rest, expected, "{}", row.name);
        }
    }

    /// 0003 AC29: `snd_pcm_pause` only where ALSA accepts it (the fake
    /// answers `EBADFD` elsewhere, as ALSA does).
    #[test]
    fn ac29_pause_follows_pcm_state() {
        #[derive(Debug, Clone, Copy)]
        enum Step {
            Write,
            Pause,
            Resume,
            Discard,
            Underrun,
        }
        use Step::*;
        let rows: [(&str, &[Step], &[Call]); 6] = [
            ("pause right after open", &[Pause, Resume], &[]),
            (
                "pause, then resume",
                &[Write, Pause, Resume],
                &[Call::Pause(true), Call::Pause(false)],
            ),
            (
                "seek, then buffering pause and resume",
                &[Write, Discard, Pause, Resume],
                &[],
            ),
            (
                "seek while paused, then resume",
                &[Write, Pause, Discard, Resume],
                &[Call::Pause(true)],
            ),
            (
                "seek while paused, resume, write, pause",
                &[Write, Pause, Discard, Resume, Write, Pause],
                &[Call::Pause(true), Call::Pause(true)],
            ),
            (
                "underrun recovered, then pause",
                &[Write, Underrun, Pause],
                &[],
            ),
        ];
        for (name, steps, pauses) in rows {
            let log = Log::default();
            let script: Vec<_> = steps
                .iter()
                .filter(|s| matches!(s, Write | Underrun))
                .map(|s| match s {
                    Underrun => Err(PcmError::Underrun),
                    _ => Ok(1),
                })
                .collect();
            let backend = device(&log).with_write_results(script);
            let mut s = sink(&log, backend);
            s.open(&HIRES).unwrap();
            for step in steps {
                let result = match step {
                    Write | Underrun => s.write(&[0, 0]).map(|_| ()),
                    Pause => s.set_paused(true),
                    Resume => s.set_paused(false),
                    Discard => s.discard(),
                };
                assert_eq!(result, Ok(()), "{name}: {step:?}");
            }
            let seen: Vec<Call> = log
                .calls()
                .into_iter()
                .filter(|c| matches!(c, Call::Pause(_)))
                .collect();
            assert_eq!(seen, pauses, "{name}");
        }
    }

    /// 0005 AC20: `close_with` closes the PCM, then calls `between` (the
    /// answer to `RequestRelease`), then releases the name; with nothing
    /// open it only calls `between`.
    #[test]
    fn ac20_close_with_answers_between() {
        let log = Log::default();
        let mut s = sink(&log, device(&log));
        s.open(&HIRES).unwrap();
        log.clear();
        let marks = log.clone();
        s.close_with(&mut || marks.push(Call::Mark("answer")));
        assert_eq!(
            log.calls(),
            vec![Call::Close, Call::Mark("answer"), Call::Release(1)]
        );
        log.clear();
        s.close_with(&mut || marks.push(Call::Mark("answer")));
        assert_eq!(log.calls(), vec![Call::Mark("answer")]);
    }

    #[test]
    fn factory_shares_the_fallback_memo() {
        let log = Log::default();
        let backend = FakeBackend::new(&log)
            .with_device(DEVICE, FakeDevice::new(&[SampleFormat::S16Le], &[44_100]))
            .with_device("plughw:1,0", FakeDevice::plug());
        let reserver_log = log.clone();
        let make = backend.clone();
        let mut factory = PcmSinkFactory::new(
            move || make.clone(),
            move || {
                Box::new(FakeReserver::new(
                    &reserver_log,
                    ReleaseReply::NoBus,
                    Ok(()),
                )) as Box<dyn Reserver>
            },
            Arc::new(FakeClock::new(&log)),
        );
        let mut first = factory.create(DEVICE);
        assert_eq!(first.open(&HIRES).unwrap().device, "plughw:1,0");
        first.close();
        log.clear();
        let mut second = factory.create(DEVICE);
        assert_eq!(second.open(&HIRES).unwrap().device, "plughw:1,0");
        assert!(!log.calls().contains(&Call::Open(DEVICE.into())));
    }
}
