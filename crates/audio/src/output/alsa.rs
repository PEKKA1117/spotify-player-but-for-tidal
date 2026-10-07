//! The real [`PcmBackend`] over alsa-lib (feature `alsa`). Deliberately
//! thin: every decision lives in `open.rs` / `pcm_sink.rs`, tested against a
//! fake. Checked by hand on real hardware at acceptance (spec 0003 test plan).

use std::ffi::CString;
use std::sync::Arc;

use alsa::pcm::{Access, Format, HwParams, PCM};
use alsa::{Direction, ValueOr};

use crate::output::clock::SystemClock;
use crate::output::open::{HwConfig, PcmBackend, PcmError};
use crate::output::pcm_sink::PcmSinkFactory;
use crate::output::reserve::Reserver;
use crate::sink::SampleFormat;

// Linux errno values (alsa-lib returns them negated; the alsa crate
// reports them positive).
const ENOENT: i32 = 2;
const EIO: i32 = 5;
const ENXIO: i32 = 6;
const EBUSY: i32 = 16;
const ENODEV: i32 = 19;
const EINVAL: i32 = 22;
const EPIPE: i32 = 32;
const ESTRPIPE: i32 = 86;

const FORMATS: [SampleFormat; 4] = [
    SampleFormat::S16Le,
    SampleFormat::S24Le,
    SampleFormat::S24_3Le,
    SampleFormat::S32Le,
];

fn alsa_format(format: SampleFormat) -> Format {
    match format {
        SampleFormat::S16Le => Format::S16LE,
        SampleFormat::S24Le => Format::S24LE,
        SampleFormat::S24_3Le => Format::S243LE,
        SampleFormat::S32Le => Format::S32LE,
    }
}

fn other(error: alsa::Error) -> PcmError {
    PcmError::Other(error.to_string())
}

fn refused(error: alsa::Error) -> PcmError {
    match error.errno() {
        EINVAL => PcmError::Refused(error.to_string()),
        _ => other(error),
    }
}

fn io_error(error: alsa::Error) -> PcmError {
    match error.errno() {
        EPIPE => PcmError::Underrun,
        ESTRPIPE => PcmError::Suspended,
        ENODEV | EIO | ENXIO => PcmError::Lost,
        _ => other(error),
    }
}

/// One ALSA PCM handle, opened for blocking playback.
#[derive(Default)]
pub struct AlsaBackend {
    pcm: Option<PCM>,
    can_pause: bool,
}

impl AlsaBackend {
    pub fn new() -> Self {
        Self::default()
    }

    fn pcm(&self) -> Result<&PCM, PcmError> {
        self.pcm
            .as_ref()
            .ok_or_else(|| PcmError::Other("PCM not open".into()))
    }
}

/// Interleaved access, resampling as asked, exactly `channels` and `rate`.
/// `set_rate` with `ValueOr::Nearest` passes `dir = 0`, which alsa-lib
/// takes as "exactly this rate" (unlike `set_rate_near`).
fn base_params<'a>(
    pcm: &'a PCM,
    rate: u32,
    channels: u16,
    resample: bool,
) -> Result<HwParams<'a>, PcmError> {
    let hw = HwParams::any(pcm).map_err(other)?;
    hw.set_access(Access::RWInterleaved).map_err(refused)?;
    hw.set_rate_resample(resample).map_err(refused)?;
    hw.set_channels(u32::from(channels)).map_err(refused)?;
    hw.set_rate(rate, ValueOr::Nearest).map_err(refused)?;
    Ok(hw)
}

