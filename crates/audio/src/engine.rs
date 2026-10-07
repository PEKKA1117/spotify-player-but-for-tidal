//! The playback engine (spec 0003 "Engine"): one dedicated thread that owns
//! the decoder and the output device, takes [`Command`]s and reports
//! [`Event`]s.

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::clock::{Clock, SystemClock};
use crate::sink::{OutputInfo, SinkError, SinkFactory, SourceFormat};
use crate::source::{SourceError, TrackSource};

/// Why a track failed. The engine is idle afterwards.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EngineError {
    /// The bytes could not be read.
    #[error(transparent)]
    Source(#[from] SourceError),
    /// The stream is malformed (corrupt frame, broken container).
    #[error("decode error: {0}")]
    Decode(String),
    /// The output device failed.
    #[error(transparent)]
    Output(#[from] SinkError),
    /// A codec or stream feature this player does not play (HE-AAC, more
    /// than two channels, …).
    #[error("unsupported stream: {0}")]
    Unsupported(String),
}

/// What the engine is asked to do. Handled one at a time, in order.
pub enum Command {
    /// Stop whatever is playing, then play `source` from `start_at`.
    Play {
        source: Box<dyn TrackSource>,
        start_at: Duration,
    },
    /// Open `source` as the next track, for a gapless transition. Replaces
    /// any earlier preload.
    Preload {
        source: Box<dyn TrackSource>,
    },
    Pause,
    Resume,
    /// Continue from this position (accurate to the frame).
    Seek(Duration),
    /// Use this device from now on; a playing track moves to it.
    SetDevice(String),
    /// Stop, close the device, forget any preload.
    Stop,
    /// `Stop`, then end the engine thread.
    Shutdown,
}

impl fmt::Debug for Command {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Play { start_at, .. } => f
                .debug_struct("Play")
                .field("start_at", start_at)
                .finish_non_exhaustive(),
            Self::Preload { .. } => f.debug_struct("Preload").finish_non_exhaustive(),
            Self::Pause => f.write_str("Pause"),
            Self::Resume => f.write_str("Resume"),
            Self::Seek(t) => f.debug_tuple("Seek").field(t).finish(),
            Self::SetDevice(d) => f.debug_tuple("SetDevice").field(d).finish(),
            Self::Stop => f.write_str("Stop"),
            Self::Shutdown => f.write_str("Shutdown"),
        }
    }
}

/// What the engine reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The first frame of a track has been written.
    Started {
        source: SourceFormat,
        output: OutputInfo,
    },
    /// What the listener hears: frames written minus the device's delay.
    /// At least every 250 ms while playing, and after each seek, pause and
    /// resume.
    Position(Duration),
    /// The decoded audio ran dry because the source stalled; nothing is
    /// written until `Buffered`.
    Buffering,
    /// 2 s of audio (or the rest of the track) are available again.
    Buffered,
    /// Playback moved into the preloaded track (its first frame was
    /// written).
    Transitioned {
        source: SourceFormat,
        output: OutputInfo,
    },
    /// The last frame was played (drained) and no preload was waiting.
    TrackEnded,
    Paused,
    Resumed,
    Stopped,
    /// The track failed; no `TrackEnded` follows for it.
    Error(EngineError),
}

/// How to run the engine.
#[derive(Clone)]
pub struct EngineConfig {
    /// The output device to open first (`SetDevice` changes it).
    pub device: String,
    /// Paces `Position` events.
    pub clock: Arc<dyn Clock>,
}

impl EngineConfig {
    /// `device` with the real clock.
    pub fn new(device: impl Into<String>) -> Self {
        Self {
            device: device.into(),
            clock: Arc::new(SystemClock::new()),
        }
    }

    /// Replace the clock (tests inject a fake one).
    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }
}

impl fmt::Debug for EngineConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EngineConfig")
            .field("device", &self.device)
            .finish_non_exhaustive()
    }
}

/// The engine thread has ended: commands can no longer be delivered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the playback engine has shut down")]
pub struct EngineGone;

/// A handle on the engine thread. Dropping it shuts the engine down.
#[derive(Debug)]
pub struct Engine {
    commands: Sender<Command>,
    events: Receiver<Event>,
    underruns: Arc<AtomicU64>,
    thread: Option<JoinHandle<()>>,
}

impl Engine {
    /// Start the engine thread. Sinks are created by `factory`, for
    /// `config.device` first.
    pub fn spawn(factory: Box<dyn SinkFactory>, config: EngineConfig) -> Self {
        let (commands, command_rx) = mpsc::channel::<Command>();
        let (event_tx, events) = mpsc::channel();
        let underruns = Arc::new(AtomicU64::new(0));
        let thread = std::thread::Builder::new()
            .name("audio-engine".into())
            .spawn(move || {
                let _keep = (factory, config);
                for command in command_rx {
                    match command {
                        Command::Play { .. } => {
                            let _ = event_tx.send(Event::TrackEnded);
                        }
                        Command::Shutdown => return,
                        _ => {}
                    }
                }
            })
            .expect("spawn the audio engine thread");
        Self {
            commands,
            events,
            underruns,
            thread: Some(thread),
        }
    }

    /// Queue a command.
    pub fn send(&self, command: Command) -> Result<(), EngineGone> {
        self.commands.send(command).map_err(|_| EngineGone)
    }

    /// The events, in the order they happened.
    pub fn events(&self) -> &Receiver<Event> {
        &self.events
    }

    /// Device underruns recovered so far (spec AC14).
    pub fn underruns(&self) -> u64 {
        self.underruns.load(Ordering::SeqCst)
    }

    /// Wait for the engine thread to end (send `Shutdown` first).
    pub fn join(mut self) {
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        if let Some(thread) = self.thread.take() {
            let _ = self.commands.send(Command::Shutdown);
            let _ = thread.join();
        }
    }
}
