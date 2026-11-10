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
    /// Answers library requests (each on its own thread, so a held
    /// one blocks nothing); without one every request fails.
    pub library: Option<Arc<dyn Library>>,
    pub library_settings: LibrarySettings,
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
            library: None,
            library_settings: LibrarySettings::default(),
        }
    }

    /// Serves library requests with `library`.
    #[must_use]
    pub fn with_library(mut self, library: Arc<dyn Library>) -> Self {
        self.library = Some(library);
        self
    }
}

/// A library from memory: it records each request (by [`label`]) when
/// it starts, fails the labels given a message, and holds the labels
/// given a gate until the test opens it.
#[derive(Default)]
pub struct FakeLibrary {
    started: Mutex<Vec<String>>,
    seen: Mutex<Vec<(LibraryRequest, u32, Vec<String>)>>,
    errors: Mutex<HashMap<String, String>>,
    gates: Mutex<HashMap<String, Receiver<()>>>,
}

/// A request's name in the fakes: the UUID of a `DeletePlaylist`.
pub fn label(request: &LibraryRequest) -> String {
    match request {
        LibraryRequest::DeletePlaylist { uuid } => uuid.clone(),
        other => format!("{other:?}"),
    }
}

pub fn delete(label: &str) -> LibraryRequest {
    LibraryRequest::DeletePlaylist {
        uuid: label.to_owned(),
    }
}

impl FakeLibrary {
    /// The request `label` fails with `message`.
    pub fn fail(&self, label: &str, message: &str) {
        let mut errors = self.errors.lock().unwrap();
        errors.insert(label.to_owned(), message.to_owned());
    }

    /// The request `label` waits until the returned sender fires (or is
    /// dropped).
    pub fn hold(&self, label: &str) -> Sender<()> {
        let (open, gate) = mpsc::channel();
        self.gates.lock().unwrap().insert(label.to_owned(), gate);
        open
    }

    /// The labels of the requests started, in order.
    pub fn started(&self) -> Vec<String> {
        self.started.lock().unwrap().clone()
    }

    /// Every request started, with the settings it got.
    pub fn seen(&self) -> Vec<(LibraryRequest, u32, Vec<String>)> {
        self.seen.lock().unwrap().clone()
    }
}

impl Library for FakeLibrary {
    fn request(
        &self,
        request: LibraryRequest,
        page_size: u32,
        hidden_words: Vec<String>,
    ) -> BoxFuture<'_, Result<LibraryResponse, String>> {
        Box::pin(async move {
            let name = label(&request);
            self.started.lock().unwrap().push(name.clone());
            self.seen
                .lock()
                .unwrap()
                .push((request, page_size, hidden_words));
            let gate = self.gates.lock().unwrap().remove(&name);
            if let Some(gate) = gate {
                // Blocks this job's own thread only.
                let _ = gate.recv();
            }
            match self.errors.lock().unwrap().get(&name) {
                Some(message) => Err(message.clone()),
                None => Ok(LibraryResponse::Done),
            }
        })
    }
}

impl Jobs for FakeJobs {
    fn library(&mut self, client: ClientId, id: u64, request: LibraryRequest) {
        let results = self.results.clone();
        let settings = self.library_settings.clone();
        let page_size = settings.page_size_for(&request);
        let library = self.library.clone();
        std::thread::spawn(move || {
            let result = match library {
                Some(library) => tokio::runtime::Builder::new_current_thread()
                    .build()
                    .expect("a test runtime")
                    .block_on(library.request(request, page_size, settings.hidden_words)),
                None => Err("library not available".to_owned()),
            };
            let _ = results.send(RuntimeInput::LibraryDone { client, id, result });
        });
    }

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
            cover: None,
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
