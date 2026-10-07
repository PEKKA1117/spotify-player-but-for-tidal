//! The decode worker: one thread per track that runs the [`Decoder`] and
//! feeds the engine through a bounded channel.
//!
//! The engine never waits on a source: it only waits (briefly) on this
//! channel. A worker stuck in a source read is abandoned by dropping its
//! [`Worker`] handle (spec AC22); it notices when the read returns, or never,
//! and its thread is not joined.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender, TryRecvError};
use std::time::Duration;

use crate::clock::duration_to_frames;
use crate::decode::{Chunk, Decoder, SeekOutcome, Signals};
use crate::engine::EngineError;
use crate::sink::SourceFormat;
use crate::source::TrackSource;

/// Decoded chunks a worker may queue ahead of the engine (a packet each:
/// 4096–4608 frames for FLAC, 1024 for AAC).
const CHANNEL_CHUNKS: usize = 16;

/// A message from the worker. Chunks, ends and failures carry the
/// generation of the seek they follow, so the engine can drop stale ones.
#[derive(Debug)]
pub(crate) enum FromWorker {
    /// The stream was probed: always first, unless `OpenFailed`.
    Opened(SourceFormat),
    /// Probing or setting up the decoder failed.
    OpenFailed(EngineError),
    Chunk {
        generation: u64,
        chunk: Chunk,
    },
    /// The track's last frame was decoded (or a seek went past the end).
    End {
        generation: u64,
    },
    Failed {
        generation: u64,
        error: EngineError,
    },
}

#[derive(Debug)]
struct SeekRequest {
    generation: u64,
    position: Duration,
}

/// The engine's handle on a worker. Dropping it abandons the worker.
#[derive(Debug)]
pub(crate) struct Worker {
    messages: Receiver<FromWorker>,
    seeks: Sender<SeekRequest>,
    signals: Arc<Signals>,
}

impl Worker {
    /// Start decoding `source` from `start_at`.
    pub fn spawn(source: Box<dyn TrackSource>, start_at: Duration) -> Self {
        let (tx, messages) = mpsc::sync_channel(CHANNEL_CHUNKS);
        let (seeks, seek_rx) = mpsc::channel();
        let signals = Arc::new(Signals::default());
        let thread_signals = signals.clone();
        let spawned = std::thread::Builder::new()
            .name("audio-decode".into())
            .spawn(move || run(source, start_at, thread_signals, tx, seek_rx));
        if let Err(e) = spawned {
            // The closure (and its sender) was dropped: report through a
            // fresh channel so the engine sees the failure.
            let (tx, messages) = mpsc::sync_channel(1);
            let _ = tx.send(FromWorker::OpenFailed(EngineError::Decode(format!(
                "cannot start the decode thread: {e}"
            ))));
            return Self {
                messages,
                seeks,
                signals,
            };
        }
        Self {
            messages,
            seeks,
            signals,
        }
    }

    /// Ask the worker to continue from `position`; its next messages carry
    /// `generation`. `opened`: the worker is past probing, so a read stuck
    /// on a stalled source may be abandoned for the seek.
    pub fn seek(&self, generation: u64, position: Duration, opened: bool) {
        if opened {
            self.signals.interrupt.store(true, Ordering::SeqCst);
        }
        let _ = self.seeks.send(SeekRequest {
            generation,
            position,
        });
    }

    /// The next message, waiting at most `wait`. `None` on timeout.
    pub fn receive(&self, wait: Duration) -> Option<Result<FromWorker, ()>> {
        match self.messages.recv_timeout(wait) {
            Ok(message) => Some(Ok(message)),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => Some(Err(())),
        }
    }

    /// The source has no data for the decoder right now.
    pub fn stalled(&self) -> bool {
        self.signals.stalled.load(Ordering::SeqCst)
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.signals.cancelled.store(true, Ordering::SeqCst);
    }
}

fn run(
    source: Box<dyn TrackSource>,
    start_at: Duration,
    signals: Arc<Signals>,
    tx: SyncSender<FromWorker>,
    seeks: Receiver<SeekRequest>,
) {
    let mut decoder = match Decoder::open_with(source, signals.clone()) {
        Ok(decoder) => decoder,
        Err(error) => {
            let _ = tx.send(FromWorker::OpenFailed(error));
            return;
        }
    };
    let rate = decoder.format().sample_rate;
    if tx.send(FromWorker::Opened(decoder.format())).is_err() {
        return;
    }
    let cancelled = || signals.cancelled.load(Ordering::SeqCst);
    let mut generation = 0;
    // `Some(position)`: seek before decoding further.
    let mut pending = (!start_at.is_zero()).then_some(start_at);
    let mut done = false;
    loop {
        if cancelled() {
            return;
        }
        let request = if done && pending.is_none() {
            match seeks.recv() {
                Ok(request) => Some(request),
                Err(_) => return,
            }
        } else {
            match seeks.try_recv() {
                Ok(request) => Some(request),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => return,
            }
        };
        if let Some(mut request) = request {
            // Only the latest seek matters.
            while let Ok(newer) = seeks.try_recv() {
                request = newer;
            }
            signals.interrupt.store(false, Ordering::SeqCst);
            generation = request.generation;
            pending = Some(request.position);
        }
        if let Some(position) = pending.take() {
            done = false;
            match decoder.seek(duration_to_frames(position, rate)) {
                Ok(SeekOutcome::Seeked) => {}
                Ok(SeekOutcome::PastEnd) => {
                    done = true;
                    if tx.send(FromWorker::End { generation }).is_err() {
                        return;
                    }
                    continue;
                }
                Err(error) => {
                    if cancelled() {
                        return;
                    }
                    if signals.interrupt.load(Ordering::SeqCst) {
                        continue;
                    }
                    done = true;
                    if tx.send(FromWorker::Failed { generation, error }).is_err() {
                        return;
                    }
                    continue;
                }
            }
        }
        let message = match decoder.next_chunk() {
            Ok(Some(chunk)) => FromWorker::Chunk { generation, chunk },
            Ok(None) => {
                done = true;
                FromWorker::End { generation }
            }
            Err(error) => {
                if cancelled() {
                    return;
                }
                if signals.interrupt.load(Ordering::SeqCst) {
                    // A seek is on its way: the read was abandoned for it.
                    continue;
                }
                done = true;
                FromWorker::Failed { generation, error }
            }
        };
        if tx.send(message).is_err() {
            return;
        }
    }
}
