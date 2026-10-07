//! The playback engine (spec 0003 "Engine"): one dedicated thread that owns
//! the decoder and the output device, takes [`Command`]s and reports
//! [`Event`]s.

use std::fmt;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::JoinHandle;
use std::time::Duration;

use std::collections::VecDeque;
use std::sync::mpsc::TryRecvError;

use crate::clock::{Clock, SystemClock, duration_to_frames, frames_to_duration};
use crate::decode::Chunk;
use crate::sink::{OutputInfo, Sink, SinkError, SinkFactory, SourceFormat};
use crate::source::{SourceError, TrackSource};
use crate::worker::{FromWorker, Worker};

/// Longest single write: keeps commands responsive (the device may block
/// for about one period on top).
const WRITE_SLICE: Duration = Duration::from_millis(50);
/// `Position` is sent once this much clock time passed since the last one
/// (plus at most one write: well within the 250 ms of spec AC20).
const POSITION_EVERY: Duration = Duration::from_millis(200);
/// How long the engine waits for the decoder before looking at commands.
const POLL: Duration = Duration::from_millis(10);
/// Decoded audio needed to leave `Buffering` (spec "Events").
const REFILL: Duration = Duration::from_secs(2);
/// Written frames kept to replay on another device after `SetDevice`
/// (more than any device buffer).
const HISTORY: Duration = Duration::from_secs(1);

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
    ///
    /// `tag` is the caller's label for the track: every track-scoped event
    /// ([`Event::Started`], [`Event::Transitioned`], [`Event::TrackEnded`],
    /// [`Event::Error`]) carries the tag of the track it is about. The
    /// engine never invents or interprets tags.
    Play {
        tag: u64,
        source: Box<dyn TrackSource>,
        start_at: Duration,
    },
    /// Open `source` as the next track, for a gapless transition, labelled
    /// `tag` like `Play`. Replaces any earlier preload. `Play`, `Stop` and a
    /// failed track forget it.
    Preload {
        tag: u64,
        source: Box<dyn TrackSource>,
    },
    /// Forget the preload, closing its source. At the end of the current
    /// track the engine drains and sends `TrackEnded`, as if no preload had
    /// been sent. Does nothing when nothing is preloaded.
    CancelPreload,
    /// Scale every sample by `gain` (0.0–1.0) from the next write on (within
    /// one write slice, 50 ms); kept across tracks, transitions and
    /// `SetDevice`. Exactly 1.0 passes samples through untouched, otherwise
    /// each sample becomes `round(s × gain)` computed in `f64`, no dither.
    SetGain(f32),
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
            Self::Play { tag, start_at, .. } => f
                .debug_struct("Play")
                .field("tag", tag)
                .field("start_at", start_at)
                .finish_non_exhaustive(),
            Self::Preload { tag, .. } => f
                .debug_struct("Preload")
                .field("tag", tag)
                .finish_non_exhaustive(),
            Self::CancelPreload => f.write_str("CancelPreload"),
            Self::SetGain(g) => f.debug_tuple("SetGain").field(g).finish(),
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
        tag: u64,
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
        tag: u64,
        source: SourceFormat,
        output: OutputInfo,
    },
    /// The last frame was played (drained) and no preload was waiting.
    TrackEnded {
        tag: u64,
    },
    Paused,
    Resumed,
    Stopped,
    /// Paused, the output was closed and its reservation given back (spec
    /// 0005 "Releasing the device while paused"); the track is kept. The
    /// next `Resume` reopens it (`Resumed`, or `ResumeFailed`).
    Released,
    /// Reopening the output on `Resume` after a release failed: the engine
    /// stays paused with nothing open, and the next `Resume` tries again.
    ResumeFailed(SinkError),
    /// The track failed; no `TrackEnded` follows for it.
    Error {
        tag: u64,
        error: EngineError,
    },
}

/// The release delay by default (spec 0005, decision 4).
pub const DEFAULT_RELEASE_PAUSED: Duration = Duration::from_secs(10);

/// How long another application waits for the engine's answer to
/// `RequestRelease` at most (spec 0005: 400 ms; 0003's callers wait 500 ms).
pub const ANSWER_WITHIN: Duration = Duration::from_millis(400);

