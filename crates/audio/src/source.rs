//! The byte side of the engine (spec 0003 "Fetching"): a [`TrackSource`] hands
//! the decoder a track's bytes, either one file addressable by byte offset or
//! an init segment followed by media segments (MPEG-DASH).
//!
//! The binary's `HttpSource` implements it over HTTP `Range` requests and
//! DASH segment URLs, with its own fetch thread reading ahead into a bounded
//! buffer; [`crate::testing::FakeSource`] implements it in memory for tests.
//!
//! # Contract
//!
//! The engine decodes on a worker thread that calls [`TrackSource::read`]
//! from inside the demuxer (symphonia), so:
//!
//! - `read` should **not block for long**. When the read-ahead buffer is
//!   empty, wait at most about 100 ms for data, then return
//!   [`ReadOutcome::Pending`]. The engine calls again; if its decoded queue
//!   has run dry meanwhile, it reports `Buffering` (spec AC23). `Pending` is
//!   how the engine tells "no data yet" from [`ReadOutcome::End`] (the
//!   stream's last byte was delivered) and from `Err` (the track failed:
//!   return the error only after the retries of spec AC24 are spent).
//! - A `read` that blocks anyway does not stop the engine: on `Stop`,
//!   `Shutdown`, a new `Play` or a seek on a segmented stream, the engine
//!   abandons the worker without waiting for it (spec AC22). The source is
//!   dropped when the stuck `read` returns, so implementations should still
//!   bound their waits (socket timeouts) to free their thread eventually.
//! - Single-file streams are repositioned with [`TrackSource::seek`] (the
//!   demuxer jumps by byte offset using the FLAC seek table or the MP4
//!   sample table); segmented ones with [`TrackSource::restart_at_segment`]
//!   (no segment before the target is fetched again, AC19).
//! - The source is used from one thread at a time, but moves between threads
//!   (`Send`).

use std::fmt;

/// Why a track's bytes could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SourceError {
    /// Transport failure that outlasted the retries (spec AC24).
    #[error("network error: {0}")]
    Network(String),
    /// The source cannot reposition this way (e.g. `seek` on a segmented
    /// stream).
    #[error("the stream cannot be repositioned")]
    Unseekable,
    /// Anything else (expired URL that could not be renewed, HTTP status, …).
    #[error("{0}")]
    Other(String),
}

/// Result of a successful [`TrackSource::read`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadOutcome {
    /// This many bytes were copied to the start of the buffer (at least 1).
    Data(usize),
    /// No byte is available yet (the fetcher is behind): call again.
    Pending,
    /// The end of the stream: every byte has been delivered.
    End,
}

/// One media segment of a segmented stream, in timescale ticks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SegmentSpan {
    /// Start time of the segment's first frame.
    pub start: u64,
    /// Duration of the segment.
    pub duration: u64,
}

impl SegmentSpan {
    /// End of the segment (exclusive), in ticks.
    pub fn end(&self) -> u64 {
        self.start + self.duration
    }
}

/// How a track's bytes are laid out, which decides how the engine seeks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceLayout {
    /// One file (BTS manifest). Seekable by byte offset.
    SingleFile {
        /// Total length in bytes, when known (needed for `SeekFrom::End`).
        len: Option<u64>,
    },
    /// An init segment followed by media segments (DASH `SegmentTimeline`),
    /// read as one non-seekable stream.
    Segmented {
        /// Ticks per second of `segments`.
        timescale: u32,
        /// Every media segment in order, starting with the first.
        segments: Vec<SegmentSpan>,
    },
}

/// A track's bytes, as the engine reads them. See the module docs for the
/// contract.
pub trait TrackSource: Send {
    /// The stream's layout. Called once, before the first read.
    fn layout(&self) -> SourceLayout;

    /// A file extension that helps the demuxer guess the container
    /// (`"flac"`, `"mp4"`), if known.
    fn extension_hint(&self) -> Option<String> {
        None
    }

    /// Copy the next bytes into `buf` (see [`ReadOutcome`]).
    fn read(&mut self, buf: &mut [u8]) -> Result<ReadOutcome, SourceError>;

    /// Single-file streams: continue reading at byte `offset`.
    fn seek(&mut self, offset: u64) -> Result<(), SourceError> {
        let _ = offset;
        Err(SourceError::Unseekable)
    }

    /// Segmented streams: from now on, reads return the init segment and
    /// then the media segments from `index` (0-based) on, in order.
    fn restart_at_segment(&mut self, index: usize) -> Result<(), SourceError> {
        let _ = index;
        Err(SourceError::Unseekable)
    }
}

impl fmt::Debug for dyn TrackSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TrackSource")
            .field("layout", &self.layout())
            .finish_non_exhaustive()
    }
}
