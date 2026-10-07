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

use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use reqwest::StatusCode;
use reqwest::header::{CONTENT_RANGE, RANGE};
use tidal_player_api::auth::{BoxFuture, Sleeper, SystemClock};
use tidal_player_api::stream::{Codec, ResolvedStream, StreamError, StreamPlan};
use tidal_player_audio::{ReadOutcome, SegmentSpan, SourceError, SourceLayout, TrackSource};
use tokio::sync::{Notify, oneshot};

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
///
/// Dropping it stops the fetch task.
pub struct HttpSource {
    shared: Arc<Shared>,
    layout: SourceLayout,
    hint: &'static str,
}

impl std::fmt::Debug for HttpSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpSource")
            .field("layout", &self.layout)
            .finish_non_exhaustive()
    }
}

/// Where the fetcher is in the stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Position {
    /// Single file: the next byte to fetch.
    Single(u64),
    /// Segmented: in the init segment, to be followed by media segment
    /// `then` (0-based).
    Init { offset: u64, then: usize },
    /// Segmented: in media segment `index` (0-based).
    Media { index: usize, offset: u64 },
}

impl Position {
    fn offset(self) -> u64 {
        match self {
            Self::Single(offset) | Self::Init { offset, .. } | Self::Media { offset, .. } => offset,
        }
    }

    fn advance(&mut self, by: u64) {
        match self {
            Self::Single(offset) | Self::Init { offset, .. } | Self::Media { offset, .. } => {
                *offset += by;
            }
        }
    }
}

/// The read-ahead buffer and the reader's requests to the fetcher.
#[derive(Debug)]
struct Buffer {
    data: VecDeque<u8>,
    cap: usize,
    high_water: usize,
    /// Every byte up to the end of the stream is in `data`.
    end: bool,
    /// The fetch failed for good (until the next reposition).
    error: Option<SourceError>,
    /// Where the reader wants the fetcher to continue.
    reposition: Option<Position>,
    /// The source was dropped.
    closed: bool,
}

#[derive(Debug)]
struct Shared {
    buffer: Mutex<Buffer>,
    /// Wakes the reader: data, end or error arrived.
    readable: Condvar,
    /// Wakes the fetcher: space was freed, a reposition or close asked.
    fetcher: Notify,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Buffer> {
        self.buffer.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Whether the fetcher must drop what it is doing.
    fn interrupted(&self) -> bool {
        let b = self.lock();
        b.closed || b.reposition.is_some()
    }

    /// Resolves once the source is dropped or repositioned.
    async fn until_interrupted(&self) {
        loop {
            let notified = self.fetcher.notified();
            if self.interrupted() {
                return;
            }
            notified.await;
        }
    }

    /// Marks the end of the stream, unless the reader moved meanwhile.
    fn set_end(&self) {
        let mut b = self.lock();
        if b.reposition.is_none() {
            b.end = true;
            self.readable.notify_all();
        }
    }

    /// Fails the stream, unless the reader moved meanwhile.
    fn set_error(&self, error: SourceError) {
        let mut b = self.lock();
        if b.reposition.is_none() {
            b.error = Some(error);
            self.readable.notify_all();
        }
    }
}

/// The URLs the fetcher reads from (replaced on a re-resolve).
#[derive(Debug, Clone)]
enum Urls {
    Single(String),
    Segmented { init: String, segments: Vec<String> },
}

impl Urls {
    fn of(plan: &StreamPlan) -> Self {
        match plan {
            StreamPlan::Single { url, .. } => Self::Single(url.clone()),
            StreamPlan::Segmented {
                init_url, segments, ..
            } => Self::Segmented {
                init: init_url.clone(),
                segments: segments.iter().map(|s| s.url.clone()).collect(),
            },
        }
    }

