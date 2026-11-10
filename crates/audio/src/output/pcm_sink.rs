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
            let left = budget.saturating_sub(self.clock.now().saturating_sub(start));
            if left.is_zero() {
                break;
            }
            self.clock.sleep(DRAIN_POLL.min(left));
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
mod tests;
