//! Opening an output: the [`PcmBackend`] seam over the ALSA calls, and the
//! decisions taken over it (spec 0003 "Output kinds", "Output (ALSA)"; AC12).

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::output::clock::Clock;
use crate::output::format::{choose_format, format_bits, format_name, preference};
use crate::output::reserve::{Reserver, reserve};
use crate::sink::{OutputInfo, OutputKind, SampleFormat, SinkError, SourceFormat};

/// Frames per device period.
pub const PERIOD_FRAMES: u32 = 1024;
/// Periods in the buffer for `hw:` and `plughw:`.
pub const EXCLUSIVE_PERIODS: u32 = 4;
/// Periods in the buffer through the system mixer.
pub const SHARED_PERIODS: u32 = 8;
/// The engine always outputs stereo.
pub const OUTPUT_CHANNELS: u16 = 2;
/// Wait between two opens of a device that answered `EBUSY`.
pub const BUSY_RETRY_INTERVAL: Duration = Duration::from_millis(100);
/// Give up on `EBUSY` once this much time has passed since the first open.
pub const BUSY_RETRY_BUDGET: Duration = Duration::from_millis(800);

/// Errors from a [`PcmBackend`], by what the decision logic does with them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PcmError {
    /// `EBUSY`: another application has the device open.
    Busy,
    /// `ENOENT`/`ENODEV` on open: no such device.
    NotFound,
    /// The device refused the hardware parameters (`EINVAL`).
    Refused(String),
    /// `EPIPE`: underrun. Recoverable.
    Underrun,
    /// `ESTRPIPE`: suspended. Recoverable.
    Suspended,
    /// `ENODEV`/`EIO` while open: the device went away.
    Lost,
    /// Anything else.
    Other(String),
}

/// Hardware parameters to set on an open device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HwConfig {
    pub format: SampleFormat,
    /// Exact rate (never "nearest").
    pub rate: u32,
    pub channels: u16,
    /// Whether ALSA may resample (`snd_pcm_hw_params_set_rate_resample`).
    pub resample: bool,
    pub period_frames: u32,
    pub periods: u32,
}

/// The thin seam over one ALSA PCM handle. At most one device is open at a
/// time; [`PcmBackend::open`] is only called when none is.
pub trait PcmBackend: Send {
    /// The index of the card named `card` (an ALSA card id such as `DAC`),
    /// for the reservation. Numeric cards are not asked.
    fn card_index(&mut self, card: &str) -> Option<u32>;
    /// Open the PCM `name` for blocking playback.
    fn open(&mut self, name: &str) -> Result<(), PcmError>;
    /// The sample formats the open device accepts with interleaved access,
    /// exactly `rate` and `channels`, and resampling on or off. Empty when
    /// the rate or channel count is refused. Sets nothing.
    fn accepted_formats(
        &mut self,
        rate: u32,
        channels: u16,
        resample: bool,
    ) -> Result<Vec<SampleFormat>, PcmError>;
    /// Install `config` (interleaved access) and prepare the device.
    fn configure(&mut self, config: &HwConfig) -> Result<(), PcmError>;
    /// Write whole frames packed in the configured format; returns the frames
    /// written. Blocks until there is room.
    fn write(&mut self, bytes: &[u8]) -> Result<usize, PcmError>;
    /// `snd_pcm_delay`: frames written but not yet played.
    fn delay(&self) -> Result<i64, PcmError>;
    /// Stop or restart playback, keeping the buffered frames.
    fn pause(&mut self, paused: bool) -> Result<(), PcmError>;
    /// `snd_pcm_drop`: discard the buffered frames.
    fn drop_frames(&mut self) -> Result<(), PcmError>;
    /// `snd_pcm_prepare`: make the device ready for writes again.
    fn prepare(&mut self) -> Result<(), PcmError>;
    /// `snd_pcm_recover` after [`PcmError::Underrun`] or [`PcmError::Suspended`].
    fn recover(&mut self, error: &PcmError) -> Result<(), PcmError>;
    /// Close the device, if open. Idempotent.
    fn close(&mut self);
}

/// Devices whose exclusive negotiation was refused, for the rest of the
/// process: later opens go straight to `plughw:`. Clones share the set.
#[derive(Debug, Clone, Default)]
pub struct FallbackMemo(Arc<Mutex<HashSet<String>>>);

impl FallbackMemo {
    pub fn contains(&self, device: &str) -> bool {
        self.0.lock().is_ok_and(|set| set.contains(device))
    }

    pub fn insert(&self, device: &str) {
        if let Ok(mut set) = self.0.lock() {
            set.insert(device.to_owned());
        }
    }
}