    /// The URL to fetch at `position`, or `None` past the last segment.
    fn at(&self, position: Position) -> Option<&str> {
        match (self, position) {
            (Self::Single(url), Position::Single(_)) => Some(url),
            (Self::Segmented { init, .. }, Position::Init { .. }) => Some(init),
            (Self::Segmented { segments, .. }, Position::Media { index, .. }) => {
                segments.get(index).map(String::as_str)
            }
            _ => None,
        }
    }
}

/// Whether a re-resolved stream can continue where the old one stopped:
/// same codec, rate, bit depth and layout (spec 0003 "Fetching").
fn compatible(old: &ResolvedStream, new: &ResolvedStream) -> bool {
    let shape = match (&old.plan, &new.plan) {
        (StreamPlan::Single { .. }, StreamPlan::Single { .. }) => true,
        (StreamPlan::Segmented { segments: a, .. }, StreamPlan::Segmented { segments: b, .. }) => {
            a.len() == b.len()
        }
        _ => false,
    };
    shape
        && old.plan.codec() == new.plan.codec()
        && old.sample_rate == new.sample_rate
        && old.bit_depth == new.bit_depth
}

/// `bytes 400-999/1000` → `(400, 999, Some(1000))`.
fn parse_content_range(value: &str) -> Option<(u64, u64, Option<u64>)> {
    let range = value.trim().strip_prefix("bytes ")?;
    let (span, total) = range.split_once('/')?;
    let (first, last) = span.split_once('-')?;
    Some((
        first.trim().parse().ok()?,
        last.trim().parse().ok()?,
        total.trim().parse().ok(),
    ))
}

/// How one request ended.
#[derive(Debug)]
enum Attempt {
    /// The part (file or segment) was delivered to its end.
    Done,
    /// The reader moved or the source was dropped.
    Interrupted,
    /// The request failed; `progressed` if it delivered bytes first.
    Failed { progressed: bool, failure: Failure },
}

#[derive(Debug)]
enum Failure {
    /// Transport error, timeout, `5xx`/`429`, body cut short: retried.
    Retryable(String),
    /// `403`/`410`: the URL expired, re-resolve.
    Expired(StatusCode),
    /// Anything else: the track fails.
    Fatal(String),
}

/// The fetch task.
struct Fetcher {
    client: reqwest::Client,
    shared: Arc<Shared>,
    stream: ResolvedStream,
    urls: Urls,
    /// Single file: its length, once a response said.
    len: Option<u64>,
    reresolve: Reresolve,
    config: HttpSourceConfig,
    /// A `403`/`410` may re-resolve (once, until the new URL served data).
    may_reresolve: bool,
    /// Single file: tells `open` the length (or the failure).
    opened: Option<oneshot::Sender<Result<Option<u64>, SourceError>>>,
}

impl Fetcher {
    async fn run(mut self, mut position: Position) {
        let mut failures = 0;
        loop {
            let idle = {
                let mut b = self.shared.lock();
                if b.closed {
                    return;
                }
                if let Some(to) = b.reposition.take() {
                    position = to;
                    failures = 0;
                    b.end = false;
                    b.error = None;
                }
                b.end || b.error.is_some()
            };
            if idle {
                self.shared.until_interrupted().await;
                continue;
            }
            let past_end = matches!(
                (position, self.len),
                (Position::Single(offset), Some(len)) if offset >= len
            );
            let Some(url) = self.urls.at(position).filter(|_| !past_end) else {
                self.shared.set_end();
                continue;
            };
            let url = url.to_owned();
            match self.fetch(&url, &mut position).await {
                Attempt::Done => {
                    failures = 0;
                    match position {
                        Position::Single(_) => self.shared.set_end(),
                        Position::Init { then, .. } => {
                            position = Position::Media {
                                index: then,
                                offset: 0,
                            };
                        }
                        Position::Media { index, .. } => {
                            position = Position::Media {
                                index: index + 1,
                                offset: 0,
                            };
                        }
                    }
                }
                Attempt::Interrupted => {}
                Attempt::Failed {
                    progressed,
                    failure,
                } => {
                    if progressed {
                        failures = 0;
                    }
                    match failure {
                        Failure::Retryable(message) => {
                            let Some(&delay) = self.config.retry_delays.get(failures) else {
                                self.fail(SourceError::Network(message));
                                continue;
                            };
                            failures += 1;
                            tracing::warn!(
                                attempt = failures,
                                ?delay,
                                "stream fetch failed: {message}; retrying"
                            );
                            self.sleep(delay).await;
                        }
                        Failure::Expired(status) => self.renew(status).await,
                        Failure::Fatal(message) => self.fail(SourceError::Other(message)),
                    }
                }
            }
        }
    }

    /// Fails the stream, and `open` if it is still waiting.
    fn fail(&mut self, error: SourceError) {
        if let Some(opened) = self.opened.take() {
            let _ = opened.send(Err(error.clone()));
        }
        self.shared.set_error(error);
    }

    /// Waits `delay` before a retry; cut short only by a reposition or
    /// close (the loop then looks at them).
    async fn sleep(&self, delay: Duration) {
        tokio::select! {
            () = self.config.sleeper.sleep(delay) => {}
            () = self.shared.until_interrupted() => {}
        }
    }

    /// Re-resolves the track after an expired URL, once (spec AC24).
    async fn renew(&mut self, status: StatusCode) {
        if !self.may_reresolve {
            self.fail(SourceError::Other(format!(
                "the stream URL was refused again (HTTP {}) after renewing it",
                status.as_u16()
            )));
            return;
        }
        self.may_reresolve = false;
        tracing::info!(
            status = status.as_u16(),
            "stream URL expired; re-resolving the track"
        );
        match (self.reresolve)().await {
            Ok(new) if compatible(&self.stream, &new) => {
                self.urls = Urls::of(&new.plan);
                self.stream = new;
            }
            Ok(_) => self.fail(SourceError::Other(
                "the renewed stream has a different format".into(),
            )),
            Err(e) => self.fail(SourceError::Other(format!(
                "cannot renew the expired stream URL: {e}"
            ))),
        }
    }

