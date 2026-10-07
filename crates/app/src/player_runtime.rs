//! The player runtime (spec 0004 "Crate placement"): runs
//! [`tidal_player_core::player::update`], executes its effects in order
//! against the engine, the stream resolver and the clients, and maps what
//! comes back (engine events, resolutions, suggestions) into player inputs
//! with their tags.
//!
//! Everything it talks to sits behind a small seam so the tests run it with
//! fakes, without threads, network or a sound card:
//!
//! - [`EngineControl`]: sends `tidal_player_audio::Command`s, yields events
//! - [`Jobs`]: starts a resolution or a suggestions fetch; the result comes
//!   back later as a [`RuntimeInput`] with its tag
//! - [`Metadata`] and [`StreamOpener`]: what the real [`TokioJobs`] and the
//!   item expansion call
//!
//! The runtime keeps the opened stream of each resolved tag and hands it to
//! the engine on `EnginePlay`/`EnginePreload`; core never sees a URL. A
//! stream whose tag the player has moved past is dropped on arrival (which
//! closes it), so a mash of skips leaves nothing fetching.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tidal_player_api::auth::BoxFuture;
use tidal_player_api::metadata::{MetadataClient, MetadataError};
use tidal_player_audio::{self as audio, OutputInfo, SourceFormat, TrackSource};
use tidal_player_core::item::parse_item;
use tidal_player_core::player::{
    self, EngineEvent, Failure, PlayerConfig, PlayerEffect, PlayerInput, PlayerState, Purpose,
    TrackDetails,
};
use tidal_player_core::protocol::{Command, Event, InsertAt, PlaybackState, PlayerSnapshot};
use tidal_player_core::{AudioQuality, Item, ItemError, Track, TrackId};

use crate::ipc::server::{ClientId, ClientInput, Hub};
use crate::play::{engine_failure, output_description, source_description};

/// How long the runtime thread waits for an engine event before it looks at
/// its other inputs again (the latency of a key press, at worst).
pub const POLL: Duration = Duration::from_millis(10);

// --- seams ---------------------------------------------------------------------

/// The engine, as the runtime drives it.
pub trait EngineControl {
    /// Queue a command. An engine that has gone away is ignored.
    fn send(&mut self, command: audio::Command);
    /// The next event, waiting at most `wait` for one.
    fn poll_event(&mut self, wait: Duration) -> Option<audio::Event>;
}

impl EngineControl for audio::Engine {
    fn send(&mut self, command: audio::Command) {
        let _ = audio::Engine::send(self, command);
    }

    fn poll_event(&mut self, wait: Duration) -> Option<audio::Event> {
        match self.events().recv_timeout(wait) {
            Ok(event) => Some(event),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => {
                std::thread::sleep(wait);
                None
            }
        }
    }
}

impl<T: EngineControl + ?Sized> EngineControl for Box<T> {
    fn send(&mut self, command: audio::Command) {
        (**self).send(command);
    }

    fn poll_event(&mut self, wait: Duration) -> Option<audio::Event> {
        (**self).poll_event(wait)
    }
}

/// Starts asynchronous work; each result comes back as a [`RuntimeInput`]
/// carrying the tag it was started with.
pub trait Jobs {
    /// Resolve and open the stream of `track` (answer: `Resolved`).
    fn resolve(&mut self, tag: u64, track: TrackId);
    /// Fetch autoplay suggestions seeded by `seed` (answer: `Suggestions`).
    fn suggest(&mut self, tag: u64, seed: TrackId);
    /// Expand the items of an `Open`, in order (answer: `Expanded`).
    fn expand(&mut self, tag: u64, items: Vec<Item>);
}

/// A resolved stream, opened and ready for the engine.
pub struct Prepared {
    pub track: TrackId,
    /// The quality Tidal granted.
    pub quality: AudioQuality,
    /// The track's length, when the stream says (DASH timelines do).
    pub duration: Option<Duration>,
    pub source: Box<dyn TrackSource>,
}

impl fmt::Debug for Prepared {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Prepared")
            .field("track", &self.track)
            .field("quality", &self.quality)
            .field("duration", &self.duration)
            .finish_non_exhaustive()
    }
}

/// Resolves a track and opens its source; a failure is already classified
/// and worded (spec 0004 "Failures").
pub trait StreamOpener: Send + Sync {
    fn open(&self, track: TrackId) -> BoxFuture<'static, Result<Prepared, Failure>>;
}

/// Track metadata and suggestions (implemented by [`MetadataClient`]).
pub trait Metadata: Send + Sync {
    fn track(&self, id: TrackId) -> BoxFuture<'_, Result<Track, MetadataError>>;
    fn album(&self, id: u64) -> BoxFuture<'_, Result<Vec<Track>, MetadataError>>;
    fn playlist(&self, uuid: String) -> BoxFuture<'_, Result<Vec<Track>, MetadataError>>;
    fn suggestions(&self, seed: TrackId) -> BoxFuture<'_, Result<Vec<Track>, MetadataError>>;
}

impl Metadata for MetadataClient {
    fn track(&self, id: TrackId) -> BoxFuture<'_, Result<Track, MetadataError>> {
        Box::pin(self.get_track(id))
    }

    fn album(&self, id: u64) -> BoxFuture<'_, Result<Vec<Track>, MetadataError>> {
        Box::pin(self.get_album_tracks(id))
    }

    fn playlist(&self, uuid: String) -> BoxFuture<'_, Result<Vec<Track>, MetadataError>> {
        Box::pin(async move { self.get_playlist_tracks(&uuid).await })
    }

    fn suggestions(&self, seed: TrackId) -> BoxFuture<'_, Result<Vec<Track>, MetadataError>> {
        Box::pin(self.get_suggestions(seed))
    }
}

// --- items -----------------------------------------------------------------------

/// Why the queue could not be filled from the given items.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ExpandError {
    /// Not a Tidal track, album or playlist (exit 2).
    #[error(transparent)]
    BadItem(#[from] ItemError),
    /// An album or playlist without a single track (exit 2).
    #[error("{0} has no tracks")]
    NoTracks(Item),
    /// The metadata could not be fetched; the message (exit 1).
    #[error("{0}")]
    Fetch(String),
}

impl ExpandError {
    /// `2` for items that can never play, `1` for a failed fetch.
    pub fn exit_code(&self) -> u8 {
        match self {
            Self::BadItem(_) | Self::NoTracks(_) => 2,
            Self::Fetch(_) => 1,
        }
    }
}

/// Parses every command-line item, refusing the first bad one.
pub fn parse_items(args: &[String]) -> Result<Vec<Item>, ExpandError> {
    args.iter()
        .map(|arg| parse_item(arg).map_err(ExpandError::from))
        .collect()
}

/// Expands `items` in order into one list of tracks (every page of an album
/// or playlist): the queue of `play` and of `tidal-player [ITEM]...`.
pub async fn expand_items(meta: &dyn Metadata, items: &[Item]) -> Result<Vec<Track>, ExpandError> {
    let mut tracks = Vec::new();
    for item in items {
        let found = match item {
            Item::Track(id) => meta.track(*id).await.map(|t| vec![t]),
            Item::Album(id) => meta.album(*id).await,
            Item::Playlist(uuid) => meta.playlist(uuid.clone()).await,
        }
        .map_err(|e| ExpandError::Fetch(metadata_error_message(&e)))?;
        if found.is_empty() {
            return Err(ExpandError::NoTracks(item.clone()));
        }
        tracks.extend(found);
    }
    Ok(tracks)
}

/// The one-line message for a failed metadata fetch.
pub fn metadata_error_message(error: &MetadataError) -> String {
    let message = error.to_string();
    let mut chars = message.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

/// A seed for the shuffle PRNG, from the clock.
pub fn time_seed() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0x5eed, |d| d.as_nanos() as u64)
}

// --- the runtime -------------------------------------------------------------------

/// An input to the runtime.
#[derive(Debug)]
pub enum RuntimeInput {
    /// From a client.
    Command(Command),
    /// From the engine.
    Engine(audio::Event),
    /// A [`Jobs::resolve`] result.
    Resolved {
        tag: u64,
        result: Result<Prepared, Failure>,
    },
    /// A [`Jobs::suggest`] result: tracks, or why there are none.
    Suggestions {
        tag: u64,
        result: Result<Vec<Track>, String>,
    },
    /// From a socket client (spec 0005 "Messages").
    Client(ClientInput),
    /// A [`Jobs::expand`] result: the tracks, or the message.
    Expanded {
        tag: u64,
        result: Result<Vec<Track>, String>,
    },
    /// The authenticator's status changed (spec 0005 "The daemon").
    Login { required: bool },
}

