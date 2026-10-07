//! In-memory test doubles for the engine's byte side (spec 0003 test plan):
//! [`FakeSource`] serves a file or a segmented stream from memory, records
//! what was fetched in a [`FetchLog`] (AC19), and can stall until a [`Gate`]
//! opens (AC23), block forever (AC22) or fail (AC16).
//!
//! The fake device is [`crate::MemorySink`]; the fake clock is
//! [`crate::clock::FakeClock`].

use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use crate::source::{ReadOutcome, SegmentSpan, SourceError, SourceLayout, TrackSource};

/// How long a stalled [`FakeSource`] waits for its gate before answering
/// `Pending` (the source contract's "about 100 ms", shortened).
const PENDING_WAIT: Duration = Duration::from_millis(20);

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// One thing a [`FakeSource`] was asked to fetch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fetch {
    /// Single file: bytes `offset..offset + len` were delivered.
    Read { offset: u64, len: usize },
    /// Single file: reading continues at `offset`.
    Seek { offset: u64 },
    /// Segmented: restarted at media segment `segment` (0-based).
    Restart { segment: usize },
    /// Segmented: the init segment's first byte was delivered.
    Init,
    /// Segmented: media segment `n`'s (0-based) first byte was delivered.
    Segment(usize),
    /// The end of the stream was reported.
    End,
}

/// The shared record of a [`FakeSource`]'s fetches. Clones share it.
#[derive(Debug, Clone, Default)]
pub struct FetchLog {
    inner: Arc<(Mutex<Vec<Fetch>>, Condvar)>,
}

impl FetchLog {
    /// Everything fetched so far, in order.
    pub fn entries(&self) -> Vec<Fetch> {
        lock(&self.inner.0).clone()
    }