    /// One request for the part at `position`, from its offset; advances
    /// `position` by every byte handed to the reader.
    async fn fetch(&mut self, url: &str, position: &mut Position) -> Attempt {
        let offset = position.offset();
        let single = matches!(position, Position::Single(_));
        let mut request = self.client.get(url);
        if single || offset > 0 {
            request = request.header(RANGE, format!("bytes={offset}-"));
        }
        let timeout = self.config.network_timeout;
        let failed = |progressed, failure| Attempt::Failed {
            progressed,
            failure,
        };
        let response = tokio::select! {
            r = tokio::time::timeout(timeout, request.send()) => r,
            () = self.shared.until_interrupted() => return Attempt::Interrupted,
        };
        let mut response = match response {
            Err(_) => return failed(false, Failure::Retryable("connection timed out".into())),
            Ok(Err(e)) => return failed(false, Failure::Retryable(e.without_url().to_string())),
            Ok(Ok(response)) => response,
        };
        let status = response.status();
        if status == StatusCode::FORBIDDEN || status == StatusCode::GONE {
            return failed(false, Failure::Expired(status));
        }
        if status == StatusCode::RANGE_NOT_SATISFIABLE && single {
            // At or past the end of the file.
            return Attempt::Done;
        }
        if status.is_server_error() || status == StatusCode::TOO_MANY_REQUESTS {
            return failed(
                false,
                Failure::Retryable(format!("HTTP {}", status.as_u16())),
            );
        }
        if !status.is_success() {
            return failed(
                false,
                Failure::Fatal(format!(
                    "the stream server answered HTTP {}",
                    status.as_u16()
                )),
            );
        }

        // Where this body starts and ends, in the part's bytes.
        let (mut skip, end, total) = if status == StatusCode::PARTIAL_CONTENT {
            let range = response
                .headers()
                .get(CONTENT_RANGE)
                .and_then(|v| v.to_str().ok())
                .and_then(parse_content_range);
            match range {
                Some((first, last, total)) if first == offset => (0, Some(last + 1), total),
                _ => {
                    return failed(
                        false,
                        Failure::Fatal("the stream server answered the wrong range".into()),
                    );
                }
            }
        } else {
            // A full body: skip what was delivered already.
            let len = response.content_length();
            (offset, len, len)
        };
        if single {
            match (self.len, total) {
                (Some(known), Some(total)) if known != total => {
                    return failed(false, Failure::Fatal("the stream changed length".into()));
                }
                (None, total) => self.len = total,
                _ => {}
            }
            if let Some(opened) = self.opened.take() {
                let _ = opened.send(Ok(self.len));
            }
        }

        let mut progressed = false;
        loop {
            let chunk = tokio::select! {
                c = tokio::time::timeout(timeout, response.chunk()) => c,
                () = self.shared.until_interrupted() => return Attempt::Interrupted,
            };
            let mut bytes = match chunk {
                Err(_) => {
                    return failed(progressed, Failure::Retryable("the stream stalled".into()));
                }
                Ok(Err(e)) => {
                    return failed(progressed, Failure::Retryable(e.without_url().to_string()));
                }
                Ok(Ok(None)) => break,
                Ok(Ok(Some(bytes))) => bytes,
            };
            let skipped = usize::try_from(skip).unwrap_or(usize::MAX).min(bytes.len());
            let _ = bytes.split_to(skipped);
            skip -= skipped as u64;
            if bytes.is_empty() {
                continue;
            }
            if !self.push(&bytes, position).await {
                return Attempt::Interrupted;
            }
            progressed = true;
            self.may_reresolve = true;
        }
        match end {
            Some(end) if position.offset() < end => failed(
                progressed,
                Failure::Retryable("the stream body was cut short".into()),
            ),
            _ => Attempt::Done,
        }
    }