/// A track the player accepted as started (engine `Started`, or a gapless
/// `Transitioned`).
#[derive(Debug, Clone, PartialEq)]
pub struct TrackStart {
    pub track: TrackId,
    pub quality: AudioQuality,
    /// From the stream, else from the metadata.
    pub duration: Option<Duration>,
    pub source: SourceFormat,
    pub output: OutputInfo,
}

/// What one input did, for the caller to pass on.
#[derive(Debug, Default)]
pub struct Handled {
    /// Events for the clients, in order.
    pub events: Vec<Event>,
    /// A track started.
    pub started: Option<TrackStart>,
    /// A track played to its end.
    pub ended: bool,
    /// Failures handed to the player (now or for later: a failed preload).
    pub failures: Vec<Failure>,
    /// The runtime shut down (the engine is stopped).
    pub shutdown: bool,
}

/// A track handed to the engine.
#[derive(Debug, Clone, Copy)]
struct Sent {
    track: TrackId,
    quality: AudioQuality,
    duration: Option<Duration>,
}

/// Runs the player against an engine and a job runner.
pub struct PlayerRuntime<E, J> {
    state: PlayerState,
    engine: E,
    jobs: J,
    /// Opened streams waiting for `EnginePlay`/`EnginePreload`, by tag.
    ready: HashMap<u64, Prepared>,
    /// The tag of the latest `Resolve { Play }` and `Resolve { Preload }`:
    /// a resolution for any other tag is stale.
    live_play: Option<u64>,
    live_preload: Option<u64>,
    /// Tracks the engine holds, by tag (the playing one and the preload).
    sent: HashMap<u64, Sent>,
    /// The tag of the preload the engine holds.
    engine_preload: Option<u64>,
    /// `FetchSuggestions` without an answer yet.
    suggesting: HashSet<u64>,
    /// The socket's clients (spec 0005).
    hub: Hub,
    /// `Open`s in the order received, expanding or waiting for an
    /// earlier one.
    opens: VecDeque<PendingOpen>,
    next_open: u64,
}

impl<E: EngineControl, J: Jobs> PlayerRuntime<E, J> {
    pub fn new(config: PlayerConfig, seed: u64, engine: E, jobs: J) -> Self {
        Self {
            state: PlayerState::new(config, seed),
            engine,
            jobs,
            ready: HashMap::new(),
            live_play: None,
            live_preload: None,
            sent: HashMap::new(),
            engine_preload: None,
            suggesting: HashSet::new(),
            hub: Hub::default(),
            opens: VecDeque::new(),
            next_open: 0,
        }
    }

    /// The player's state, as clients see it.
    pub fn snapshot(&self) -> PlayerSnapshot {
        self.state.snapshot()
    }

    /// Whether a suggestions request is out (a stopped player may still
    /// continue with its answer).
    pub fn suggestions_pending(&self) -> bool {
        !self.suggesting.is_empty()
    }

    /// The next input: a pending one from `inputs` first, else an engine
    /// event (waiting at most `wait`).
    pub fn next_input(
        &mut self,
        inputs: &Receiver<RuntimeInput>,
        wait: Duration,
    ) -> Option<RuntimeInput> {
        if let Ok(input) = inputs.try_recv() {
            return Some(input);
        }
        self.engine
            .poll_event(wait)
            .map(RuntimeInput::Engine)
            .or_else(|| inputs.try_recv().ok())
    }

    /// Applies one input and sends the events it caused to the socket's
    /// subscribers; the returned events are the same, for an in-process
    /// client (spec 0005 "Sync").
    pub fn handle(&mut self, input: RuntimeInput) -> Handled {
        match input {
            RuntimeInput::Client(input) => self.client_input(input),
            RuntimeInput::Expanded { tag, result } => self.expanded(tag, result),
            RuntimeInput::Login { required } => {
                let events: Vec<Event> =
                    self.hub.set_login_required(required).into_iter().collect();
                self.hub.broadcast(&events);
                Handled {
                    events,
                    ..Handled::default()
                }
            }
            RuntimeInput::Command(Command::Open { items, at }) => {
                self.open(items, at, None);
                Handled::default()
            }
            input => self.step(input),
        }
    }

    /// [`Self::apply`], then the events to the subscribers.
    fn step(&mut self, input: RuntimeInput) -> Handled {
        let handled = self.apply(input);
        self.hub.broadcast(&handled.events);
        handled
    }

    /// Applies one input: maps it into the player's terms, runs `update`
    /// and executes the effects in order.
    fn apply(&mut self, input: RuntimeInput) -> Handled {
        let mut handled = Handled::default();
        let mut resolved_tag = None;
        let mut notice = Notice::None;
        let input = match input {
            RuntimeInput::Command(Command::Shutdown) => {
                self.engine.send(audio::Command::Stop);
                self.engine.send(audio::Command::Shutdown);
                self.ready.clear();
                handled.events.push(Event::ShuttingDown);
                handled.shutdown = true;
                return handled;
            }
            RuntimeInput::Command(command) => PlayerInput::Command(command),
            RuntimeInput::Engine(event) => {
                let (input, n) = self.engine_input(event, &mut handled);
                notice = n;
                input
            }
            RuntimeInput::Resolved { tag, result } => {
                resolved_tag = Some(tag);
                let result = match result {
                    Ok(prepared) => {
                        let quality = prepared.quality;
                        self.ready.insert(tag, prepared);
                        Ok(quality)
                    }
                    Err(failure) => {
                        handled.failures.push(failure.clone());
                        Err(failure)
                    }
                };
                PlayerInput::Resolved { tag, result }
            }
            RuntimeInput::Suggestions { tag, result } => {
                self.suggesting.remove(&tag);
                PlayerInput::Suggestions { tag, result }
            }
            // Routed by `handle`.
            RuntimeInput::Client(_)
            | RuntimeInput::Expanded { .. }
            | RuntimeInput::Login { .. } => {
                return handled;
            }
        };

        let effects = player::update(&mut self.state, input);
        // A stale input changes nothing and returns nothing (AC6, AC11).
        let accepted = !effects.is_empty();
        for effect in effects {
            self.execute(effect, &mut handled);
        }

        if accepted {
            match notice {
                Notice::None => {}
                Notice::Started(start) => handled.started = Some(self.with_duration(start)),
                Notice::Transitioned(start) => {
                    // The preload is now the playing track.
                    if let Some(tag) = self.engine_preload.take() {
                        self.sent.retain(|t, _| *t == tag);
                    }
                    handled.started = Some(self.with_duration(start));
                    handled.ended = true;
                }
                Notice::Ended => handled.ended = true,
            }
        }
        if let Some(tag) = resolved_tag
            && !self.is_live(tag)
        {
            // Superseded while it resolved: close it now.
            self.ready.remove(&tag);
        }
        handled
    }

    fn is_live(&self, tag: u64) -> bool {
        self.live_play == Some(tag) || self.live_preload == Some(tag)
    }

    /// The metadata duration when the stream gave none.
    fn with_duration(&self, mut start: TrackStart) -> TrackStart {
        if start.duration.is_none() {
            let snapshot = self.state.snapshot();
            start.duration = snapshot
                .current
                .and_then(|id| snapshot.queue.iter().find(|e| e.id == id))
                .and_then(|e| e.track.duration);
        }
        start
    }

    /// Maps an engine event into the player's terms.
    fn engine_input(&self, event: audio::Event, handled: &mut Handled) -> (PlayerInput, Notice) {
        let sent = |tag: u64| self.sent.get(&tag).copied();
        let start = |tag: u64, source: &SourceFormat, output: &OutputInfo| {
            sent(tag).map(|s| TrackStart {
                track: s.track,
                quality: s.quality,
                duration: s.duration,
                source: *source,
                output: output.clone(),
            })
        };
        let (event, notice) = match event {
            audio::Event::Started {
                tag,
                source,
                output,
            } => {
                let notice = start(tag, &source, &output).map_or(Notice::None, Notice::Started);
                let details = details(&source, &output);
                (EngineEvent::Started { tag, details }, notice)
            }
            audio::Event::Transitioned {
                tag,
                source,
                output,
            } => {
                let notice =
                    start(tag, &source, &output).map_or(Notice::None, Notice::Transitioned);
                let details = details(&source, &output);
                (EngineEvent::Transitioned { tag, details }, notice)
            }
            audio::Event::Position(position) => (EngineEvent::Position(position), Notice::None),
            audio::Event::Buffering => (EngineEvent::Buffering, Notice::None),
            audio::Event::Buffered => (EngineEvent::Buffered, Notice::None),
            audio::Event::TrackEnded { tag } => (EngineEvent::TrackEnded { tag }, Notice::Ended),
            audio::Event::Paused => (EngineEvent::Paused, Notice::None),
            audio::Event::Resumed => (EngineEvent::Resumed, Notice::None),
            audio::Event::Stopped => (EngineEvent::Stopped, Notice::None),
            audio::Event::Released => (EngineEvent::Released, Notice::None),
            audio::Event::ResumeFailed(error) => {
                // 0003's output message (it names the device, not the track).
                let failure = engine_failure(0, &audio::EngineError::Output(error));
                handled.failures.push(failure.clone());
                (EngineEvent::ResumeFailed { failure }, Notice::None)
            }
            audio::Event::Error { tag, error } => {
                let track = sent(tag).map_or(0, |s| s.track.0);
                let failure = engine_failure(track, &error);
                handled.failures.push(failure.clone());
                (EngineEvent::Error { tag, failure }, Notice::None)
            }
        };
        (PlayerInput::Engine(event), notice)
    }

