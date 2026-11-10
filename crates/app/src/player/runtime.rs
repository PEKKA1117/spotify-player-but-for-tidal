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
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tidal_player_api::auth::BoxFuture;
use tidal_player_api::library::LibraryClient;
use tidal_player_api::metadata::{MetadataClient, MetadataError};
use tidal_player_audio::{self as audio, OutputInfo, SourceFormat, TrackSource};
use tidal_player_core::hook::{HookTracker, Transition};
use tidal_player_core::item::parse_item;
use tidal_player_core::library::{
    DEFAULT_HIDDEN_VERSIONS, LibraryRequest, LibraryResponse, PageData, PageRequest,
};
use tidal_player_core::lyrics::Lyrics;
use tidal_player_core::player::{
    self, EngineEvent, Failure, PlayerConfig, PlayerEffect, PlayerInput, PlayerState, Purpose,
    TrackDetails,
};
use tidal_player_core::protocol::{
    Command, DeviceEntry, Event, InsertAt, PlaybackState, PlayerSnapshot,
};
use tidal_player_core::{AudioQuality, Item, ItemError, Track, TrackId};

use crate::cli::play::{engine_failure, output_description, source_description};
use crate::ipc::server::{ClientId, ClientInput, Hub};
use crate::logging::LogHandle;
use crate::logging::events::{self, PlayerLog, Playing, Shutdown};
use crate::player::hook::HookSink;
use crate::player::persist::{Loaded, Persister, SaveResult, SaveSchedule};
use crate::settings::{DEFAULT_QUALITY, PlayerSettings, start_autoplay};

/// The reply to a `SetDevice` with an empty name (spec 0014).
pub const EMPTY_DEVICE_NAME: &str = "Device name is empty";

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
    /// Answer a library request of `client` (answer: `LibraryDone`).
    fn library(&mut self, client: ClientId, id: u64, request: LibraryRequest);
}

/// The player's machine's output devices (spec 0014 "Listing devices"),
/// read at each request. An `Err` is the message the client shows.
pub trait DeviceLister: Send {
    fn list(&self) -> Result<Vec<DeviceEntry>, String>;
}

/// The devices under an `asound` directory (`/proc/asound`, or
/// `TIDAL_PLAYER_ASOUND_DIR`), read fresh on every request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AsoundDevices {
    pub dir: PathBuf,
}

impl AsoundDevices {
    /// From the process environment.
    pub fn from_env() -> Self {
        Self {
            dir: crate::settings::asound_dir(|key| std::env::var(key).ok()),
        }
    }
}

impl DeviceLister for AsoundDevices {
    fn list(&self) -> Result<Vec<DeviceEntry>, String> {
        crate::cli::play::read_devices(&self.dir).map(|devices| {
            devices
                .into_iter()
                .map(|d| DeviceEntry {
                    name: d.name,
                    description: d.description,
                })
                .collect()
        })
    }
}

/// The library as the player reaches it (spec 0006 "Talking to the
/// player"); the real one adapts `tidal_player_api::library::LibraryClient`.
/// An `Err` is the message the client shows.
pub trait Library: Send + Sync {
    fn request(
        &self,
        request: LibraryRequest,
        page_size: u32,
        hidden_words: Vec<String>,
    ) -> BoxFuture<'_, Result<LibraryResponse, String>>;
}

/// The library until a real one is connected: every request fails.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoLibrary;

impl Library for NoLibrary {
    fn request(
        &self,
        _: LibraryRequest,
        _: u32,
        _: Vec<String>,
    ) -> BoxFuture<'_, Result<LibraryResponse, String>> {
        Box::pin(async { Err("library not available".to_owned()) })
    }
}

/// What the player passes to every [`Library::request`] (spec 0006 "Lists
/// load as you scroll", "The artist's *All tracks*").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibrarySettings {
    pub page_size: u32,
    /// Items per page of a search's lists (spec 0007 "Lists load as you
    /// scroll").
    pub search_page_size: u32,
    pub hidden_words: Vec<String>,
}

