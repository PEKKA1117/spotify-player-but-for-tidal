//! `HttpSource`: a resolved Tidal stream as a [`TrackSource`] (spec 0003
//! "Fetching", AC24).
//!
//! A tokio task fetches the bytes and reads ahead into a bounded buffer; the
//! engine's [`TrackSource::read`] takes them from there and never waits on
//! the network for more than [`READ_WAIT`]. Single-file streams are fetched
//! with HTTP `Range` requests from the reader's byte offset; segmented
//! (DASH) streams fetch the init segment and then the media segments in
//! order. Transport errors, `5xx` answers and bodies cut short are retried
//! from the exact offset reached, after the configured delays (0.5, 1, 2 s);
//! a `403`/`410` re-resolves the track once and continues on the new URLs.

use std::sync::Arc;
use std::time::Duration;

use tidal_player_api::auth::{BoxFuture, Sleeper, SystemClock};
use tidal_player_api::stream::{ResolvedStream, StreamError};
use tidal_player_audio::{ReadOutcome, SourceError, SourceLayout, TrackSource};

/// The longest a [`TrackSource::read`] waits for data before answering
/// `Pending` (the source contract's "about 100 ms").
pub const READ_WAIT: Duration = Duration::from_millis(100);
/// The read-ahead cap (spec 0003 "Fetching": at most 16 MiB).
pub const MAX_READ_AHEAD: usize = 16 * 1024 * 1024;
/// Audio the read-ahead aims to hold.
pub const READ_AHEAD_TARGET: Duration = Duration::from_secs(10);
/// Waits before each retry; the track fails after the last.
pub const RETRY_DELAYS: [Duration; 3] = [
    Duration::from_millis(500),
    Duration::from_secs(1),
    Duration::from_secs(2),
];
/// How long a connection or a body chunk may take before the attempt counts
/// as a transport error.
pub const NETWORK_TIMEOUT: Duration = Duration::from_secs(10);

/// Resolves the track again, for a stream URL that expired (`403`/`410`).
pub type Reresolve =
    Arc<dyn Fn() -> BoxFuture<'static, Result<ResolvedStream, StreamError>> + Send + Sync>;

/// How an [`HttpSource`] fetches.
#[derive(Clone)]
pub struct HttpSourceConfig {
    /// The most bytes held ahead of the reader.
    pub read_ahead: usize,
    /// Wait before each retry; as many retries as delays.
    pub retry_delays: Vec<Duration>,
    /// Bound on each connection and each body chunk.
    pub network_timeout: Duration,
    /// Sleeps between retries (tests inject one that does not wait).
    pub sleeper: Arc<dyn Sleeper>,
}

impl std::fmt::Debug for HttpSourceConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpSourceConfig")
            .field("read_ahead", &self.read_ahead)
            .field("retry_delays", &self.retry_delays)
            .field("network_timeout", &self.network_timeout)
            .finish_non_exhaustive()
    }
}

impl HttpSourceConfig {
    /// The spec's budgets for `stream`: 10 s of audio at the stream's rate
    /// and bit depth (uncompressed, so an upper bound), at most 16 MiB.
    pub fn for_stream(stream: &ResolvedStream) -> Self {
        let read_ahead = match (stream.sample_rate, stream.bit_depth) {
            (Some(rate), Some(bits)) => {
                let per_second = u64::from(rate) * 2 * u64::from(bits).div_ceil(8);
                usize::try_from(per_second * READ_AHEAD_TARGET.as_secs())
                    .unwrap_or(MAX_READ_AHEAD)
                    .min(MAX_READ_AHEAD)
            }
            _ => MAX_READ_AHEAD,
        };
        Self {
            read_ahead,
            retry_delays: RETRY_DELAYS.to_vec(),
            network_timeout: NETWORK_TIMEOUT,
            sleeper: Arc::new(SystemClock),
        }
    }
}

/// A resolved stream read over HTTP. See the module docs.
pub struct HttpSource {
    layout: SourceLayout,
}

impl HttpSource {
    /// Starts fetching `stream` on the current tokio runtime. For a
    /// single-file stream, waits for the first response (its length is the
    /// layout's); fails as a read would, after the retries.
    pub async fn open(
        client: reqwest::Client,
        stream: ResolvedStream,
        reresolve: Reresolve,
        config: HttpSourceConfig,
    ) -> Result<Self, SourceError> {
        let _ = (client, stream, reresolve, config);
        Ok(Self {
            layout: SourceLayout::SingleFile { len: None },
        })
    }

    /// Bytes fetched and not yet read.
    pub fn buffered(&self) -> usize {
        0
    }

    /// The most bytes ever held ahead of the reader.
    pub fn high_water(&self) -> usize {
        0
    }

    /// Waits until at least `bytes` are buffered (or the stream ended, or
    /// failed), for at most `timeout`; whether `bytes` are buffered.
    pub fn wait_buffered(&self, bytes: usize, timeout: Duration) -> bool {
        let _ = (bytes, timeout);
        false
    }
}

impl TrackSource for HttpSource {
    fn layout(&self) -> SourceLayout {
        self.layout.clone()
    }

    fn read(&mut self, buf: &mut [u8]) -> Result<ReadOutcome, SourceError> {
        let _ = buf;
        Ok(ReadOutcome::End)
    }
}