    fn execute(&mut self, effect: PlayerEffect, handled: &mut Handled) {
        match effect {
            PlayerEffect::Resolve {
                track,
                tag,
                purpose,
                ..
            } => {
                match purpose {
                    Purpose::Play => {
                        self.live_play = Some(tag);
                        self.live_preload = None;
                    }
                    Purpose::Preload => self.live_preload = Some(tag),
                }
                let (play, preload) = (self.live_play, self.live_preload);
                self.ready
                    .retain(|t, _| Some(*t) == play || Some(*t) == preload);
                self.jobs.resolve(tag, track);
            }
            PlayerEffect::EnginePlay { tag, start_at } => {
                let Some(prepared) = self.ready.remove(&tag) else {
                    tracing::warn!(tag, "no stream to play for this tag");
                    return;
                };
                self.sent.clear();
                self.sent.insert(tag, sent_of(&prepared));
                self.engine_preload = None;
                self.engine.send(audio::Command::Play {
                    tag,
                    source: prepared.source,
                    start_at,
                });
            }
            PlayerEffect::EnginePreload { tag } => {
                let Some(prepared) = self.ready.remove(&tag) else {
                    tracing::warn!(tag, "no stream to preload for this tag");
                    return;
                };
                // The engine holds one preload: it replaces the earlier one.
                if let Some(old) = self.engine_preload.replace(tag) {
                    self.sent.remove(&old);
                }
                self.sent.insert(tag, sent_of(&prepared));
                self.engine.send(audio::Command::Preload {
                    tag,
                    source: prepared.source,
                });
            }
            PlayerEffect::EngineCancelPreload => self.engine.send(audio::Command::CancelPreload),
            PlayerEffect::EnginePause => self.engine.send(audio::Command::Pause),
            PlayerEffect::EngineResume => self.engine.send(audio::Command::Resume),
            PlayerEffect::EngineSeek(position) => self.engine.send(audio::Command::Seek(position)),
            PlayerEffect::EngineStop => self.engine.send(audio::Command::Stop),
            PlayerEffect::EngineSetGain(gain) => self.engine.send(audio::Command::SetGain(gain)),
            PlayerEffect::FetchSuggestions { seed, tag } => {
                self.suggesting.insert(tag);
                self.jobs.suggest(tag, seed);
            }
            PlayerEffect::Broadcast(event) => handled.events.push(event),
        }
    }
}

// --- clients and `Open` (spec 0005) ------------------------------------------

/// An `Open` waiting for its expansion, or for an earlier `Open`'s.
#[derive(Debug)]
struct PendingOpen {
    tag: u64,
    at: Option<InsertAt>,
    /// The request to answer, for a socket client.
    reply: Option<(ClientId, u64)>,
    result: Option<Result<Vec<Track>, String>>,
}

impl<E: EngineControl, J: Jobs> PlayerRuntime<E, J> {
    fn client_input(&mut self, input: ClientInput) -> Handled {
        match input {
            ClientInput::Attach { client, peer } => self.hub.attach(client, peer),
            ClientInput::Detach(client) => self.hub.detach(client),
            ClientInput::Subscribe(client) => {
                let snapshot = self.snapshot();
                self.hub.subscribe(client, snapshot);
            }
            ClientInput::Request {
                client,
                id,
                command: Command::Open { items, at },
            } => self.open(items, at, Some((client, id))),
            ClientInput::Request {
                client,
                id,
                command,
            } => {
                let handled = self.step(RuntimeInput::Command(command));
                self.hub.reply(client, id, Ok(()));
                return handled;
            }
        }
        Handled::default()
    }

    /// Starts expanding an `Open`'s items; it is applied in turn.
    fn open(&mut self, items: Vec<Item>, at: Option<InsertAt>, reply: Option<(ClientId, u64)>) {
        let tag = self.next_open;
        self.next_open += 1;
        self.opens.push_back(PendingOpen {
            tag,
            at,
            reply,
            result: None,
        });
        self.jobs.expand(tag, items);
    }

    /// An expansion finished: applies every `Open` that is ready, in the
    /// order received.
    fn expanded(&mut self, tag: u64, result: Result<Vec<Track>, String>) -> Handled {
        if let Some(open) = self.opens.iter_mut().find(|o| o.tag == tag) {
            open.result = Some(result);
        }
        let mut handled = Handled::default();
        while self.opens.front().is_some_and(|o| o.result.is_some()) {
            let Some(PendingOpen {
                at,
                reply,
                result: Some(result),
                ..
            }) = self.opens.pop_front()
            else {
                break;
            };
            let outcome = result.map(|tracks| {
                let queued = self.queue_tracks(tracks, at);
                merge(&mut handled, queued);
            });
            if let Some((client, id)) = reply {
                self.hub.reply(client, id, outcome);
            }
        }
        handled
    }

    /// An expanded `Open`: `LoadQueue`, or `AddToQueue` then `PlayEntry` of
    /// the first added entry when nothing was playing (0004 AC28's rule).
    fn queue_tracks(&mut self, tracks: Vec<Track>, at: Option<InsertAt>) -> Handled {
        if tracks.is_empty() {
            return Handled::default();
        }
        let Some(at) = at else {
            return self.step(RuntimeInput::Command(Command::LoadQueue {
                tracks,
                start: 0,
            }));
        };
        let before = self.snapshot();
        let idle = before.queue.is_empty()
            || (before.state == PlaybackState::Stopped && before.current.is_none());
        let mut handled = self.step(RuntimeInput::Command(Command::AddToQueue { tracks, at }));
        if idle {
            let first = self
                .snapshot()
                .queue
                .iter()
                .map(|e| e.id)
                .find(|id| !before.queue.iter().any(|e| e.id == *id));
            if let Some(first) = first {
                let started = self.step(RuntimeInput::Command(Command::PlayEntry(first)));
                merge(&mut handled, started);
            }
        }
        handled
    }
}

/// Appends what `more` did to `into`.
fn merge(into: &mut Handled, more: Handled) {
    into.events.extend(more.events);
    into.failures.extend(more.failures);
    into.started = more.started.or(into.started.take());
    into.ended |= more.ended;
    into.shutdown |= more.shutdown;
}

/// What an engine event means for the caller, once the player accepted it.
enum Notice {
    None,
    Started(TrackStart),
    Transitioned(TrackStart),
    Ended,
}

fn sent_of(prepared: &Prepared) -> Sent {
    Sent {
        track: prepared.track,
        quality: prepared.quality,
        duration: prepared.duration,
    }
}

/// The engine's report on a started track, in the snapshot's words.
fn details(source: &SourceFormat, output: &OutputInfo) -> TrackDetails {
    TrackDetails {
        source: source_description(source),
        output: output_description(output),
        bit_perfect: output.bit_perfect,
        reason: output.not_bit_perfect_reason.clone(),
    }
}

// --- standalone: the runtime on its own thread -----------------------------------------

/// The player running on its own thread (0001's standalone mode): clients
/// send commands and read events; nothing is shared but the channels.
#[derive(Debug)]
pub struct RuntimeHandle {
    inputs: Sender<RuntimeInput>,
    events: Receiver<Event>,
    thread: Option<JoinHandle<()>>,
}