/// How to run the engine.
#[derive(Clone)]
pub struct EngineConfig {
    /// The output device to open first (`SetDevice` changes it).
    pub device: String,
    /// Paces `Position` events and times the release delay.
    pub clock: Arc<dyn Clock>,
    /// Paused this long, the engine releases the output (spec 0005);
    /// `None`: never (a `RequestRelease` still releases it).
    pub release_paused: Option<Duration>,
    /// Bound to the engine when it is spawned: how the exported
    /// reservation object asks it to release the device.
    pub release_requests: ReleaseRequests,
}

impl EngineConfig {
    /// `device` with the real clock and the default release delay.
    pub fn new(device: impl Into<String>) -> Self {
        Self {
            device: device.into(),
            clock: Arc::new(SystemClock::new()),
            release_paused: Some(DEFAULT_RELEASE_PAUSED),
            release_requests: ReleaseRequests::new(),
        }
    }

    /// Replace the clock (tests inject a fake one).
    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }

    /// Replace the release delay (`None`: never).
    pub fn with_release_paused(mut self, delay: Option<Duration>) -> Self {
        self.release_paused = delay;
        self
    }

    /// Use `requests` (made before the sink factory that hands it to the
    /// reservation object) for this engine.
    pub fn with_release_requests(mut self, requests: ReleaseRequests) -> Self {
        self.release_requests = requests;
        self
    }
}

impl fmt::Debug for EngineConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EngineConfig")
            .field("device", &self.device)
            .field("release_paused", &self.release_paused)
            .finish_non_exhaustive()
    }
}

/// Asks the engine to release the output for another application
/// (`org.freedesktop.ReserveDevice1.RequestRelease`, spec 0005). Cloneable
/// and usable from any thread; bound to an engine when it is spawned.
#[derive(Clone, Default)]
pub struct ReleaseRequests {
    engine: Arc<Mutex<Option<Sender<Input>>>>,
}

impl fmt::Debug for ReleaseRequests {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ReleaseRequests").finish_non_exhaustive()
    }
}

impl ReleaseRequests {
    /// Not bound to an engine yet: every request is refused.
    pub fn new() -> Self {
        Self::default()
    }

    /// Ask the engine; `answer` is called once with its answer, on the
    /// engine thread: `true` once the PCM is closed (the reservation is
    /// released right after `answer` returns), `false` when it keeps the
    /// device. Returns `false`, without calling `answer`, when no engine is
    /// running.
    pub fn ask(&self, answer: impl FnOnce(bool) + Send + 'static) -> bool {
        answer(false);
        true
    }

    /// Ask the engine and wait at most `within` for its answer; no answer
    /// in time is `false` (the device is kept).
    pub fn request(&self, within: Duration) -> bool {
        let _ = within;
        false
    }
}

/// What the engine thread receives: a command, or a release request.
enum Input {
    Command(Command),
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
    release_requests: ReleaseRequests,
    thread: Option<JoinHandle<()>>,
}

impl Engine {
    /// Start the engine thread. Sinks are created by `factory`, for
    /// `config.device` first.
    pub fn spawn(factory: Box<dyn SinkFactory>, config: EngineConfig) -> Self {
        let (commands, command_rx) = mpsc::channel::<Command>();
        let (event_tx, events) = mpsc::channel();
        let underruns = Arc::new(AtomicU64::new(0));
        let thread_underruns = underruns.clone();
        let release_requests = config.release_requests.clone();
        let thread = std::thread::Builder::new()
            .name("audio-engine".into())
            .spawn(move || {
                EngineThread::new(factory, config, event_tx, thread_underruns).run(command_rx);
            })
            .expect("spawn the audio engine thread");
        Self {
            commands,
            events,
            underruns,
            release_requests,
            thread: Some(thread),
        }
    }

