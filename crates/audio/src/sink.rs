//! The audio output seam (spec 0003): the engine writes decoded PCM into a
//! [`Sink`]. The ALSA output and the in-memory [`MemorySink`] used by tests
//! implement it.
//!
//! Samples are interleaved `i32`, left-justified: a 16-bit sample `s` is
//! `s << 16` (spec 0001 AC7, spec 0003 "Decode").

/// Codec of the decoded source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Codec {
    /// FLAC, raw or in (fragmented) MP4. Lossless.
    Flac,
    /// AAC-LC in MP4. Lossy.
    AacLc,
}

/// What the decoder found in the stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceFormat {
    pub codec: Codec,
    /// Frames per second.
    pub sample_rate: u32,
    /// Channels in the source (the engine always outputs stereo, AC9).
    pub channels: u16,
    /// Significant bits per sample; `None` for lossy sources.
    pub bits_per_sample: Option<u16>,
}

impl SourceFormat {
    /// Whether the source is lossless (a precondition for `bit_perfect`).
    pub fn is_lossless(&self) -> bool {
        self.bits_per_sample.is_some()
    }
}

/// PCM sample formats the output may negotiate (spec 0003 "Output (ALSA)").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SampleFormat {
    /// 16-bit little-endian.
    S16Le,
    /// 24 bits in the low 3 bytes of a 4-byte little-endian word, sign-extended.
    S24Le,
    /// 24-bit, 3 bytes little-endian.
    S24_3Le,
    /// 32-bit little-endian.
    S32Le,
}

/// How the output reaches the device (spec 0003 "Output kinds").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputKind {
    /// `hw:` opened directly, card reserved.
    Exclusive,
    /// `plughw:`, given by the user or after a refused negotiation.
    Fallback,
    /// `default` or any other PCM name: through the system mixer.
    Shared,
}

/// What [`Sink::open`] actually opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputInfo {
    /// The device name the sink was created for.
    pub requested: String,
    /// The device actually opened (differs on the `plughw:` fallback).
    pub device: String,
    pub kind: OutputKind,
    pub sample_format: SampleFormat,
    pub sample_rate: u32,
    pub channels: u16,
    /// Whether samples reach the device untouched (spec 0003 "Output kinds").
    pub bit_perfect: bool,
    /// Why `bit_perfect` is false, for the user (`None` when it is true).
    pub not_bit_perfect_reason: Option<String>,
}

/// Errors reported by a [`Sink`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SinkError {
    /// Called before a successful `open`.
    #[error("sink used before it was opened")]
    NotOpen,
    /// The device is in use (`EBUSY` past the retry budget, or the
    /// reservation was refused or timed out). Never downgraded.
    #[error("output {device} is busy{}", holder.as_deref().map(|h| format!(" (used by {h})")).unwrap_or_default())]
    Busy {
        device: String,
        holder: Option<String>,
    },
    /// No such device.
    #[error("no such output device {0}")]
    NotFound(String),
    /// The device went away while open (`ENODEV`, `EIO`, …).
    #[error("output {0} was lost")]
    Lost(String),
    /// Any other backend failure.
    #[error("audio backend error: {0}")]
    Backend(String),
}

/// Result of one [`Sink::write`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WriteOutcome {
    /// Frames consumed from the start of the buffer (may be fewer than given).
    pub frames: usize,
    /// The device underran (or was suspended) and has been recovered.
    /// Frames not consumed must be written again (spec 0003 AC14).
    pub underrun: bool,
}

/// A destination for decoded audio. Owned and driven by the engine thread.
pub trait Sink: Send {
    /// Open the device for `source` (always output as stereo) and negotiate
    /// the format. Re-opening an open sink closes it first.
    fn open(&mut self, source: &SourceFormat) -> Result<OutputInfo, SinkError>;
    /// Write interleaved frames (`samples.len()` is a multiple of the output
    /// channel count). May consume fewer frames than given; never blocks
    /// longer than about one device period.
    fn write(&mut self, samples: &[i32]) -> Result<WriteOutcome, SinkError>;
    /// Frames written but not yet heard (the device's delay).
    fn delay_frames(&self) -> Result<u64, SinkError>;
    /// Stop (`true`) or restart (`false`) output, keeping buffered frames.
    fn set_paused(&mut self, paused: bool) -> Result<(), SinkError>;
    /// Drop buffered frames without playing them (used by seek).
    fn discard(&mut self) -> Result<(), SinkError>;
    /// Block until every buffered frame has been played.
    fn drain(&mut self) -> Result<(), SinkError>;
    /// Close the device and release any reservation. Idempotent.
    fn close(&mut self);
}

/// Creates a [`Sink`] for a device name (`SetDevice`, spec 0003 AC21).
pub trait SinkFactory: Send {
    fn create(&mut self, device: &str) -> Box<dyn Sink>;
}

/// A [`Sink`] that records everything written to it, for tests.
#[derive(Debug, Default)]
pub struct MemorySink {
    source: Option<SourceFormat>,
    samples: Vec<i32>,
}

impl MemorySink {
    /// The source format passed to `open`, if open.
    pub fn source(&self) -> Option<SourceFormat> {
        self.source
    }

    /// All samples written so far, in order.
    pub fn samples(&self) -> &[i32] {
        &self.samples
    }
}

impl Sink for MemorySink {
    fn open(&mut self, source: &SourceFormat) -> Result<OutputInfo, SinkError> {
        self.source = Some(*source);
        Ok(OutputInfo {
            requested: "memory".into(),
            device: "memory".into(),
            kind: OutputKind::Exclusive,
            sample_format: SampleFormat::S32Le,
            sample_rate: source.sample_rate,
            channels: 2,
            bit_perfect: source.is_lossless(),
            not_bit_perfect_reason: (!source.is_lossless()).then(|| "lossy source".into()),
        })
    }

    fn write(&mut self, samples: &[i32]) -> Result<WriteOutcome, SinkError> {
        if self.source.is_none() {
            return Err(SinkError::NotOpen);
        }
        self.samples.extend_from_slice(samples);
        Ok(WriteOutcome {
            frames: samples.len() / 2,
            underrun: false,
        })
    }

    fn delay_frames(&self) -> Result<u64, SinkError> {
        Ok(0)
    }

    fn set_paused(&mut self, _paused: bool) -> Result<(), SinkError> {
        Ok(())
    }

    fn discard(&mut self) -> Result<(), SinkError> {
        Ok(())
    }

    fn drain(&mut self) -> Result<(), SinkError> {
        Ok(())
    }

    fn close(&mut self) {
        self.source = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE: SourceFormat = SourceFormat {
        codec: Codec::Flac,
        sample_rate: 44_100,
        channels: 2,
        bits_per_sample: Some(16),
    };

    #[test]
    fn ac7_memory_sink_records_samples() {
        let mut sink = MemorySink::default();
        sink.open(&SOURCE).unwrap();
        sink.write(&[1, 2, 3, 4]).unwrap();
        sink.write(&[5, 6]).unwrap();
        assert_eq!(sink.source(), Some(SOURCE));
        assert_eq!(sink.samples(), &[1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn ac7_write_before_open_fails() {
        let mut sink = MemorySink::default();
        assert!(matches!(sink.write(&[1, 2]), Err(SinkError::NotOpen)));
        assert!(sink.samples().is_empty());
    }
}