/// Starts `runtime` on a thread reading `inputs` (whose sender `sender` is,
/// shared with the jobs).
pub fn spawn_runtime<E, J>(
    mut runtime: PlayerRuntime<E, J>,
    inputs: Receiver<RuntimeInput>,
    sender: Sender<RuntimeInput>,
) -> RuntimeHandle
where
    E: EngineControl + Send + 'static,
    J: Jobs + Send + 'static,
{
    let (events_tx, events) = mpsc::channel();
    let thread = std::thread::Builder::new()
        .name("player".into())
        .spawn(move || {
            loop {
                let Some(input) = runtime.next_input(&inputs, POLL) else {
                    continue;
                };
                let handled = runtime.handle(input);
                for event in handled.events {
                    let _ = events_tx.send(event);
                }
                if handled.shutdown {
                    break;
                }
            }
            // Dropping the runtime drops the engine, which joins its thread:
            // the device is closed and released when this thread ends.
        })
        .expect("spawn the player thread");
    RuntimeHandle {
        inputs: sender,
        events,
        thread: Some(thread),
    }
}

impl RuntimeHandle {
    /// Sends a client command.
    pub fn send(&self, command: Command) {
        let _ = self.inputs.send(RuntimeInput::Command(command));
    }

    /// The player's events, in order.
    pub fn events(&self) -> &Receiver<Event> {
        &self.events
    }

    /// The player's input channel (for the socket's clients).
    pub fn inputs(&self) -> Sender<RuntimeInput> {
        self.inputs.clone()
    }

    /// Waits until the player thread ends (after a `Shutdown`).
    pub fn wait(mut self) {
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }

    /// Stops the engine and waits until the player thread (and with it the
    /// engine and its device) is gone.
    pub fn shutdown(mut self) {
        self.stop();
    }

    fn stop(&mut self) {
        if let Some(thread) = self.thread.take() {
            let _ = self.inputs.send(RuntimeInput::Command(Command::Shutdown));
            let _ = thread.join();
        }
    }
}

impl Drop for RuntimeHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

/// [`Jobs`] on a tokio runtime: each job is a task that sends its result
/// back on `results`.
pub struct TokioJobs {
    runtime: tokio::runtime::Handle,
    opener: Arc<dyn StreamOpener>,
    metadata: Arc<dyn Metadata>,
    results: Sender<RuntimeInput>,
}

impl TokioJobs {
    pub fn new(
        runtime: tokio::runtime::Handle,
        opener: Arc<dyn StreamOpener>,
        metadata: Arc<dyn Metadata>,
        results: Sender<RuntimeInput>,
    ) -> Self {
        Self {
            runtime,
            opener,
            metadata,
            results,
        }
    }
}

impl Jobs for TokioJobs {
    fn resolve(&mut self, tag: u64, track: TrackId) {
        let opening = self.opener.open(track);
        let results = self.results.clone();
        self.runtime.spawn(async move {
            let result = opening.await;
            let _ = results.send(RuntimeInput::Resolved { tag, result });
        });
    }

    fn suggest(&mut self, tag: u64, seed: TrackId) {
        let metadata = Arc::clone(&self.metadata);
        let results = self.results.clone();
        self.runtime.spawn(async move {
            let result = metadata.suggestions(seed).await.map_err(|e| e.to_string());
            let _ = results.send(RuntimeInput::Suggestions { tag, result });
        });
    }

    fn expand(&mut self, tag: u64, items: Vec<Item>) {
        let metadata = Arc::clone(&self.metadata);
        let results = self.results.clone();
        self.runtime.spawn(async move {
            let result = expand_items(metadata.as_ref(), &items)
                .await
                .map_err(|e| e.to_string());
            let _ = results.send(RuntimeInput::Expanded { tag, result });
        });
    }
}

// --- fakes ---------------------------------------------------------------------------

/// Fakes of the engine and the jobs, shared by the runtime's and `play`'s
/// tests.
#[cfg(test)]
pub(crate) mod fakes {
    use std::collections::{HashMap, VecDeque};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use super::*;
    use tidal_player_audio::{
        Codec, EngineError, OutputKind, ReadOutcome, SampleFormat, SourceError, SourceLayout,
    };
    use tidal_player_core::player::FailureKind;
    use tidal_player_core::{AlbumRef, ArtistRef};

    /// One call the runtime made, on the engine or the jobs, in order.
    #[derive(Debug, Clone, PartialEq)]
    pub enum Call {
        Resolve {
            tag: u64,
            track: u64,
        },
        Suggest {
            tag: u64,
            seed: u64,
        },
        Play {
            tag: u64,
            track: u64,
            start_at: Duration,
        },
        Preload {
            tag: u64,
            track: u64,
        },
        CancelPreload,
        SetGain(f32),
        Pause,
        Resume,
        Seek(Duration),
        SetDevice(String),
        Stop,
        Shutdown,
        Expand {
            tag: u64,
            items: Vec<Item>,
        },
    }

    pub type Log = Arc<Mutex<Vec<Call>>>;

    /// A source that says which track it is (as its "length") and counts
    /// how many sources were dropped (closed).
    pub struct FakeTrack {
        pub track: u64,
        pub drops: Arc<AtomicUsize>,
    }

    impl TrackSource for FakeTrack {
        fn layout(&self) -> SourceLayout {
            SourceLayout::SingleFile {
                len: Some(self.track),
            }
        }

        fn read(&mut self, _: &mut [u8]) -> Result<ReadOutcome, SourceError> {
            Ok(ReadOutcome::End)
        }
    }

    impl Drop for FakeTrack {
        fn drop(&mut self) {
            self.drops.fetch_add(1, Ordering::SeqCst);
        }
    }

    pub fn track_of(source: &dyn TrackSource) -> u64 {
        match source.layout() {
            SourceLayout::SingleFile { len } => len.unwrap_or(0),
            SourceLayout::Segmented { .. } => 0,
        }
    }

    pub fn prepared(track: u64, drops: &Arc<AtomicUsize>) -> Prepared {
        Prepared {
            track: TrackId(track),
            quality: AudioQuality::Lossless,
            duration: None,
            source: Box::new(FakeTrack {
                track,
                drops: Arc::clone(drops),
            }),
        }
    }

    pub const SOURCE: SourceFormat = SourceFormat {
        codec: Codec::Flac,
        sample_rate: 44_100,
        channels: 2,
        bits_per_sample: Some(16),
    };

    pub fn output() -> OutputInfo {
        OutputInfo {
            requested: "hw:1,0".into(),
            device: "hw:1,0".into(),
            kind: OutputKind::Exclusive,
            sample_format: SampleFormat::S32Le,
            sample_rate: 44_100,
            channels: 2,
            bit_perfect: true,
            not_bit_perfect_reason: None,
        }
    }

    /// What the fake engine does with one `Play` of a track.
    #[derive(Debug, Clone)]
    pub enum Script {
        /// `Started`, then `TrackEnded`.
        Ends,
        /// `Started`, then keeps playing.
        Plays,
        /// `Error` (before `Started`).
        Fails(EngineError),
    }

    /// An engine that answers each command at once with scripted events.
    pub struct FakeEngine {
        pub log: Log,
        /// Per track, the outcome of each successive `Play` (default
        /// [`Script::Ends`]).
        pub script: HashMap<u64, VecDeque<Script>>,
        pub default: Script,
        events: VecDeque<audio::Event>,
        /// The source of the track playing (an engine keeps it open).
        playing: Option<Box<dyn TrackSource>>,
    }

    impl FakeEngine {
        pub fn new(log: &Log, default: Script) -> Self {
            Self {
                log: Arc::clone(log),
                script: HashMap::new(),
                default,
                events: VecDeque::new(),
                playing: None,
            }
        }

        pub fn script(mut self, track: u64, outcomes: Vec<Script>) -> Self {
            self.script.insert(track, outcomes.into());
            self
        }
    }

    impl EngineControl for FakeEngine {
        fn send(&mut self, command: audio::Command) {
            let call = match command {
                audio::Command::Play {
                    tag,
                    source,
                    start_at,
                } => {
                    let track = track_of(source.as_ref());
                    let outcome = self
                        .script
                        .get_mut(&track)
                        .and_then(VecDeque::pop_front)
                        .unwrap_or_else(|| self.default.clone());
                    let started = audio::Event::Started {
                        tag,
                        source: SOURCE,
                        output: output(),
                    };
                    match outcome {
                        Script::Ends => {
                            self.events.push_back(started);
                            self.events.push_back(audio::Event::TrackEnded { tag });
                        }
                        Script::Plays => self.events.push_back(started),
                        Script::Fails(error) => {
                            self.events.push_back(audio::Event::Error { tag, error });
                        }
                    }
                    self.playing = Some(source);
                    Call::Play {
                        tag,
                        track,
                        start_at,
                    }
                }
                audio::Command::Preload { tag, source } => Call::Preload {
                    tag,
                    track: track_of(source.as_ref()),
                },
                audio::Command::CancelPreload => Call::CancelPreload,
                audio::Command::SetGain(gain) => Call::SetGain(gain),
                audio::Command::Pause => Call::Pause,
                audio::Command::Resume => Call::Resume,
                audio::Command::Seek(position) => Call::Seek(position),
                audio::Command::SetDevice(device) => Call::SetDevice(device),
                audio::Command::Stop => Call::Stop,
                audio::Command::Shutdown => Call::Shutdown,
            };
            self.log.lock().unwrap().push(call);
        }