impl Default for LibrarySettings {
    fn default() -> Self {
        Self {
            page_size: 100,
            search_page_size: 20,
            hidden_words: DEFAULT_HIDDEN_VERSIONS
                .iter()
                .map(|w| (*w).to_owned())
                .collect(),
        }
    }
}

impl LibrarySettings {
    /// The page size [`Library::request`] gets for `request`: the search
    /// page size for a search page, else the library's (a `More` carries
    /// its own `limit`).
    pub fn page_size_for(&self, request: &LibraryRequest) -> u32 {
        match request {
            LibraryRequest::Page(PageRequest::Search(_)) => self.search_page_size,
            _ => self.page_size,
        }
    }
}

impl Library for LibraryClient {
    fn request(
        &self,
        request: LibraryRequest,
        page_size: u32,
        hidden_words: Vec<String>,
    ) -> BoxFuture<'_, Result<LibraryResponse, String>> {
        Box::pin(async move {
            LibraryClient::request(self, request, page_size, &hidden_words)
                .await
                .map_err(|e| e.to_string())
        })
    }
}

/// How many tracks' lyrics the player keeps (spec 0021 decision 2).
pub const LYRICS_CACHE_SIZE: usize = 50;

/// The [`Library`] with the lyrics of the last [`LYRICS_CACHE_SIZE`]
/// tracks kept in memory (spec 0021 "What the player fetches and keeps"):
/// "no lyrics" included, failures not. Shared by every client of the
/// player; the oldest-asked place is the front.
pub struct LyricsCache {
    inner: Arc<dyn Library>,
    kept: std::sync::Mutex<VecDeque<(u64, Option<Lyrics>)>>,
}

impl LyricsCache {
    pub fn new(inner: Arc<dyn Library>) -> Self {
        Self {
            inner,
            kept: std::sync::Mutex::default(),
        }
    }

    /// The kept answer for `track`, which becomes the newest.
    fn hit(&self, track: u64) -> Option<Option<Lyrics>> {
        let mut kept = self.kept.lock().unwrap();
        let at = kept.iter().position(|(t, _)| *t == track)?;
        let entry = kept.remove(at)?;
        let lyrics = entry.1.clone();
        kept.push_back(entry);
        Some(lyrics)
    }

    fn keep(&self, track: u64, lyrics: Option<Lyrics>) {
        let mut kept = self.kept.lock().unwrap();
        kept.retain(|(t, _)| *t != track);
        kept.push_back((track, lyrics));
        while kept.len() > LYRICS_CACHE_SIZE {
            kept.pop_front();
        }
    }
}

impl Library for LyricsCache {
    fn request(
        &self,
        request: LibraryRequest,
        page_size: u32,
        hidden_words: Vec<String>,
    ) -> BoxFuture<'_, Result<LibraryResponse, String>> {
        let LibraryRequest::Page(PageRequest::Lyrics(track)) = request else {
            return self.inner.request(request, page_size, hidden_words);
        };
        Box::pin(async move {
            if let Some(lyrics) = self.hit(track) {
                return Ok(LibraryResponse::Page(PageData::Lyrics { track, lyrics }));
            }
            let response = self.inner.request(request, page_size, hidden_words).await;
            if let Ok(LibraryResponse::Page(PageData::Lyrics { lyrics, .. })) = &response {
                self.keep(track, lyrics.clone());
            }
            response
        })
    }
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
    /// A [`Jobs::library`] result, for the client that asked.
    LibraryDone {
        client: ClientId,
        id: u64,
        result: Result<LibraryResponse, String>,
    },
    /// The hook's failure to say (spec 0025 "Failures").
    HookFailed(String),
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
    /// Each client's library requests: one runs at a time, the rest wait in
    /// arrival order. Different clients' run independently.
    library: HashMap<ClientId, LibraryQueue>,
    /// The remembered playback state (spec 0009): `None` for a player
    /// that remembers nothing (`play`, and the tests of 0004–0008).
    persistence: Option<Persistence>,
    /// Answers `Devices` (spec 0014 AC7).
    devices: Box<dyn DeviceLister>,
    /// The player event hook (spec 0025), when `app.toml` sets one.
    hook: Option<PlayerHook>,
    /// The log buffer `Logs` is answered from (spec 0027); `None`: the
    /// process keeps no log.
    logs: Option<LogHandle>,
    /// What the player tells its log between inputs (spec 0027).
    plog: PlayerLog,
    /// The library request each client has out, for the name of a failed
    /// one.
    running: HashMap<ClientId, LibraryRequest>,
}