    /// Wait (at most `timeout`) until an entry matches `pred`.
    pub fn wait_for(&self, timeout: Duration, pred: impl Fn(&Fetch) -> bool) -> bool {
        let deadline = Instant::now() + timeout;
        let mut entries = lock(&self.inner.0);
        loop {
            if entries.iter().any(&pred) {
                return true;
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            entries = self
                .inner
                .1
                .wait_timeout(entries, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    fn push(&self, fetch: Fetch) {
        lock(&self.inner.0).push(fetch);
        self.inner.1.notify_all();
    }
}

/// Opens a stalled [`FakeSource`]. Clones share the gate.
#[derive(Debug, Clone, Default)]
pub struct Gate {
    inner: Arc<(Mutex<bool>, Condvar)>,
}

impl Gate {
    /// Let the source deliver the rest of its bytes.
    pub fn open(&self) {
        *lock(&self.inner.0) = true;
        self.inner.1.notify_all();
    }

    fn wait_open(&self, timeout: Option<Duration>) -> bool {
        let mut open = lock(&self.inner.0);
        match timeout {
            Some(timeout) => {
                if !*open {
                    open = self
                        .inner
                        .1
                        .wait_timeout(open, timeout)
                        .unwrap_or_else(PoisonError::into_inner)
                        .0;
                }
            }
            None => {
                while !*open {
                    open = self
                        .inner
                        .1
                        .wait(open)
                        .unwrap_or_else(PoisonError::into_inner);
                }
            }
        }
        *open
    }
}

#[derive(Debug, Clone)]
enum Behaviour {
    Normal,
    /// After `bytes` delivered: `Pending` until the gate opens.
    StallAfter {
        bytes: u64,
        gate: Gate,
    },
    /// After `bytes` delivered: block in `read` until the gate opens (never,
    /// in AC22's test).
    BlockAfter {
        bytes: u64,
        gate: Gate,
    },
    /// After `bytes` delivered: fail with `error`.
    FailAfter {
        bytes: u64,
        error: SourceError,
    },
}

#[derive(Debug)]
enum Data {
    Single {
        bytes: Vec<u8>,
        pos: u64,
    },
    Segmented {
        init: Vec<u8>,
        segments: Vec<Vec<u8>>,
        timescale: u32,
        spans: Vec<SegmentSpan>,
        /// Next piece to read: `None` = init, `Some(i)` = media segment i.
        piece: Option<usize>,
        /// Media segment the stream (re)started at.
        first: usize,
        offset: usize,
    },
}

/// An in-memory [`TrackSource`].
#[derive(Debug)]
pub struct FakeSource {
    data: Data,
    extension: Option<String>,
    behaviour: Behaviour,
    delivered: u64,
    log: FetchLog,
}

impl FakeSource {
    /// A single-file stream (seekable by byte offset).
    pub fn file(bytes: Vec<u8>, extension: &str) -> Self {
        Self::new(Data::Single { bytes, pos: 0 }, extension)
    }

    /// A segmented stream: `init`, then `segments` (each with its span).
    pub fn segmented(init: Vec<u8>, segments: Vec<(Vec<u8>, SegmentSpan)>, timescale: u32) -> Self {
        let (segments, spans) = segments.into_iter().unzip();
        Self::new(
            Data::Segmented {
                init,
                segments,
                timescale,
                spans,
                piece: None,
                first: 0,
                offset: 0,
            },
            "mp4",
        )
    }

    fn new(data: Data, extension: &str) -> Self {
        Self {
            data,
            extension: Some(extension.to_owned()),
            behaviour: Behaviour::Normal,
            delivered: 0,
            log: FetchLog::default(),
        }
    }

    /// After `bytes` bytes, answer `Pending` until the returned gate opens.
    pub fn stall_after(mut self, bytes: u64) -> (Self, Gate) {
        let gate = Gate::default();
        self.behaviour = Behaviour::StallAfter {
            bytes,
            gate: gate.clone(),
        };
        (self, gate)
    }

    /// After `bytes` bytes, block inside `read` until the returned gate opens.
    pub fn block_after(mut self, bytes: u64) -> (Self, Gate) {
        let gate = Gate::default();
        self.behaviour = Behaviour::BlockAfter {
            bytes,
            gate: gate.clone(),
        };
        (self, gate)
    }

    /// After `bytes` bytes, fail every read with `error`.
    pub fn fail_after(mut self, bytes: u64, error: SourceError) -> Self {
        self.behaviour = Behaviour::FailAfter { bytes, error };
        self
    }

    /// The record of what this source fetched.
    pub fn log(&self) -> FetchLog {
        self.log.clone()
    }

    /// How many bytes the next read may deliver before the behaviour kicks
    /// in, or the outcome to return instead.
    fn allowance(&mut self) -> Result<Option<u64>, Result<ReadOutcome, SourceError>> {
        match &self.behaviour {
            Behaviour::Normal => Ok(None),
            Behaviour::StallAfter { bytes, gate } => {
                if self.delivered < *bytes {
                    Ok(Some(bytes - self.delivered))
                } else if gate.wait_open(Some(PENDING_WAIT)) {
                    self.behaviour = Behaviour::Normal;
                    Ok(None)
                } else {
                    Err(Ok(ReadOutcome::Pending))
                }
            }
            Behaviour::BlockAfter { bytes, gate } => {
                if self.delivered < *bytes {
                    Ok(Some(bytes - self.delivered))
                } else {
                    gate.wait_open(None);
                    self.behaviour = Behaviour::Normal;
                    Ok(None)
                }
            }
            Behaviour::FailAfter { bytes, error } => {
                if self.delivered < *bytes {
                    Ok(Some(bytes - self.delivered))
                } else {
                    Err(Err(error.clone()))
                }
            }
        }
    }
}

impl TrackSource for FakeSource {
    fn layout(&self) -> SourceLayout {
        match &self.data {
            Data::Single { bytes, .. } => SourceLayout::SingleFile {
                len: Some(bytes.len() as u64),
            },
            Data::Segmented {
                timescale, spans, ..
            } => SourceLayout::Segmented {
                timescale: *timescale,
                segments: spans.clone(),
            },
        }
    }

    fn extension_hint(&self) -> Option<String> {
        self.extension.clone()
    }

    fn read(&mut self, buf: &mut [u8]) -> Result<ReadOutcome, SourceError> {
        let limit = match self.allowance() {
            Ok(limit) => limit,
            Err(outcome) => return outcome,
        };
        let max = limit.map_or(buf.len(), |l| buf.len().min(l as usize));
        let n = match &mut self.data {
            Data::Single { bytes, pos } => {
                let start = (*pos as usize).min(bytes.len());
                let n = max.min(bytes.len() - start);
                if n == 0 {
                    self.log.push(Fetch::End);
                    return Ok(ReadOutcome::End);
                }
                buf[..n].copy_from_slice(&bytes[start..start + n]);
                self.log.push(Fetch::Read {
                    offset: *pos,
                    len: n,
                });
                *pos += n as u64;
                n
            }
            Data::Segmented {
                init,
                segments,
                piece,
                first,
                offset,
                ..
            } => loop {
                let current: &[u8] = match piece {
                    None => init,
                    Some(i) if *i < segments.len() => &segments[*i],
                    Some(_) => {
                        self.log.push(Fetch::End);
                        return Ok(ReadOutcome::End);
                    }
                };
                if *offset >= current.len() {
                    *piece = Some(piece.map_or(*first, |i| i + 1));
                    *offset = 0;
                    continue;
                }
                if *offset == 0 {
                    self.log.push(match piece {
                        None => Fetch::Init,
                        Some(i) => Fetch::Segment(*i),
                    });
                }
                let n = max.min(current.len() - *offset);
                buf[..n].copy_from_slice(&current[*offset..*offset + n]);
                *offset += n;
                break n;
            },
        };
        self.delivered += n as u64;
        Ok(ReadOutcome::Data(n))
    }

    fn seek(&mut self, offset: u64) -> Result<(), SourceError> {
        match &mut self.data {
            Data::Single { pos, .. } => {
                *pos = offset;
                self.log.push(Fetch::Seek { offset });
                Ok(())
            }
            Data::Segmented { .. } => Err(SourceError::Unseekable),
        }
    }

    fn restart_at_segment(&mut self, index: usize) -> Result<(), SourceError> {
        match &mut self.data {
            Data::Single { .. } => Err(SourceError::Unseekable),
            Data::Segmented {
                piece,
                first,
                offset,
                ..
            } => {
                *piece = None;
                *first = index;
                *offset = 0;
                self.log.push(Fetch::Restart { segment: index });
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_all(source: &mut FakeSource) -> Vec<u8> {
        let mut out = Vec::new();
        let mut buf = [0u8; 3];
        loop {
            match source.read(&mut buf).unwrap() {
                ReadOutcome::Data(n) => out.extend_from_slice(&buf[..n]),
                ReadOutcome::End => return out,
                ReadOutcome::Pending => panic!("unexpected Pending"),
            }
        }
    }

    fn span(start: u64) -> SegmentSpan {
        SegmentSpan {
            start,
            duration: 10,
        }
    }

    #[test]
    fn single_file_reads_seeks_and_logs() {
        let mut source = FakeSource::file(b"abcdefgh".to_vec(), "flac");
        let mut buf = [0u8; 5];
        assert_eq!(source.read(&mut buf), Ok(ReadOutcome::Data(5)));
        source.seek(6).unwrap();
        assert_eq!(read_all(&mut source), b"gh");
        assert_eq!(
            source.log().entries(),
            [
                Fetch::Read { offset: 0, len: 5 },
                Fetch::Seek { offset: 6 },
                Fetch::Read { offset: 6, len: 2 },
                Fetch::End
            ]
        );
        assert_eq!(source.layout(), SourceLayout::SingleFile { len: Some(8) });
    }

    #[test]
    fn segmented_reads_init_then_segments_and_restarts() {
        let segments = vec![
            (b"AA".to_vec(), span(0)),
            (b"BB".to_vec(), span(10)),
            (b"CC".to_vec(), span(20)),
        ];
        let mut source = FakeSource::segmented(b"ii".to_vec(), segments, 10);
        assert_eq!(read_all(&mut source), b"iiAABBCC");
        source.restart_at_segment(2).unwrap();
        assert_eq!(read_all(&mut source), b"iiCC");
        assert_eq!(
            source.log().entries(),
            [
                Fetch::Init,
                Fetch::Segment(0),
                Fetch::Segment(1),
                Fetch::Segment(2),
                Fetch::End,
                Fetch::Restart { segment: 2 },
                Fetch::Init,
                Fetch::Segment(2),
                Fetch::End
            ]
        );
        assert!(source.seek(0).is_err());
    }

    #[test]
    fn stall_then_gate_opens() {
        let (mut source, gate) = FakeSource::file(b"abcdef".to_vec(), "flac").stall_after(4);
        let mut buf = [0u8; 8];
        assert_eq!(source.read(&mut buf), Ok(ReadOutcome::Data(4)));
        assert_eq!(source.read(&mut buf), Ok(ReadOutcome::Pending));
        gate.open();
        assert_eq!(source.read(&mut buf), Ok(ReadOutcome::Data(2)));
        assert_eq!(source.read(&mut buf), Ok(ReadOutcome::End));
    }

    #[test]
    fn fail_after() {
        let error = SourceError::Network("reset".into());
        let mut source = FakeSource::file(b"abcdef".to_vec(), "flac").fail_after(2, error.clone());
        let mut buf = [0u8; 8];
        assert_eq!(source.read(&mut buf), Ok(ReadOutcome::Data(2)));
        assert_eq!(source.read(&mut buf), Err(error));
    }
}