        fn poll_event(&mut self, wait: Duration) -> Option<audio::Event> {
            let event = self.events.pop_front();
            if event.is_none() && !wait.is_zero() {
                std::thread::sleep(wait);
            }
            event
        }
    }

    /// Jobs that record each request and, when `immediate`, answer it at
    /// once on `results` (per track: a failure, else a stream).
    pub struct FakeJobs {
        pub log: Log,
        pub results: Sender<RuntimeInput>,
        pub immediate: bool,
        pub failures: HashMap<u64, Failure>,
        pub suggestions: Vec<Track>,
        pub drops: Arc<AtomicUsize>,
        /// Expands `Open`s at once when `immediate` (else the test answers).
        pub metadata: Option<Arc<dyn Metadata>>,
    }

    impl FakeJobs {
        pub fn new(log: &Log, results: &Sender<RuntimeInput>, immediate: bool) -> Self {
            Self {
                log: Arc::clone(log),
                results: results.clone(),
                immediate,
                failures: HashMap::new(),
                suggestions: Vec::new(),
                drops: Arc::new(AtomicUsize::new(0)),
                metadata: None,
            }
        }
    }

    impl Jobs for FakeJobs {
        fn resolve(&mut self, tag: u64, track: TrackId) {
            self.log.lock().unwrap().push(Call::Resolve {
                tag,
                track: track.0,
            });
            if self.immediate {
                let result = match self.failures.get(&track.0) {
                    Some(failure) => Err(failure.clone()),
                    None => Ok(prepared(track.0, &self.drops)),
                };
                let _ = self.results.send(RuntimeInput::Resolved { tag, result });
            }
        }

        fn suggest(&mut self, tag: u64, seed: TrackId) {
            self.log
                .lock()
                .unwrap()
                .push(Call::Suggest { tag, seed: seed.0 });
            if self.immediate {
                let result = Ok(self.suggestions.clone());
                let _ = self.results.send(RuntimeInput::Suggestions { tag, result });
            }
        }

        fn expand(&mut self, tag: u64, items: Vec<Item>) {
            self.log.lock().unwrap().push(Call::Expand {
                tag,
                items: items.clone(),
            });
            if let (true, Some(metadata)) = (self.immediate, &self.metadata) {
                let result = tokio::runtime::Builder::new_current_thread()
                    .build()
                    .expect("a test runtime")
                    .block_on(expand_items(metadata.as_ref(), &items))
                    .map_err(|e| e.to_string());
                let _ = self.results.send(RuntimeInput::Expanded { tag, result });
            }
        }
    }

    pub fn track(id: u64, duration: Option<u64>) -> Track {
        Track {
            id: TrackId(id),
            title: format!("Title {id}"),
            version: None,
            artists: vec![ArtistRef {
                id: 1,
                name: "Artist".into(),
            }],
            album: Some(AlbumRef {
                id: 1,
                title: "Album".into(),
            }),
            duration: duration.map(Duration::from_secs),
            streamable: true,
        }
    }

    pub fn not_available(track: u64) -> Failure {
        Failure {
            kind: FailureKind::TrackOnly,
            message: format!("Track {track} is not available in NO"),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::fakes::*;
    use super::*;
    use crate::ipc::server::ClientId;
    use tidal_player_audio::{EngineError, SinkError};
    use tidal_player_core::EntryId;
    use tidal_player_core::protocol::PlaybackState;
    use tidal_player_core::ui;

    fn runtime(
        default: Script,
        immediate: bool,
    ) -> (
        PlayerRuntime<FakeEngine, FakeJobs>,
        Log,
        Receiver<RuntimeInput>,
    ) {
        let log: Log = Arc::default();
        let (tx, rx) = mpsc::channel();
        let engine = FakeEngine::new(&log, default);
        let jobs = FakeJobs::new(&log, &tx, immediate);
        (
            PlayerRuntime::new(PlayerConfig::default(), 7, engine, jobs),
            log,
            rx,
        )
    }

    fn take(log: &Log) -> Vec<Call> {
        std::mem::take(&mut *log.lock().unwrap())
    }

    fn load(ids: &[u64]) -> RuntimeInput {
        RuntimeInput::Command(Command::LoadQueue {
            tracks: ids.iter().map(|id| track(*id, Some(200))).collect(),
            start: 0,
        })
    }

    fn last_snapshot(handled: &Handled) -> PlayerSnapshot {
        match handled.events.last() {
            Some(Event::Player(snapshot)) => snapshot.clone(),
            other => panic!("expected a snapshot last, got {other:?}"),
        }
    }

    fn resolve_tag(call: &Call) -> u64 {
        match call {
            Call::Resolve { tag, .. } => *tag,
            other => panic!("expected a resolution, got {other:?}"),
        }
    }

    /// AC19: effects run in order (engine and jobs in one log), engine
    /// events and resolutions come back as inputs with their tags, and a
    /// mash of `Next` while loading plays only the last entry.
    #[test]
    fn ac19_effects_and_mash() {
        let drops = Arc::new(AtomicUsize::new(0));
        let (mut rt, log, rx) = runtime(Script::Plays, false);

        // Load: one resolution, nothing on the engine yet.
        let h = rt.handle(load(&[1, 2, 3]));
        let calls = take(&log);
        assert_eq!(calls.len(), 1, "{calls:?}");
        let tag1 = resolve_tag(&calls[0]);
        assert_eq!(
            calls[0],
            Call::Resolve {
                tag: tag1,
                track: 1
            }
        );
        assert_eq!(last_snapshot(&h).state, PlaybackState::Loading);

        // The resolution comes back with its tag: the engine plays it.
        let h = rt.handle(RuntimeInput::Resolved {
            tag: tag1,
            result: Ok(prepared(1, &drops)),
        });
        assert_eq!(
            take(&log),
            vec![Call::Play {
                tag: tag1,
                track: 1,
                start_at: Duration::ZERO
            }]
        );
        assert!(h.started.is_none());

        // The engine's `Started` (same tag) makes it playing.
        let input = rt.next_input(&rx, Duration::ZERO).expect("Started");
        let h = rt.handle(input);
        assert_eq!(last_snapshot(&h).state, PlaybackState::Playing);
        let started = h.started.expect("a track start");
        assert_eq!(started.track, TrackId(1));
        assert_eq!(started.duration, Some(Duration::from_secs(200)));

        // Commands map to engine commands, in order.
        rt.handle(RuntimeInput::Command(Command::TogglePause));
        rt.handle(RuntimeInput::Command(Command::ChangeVolume(-50)));
        rt.handle(RuntimeInput::Command(Command::TogglePause));
        assert_eq!(
            take(&log),
            vec![Call::Pause, Call::SetGain(0.125), Call::Resume]
        );

        // A position is forwarded on its own.
        let h = rt.handle(RuntimeInput::Engine(audio::Event::Position(
            Duration::from_secs(1),
        )));
        assert_eq!(
            h.events,
            vec![Event::Position {
                entry: EntryId(1),
                position: Duration::from_secs(1)
            }]
        );

        // Next while playing: stop first, then resolve the next entry.
        rt.handle(RuntimeInput::Command(Command::Next));
        let calls = take(&log);
        assert_eq!(calls.len(), 2, "{calls:?}");
        assert_eq!(calls[0], Call::Stop);
        let tag2 = resolve_tag(&calls[1]);
        assert_eq!(
            calls[1],
            Call::Resolve {
                tag: tag2,
                track: 2
            }
        );

        // The replaced track's late `TrackEnded` changes nothing.
        let h = rt.handle(RuntimeInput::Engine(audio::Event::TrackEnded { tag: tag1 }));
        assert!(h.events.is_empty() && !h.ended, "{h:?}");

        // An engine error comes back classified and worded, with its tag.
        rt.handle(RuntimeInput::Resolved {
            tag: tag2,
            result: Ok(prepared(2, &drops)),
        });
        assert!(matches!(take(&log)[..], [Call::Play { track: 2, .. }]));
        let busy = EngineError::Output(SinkError::Busy {
            device: "hw:1,0".into(),
            holder: None,
        });
        let h = rt.handle(RuntimeInput::Engine(audio::Event::Error {
            tag: tag2,
            error: busy,
        }));
        let snapshot = last_snapshot(&h);
        assert_eq!(snapshot.state, PlaybackState::Stopped);
        assert_eq!(
            snapshot.message.as_deref(),
            Some("Output hw:1,0 is busy: close it, or use --device default")
        );
        assert_eq!(h.failures.len(), 1);

        // The mash: load, then 5 × Next while loading. Every press resolves;
        // the results come back (in order, or reversed) and only the last
        // entry plays. The superseded streams are closed.
        for reversed in [false, true] {
            let drops = Arc::new(AtomicUsize::new(0));
            let (mut rt, log, _rx) = runtime(Script::Plays, false);
            rt.handle(load(&[1, 2, 3, 4, 5, 6]));
            for _ in 0..5 {
                rt.handle(RuntimeInput::Command(Command::Next));
            }
            let calls = take(&log);
            let tags: Vec<u64> = calls.iter().map(resolve_tag).collect();
            assert_eq!(tags.len(), 6, "{calls:?}");
            let mut order: Vec<(u64, u64)> = calls
                .iter()
                .map(|c| match c {
                    Call::Resolve { tag, track } => (*tag, *track),
                    _ => unreachable!(),
                })
                .collect();
            if reversed {
                order.reverse();
            }
            for (tag, track) in order {
                rt.handle(RuntimeInput::Resolved {
                    tag,
                    result: Ok(prepared(track, &drops)),
                });
            }
            let calls = take(&log);
            assert_eq!(
                calls,
                vec![Call::Play {
                    tag: tags[5],
                    track: 6,
                    start_at: Duration::ZERO
                }],
                "reversed: {reversed}"
            );
            assert_eq!(drops.load(Ordering::SeqCst), 5, "reversed: {reversed}");
        }
    }

    /// AC19: quitting stops the engine (then shuts it down) before the
    /// player thread ends.
    #[test]
    fn ac19_quit_stops_engine() {
        // Without a thread: `Shutdown` stops the engine first.
        let (mut rt, log, rx) = runtime(Script::Plays, true);
        rt.handle(load(&[1]));
        while let Some(input) = rt.next_input(&rx, Duration::ZERO) {
            rt.handle(input);
        }
        assert_eq!(rt.snapshot().state, PlaybackState::Playing);
        take(&log);
        let h = rt.handle(RuntimeInput::Command(Command::Shutdown));
        assert!(h.shutdown);
        assert_eq!(h.events, vec![Event::ShuttingDown]);
        assert_eq!(take(&log), vec![Call::Stop, Call::Shutdown]);

        // Through the standalone handle, as the TUI quits.
        let log: Log = Arc::default();
        let (tx, rx) = mpsc::channel();
        let engine = FakeEngine::new(&log, Script::Plays);
        let jobs = FakeJobs::new(&log, &tx, true);
        let rt = PlayerRuntime::new(PlayerConfig::default(), 7, engine, jobs);
        let handle = spawn_runtime(rt, rx, tx);
        handle.send(Command::LoadQueue {
            tracks: vec![track(1, Some(200))],
            start: 0,
        });
        let playing = loop {
            match handle.events().recv_timeout(Duration::from_secs(5)) {
                Ok(Event::Player(s)) if s.state == PlaybackState::Playing => break true,
                Ok(_) => {}
                Err(_) => break false,
            }
        };
        assert!(playing, "the queue never started");
        handle.shutdown();
        let calls = take(&log);
        assert!(
            matches!(
                calls[..],
                [
                    Call::Resolve { track: 1, .. },
                    Call::Play { track: 1, .. },
                    Call::Stop,
                    Call::Shutdown
                ]
            ),
            "{calls:?}"
        );
    }

    /// Metadata from memory for the open prompt: album 10 is tracks 1 and
    /// 2, track 3 exists, anything else is not found.
    struct PromptMeta;

    impl Metadata for PromptMeta {
        fn track(&self, id: TrackId) -> BoxFuture<'_, Result<Track, MetadataError>> {
            let result = match id.0 {
                3 => Ok(track(3, Some(200))),
                _ => Err(MetadataError::NotFound(Item::Track(id))),
            };
            Box::pin(async move { result })
        }

        fn album(&self, id: u64) -> BoxFuture<'_, Result<Vec<Track>, MetadataError>> {
            let result = match id {
                10 => Ok(vec![track(1, Some(200)), track(2, Some(200))]),
                _ => Err(MetadataError::NotFound(Item::Album(id))),
            };
            Box::pin(async move { result })
        }

        fn playlist(&self, uuid: String) -> BoxFuture<'_, Result<Vec<Track>, MetadataError>> {
            Box::pin(async move { Err(MetadataError::NotFound(Item::Playlist(uuid))) })
        }

