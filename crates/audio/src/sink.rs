//! The audio output seam: a [`Sink`] receives PCM samples. Real backends
//! (ALSA) and the in-memory [`MemorySink`] used by tests implement it.

/// PCM stream format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioFormat {
    /// Frames per second.
    pub sample_rate: u32,
    /// Number of interleaved channels.
    pub channels: u16,
    /// Significant bits of the source material (e.g. 16 or 24).
    pub bits_per_sample: u16,
}

/// Errors reported by a [`Sink`].
#[derive(Debug, thiserror::Error)]
pub enum SinkError {
    /// `write` was called before a successful `open`.
    #[error("sink written to before it was opened")]
    NotOpen,
    /// The backend failed or refused the requested format.
    #[error("audio backend error: {0}")]
    Backend(String),
}

/// A destination for decoded audio.
///
/// Samples are interleaved and left-justified in 32 bits.
pub trait Sink {
    /// Prepare the sink for a stream in `format`.
    fn open(&mut self, format: AudioFormat) -> Result<(), SinkError>;
    /// Write interleaved samples; fails with [`SinkError::NotOpen`] before `open`.
    fn write(&mut self, samples: &[i32]) -> Result<(), SinkError>;
}

/// A [`Sink`] that records everything written to it, for tests.
#[derive(Debug, Default)]
pub struct MemorySink {
    format: Option<AudioFormat>,
    samples: Vec<i32>,
}

impl MemorySink {
    /// The format passed to `open`, if opened.
    pub fn format(&self) -> Option<AudioFormat> {
        self.format
    }

    /// All samples written so far, in order.
    pub fn samples(&self) -> &[i32] {
        &self.samples
    }
}

impl Sink for MemorySink {
    fn open(&mut self, format: AudioFormat) -> Result<(), SinkError> {
        self.format = Some(format);
        Ok(())
    }

    fn write(&mut self, samples: &[i32]) -> Result<(), SinkError> {
        if self.format.is_none() {
            return Err(SinkError::NotOpen);
        }
        self.samples.extend_from_slice(samples);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FORMAT: AudioFormat = AudioFormat {
        sample_rate: 44_100,
        channels: 2,
        bits_per_sample: 16,
    };

    #[test]
    fn ac7_memory_sink_records_samples() {
        let mut sink = MemorySink::default();
        sink.open(FORMAT).unwrap();
        sink.write(&[1, 2, 3, 4]).unwrap();
        sink.write(&[5, 6]).unwrap();
        assert_eq!(sink.format(), Some(FORMAT));
        assert_eq!(sink.samples(), &[1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn ac7_write_before_open_fails() {
        let mut sink = MemorySink::default();
        assert!(matches!(sink.write(&[1]), Err(SinkError::NotOpen)));
        assert!(sink.samples().is_empty());
    }
}