/// The hook and what it remembers between inputs.
struct PlayerHook {
    sink: Box<dyn HookSink>,
    tracker: HookTracker,
}

/// The playback file and when to write it (spec 0009 "Saving").
#[derive(Debug)]
struct Persistence {
    persister: Persister,
    schedule: SaveSchedule,
}

/// The state a player starts with (spec 0009 "Starting from the
/// remembered state"): the remembered one when `loaded` holds it (its
/// autoplay by [`start_autoplay`]), else a fresh one; `loaded`'s message
/// is the player's message.
pub fn starting_state(
    config: PlayerConfig,
    seed: u64,
    settings: &PlayerSettings,
    loaded: Loaded,
) -> PlayerState {
    let Loaded { saved, message } = loaded;
    let mut state = match saved {
        Some(mut saved) => {
            saved.autoplay = start_autoplay(settings, Some(saved.autoplay));
            PlayerState::restore(config, seed, saved)
        }
        None => PlayerState::new(config, seed),
    };
    if let Some(message) = message {
        // No client yet: the `Welcome` carries it.
        player::update(&mut state, PlayerInput::Notice(message));
    }
    state
}

/// One client's library requests.
#[derive(Debug, Default)]
struct LibraryQueue {
    /// A request of this client is out.
    busy: bool,
    waiting: VecDeque<(u64, LibraryRequest)>,
}

impl<E: EngineControl, J: Jobs> PlayerRuntime<E, J> {
    pub fn new(config: PlayerConfig, seed: u64, engine: E, jobs: J) -> Self {
        Self::with_state(PlayerState::new(config, seed), engine, jobs)
    }

    /// The player of the standalone TUI and the daemon (spec 0009): reads
    /// the remembered state through `persister` now, before any input,
    /// starts from it, and saves through it from then on.
    pub fn restored(
        config: PlayerConfig,
        seed: u64,
        settings: &PlayerSettings,
        persister: Persister,
        mut engine: E,
        jobs: J,
    ) -> Self {
        let state = starting_state(config, seed, settings, persister.load());
        let schedule = SaveSchedule::new(state.saved(), Instant::now());
        // The engine starts at gain 1.0: hand it the restored volume and
        // mute (AC15), or they are shown but not heard.
        engine.send(audio::Command::SetGain(state.gain()));
        let mut runtime = Self::with_state(state, engine, jobs);
        runtime.persistence = Some(Persistence {
            persister,
            schedule,
        });
        runtime
    }

    /// Writes what the save schedule has due at `now` (spec 0009
    /// "Saving"); the events are the player's message after a failed
    /// write, already sent to the subscribers.
    pub fn tick(&mut self, now: Instant) -> Vec<Event> {
        let due = self
            .persistence
            .as_mut()
            .and_then(|p| p.schedule.on_tick(now));
        due.map(|saved| self.write(&saved)).unwrap_or_default()
    }

    /// After an input: hands the new state to the save schedule, and
    /// writes it when due (spec 0009 "Saving"). `urgent`: a pause, a stop,
    /// a seek or a track change saves at once.
    fn track_change(&mut self, before: &PlayerSnapshot, seek: bool, handled: &mut Handled) {
        let Some(persistence) = self.persistence.as_mut() else {
            return;
        };
        let after = self.state.snapshot();
        let stopped_or_paused = before.state != after.state
            && matches!(after.state, PlaybackState::Paused | PlaybackState::Stopped);
        let urgent = seek
            || stopped_or_paused
            || before.current != after.current
            || handled.started.is_some();
        let playing = matches!(
            after.state,
            PlaybackState::Playing | PlaybackState::Buffering | PlaybackState::Loading
        );
        let due =
            persistence
                .schedule
                .on_change(Instant::now(), self.state.saved(), playing, urgent);
        if let Some(saved) = due {
            let events = self.write(&saved);
            handled.events.extend(events);
        }
    }

