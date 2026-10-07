//! The output [`Sink`] over a [`PcmBackend`] and a [`Reserver`] (spec 0003
//! "Output (ALSA)"). The ALSA build plugs in `AlsaBackend`; tests a fake.

use std::sync::Arc;

use crate::output::clock::Clock;
use crate::output::format::pack_into;
use crate::output::open::{FallbackMemo, Opened, PcmBackend, PcmError, open_output};
use crate::output::reserve::Reserver;
use crate::sink::{
    OutputInfo, SampleFormat, Sink, SinkError, SinkFactory, SourceFormat, WriteOutcome,
};

#[derive(Debug)]
struct OpenState {
    format: SampleFormat,
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

    fn drain(&mut self) -> Result<(), SinkError> {
        self.state()?;
        self.backend.drain().map_err(|e| self.error(e))?;
        self.backend.prepare().map_err(|e| self.error(e))?;
        self.set_pcm(Pcm::Prepared);
        Ok(())
    }

    fn close(&mut self) {
        if let Some(state) = self.open.take() {
            self.backend.close();
            if let Some(card) = state.reserved {
                self.reserver.release(card);
            }
        }
    }
}

impl<B: PcmBackend> Drop for PcmSink<B> {
    fn drop(&mut self) {
        self.close();
    }
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
        s.drain().unwrap();
        assert_eq!(
            log.calls(),
            vec![
                Call::Pause(true),
                Call::Pause(false),
                Call::Drop,
                Call::Prepare,
                Call::Drain,
                Call::Prepare,
            ]
        );
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