/// Install `config`; returns whether the device can pause.
fn configure(pcm: &PCM, config: &HwConfig) -> Result<bool, PcmError> {
    let hw = base_params(pcm, config.rate, config.channels, config.resample)?;
    hw.set_format(alsa_format(config.format)).map_err(refused)?;
    let period = alsa::pcm::Frames::from(config.period_frames);
    hw.set_period_size_near(period, ValueOr::Nearest)
        .map_err(refused)?;
    hw.set_buffer_size_near(period * alsa::pcm::Frames::from(config.periods))
        .map_err(refused)?;
    pcm.hw_params(&hw).map_err(refused)?;
    let current = pcm.hw_params_current().map_err(other)?;
    if current.get_rate().map_err(other)? != config.rate {
        return Err(PcmError::Refused(format!(
            "device did not keep {} Hz",
            config.rate
        )));
    }
    Ok(current.can_pause())
}

impl PcmBackend for AlsaBackend {
    fn card_index(&mut self, card: &str) -> Option<u32> {
        let name = CString::new(card).ok()?;
        let card = alsa::card::Card::from_str(&name).ok()?;
        u32::try_from(card.get_index()).ok()
    }

    fn open(&mut self, name: &str) -> Result<(), PcmError> {
        self.close();
        let pcm = PCM::new(name, Direction::Playback, false).map_err(|e| match e.errno() {
            EBUSY => PcmError::Busy,
            ENOENT | ENODEV | ENXIO => PcmError::NotFound,
            _ => other(e),
        })?;
        self.pcm = Some(pcm);
        Ok(())
    }

    fn accepted_formats(
        &mut self,
        rate: u32,
        channels: u16,
        resample: bool,
    ) -> Result<Vec<SampleFormat>, PcmError> {
        let pcm = self.pcm()?;
        let hw = match base_params(pcm, rate, channels, resample) {
            Ok(hw) => hw,
            Err(PcmError::Refused(_)) => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };
        Ok(FORMATS
            .into_iter()
            .filter(|f| hw.test_format(alsa_format(*f)).is_ok())
            .collect())
    }

    fn configure(&mut self, config: &HwConfig) -> Result<(), PcmError> {
        self.can_pause = configure(self.pcm()?, config)?;
        Ok(())
    }

    fn write(&mut self, bytes: &[u8]) -> Result<usize, PcmError> {
        self.pcm()?.io_bytes().writei(bytes).map_err(io_error)
    }

    // `Frames` is a C `long`: `i32` on 32-bit targets.
    #[allow(clippy::useless_conversion)]
    fn delay(&self) -> Result<i64, PcmError> {
        self.pcm()?.delay().map(i64::from).map_err(io_error)
    }

    fn pause(&mut self, paused: bool) -> Result<(), PcmError> {
        // A device without hardware pause keeps its buffered frames anyway:
        // it plays them out and underruns, recovered on the next write.
        if !self.can_pause {
            return Ok(());
        }
        self.pcm()?.pause(paused).map_err(io_error)
    }

    fn drop_frames(&mut self) -> Result<(), PcmError> {
        self.pcm()?.drop().map_err(io_error)
    }

    fn prepare(&mut self) -> Result<(), PcmError> {
        self.pcm()?.prepare().map_err(io_error)
    }

    fn drain(&mut self) -> Result<(), PcmError> {
        self.pcm()?.drain().map_err(io_error)
    }

    fn recover(&mut self, error: &PcmError) -> Result<(), PcmError> {
        let errno = match error {
            PcmError::Underrun => EPIPE,
            PcmError::Suspended => ESTRPIPE,
            other => return Err(other.clone()),
        };
        self.pcm()?.recover(errno, true).map_err(io_error)
    }

    fn close(&mut self) {
        self.pcm = None;
    }
}

/// The ALSA [`crate::SinkFactory`]: real backend, real clock, and the given
/// ReserveDevice1 implementation (the binary's zbus one, or
/// [`crate::output::NoReserver`]).
pub fn alsa_sink_factory(
    make_reserver: impl FnMut() -> Box<dyn Reserver> + Send + 'static,
) -> PcmSinkFactory<AlsaBackend> {
    PcmSinkFactory::new(
        AlsaBackend::new,
        make_reserver,
        Arc::new(SystemClock::new()),
    )
}