    /// At `Shutdown`: the last write (nothing is written after it).
    fn save_at_exit(&mut self) {
        let Some(persistence) = self.persistence.as_mut() else {
            return;
        };
        if let Some(saved) = persistence
            .schedule
            .on_exit(Instant::now(), self.state.saved())
        {
            self.write(&saved);
        }
    }

    /// Writes `saved`; a failure's message (the first since the last
    /// success) becomes the player's message, broadcast.
    fn write(&mut self, saved: &tidal_player_core::SavedPlayback) -> Vec<Event> {
        let Some(persistence) = self.persistence.as_mut() else {
            return Vec::new();
        };
        let SaveResult::Failed { message } = persistence.persister.save(saved) else {
            return Vec::new();
        };
        persistence.schedule.write_failed();
        let Some(message) = message else {
            return Vec::new();
        };
        let mut handled = Handled::default();
        for effect in player::update(&mut self.state, PlayerInput::Notice(message)) {
            self.execute(effect, &mut handled);
        }
        self.hub.broadcast(&handled.events);
        self.plog.snapshot(&self.state.snapshot());
        handled.events
    }

    fn with_state(state: PlayerState, engine: E, jobs: J) -> Self {
        let state_now = state.snapshot().state;
        Self {
            state,
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
            library: HashMap::new(),
            persistence: None,
            devices: Box::new(AsoundDevices::from_env()),
            hook: None,
            logs: None,
            plog: PlayerLog::new(DEFAULT_QUALITY, state_now),
            running: HashMap::new(),
        }
    }

    /// Hands the player's events to `hook` from now on (spec 0025).
    #[must_use]
    pub fn with_hook(mut self, hook: Option<Box<dyn HookSink>>) -> Self {
        self.hook = hook.map(|sink| PlayerHook {
            sink,
            tracker: HookTracker::default(),
        });
        self
    }

    /// Answers `Logs` from `logs` (spec 0027): the process's log buffer.
    #[must_use]
    pub fn with_logs(mut self, logs: LogHandle) -> Self {
        self.logs = Some(logs);
        self
    }

    /// The quality the player asks Tidal for, as `playing` says it (spec
    /// 0027).
    #[must_use]
    pub fn with_requested(mut self, requested: AudioQuality) -> Self {
        self.plog.set_requested(requested);
        self
    }

    /// Answers `Devices` with `devices` instead of `/proc/asound`.
    #[must_use]
    pub fn with_devices(mut self, devices: Box<dyn DeviceLister>) -> Self {
        self.devices = devices;
        self
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
        let wanted = self.persistence.is_some() || self.hook.is_some();
        let before = wanted.then(|| self.snapshot());
        let command = match &input {
            RuntimeInput::Command(command)
            | RuntimeInput::Client(ClientInput::Request { command, .. }) => Some(command),
            _ => None,
        };
        let seek = matches!(
            command,
            Some(Command::SeekBy(_) | Command::SeekTo(_) | Command::SetPosition { .. })
        );
        // `Previous` can restart the track, a seek to the hook's eyes.
        let moved_by_hand = seek || matches!(command, Some(Command::Previous));
        let position_only = matches!(input, RuntimeInput::Engine(audio::Event::Position(_)));
        let mut handled = self.handle_input(input);
        if !position_only {
            self.plog.snapshot(&self.state.snapshot());
        }
        if handled.shutdown {
            self.save_at_exit();
            self.hook_shutdown();
        } else if let Some(before) = before {
            self.track_change(&before, seek, &mut handled);
            self.hook_input(&before, moved_by_hand, &handled);
        }
        handled
    }