        fn suggestions(&self, _: TrackId) -> BoxFuture<'_, Result<Vec<Track>, MetadataError>> {
            Box::pin(async { Ok(Vec::new()) })
        }
    }

    /// A TUI client (the UI model) joined in-process to a runtime with
    /// fakes, as `main` wires the standalone TUI (spec 0005 "Roles": its
    /// screen is a client too): the player expands `Open`s (fake metadata)
    /// and its messages come back as actions.
    struct Client {
        ui: ui::State,
        rt: PlayerRuntime<FakeEngine, FakeJobs>,
        inputs: Receiver<RuntimeInput>,
        session: crate::client::Session<crate::client::InProcess>,
        /// Every command the client sent, in order.
        sent: Vec<Command>,
    }

    impl Client {
        fn new() -> (Self, Log) {
            use crate::client::{Connector, InProcess, Session};
            let log: Log = Arc::default();
            let (tx, inputs) = mpsc::channel();
            let engine = FakeEngine::new(&log, Script::Plays);
            let mut jobs = FakeJobs::new(&log, &tx, true);
            jobs.metadata = Some(Arc::new(PromptMeta));
            let rt = PlayerRuntime::new(PlayerConfig::default(), 7, engine, jobs);
            let mut connector = InProcess::new(tx);
            let link = connector.connect().expect("an in-process link");
            let session = Session::new(connector, link, None);
            let mut client = Self {
                ui: ui::State::default(),
                rt,
                inputs,
                session,
                sent: Vec::new(),
            };
            client.act(Vec::new());
            (client, log)
        }

        /// Applies `actions`, sends their commands, runs the player until
        /// it is idle and hands what it sent back to the model, until
        /// nothing is left.
        fn act(&mut self, actions: Vec<ui::Action>) {
            let mut pending: VecDeque<ui::Action> = actions.into();
            loop {
                while let Some(action) = pending.pop_front() {
                    for effect in ui::update(&mut self.ui, action) {
                        if let ui::Effect::Send(command) = effect {
                            self.sent.push(command.clone());
                            self.session.send(command);
                        }
                    }
                }
                while let Some(input) = self.rt.next_input(&self.inputs, Duration::ZERO) {
                    self.rt.handle(input);
                }
                pending.extend(self.session.poll(std::time::Instant::now()));
                if pending.is_empty() {
                    break;
                }
            }
        }

        fn open(&mut self, key: char, text: &str) {
            self.act(vec![
                ui::Action::Key(ui::Key::Char(key)),
                ui::Action::Paste(text.into()),
                ui::Action::Key(ui::Key::Enter),
            ]);
        }

        fn queue(&self) -> Vec<u64> {
            self.rt
                .snapshot()
                .queue
                .iter()
                .map(|e| e.track.id.0)
                .collect()
        }

        /// The queue as the client shows it.
        fn shown(&self) -> Vec<u64> {
            self.ui.queue().iter().map(|e| e.track.id.0).collect()
        }

        fn entry_of(&self, track: u64) -> EntryId {
            let snapshot = self.rt.snapshot();
            snapshot
                .queue
                .iter()
                .find(|e| e.track.id.0 == track)
                .map(|e| e.id)
                .expect("the track is queued")
        }
    }

    /// AC28, as spec 0005 changes it ("Opening items in the player"): the
    /// prompt's item goes to the player as `Open { at }`, which expands it
    /// (fake metadata) and, with nothing playing, starts the first added
    /// entry; the client shows the player's queue; a fetch error is the
    /// reply's message in the playback window.
    #[test]
    fn ac28_open_adds_and_starts() {
        // `o` on an empty player: added at the end, the first one starts.
        let (mut client, log) = Client::new();
        client.open('o', "https://tidal.com/browse/album/10");
        assert_eq!(
            client.sent,
            vec![Command::Open {
                items: vec![Item::Album(10)],
                at: Some(InsertAt::End),
            }]
        );
        assert_eq!(client.queue(), vec![1, 2]);
        assert_eq!(client.shown(), vec![1, 2]);
        let first = client.entry_of(1);
        let snapshot = client.rt.snapshot();
        assert_eq!(snapshot.current, Some(first));
        assert_eq!(snapshot.state, PlaybackState::Playing);
        assert_eq!(client.ui.current().map(|e| e.id), Some(first));
        let calls = take(&log);
        assert!(
            calls
                .iter()
                .any(|c| matches!(c, Call::Play { track: 1, .. })),
            "{calls:?}"
        );

        // `O` while playing: added after the current entry, nothing restarts.
        client.sent.clear();
        client.open('O', "3");
        assert_eq!(
            client.sent,
            vec![Command::Open {
                items: vec![Item::Track(TrackId(3))],
                at: Some(InsertAt::Next),
            }]
        );
        assert_eq!(client.queue(), vec![1, 3, 2]);
        assert_eq!(client.shown(), vec![1, 3, 2]);
        assert_eq!(client.rt.snapshot().current, Some(first));
        let calls = take(&log);
        assert!(
            !calls
                .iter()
                .any(|c| matches!(c, Call::Play { .. } | Call::Stop)),
            "{calls:?}"
        );

        // `O` on an empty player: the first added one starts too.
        let (mut client, _log) = Client::new();
        client.open('O', "https://tidal.com/browse/album/10");
        assert_eq!(client.queue(), vec![1, 2]);
        let first = client.entry_of(1);
        assert_eq!(client.rt.snapshot().current, Some(first));
        assert_eq!(client.rt.snapshot().state, PlaybackState::Playing);

        // A fetch error: the message in the playback window, nothing else.
        let (mut client, log) = Client::new();
        client.open('o', "https://tidal.com/browse/album/404");
        assert_eq!(client.ui.message(), Some("Album 404 was not found"));
        assert_eq!(client.queue(), Vec::<u64>::new());
        let calls = take(&log);
        assert!(
            calls.iter().all(|c| matches!(c, Call::Expand { .. })),
            "{calls:?}"
        );
    }

    /// 0005 AC21/AC22 (slice A's mapping): the engine's `Released`,
    /// `Resumed` and `ResumeFailed` reach the player: the snapshot's
    /// `released` is set and cleared; a failed resume keeps it paused at
    /// the same position with 0003's message, resolves and skips nothing,
    /// and the next play/pause asks the engine to resume again.
    #[test]
    fn ac21_runtime_maps_release_events() {
        let (mut rt, log, rx) = runtime(Script::Plays, true);
        let pump = |rt: &mut PlayerRuntime<FakeEngine, FakeJobs>| {
            while let Some(input) = rt.next_input(&rx, Duration::ZERO) {
                rt.handle(input);
            }
        };
        rt.handle(load(&[1, 2]));
        pump(&mut rt);
        rt.handle(RuntimeInput::Engine(audio::Event::Position(
            Duration::from_secs(42),
        )));
        rt.handle(RuntimeInput::Command(Command::TogglePause));
        assert_eq!(rt.snapshot().state, PlaybackState::Paused);
        let released = |s: &PlayerSnapshot| s.now_playing.as_ref().map(|np| np.released);
        assert_eq!(released(&rt.snapshot()), Some(false));

        let h = rt.handle(RuntimeInput::Engine(audio::Event::Released));
        assert_eq!(released(&last_snapshot(&h)), Some(true));
        take(&log);
        rt.handle(RuntimeInput::Command(Command::TogglePause));
        assert_eq!(take(&log), vec![Call::Resume]);
        let h = rt.handle(RuntimeInput::Engine(audio::Event::Resumed));
        let snapshot = last_snapshot(&h);
        assert_eq!(released(&snapshot), Some(false));
        assert_eq!(snapshot.state, PlaybackState::Playing);

        // Released again; the resume fails: still paused, same entry and
        // position, the output's message, no skip.
        rt.handle(RuntimeInput::Command(Command::TogglePause));
        rt.handle(RuntimeInput::Engine(audio::Event::Released));
        let before = rt.snapshot();
        rt.handle(RuntimeInput::Command(Command::TogglePause));
        take(&log);
        let h = rt.handle(RuntimeInput::Engine(audio::Event::ResumeFailed(
            SinkError::Busy {
                device: "hw:1,0".into(),
                holder: Some("PipeWire".into()),
            },
        )));
        let snapshot = last_snapshot(&h);
        assert_eq!(snapshot.state, PlaybackState::Paused);
        assert_eq!(snapshot.current, before.current);
        assert_eq!(snapshot.position, before.position);
        assert_eq!(
            snapshot.message.as_deref(),
            Some("Output hw:1,0 is busy (used by PipeWire): close it, or use --device default")
        );
        assert_eq!(h.failures.len(), 1);
        assert!(!h.ended && h.started.is_none(), "{h:?}");
        assert_eq!(take(&log), vec![], "resolved or played something");
        rt.handle(RuntimeInput::Command(Command::TogglePause));
        assert_eq!(take(&log), vec![Call::Resume]);
    }

    /// A runtime whose `Open`s are expanded by `PromptMeta` (or left for
    /// the test to answer, without one), with one socket client.
    struct OpenRig {
        rt: PlayerRuntime<FakeEngine, FakeJobs>,
        log: Log,
        inputs: Receiver<RuntimeInput>,
        /// What the client receives.
        client: Receiver<tidal_player_core::protocol::ServerMessage>,
        next_id: u64,
    }

    const CLIENT: ClientId = ClientId(1);

    impl OpenRig {
        /// Tracks in `ends` play to their end at once; the others keep
        /// playing.
        fn new(ends: &[u64], expand: bool) -> Self {
            use crate::ipc::server::{OUTBOX, Peer};
            let log: Log = Arc::default();
            let (tx, inputs) = mpsc::channel();
            let engine = ends
                .iter()
                .fold(FakeEngine::new(&log, Script::Plays), |e, t| {
                    e.script(*t, vec![Script::Ends])
                });
            let mut jobs = FakeJobs::new(&log, &tx, true);
            if expand {
                jobs.metadata = Some(Arc::new(PromptMeta));
            }
            let mut rt = PlayerRuntime::new(PlayerConfig::default(), 7, engine, jobs);
            let (outbox, client) = mpsc::sync_channel(OUTBOX);
            rt.handle(RuntimeInput::Client(ClientInput::Attach {
                client: CLIENT,
                peer: Peer::new(outbox, None),
            }));
            rt.handle(RuntimeInput::Client(ClientInput::Subscribe(CLIENT)));
            let mut rig = Self {
                rt,
                log,
                inputs,
                client,
                next_id: 0,
            };
            rig.received();
            rig
        }

        /// Handles every pending input (jobs answer at once).
        fn pump(&mut self) {
            while let Some(input) = self.rt.next_input(&self.inputs, Duration::ZERO) {
                self.rt.handle(input);
            }
        }

        fn command(&mut self, command: Command) {
            self.input(RuntimeInput::Command(command));
        }

        fn input(&mut self, input: RuntimeInput) {
            self.rt.handle(input);
            self.pump();
        }

        /// Sends a request; returns its id.
        fn request(&mut self, command: Command) -> u64 {
            let id = self.next_id;
            self.next_id += 1;
            self.rt.handle(RuntimeInput::Client(ClientInput::Request {
                client: CLIENT,
                id,
                command,
            }));
            id
        }

        fn received(&mut self) -> Vec<tidal_player_core::protocol::ServerMessage> {
            self.client.try_iter().collect()
        }

        fn queue(&self) -> Vec<u64> {
            self.rt
                .snapshot()
                .queue
                .iter()
                .map(|e| e.track.id.0)
                .collect()
        }

        fn current_track(&self) -> Option<u64> {
            let s = self.rt.snapshot();
            s.queue
                .iter()
                .find(|e| Some(e.id) == s.current)
                .map(|e| e.track.id.0)
        }

        fn expansions(&self) -> Vec<(u64, Vec<Item>)> {
            self.log
                .lock()
                .unwrap()
                .iter()
                .filter_map(|c| match c {
                    Call::Expand { tag, items } => Some((*tag, items.clone())),
                    _ => None,
                })
                .collect()
        }

        fn plays(&self) -> Vec<u64> {
            self.log
                .lock()
                .unwrap()
                .iter()
                .filter_map(|c| match c {
                    Call::Play { track, .. } => Some(*track),
                    _ => None,
                })
                .collect()
        }
    }

    fn album10_and_3() -> Vec<Item> {
        vec![Item::Album(10), Item::Track(TrackId(3))]
    }

    /// AC7: `Open` is expanded in the player (fake metadata) and applied:
    /// `None` loads and plays, `End`/`Next` add and start the first added
    /// entry only when nothing was playing; the reply follows the events.
    #[test]
    fn ac7_open() {
        use tidal_player_core::protocol::ServerMessage;
        struct Row {
            name: &'static str,
            /// Played through `LoadQueue` first (empty: nothing).
            before: &'static [u64],
            /// Tracks that play to their end at once.
            ends: &'static [u64],
            /// Run after `before`, as commands.
            setup: fn(&mut OpenRig),
            at: Option<InsertAt>,
            queue: &'static [u64],
            current: Option<u64>,
            state: PlaybackState,
            /// Tracks the engine was asked to play by the `Open`.
            plays: &'static [u64],
        }
        let nothing: fn(&mut OpenRig) = |_| {};
        let rows = [
            Row {
                name: "None replaces a playing queue and plays the first",
                before: &[7, 8],
                ends: &[],
                setup: nothing,
                at: None,
                queue: &[1, 2, 3],
                current: Some(1),
                state: PlaybackState::Playing,
                plays: &[1],
            },
            Row {
                name: "None on an empty player",
                before: &[],
                ends: &[],
                setup: nothing,
                at: None,
                queue: &[1, 2, 3],
                current: Some(1),
                state: PlaybackState::Playing,
                plays: &[1],
            },
            Row {
                name: "End on an empty queue starts the first added",
                before: &[],
                ends: &[],
                setup: nothing,
                at: Some(InsertAt::End),
                queue: &[1, 2, 3],
                current: Some(1),
                state: PlaybackState::Playing,
                plays: &[1],
            },
            Row {
                name: "Next on an empty queue starts the first added",
                before: &[],
                ends: &[],
                setup: nothing,
                at: Some(InsertAt::Next),
                queue: &[1, 2, 3],
                current: Some(1),
                state: PlaybackState::Playing,
                plays: &[1],
            },
            Row {
                name: "End while playing adds at the end",
                before: &[7, 8],
                ends: &[],
                setup: nothing,
                at: Some(InsertAt::End),
                queue: &[7, 8, 1, 2, 3],
                current: Some(7),
                state: PlaybackState::Playing,
                plays: &[],
            },
            Row {
                name: "Next while playing adds after the current entry",
                before: &[7, 8],
                ends: &[],
                setup: nothing,
                at: Some(InsertAt::Next),
                queue: &[7, 1, 2, 3, 8],
                current: Some(7),
                state: PlaybackState::Playing,
                plays: &[],
            },
            Row {
                name: "End while paused adds, nothing restarts",
                before: &[7],
                ends: &[],
                setup: |rig| rig.command(Command::TogglePause),
                at: Some(InsertAt::End),
                queue: &[7, 1, 2, 3],
                current: Some(7),
                state: PlaybackState::Paused,
                plays: &[],
            },
            Row {
                name: "End when stopped with nothing current starts the first added",
                before: &[7, 8],
                ends: &[7, 8],
                // Both played to their end; removing the last entry while
                // stopped leaves nothing current.
                setup: |rig| {
                    let last = rig.rt.snapshot().current.expect("the last entry");
                    rig.command(Command::RemoveFromQueue(last));
                },
                at: Some(InsertAt::End),
                queue: &[7, 1, 2, 3],
                current: Some(1),
                state: PlaybackState::Playing,
                plays: &[1],
            },
        ];
        for row in rows {
            let mut rig = OpenRig::new(row.ends, true);
            if !row.before.is_empty() {
                rig.input(load(row.before));
            }
            (row.setup)(&mut rig);
            if row.name.contains("nothing current") {
                let s = rig.rt.snapshot();
                assert_eq!(
                    (s.state, s.current),
                    (PlaybackState::Stopped, None),
                    "{}: setup",
                    row.name
                );
            }
            rig.received();
            take(&rig.log);

            let id = rig.request(Command::Open {
                items: album10_and_3(),
                at: row.at,
            });
            rig.pump();
            assert_eq!(rig.expansions(), vec![(0, album10_and_3())], "{}", row.name);
            assert_eq!(rig.queue(), row.queue, "{}", row.name);
            assert_eq!(rig.current_track(), row.current, "{}", row.name);
            assert_eq!(rig.rt.snapshot().state, row.state, "{}", row.name);
            assert_eq!(rig.plays(), row.plays, "{}", row.name);
            let got = rig.received();
            let reply = got
                .iter()
                .position(|m| matches!(m, ServerMessage::Reply { .. }))
                .unwrap_or_else(|| panic!("{}: no reply in {got:?}", row.name));
            assert_eq!(
                got[reply],
                ServerMessage::Reply { id, result: Ok(()) },
                "{}",
                row.name
            );
            // The reply follows the events that queued the tracks.
            let queued = got[..reply].iter().any(|m| {
                matches!(m, ServerMessage::Event(Event::Player(s))
                    if s.queue.iter().any(|e| e.track.id.0 == 3))
            });
            assert!(queued, "{}: {got:?}", row.name);
        }
    }

    /// AC7: an expansion error is the reply's `Err` and changes nothing;
    /// `Open`s apply in the order received, whichever expands first; the
    /// queue arrives in item order, never a play order (tidalt #21).
    #[test]
    fn ac7_open_errors_and_order() {
        use tidal_player_core::protocol::ServerMessage;

        // An error: the message, nothing else.
        let mut rig = OpenRig::new(&[], true);
        rig.input(load(&[7, 8]));
        let before = rig.rt.snapshot();
        rig.received();
        take(&rig.log);
        let id = rig.request(Command::Open {
            items: vec![Item::Track(TrackId(3)), Item::Album(404)],
            at: Some(InsertAt::End),
        });
        rig.pump();
        assert_eq!(
            rig.received(),
            vec![ServerMessage::Reply {
                id,
                result: Err("Album 404 was not found".into()),
            }]
        );
        assert_eq!(rig.rt.snapshot(), before);
        assert!(
            take(&rig.log)
                .iter()
                .all(|c| matches!(c, Call::Expand { .. })),
            "the engine and the queue were touched"
        );

        // Two `Open`s; the second expands first: applied in the order
        // received, each reply once its tracks are queued.
        let mut rig = OpenRig::new(&[], false);
        let first = rig.request(Command::Open {
            items: vec![Item::Track(TrackId(3))],
            at: Some(InsertAt::End),
        });
        let second = rig.request(Command::Open {
            items: vec![Item::Album(10)],
            at: Some(InsertAt::End),
        });
        let tags: Vec<u64> = rig.expansions().iter().map(|(tag, _)| *tag).collect();
        assert_eq!(tags.len(), 2, "both expand at once");
        rig.rt.handle(RuntimeInput::Expanded {
            tag: tags[1],
            result: Ok(vec![track(1, Some(200)), track(2, Some(200))]),
        });
        rig.pump();
        assert_eq!(
            rig.queue(),
            Vec::<u64>::new(),
            "the second waits for the first"
        );
        assert_eq!(rig.received(), vec![]);
        rig.rt.handle(RuntimeInput::Expanded {
            tag: tags[0],
            result: Ok(vec![track(3, Some(200))]),
        });
        rig.pump();
        assert_eq!(rig.queue(), vec![3, 1, 2]);
        assert_eq!(rig.current_track(), Some(3));
        let replies: Vec<ServerMessage> = rig
            .received()
            .into_iter()
            .filter(|m| matches!(m, ServerMessage::Reply { .. }))
            .collect();
        assert_eq!(
            replies,
            vec![
                ServerMessage::Reply {
                    id: first,
                    result: Ok(()),
                },
                ServerMessage::Reply {
                    id: second,
                    result: Ok(()),
                },
            ]
        );

        // Item order with shuffle on: the player keeps the original order
        // (switching shuffle off restores it), and an in-process `Open`
        // (no reply) is applied the same way.
        let mut rig = OpenRig::new(&[], true);
        rig.command(Command::ToggleShuffle);
        rig.command(Command::Open {
            items: vec![Item::Track(TrackId(3)), Item::Album(10)],
            at: None,
        });
        assert_eq!(rig.current_track(), Some(3));
        rig.command(Command::ToggleShuffle);
        assert_eq!(rig.queue(), vec![3, 1, 2]);
    }
}