/// The output kind a device name gets (spec 0003 "Output kinds").
pub fn output_kind(device: &str) -> OutputKind {
    if device.starts_with("hw:") {
        OutputKind::Exclusive
    } else if device.starts_with("plughw:") {
        OutputKind::Fallback
    } else {
        OutputKind::Shared
    }
}

/// Why the output is not bit-perfect, or `None` when it is (spec 0003
/// "Output kinds"): exclusive, lossless source, exact rate, stereo (a mono
/// source is copied to both channels), and a format that holds at least the
/// source's bits, so conversion is zero-padding only.
pub fn not_bit_perfect_reason(
    kind: OutputKind,
    source: &SourceFormat,
    format: SampleFormat,
    rate: u32,
    channels: u16,
) -> Option<String> {
    match kind {
        OutputKind::Shared => return Some("shared (system mixer)".into()),
        OutputKind::Fallback => return Some("plughw (ALSA may convert the samples)".into()),
        OutputKind::Exclusive => {}
    }
    let Some(bits) = source.bits_per_sample else {
        return Some("lossy source".into());
    };
    if rate != source.sample_rate {
        return Some(format!(
            "device runs at {} kHz, source is {} kHz",
            khz(rate),
            khz(source.sample_rate)
        ));
    }
    if channels != OUTPUT_CHANNELS || source.channels > OUTPUT_CHANNELS {
        return Some(format!(
            "{} source channels on a {channels}-channel device",
            source.channels
        ));
    }
    if format_bits(format) < bits {
        return Some(format!(
            "{bits}-bit source truncated to {}",
            format_name(format)
        ));
    }
    None
}

/// `44100` → `44.1`, `96000` → `96`.
fn khz(rate: u32) -> String {
    if rate.is_multiple_of(1000) {
        (rate / 1000).to_string()
    } else {
        format!("{}", f64::from(rate) / 1000.0)
    }
}

/// The reason shown when an exclusive device refused the source.
fn refused_reason(source: &SourceFormat) -> String {
    let formats: Vec<&str> = preference(source.bits_per_sample)
        .iter()
        .map(|f| format_name(*f))
        .collect();
    format!(
        "resampled (plughw fallback: device refused {} at {} kHz)",
        formats.join("/"),
        khz(source.sample_rate)
    )
}

/// The card part of `hw:C,D`, `hw:C`, `plughw:CARD=id,DEV=d`, …
fn card_of(device: &str) -> Option<&str> {
    let rest = device
        .strip_prefix("hw:")
        .or_else(|| device.strip_prefix("plughw:"))?;
    let card = rest.split(',').next().unwrap_or(rest);
    Some(card.strip_prefix("CARD=").unwrap_or(card)).filter(|c| !c.is_empty())
}

fn pcm_error(device: &str, error: PcmError) -> SinkError {
    match error {
        PcmError::Busy => SinkError::Busy {
            device: device.to_owned(),
            holder: None,
        },
        PcmError::NotFound => SinkError::NotFound(device.to_owned()),
        PcmError::Lost => SinkError::Lost(device.to_owned()),
        PcmError::Underrun => SinkError::Backend(format!("{device}: underrun")),
        PcmError::Suspended => SinkError::Backend(format!("{device}: suspended")),
        PcmError::Refused(e) | PcmError::Other(e) => SinkError::Backend(format!("{device}: {e}")),
    }
}

/// A successfully opened output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Opened {
    pub info: OutputInfo,
    pub config: HwConfig,
    /// The card whose ReserveDevice1 name is held; release it on close.
    pub reserved: Option<u32>,
}

/// Open `device` for `source` (AC12): reserve the card (`hw:`, `plughw:`),
/// open with the `EBUSY` budget, negotiate the exact rate and the preferred
/// format, and fall back to `plughw:` on the same card when an exclusive
/// device refuses (remembered in `memo`). A busy device is never downgraded.
/// On error nothing is left open or reserved.
pub fn open_output<B: PcmBackend + ?Sized>(
    backend: &mut B,
    reserver: &mut dyn Reserver,
    clock: &dyn Clock,
    memo: &FallbackMemo,
    device: &str,
    source: &SourceFormat,
) -> Result<Opened, SinkError> {
    let kind = output_kind(device);
    let reserved = match kind {
        OutputKind::Shared => None,
        OutputKind::Exclusive | OutputKind::Fallback => {
            let card = card_of(device).ok_or_else(|| SinkError::NotFound(device.to_owned()))?;
            let index = match card.parse() {
                Ok(index) => index,
                Err(_) => backend
                    .card_index(card)
                    .ok_or_else(|| SinkError::NotFound(device.to_owned()))?,
            };
            reserve(reserver, clock, index, device)?
        }
    };
    match open_reserved(backend, clock, memo, device, kind, source) {
        Ok((info, config)) => Ok(Opened {
            info,
            config,
            reserved,
        }),
        Err(error) => {
            backend.close();
            if let Some(card) = reserved {
                reserver.release(card);
            }
            Err(error)
        }
    }
}