    /// Hands the hook what an input did (spec 0025).
    fn hook_input(&mut self, before: &PlayerSnapshot, seek: bool, handled: &Handled) {
        let after = self.state.snapshot();
        let Some(hook) = self.hook.as_mut() else {
            return;
        };
        hook.sink.send(hook.tracker.update(Transition {
            before,
            after: &after,
            started: handled.started.is_some(),
            ended: handled.ended,
            seek,
        }));
    }

    /// The last event, then the hook's time to finish (spec 0025 "Shutting
    /// down"). The clients were told already. `play` calls it itself when
    /// its queue runs out, which is no `Shutdown`.
    pub fn hook_shutdown(&mut self) {
        let snapshot = self.state.snapshot();
        if let Some(hook) = self.hook.as_mut() {
            hook.sink
                .send(hook.tracker.shutdown(&snapshot).into_iter().collect());
            hook.sink.finish();
        }
    }

    fn handle_input(&mut self, input: RuntimeInput) -> Handled {
        match input {
            RuntimeInput::Client(input) => self.client_input(input),
            RuntimeInput::Expanded { tag, result } => self.expanded(tag, result),
            RuntimeInput::LibraryDone { client, id, result } => {
                self.library_done(client, id, result);
                Handled::default()
            }
            RuntimeInput::Login { required } => {
                let events: Vec<Event> =
                    self.hub.set_login_required(required).into_iter().collect();
                if !events.is_empty() {
                    if required {
                        events::login_required();
                    } else {
                        events::login_restored();
                    }
                }
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
            RuntimeInput::HookFailed(message) => PlayerInput::Notice(message),
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
            | RuntimeInput::LibraryDone { .. }
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

    /// Tells the log what an engine event did to the device and, for the
    /// start of an entry the player sent, which track it is (spec 0027).
    fn log_engine(&mut self, event: &audio::Event) {
        let sent = match event {
            audio::Event::Started { tag, .. } | audio::Event::Transitioned { tag, .. } => {
                self.sent.get(tag).copied()
            }
            _ => None,
        };
        let title = sent.and_then(|s| {
            self.state
                .snapshot()
                .queue
                .iter()
                .find(|e| e.track.id == s.track)
                .map(|e| e.track.title.clone())
        });
        let playing = sent.map(|s| Playing {
            track: s.track.0,
            title: title.as_deref().unwrap_or_default(),
            quality: s.quality,
        });
        self.plog.engine(event, playing);
    }

    /// Maps an engine event into the player's terms.
    fn engine_input(
        &mut self,
        event: audio::Event,
        handled: &mut Handled,
    ) -> (PlayerInput, Notice) {
        self.log_engine(&event);
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
            audio::Event::OutputChanged(output) => {
                // The source line is the player's (it keeps it).
                let details = TrackDetails {
                    source: String::new(),
                    output: output_description(&output),
                    bit_perfect: output.bit_perfect,
                    reason: output.not_bit_perfect_reason.clone(),
                };
                (EngineEvent::OutputChanged(details), Notice::None)
            }
            audio::Event::DeviceFallback {
                tried,
                error,
                device,
            } => {
                // 0003's message for the failed open (not a failure: the
                // track carries on on `device`).
                let message = engine_failure(0, &audio::EngineError::Output(error)).message;
                (
                    EngineEvent::DeviceFallback {
                        tried,
                        message,
                        device,
                    },
                    Notice::None,
                )
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
            PlayerEffect::EngineSetDevice(device) => {
                self.engine.send(audio::Command::SetDevice(device));
            }
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
            ClientInput::Attach { client, peer } => {
                self.hub.attach(client, peer);
                events::client_connected(client.0);
            }
            ClientInput::Detach(client) => {
                if self.hub.contains(client) {
                    events::client_disconnected(client.0);
                }
                self.hub.detach(client);
                self.running.remove(&client);
                // Its waiting requests never run; the one out is answered
                // to nobody.
                self.library.remove(&client);
            }
            ClientInput::Subscribe(client) => {
                let snapshot = self.snapshot();
                self.hub.subscribe(client, snapshot);
            }
            ClientInput::Library {
                client,
                id,
                request,
            } => {
                if self.hub.contains(client) {
                    let queue = self.library.entry(client).or_default();
                    queue.waiting.push_back((id, request));
                    self.library_next(client);
                }
            }
            ClientInput::Devices { client, id } => {
                // Read now: a card plugged in since shows up (spec 0014).
                let result = self.devices.list();
                if let Err(reason) = &result {
                    events::request_failed("Devices", reason);
                }
                self.hub.reply_devices(client, id, result);
            }
            // Spec 0027: from the buffer, to the asker only. Nothing here
            // logs, so serving adds no line.
            ClientInput::Logs { client, id, after } => {
                let batch = self.logs.as_ref().map_or_else(
                    || LogHandle::detached(true).batch(after),
                    |l| l.batch(after),
                );
                self.hub.reply_logs(client, id, Ok(batch));
            }
            ClientInput::Request {
                client,
                id,
                command: Command::SetDevice(name),
            } if name.is_empty() => {
                events::request_failed("SetDevice", EMPTY_DEVICE_NAME);
                self.hub
                    .reply(client, id, Err(EMPTY_DEVICE_NAME.to_owned()));
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
                if command == Command::Shutdown {
                    events::shutting_down(Shutdown::Command);
                }
                let handled = self.step(RuntimeInput::Command(command));
                self.hub.reply(client, id, Ok(()));
                return handled;
            }
        }
        Handled::default()
    }

    /// Starts `client`'s next waiting library request, unless one is out.
    fn library_next(&mut self, client: ClientId) {
        let Some(queue) = self.library.get_mut(&client) else {
            return;
        };
        if queue.busy {
            return;
        }
        if let Some((id, request)) = queue.waiting.pop_front() {
            queue.busy = true;
            self.running.insert(client, request.clone());
            self.jobs.library(client, id, request);
        }
    }

    /// A library request finished: the reply to its client, then its next.
    fn library_done(&mut self, client: ClientId, id: u64, result: Result<LibraryResponse, String>) {
        let request = self.running.remove(&client);
        if let (Err(reason), Some(request)) = (&result, request) {
            events::request_failed(&events::request_name(&format!("{request:?}")), reason);
        }
        self.hub.reply_library(client, id, result);
        if let Some(queue) = self.library.get_mut(&client) {
            queue.busy = false;
        }
        self.library_next(client);
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
                if let Err(reason) = &outcome {
                    events::request_failed("Open", reason);
                }
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
                if let Some(input) = runtime.next_input(&inputs, POLL) {
                    let handled = runtime.handle(input);
                    for event in handled.events {
                        let _ = events_tx.send(event);
                    }
                    if handled.shutdown {
                        break;
                    }
                }
                // Saves due (spec 0009): at most `POLL` late.
                for event in runtime.tick(Instant::now()) {
                    let _ = events_tx.send(event);
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
    library: Arc<dyn Library>,
    library_settings: LibrarySettings,
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
            library: Arc::new(NoLibrary),
            library_settings: LibrarySettings::default(),
        }
    }

    /// Serves library requests with `library`, passing `settings` to it.
    #[must_use]
    pub fn with_library(mut self, library: Arc<dyn Library>, settings: LibrarySettings) -> Self {
        self.library = Arc::new(LyricsCache::new(library));
        self.library_settings = settings;
        self
    }
}

impl Jobs for TokioJobs {
    fn library(&mut self, client: ClientId, id: u64, request: LibraryRequest) {
        let library = Arc::clone(&self.library);
        let page_size = self.library_settings.page_size_for(&request);
        let hidden_words = self.library_settings.hidden_words.clone();
        let results = self.results.clone();
        self.runtime.spawn(async move {
            let result = library.request(request, page_size, hidden_words).await;
            let _ = results.send(RuntimeInput::LibraryDone { client, id, result });
        });
    }

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
pub(crate) mod fakes;

#[cfg(test)]
mod tests;
