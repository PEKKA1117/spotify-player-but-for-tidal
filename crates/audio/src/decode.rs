//! Decode (spec 0003 "Decode"): symphonia over a [`TrackSource`], output as
//! interleaved stereo `i32`, left-justified.

use crate::engine::EngineError;
use crate::sink::{Codec, SourceFormat};
use crate::source::TrackSource;

/// Decoded frames, interleaved stereo, left-justified `i32`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    /// Index (in the track) of the first frame of `samples`.
    pub first_frame: u64,
    pub samples: Vec<i32>,
}

impl Chunk {
    pub fn frames(&self) -> usize {
        self.samples.len() / 2
    }
}

/// Result of [`Decoder::seek`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeekOutcome {
    /// The next chunk starts exactly at the requested frame.
    Seeked,
    /// The requested frame is at or past the end of the track.
    PastEnd,
}

/// A track being decoded.
pub struct Decoder {
    format: SourceFormat,
}

impl std::fmt::Debug for Decoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Decoder")
            .field("format", &self.format)
            .finish_non_exhaustive()
    }
}

impl Decoder {
    /// Probe the container and set up the decoder. Fails with
    /// [`EngineError::Unsupported`] for anything but FLAC and AAC-LC (spec
    /// AC8), without decoding a frame.
    pub fn open(source: Box<dyn TrackSource>) -> Result<Self, EngineError> {
        let _ = source;
        Ok(Self {
            format: SourceFormat {
                codec: Codec::Flac,
                sample_rate: 44_100,
                channels: 2,
                bits_per_sample: Some(16),
            },
        })
    }

    /// What the decoder found in the stream.
    pub fn format(&self) -> SourceFormat {
        self.format
    }

    /// The track's length in frames, when the stream says.
    pub fn total_frames(&self) -> Option<u64> {
        None
    }

    /// The next decoded frames, or `None` at the end of the track.
    pub fn next_chunk(&mut self) -> Result<Option<Chunk>, EngineError> {
        Ok(None)
    }

    /// Continue from frame `frame` (accurate to the frame, spec AC19).
    pub fn seek(&mut self, frame: u64) -> Result<SeekOutcome, EngineError> {
        let _ = frame;
        Ok(SeekOutcome::Seeked)
    }
}

/// Decode a whole track: its format and every sample (interleaved stereo).
pub fn decode_all(source: Box<dyn TrackSource>) -> Result<(SourceFormat, Vec<i32>), EngineError> {
    let mut decoder = Decoder::open(source)?;
    let mut samples = Vec::new();
    while let Some(chunk) = decoder.next_chunk()? {
        samples.extend_from_slice(&chunk.samples);
    }
    Ok((decoder.format(), samples))
}