fn open_reserved<B: PcmBackend + ?Sized>(
    backend: &mut B,
    clock: &dyn Clock,
    memo: &FallbackMemo,
    device: &str,
    kind: OutputKind,
    source: &SourceFormat,
) -> Result<(OutputInfo, HwConfig), SinkError> {
    if kind == OutputKind::Exclusive && !memo.contains(device) {
        open_retrying(backend, clock, device)?;
        if let Some(config) = negotiate(backend, device, kind, source)? {
            return Ok((info(device, device, kind, source, &config, None), config));
        }
        backend.close();
        memo.insert(device);
    }
    let (opened, reason) = match kind {
        OutputKind::Exclusive => (format!("plug{device}"), Some(refused_reason(source))),
        OutputKind::Fallback | OutputKind::Shared => (device.to_owned(), None),
    };
    let kind = match kind {
        OutputKind::Exclusive => OutputKind::Fallback,
        other => other,
    };
    open_retrying(backend, clock, &opened)?;
    let config = negotiate(backend, &opened, kind, source)?.ok_or_else(|| {
        SinkError::Backend(format!(
            "{opened} accepts no usable sample format at {} Hz",
            source.sample_rate
        ))
    })?;
    Ok((info(device, &opened, kind, source, &config, reason), config))
}

/// Open `name`, retrying `EBUSY` every 100 ms until 800 ms have passed.
fn open_retrying<B: PcmBackend + ?Sized>(
    backend: &mut B,
    clock: &dyn Clock,
    name: &str,
) -> Result<(), SinkError> {
    let start = clock.now();
    loop {
        match backend.open(name) {
            Ok(()) => return Ok(()),
            Err(PcmError::Busy) if clock.now() - start < BUSY_RETRY_BUDGET => {
                clock.sleep(BUSY_RETRY_INTERVAL);
            }
            Err(error) => return Err(pcm_error(name, error)),
        }
    }
}

/// Negotiate the open device for `source`: `None` when it refuses every
/// listed format at the exact rate and channel count.
fn negotiate<B: PcmBackend + ?Sized>(
    backend: &mut B,
    name: &str,
    kind: OutputKind,
    source: &SourceFormat,
) -> Result<Option<HwConfig>, SinkError> {
    let resample = kind != OutputKind::Exclusive;
    let accepted = backend
        .accepted_formats(source.sample_rate, OUTPUT_CHANNELS, resample)
        .map_err(|e| pcm_error(name, e))?;
    let format = match kind {
        // Shared: S32_LE first, then the same list as exclusive.
        OutputKind::Shared if accepted.contains(&SampleFormat::S32Le) => Some(SampleFormat::S32Le),
        _ => choose_format(source.bits_per_sample, &accepted),
    };
    let Some(format) = format else {
        return Ok(None);
    };
    let config = HwConfig {
        format,
        rate: source.sample_rate,
        channels: OUTPUT_CHANNELS,
        resample,
        period_frames: PERIOD_FRAMES,
        periods: match kind {
            OutputKind::Shared => SHARED_PERIODS,
            OutputKind::Exclusive | OutputKind::Fallback => EXCLUSIVE_PERIODS,
        },
    };
    match backend.configure(&config) {
        Ok(()) => Ok(Some(config)),
        Err(PcmError::Refused(_)) => Ok(None),
        Err(error) => Err(pcm_error(name, error)),
    }
}

fn info(
    requested: &str,
    device: &str,
    kind: OutputKind,
    source: &SourceFormat,
    config: &HwConfig,
    reason: Option<String>,
) -> OutputInfo {
    let reason = reason.or_else(|| {
        not_bit_perfect_reason(kind, source, config.format, config.rate, config.channels)
    });
    OutputInfo {
        requested: requested.to_owned(),
        device: device.to_owned(),
        kind,
        sample_format: config.format,
        sample_rate: config.rate,
        channels: config.channels,
        bit_perfect: reason.is_none(),
        not_bit_perfect_reason: reason,
    }
}

#[cfg(test)]
mod tests;