    /// Hands `bytes` to the reader, waiting for space under the cap;
    /// `false` if interrupted (the rest is dropped).
    async fn push(&self, mut bytes: &[u8], position: &mut Position) -> bool {
        while !bytes.is_empty() {
            let notified = self.shared.fetcher.notified();
            {
                let mut b = self.shared.lock();
                if b.closed || b.reposition.is_some() {
                    return false;
                }
                let space = b.cap.saturating_sub(b.data.len());
                if space > 0 {
                    let n = space.min(bytes.len());
                    b.data.extend(&bytes[..n]);
                    b.high_water = b.high_water.max(b.data.len());
                    position.advance(n as u64);
                    bytes = &bytes[n..];
                    self.shared.readable.notify_all();
                    continue;
                }
            }
            notified.await;
        }
        true
    }
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
        let shared = Arc::new(Shared {
            buffer: Mutex::new(Buffer {
                data: VecDeque::new(),
                cap: config.read_ahead.max(1),
                high_water: 0,
                end: false,
                error: None,
                reposition: None,
                closed: false,
            }),
            readable: Condvar::new(),
            fetcher: Notify::new(),
        });
        let (layout, start, hint) = match &stream.plan {
            StreamPlan::Single { codec, .. } => (
                SourceLayout::SingleFile { len: None },
                Position::Single(0),
                if *codec == Codec::Flac { "flac" } else { "mp4" },
            ),
            StreamPlan::Segmented { segments, .. } => (
                SourceLayout::Segmented {
                    timescale: segments.first().map_or(1, |s| s.timescale),
                    segments: segments
                        .iter()
                        .map(|s| SegmentSpan {
                            start: s.start,
                            duration: s.duration,
                        })
                        .collect(),
                },
                Position::Init { offset: 0, then: 0 },
                "mp4",
            ),
        };
        let (opened, opened_rx) = oneshot::channel();
        let single = matches!(start, Position::Single(_));
        let fetcher = Fetcher {
            client,
            shared: shared.clone(),
            urls: Urls::of(&stream.plan),
            stream,
            len: None,
            reresolve,
            config,
            may_reresolve: true,
            opened: single.then_some(opened),
        };
        tokio::spawn(fetcher.run(start));
        let mut source = Self {
            shared,
            layout,
            hint,
        };
        if single {
            let len = opened_rx
                .await
                .map_err(|_| SourceError::Other("the stream fetcher stopped".into()))??;
            source.layout = SourceLayout::SingleFile { len };
        }
        Ok(source)
    }

    /// Bytes fetched and not yet read.
    pub fn buffered(&self) -> usize {
        self.shared.lock().data.len()
    }

    /// The most bytes ever held ahead of the reader.
    pub fn high_water(&self) -> usize {
        self.shared.lock().high_water
    }

    /// Waits until at least `bytes` are buffered (or the stream ended, or
    /// failed), for at most `timeout`; whether `bytes` are buffered.
    pub fn wait_buffered(&self, bytes: usize, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut b = self.shared.lock();
        loop {
            if b.data.len() >= bytes {
                return true;
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if b.end || b.error.is_some() || left.is_zero() {
                return false;
            }
            b = self
                .shared
                .readable
                .wait_timeout(b, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    /// Asks the fetcher to continue at `to`, dropping the read-ahead.
    fn reposition(&self, to: Position) {
        let mut b = self.shared.lock();
        b.data.clear();
        b.end = false;
        b.error = None;
        b.reposition = Some(to);
        drop(b);
        self.shared.fetcher.notify_one();
    }
}

impl Drop for HttpSource {
    fn drop(&mut self) {
        self.shared.lock().closed = true;
        self.shared.fetcher.notify_one();
    }
}

impl TrackSource for HttpSource {
    fn layout(&self) -> SourceLayout {
        self.layout.clone()
    }

    fn extension_hint(&self) -> Option<String> {
        Some(self.hint.to_owned())
    }

    fn read(&mut self, buf: &mut [u8]) -> Result<ReadOutcome, SourceError> {
        if buf.is_empty() {
            return Ok(ReadOutcome::Pending);
        }
        let deadline = Instant::now() + READ_WAIT;
        let mut b = self.shared.lock();
        loop {
            if !b.data.is_empty() {
                let n = buf.len().min(b.data.len());
                for (dst, src) in buf.iter_mut().zip(b.data.drain(..n)) {
                    *dst = src;
                }
                drop(b);
                self.shared.fetcher.notify_one();
                return Ok(ReadOutcome::Data(n));
            }
            if let Some(error) = &b.error {
                return Err(error.clone());
            }
            if b.end {
                return Ok(ReadOutcome::End);
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Ok(ReadOutcome::Pending);
            }
            b = self
                .shared
                .readable
                .wait_timeout(b, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    fn seek(&mut self, offset: u64) -> Result<(), SourceError> {
        if !matches!(self.layout, SourceLayout::SingleFile { .. }) {
            return Err(SourceError::Unseekable);
        }
        self.reposition(Position::Single(offset));
        Ok(())
    }

    fn restart_at_segment(&mut self, index: usize) -> Result<(), SourceError> {
        if !matches!(self.layout, SourceLayout::Segmented { .. }) {
            return Err(SourceError::Unseekable);
        }
        self.reposition(Position::Init {
            offset: 0,
            then: index,
        });
        Ok(())
    }
}