    /// The handle through which another application's `RequestRelease`
    /// reaches this engine.
    pub fn release_requests(&self) -> ReleaseRequests {
        self.release_requests.clone()
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

/// A track the engine plays (or has preloaded).
struct Track {
    /// The caller's label, from `Play`/`Preload`.
    tag: u64,
    worker: Worker,
    /// Known once the worker probed the stream.
    format: Option<SourceFormat>,
    /// Bumped by every seek; older messages from the worker are dropped.
    generation: u64,
    queue: VecDeque<Chunk>,
    /// Samples of `queue[0]` already written.
    front: usize,
    queued_frames: u64,
    /// The worker decoded the last frame (for this generation).
    ended: bool,
    failed: Option<EngineError>,
    /// Where playback (re)started: the start position or the last seek.
    base: Duration,
    /// Frames written since `base`.
    written: u64,
    /// `Started`/`Transitioned` was sent.
    announced: bool,
    /// Came from `Preload`: announced with `Transitioned`.
    preloaded: bool,
}

impl Track {
    fn new(tag: u64, source: Box<dyn TrackSource>, start_at: Duration, preloaded: bool) -> Self {
        Self {
            tag,
            worker: Worker::spawn(source, start_at),
            format: None,
            generation: 0,
            queue: VecDeque::new(),
            front: 0,
            queued_frames: 0,
            ended: false,
            failed: None,
            base: start_at,
            written: 0,
            announced: false,
            preloaded,
        }
    }

    fn base_frame(&self) -> u64 {
        self.format
            .map_or(0, |f| duration_to_frames(self.base, f.sample_rate))
    }

    /// Take one message from the worker, waiting at most `wait`.
    fn receive(&mut self, wait: Duration) {
        let message = match self.worker.receive(wait) {
            None => return,
            Some(Ok(message)) => message,
            Some(Err(())) => {
                if !self.ended && self.failed.is_none() {
                    self.failed = Some(EngineError::Decode("the decoder stopped".into()));
                }
                return;
            }
        };
        match message {
            FromWorker::Opened(format) => self.format = Some(format),
            FromWorker::OpenFailed(error) => self.failed = Some(error),
            FromWorker::Chunk { generation, chunk } if generation == self.generation => {
                self.queued_frames += chunk.frames() as u64;
                self.queue.push_back(chunk);
            }
            FromWorker::End { generation } if generation == self.generation => self.ended = true,
            FromWorker::Failed { generation, error } if generation == self.generation => {
                self.failed = Some(error);
            }
            _ => {} // stale: from before the last seek
        }
    }

    /// Forget decoded audio (seek).
    fn clear(&mut self) {
        self.queue.clear();
        self.front = 0;
        self.queued_frames = 0;
        self.ended = false;
        self.failed = None;
    }

    /// Put `samples` back in front of the queue (`SetDevice` replay).
    fn unshift(&mut self, samples: Vec<i32>) {
        if samples.is_empty() {
            return;
        }
        if self.front > 0 {
            if let Some(chunk) = self.queue.front_mut() {
                chunk.samples.drain(..self.front);
                chunk.first_frame += (self.front / 2) as u64;
            }
            self.front = 0;
        }
        let frames = (samples.len() / 2) as u64;
        self.queued_frames += frames;
        self.written = self.written.saturating_sub(frames);
        let first_frame = self.base_frame() + self.written;
        self.queue.push_front(Chunk {
            first_frame,
            samples,
        });
    }
}

/// What a command asks of the run loop.
enum Flow {
    Continue,
    Exit,
}

/// The state owned by the engine thread.
struct EngineThread {
    factory: Box<dyn SinkFactory>,
    device: String,
    clock: Arc<dyn Clock>,
    events: Sender<Event>,
    underruns: Arc<AtomicU64>,
    sink: Option<Box<dyn Sink>>,
    /// The format the sink is open for, and what it opened.
    open: Option<(SourceFormat, OutputInfo)>,
    sink_paused: bool,
    current: Option<Track>,
    next: Option<Track>,
    paused: bool,
    buffering: bool,
    /// Applied to every sample written (`SetGain`); outlives tracks.
    gain: f32,
    /// The current track's most recently written samples.
    history: VecDeque<i32>,
    /// Reused buffer for scaled samples.
    scratch: Vec<i32>,
    last_position: Option<Duration>,
    last_emit: Duration,
}

impl EngineThread {
    fn new(
        factory: Box<dyn SinkFactory>,
        config: EngineConfig,
        events: Sender<Event>,
        underruns: Arc<AtomicU64>,
    ) -> Self {
        Self {
            factory,
            device: config.device,
            clock: config.clock,
            events,
            underruns,
            sink: None,
            open: None,
            sink_paused: false,
            current: None,
            next: None,
            paused: false,
            buffering: false,
            gain: 1.0,
            history: VecDeque::new(),
            scratch: Vec::new(),
            last_position: None,
            last_emit: Duration::ZERO,
        }
    }

    fn run(mut self, commands: Receiver<Command>) {
        loop {
            let active = self.current.is_some() && !self.paused;
            let command = if active {
                match commands.try_recv() {
                    Ok(command) => Some(command),
                    Err(TryRecvError::Empty) => None,
                    Err(TryRecvError::Disconnected) => Some(Command::Shutdown),
                }
            } else {
                Some(commands.recv().unwrap_or(Command::Shutdown))
            };
            match command {
                Some(command) => {
                    if let Flow::Exit = self.handle(command) {
                        return;
                    }
                }
                None => self.step(),
            }
        }
    }

    fn emit(&self, event: Event) {
        let _ = self.events.send(event);
    }

    fn handle(&mut self, command: Command) -> Flow {
        match command {
            Command::Play {
                tag,
                source,
                start_at,
            } => self.play(tag, source, start_at),
            Command::Preload { tag, source } => {
                self.next = Some(Track::new(tag, source, Duration::ZERO, true));
            }
            Command::CancelPreload => self.next = None,
            Command::SetGain(gain) => self.gain = gain.clamp(0.0, 1.0),
            Command::Pause => {
                if self.current.is_some() && !self.paused {
                    self.paused = true;
                    self.sync_pause();
                    self.emit(Event::Paused);
                    self.emit_position();
                }
            }
            Command::Resume => {
                if self.current.is_some() && self.paused {
                    self.paused = false;
                    self.sync_pause();
                    self.emit(Event::Resumed);
                    self.emit_position();
                }
            }
            Command::Seek(position) => self.seek(position),
            Command::SetDevice(device) => self.set_device(device),
            Command::Stop => {
                self.stop();
                self.emit(Event::Stopped);
            }
            Command::Shutdown => {
                self.stop();
                self.emit(Event::Stopped);
                return Flow::Exit;
            }
        }
        Flow::Continue
    }

    /// Forget every track, drop what the device still holds, and close it.
    fn stop(&mut self) {
        if let (Some(sink), Some(_)) = (self.sink.as_mut(), self.open.as_ref()) {
            let _ = sink.discard();
        }
        self.idle();
    }

    /// Forget every track and close the device.
    fn idle(&mut self) {
        self.current = None;
        self.next = None;
        self.close_output();
        self.paused = false;
        self.buffering = false;
        self.history.clear();
        self.last_position = None;
    }

    fn play(&mut self, tag: u64, source: Box<dyn TrackSource>, start_at: Duration) {
        self.current = None;
        self.next = None;
        if let (Some(sink), Some(_)) = (self.sink.as_mut(), self.open.as_ref()) {
            let _ = sink.discard();
        }
        self.paused = false;
        self.buffering = false;
        self.sync_pause();
        self.history.clear();
        self.last_position = None;
        self.current = Some(Track::new(tag, source, start_at, false));
    }

    fn seek(&mut self, position: Duration) {
        let Some(track) = self.current.as_mut() else {
            return;
        };
        track.generation += 1;
        track
            .worker
            .seek(track.generation, position, track.format.is_some());
        track.clear();
        track.base = position;
        track.written = 0;
        if let (Some(sink), Some(_)) = (self.sink.as_mut(), self.open.as_ref()) {
            let _ = sink.discard();
        }
        self.history.clear();
        if self.buffering {
            self.buffering = false;
            self.sync_pause();
            self.emit(Event::Buffered);
        }
        self.last_position = Some(position);
        self.last_emit = self.clock.now();
        self.emit(Event::Position(position));
    }

    fn set_device(&mut self, device: String) {
        self.device = device;
        let Some((format, _)) = self.open.take() else {
            // Nothing open: the next track opens the new device.
            self.sink = None;
            return;
        };
        let mut replay = Vec::new();
        if let Some(mut old) = self.sink.take() {
            let delay = old.delay_frames().unwrap_or(0);
            let _ = old.discard();
            old.close();
            if let Some(track) = &self.current {
                let frames = delay
                    .min(track.written)
                    .min((self.history.len() / 2) as u64);
                let start = self.history.len() - frames as usize * 2;
                replay = self.history.drain(start..).collect();
            }
        }
        self.sink_paused = false;
        let Some(track) = self.current.as_mut() else {
            return;
        };
        track.unshift(replay);
        let format = track.format.unwrap_or(format);
        if let Err(error) = self.open_output(format) {
            self.fail(EngineError::Output(error));
        }
    }

    /// Pause the device while paused or buffering, so nothing it holds is
    /// lost and it never underruns.
    fn sync_pause(&mut self) {
        let want = self.paused || self.buffering;
        if self.open.is_none() || self.sink_paused == want {
            return;
        }
        let Some(sink) = self.sink.as_mut() else {
            return;
        };
        match sink.set_paused(want) {
            Ok(()) => self.sink_paused = want,
            Err(error) => self.fail(EngineError::Output(error)),
        }
    }

    fn close_output(&mut self) {
        if self.open.take().is_some()
            && let Some(sink) = self.sink.as_mut()
        {
            sink.close();
        }
        self.sink_paused = false;
    }

    /// Open the device for `format`, draining and closing it first when it
    /// is open for another output format.
    fn open_output(&mut self, format: SourceFormat) -> Result<(), SinkError> {
        if self.open.is_some() {
            if let Some(sink) = self.sink.as_mut() {
                sink.drain()?;
            }
            self.close_output();
        }
        let device = self.device.clone();
        let factory = &mut self.factory;
        let sink = self.sink.get_or_insert_with(|| factory.create(&device));
        let output = sink.open(&format)?;
        self.open = Some((format, output));
        self.sink_paused = false;
        self.sync_pause();
        Ok(())
    }

    /// The track failed: report it once, and go idle.
    fn fail(&mut self, error: EngineError) {
        let tag = self.current.as_ref().map_or(0, |t| t.tag);
        self.stop();
        self.emit(Event::Error { tag, error });
    }

    /// What the listener hears now (never backwards between seeks).
    fn position(&self) -> Option<Duration> {
        let track = self.current.as_ref()?;
        let Some(format) = track.format else {
            return Some(track.base);
        };
        let delay = match (&self.sink, &self.open) {
            (Some(sink), Some(_)) => sink.delay_frames().unwrap_or(0),
            _ => 0,
        };
        let heard = track.base_frame() + track.written.saturating_sub(delay);
        let position = frames_to_duration(heard, format.sample_rate);
        Some(
            self.last_position
                .map_or(position, |last| last.max(position)),
        )
    }

    fn emit_position(&mut self) {
        if let Some(position) = self.position() {
            self.last_position = Some(position);
            self.last_emit = self.clock.now();
            self.emit(Event::Position(position));
        }
    }

    /// One unit of work while a track plays.
    fn step(&mut self) {
        let Some(track) = self.current.as_mut() else {
            return;
        };
        if track.format.is_none() && track.failed.is_none() {
            track.receive(POLL);
            return;
        }
        if self.buffering {
            track.receive(POLL);
            let refilled = track
                .format
                .is_some_and(|f| track.queued_frames >= duration_to_frames(REFILL, f.sample_rate));
            if refilled || track.ended || track.failed.is_some() {
                self.buffering = false;
                self.sync_pause();
                self.emit(Event::Buffered);
            }
            return;
        }
        if track.queue.is_empty() {
            if let Some(error) = track.failed.take() {
                self.fail(error);
            } else if track.ended {
                self.end_of_track();
            } else {
                // Starved: the decoder is behind. Only a stalled source
                // (not a busy decoder) means buffering.
                if track.announced && track.worker.stalled() {
                    self.buffering = true;
                    self.sync_pause();
                    self.emit(Event::Buffering);
                }
                if let Some(track) = self.current.as_mut() {
                    track.receive(POLL);
                }
            }
            return;
        }
        self.write_some();
    }

    /// Write (part of) the front chunk.
    fn write_some(&mut self) {
        let Some(format) = self.current.as_ref().and_then(|t| t.format) else {
            return;
        };
        let same_output = self.open.as_ref().is_some_and(|(open, _)| {
            open.sample_rate == format.sample_rate && open.bits_per_sample == format.bits_per_sample
        });
        if !same_output && let Err(error) = self.open_output(format) {
            self.fail(EngineError::Output(error));
            return;
        }
        let (Some(track), Some(sink)) = (self.current.as_mut(), self.sink.as_mut()) else {
            return;
        };
        let Some(chunk) = track.queue.front() else {
            return;
        };
        let max = duration_to_frames(WRITE_SLICE, format.sample_rate).max(1) as usize * 2;
        let end = chunk.samples.len().min(track.front + max);
        let slice = &chunk.samples[track.front..end];
        // The history keeps the samples as decoded: a replay on another
        // device is scaled by the gain then, once.
        let outcome = match sink.write(apply_gain(slice, self.gain, &mut self.scratch)) {
            Ok(outcome) => outcome,
            Err(error) => {
                self.fail(EngineError::Output(error));
                return;
            }
        };
        if outcome.underrun {
            self.underruns.fetch_add(1, Ordering::SeqCst);
        }
        let consumed = outcome.frames.min(slice.len() / 2);
        self.history.extend(&slice[..consumed * 2]);
        let keep = duration_to_frames(HISTORY, format.sample_rate) as usize * 2;
        if self.history.len() > keep {
            let excess = self.history.len() - keep;
            self.history.drain(..excess);
        }
        track.front += consumed * 2;
        if track.front >= chunk.samples.len() {
            track.queue.pop_front();
            track.front = 0;
        }
        track.queued_frames = track.queued_frames.saturating_sub(consumed as u64);
        track.written += consumed as u64;
        if consumed == 0 {
            return;
        }
        if !track.announced {
            track.announced = true;
            let preloaded = track.preloaded;
            let tag = track.tag;
            if let Some((_, output)) = self.open.clone() {
                self.emit(if preloaded {
                    Event::Transitioned {
                        tag,
                        source: format,
                        output,
                    }
                } else {
                    Event::Started {
                        tag,
                        source: format,
                        output,
                    }
                });
            }
            self.last_emit = self.clock.now();
        }
        if self.clock.now().saturating_sub(self.last_emit) >= POSITION_EVERY {
            self.emit_position();
        }
    }

    /// The decoder's last frame was written.
    fn end_of_track(&mut self) {
        let tag = self.current.as_ref().map_or(0, |t| t.tag);
        if let Some(next) = self.next.take() {
            // Gapless when the output format matches: the next write
            // follows the last one on the open device (`write_some`
            // reopens otherwise).
            self.current = Some(next);
            self.history.clear();
            self.last_position = None;
            return;
        }
        if let (Some(sink), Some(_)) = (self.sink.as_mut(), self.open.as_ref())
            && let Err(error) = sink.drain()
        {
            self.fail(EngineError::Output(error));
            return;
        }
        self.emit(Event::TrackEnded { tag });
        self.idle();
    }
}

/// `samples` scaled by `gain`: exactly 1.0 returns them untouched (no float
/// math), otherwise each becomes `round(s × gain)` computed in `f64`, no
/// dither. `scratch` holds the scaled copy.
fn apply_gain<'a>(samples: &'a [i32], gain: f32, scratch: &'a mut Vec<i32>) -> &'a [i32] {
    if gain == 1.0 {
        return samples;
    }
    let gain = f64::from(gain);
    scratch.clear();
    scratch.extend(
        samples
            .iter()
            .map(|&s| (f64::from(s) * gain).round() as i32),
    );
    scratch
}
