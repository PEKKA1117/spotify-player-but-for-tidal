//! ALSA playback backend. Compile-only for now: format negotiation, `hw:`
//! reservation and fallback are specified in spec 0003.

use alsa::pcm::{Access, Format, HwParams, PCM};
use alsa::{Direction, ValueOr};

use crate::sink::{AudioFormat, Sink, SinkError};

/// A [`Sink`] that plays through an ALSA PCM device (S32_LE, interleaved).
pub struct AlsaSink {
    device: String,
    pcm: Option<PCM>,
    channels: usize,
}

impl AlsaSink {
    /// Create a sink for `device` (e.g. `"default"` or `"hw:0,0"`); nothing is opened yet.
    pub fn new(device: &str) -> Self {
        Self {
            device: device.to_owned(),
            pcm: None,
            channels: 0,
        }
    }
}

fn backend(e: alsa::Error) -> SinkError {
    SinkError::Backend(e.to_string())
}

impl Sink for AlsaSink {
    fn open(&mut self, format: AudioFormat) -> Result<(), SinkError> {
        let pcm = PCM::new(&self.device, Direction::Playback, false).map_err(backend)?;
        {
            let hw = HwParams::any(&pcm).map_err(backend)?;
            hw.set_access(Access::RWInterleaved).map_err(backend)?;
            hw.set_format(Format::s32()).map_err(backend)?;
            hw.set_channels(u32::from(format.channels))
                .map_err(backend)?;
            hw.set_rate(format.sample_rate, ValueOr::Nearest)
                .map_err(backend)?;
            pcm.hw_params(&hw).map_err(backend)?;
        }
        self.channels = usize::from(format.channels);
        self.pcm = Some(pcm);
        Ok(())
    }

    fn write(&mut self, samples: &[i32]) -> Result<(), SinkError> {
        let pcm = self.pcm.as_ref().ok_or(SinkError::NotOpen)?;
        let io = pcm.io_i32().map_err(backend)?;
        let mut rest = samples;
        while !rest.is_empty() {
            match io.writei(rest) {
                Ok(frames) => rest = &rest[frames * self.channels..],
                // Recover from underruns/suspends, then retry.
                Err(e) => pcm.try_recover(e, true).map_err(backend)?,
            }
        }
        Ok(())
    }
}
