use std::sync::Mutex;
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

/// Never opens a stream (the library test below plays nothing).
struct NoOpener;

impl StreamOpener for NoOpener {
    fn open(&self, _: TrackId) -> BoxFuture<'static, Result<Prepared, Failure>> {
        Box::pin(async {
            Err(Failure {
                kind: player::FailureKind::TrackOnly,
                message: "no streams here".to_owned(),
            })
        })
    }
}

/// Spec 0007 AC4: the real jobs pass the search page size with a search
/// page, and the page size with every other request (a `More` carries
/// its own `limit`).
#[test]
fn ac4_tokio_jobs_search_page_size() {
    use tidal_player_core::library::{ListRef, PageRequest};

    let tokio = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let library = Arc::new(FakeLibrary::default());
    let settings = LibrarySettings {
        page_size: 100,
        search_page_size: 7,
        hidden_words: vec!["live".to_owned()],
    };
    let (tx, rx) = mpsc::channel();
    let mut jobs = TokioJobs::new(
        tokio.handle().clone(),
        Arc::new(NoOpener),
        Arc::new(PromptMeta),
        tx,
    )
    .with_library(Arc::clone(&library) as _, settings);
    let q = "a&b".to_owned();
    let table = [
        (LibraryRequest::Page(PageRequest::Search(q.clone())), 7),
        (
            LibraryRequest::More {
                list: ListRef::SearchTracks(q.clone()),
                offset: 20,
                limit: 20,
            },
            100,
        ),
        (LibraryRequest::Page(PageRequest::FavoriteTracks), 100),
    ];
    for (n, (request, _)) in table.iter().enumerate() {
        jobs.library(ClientId(1), n as u64, request.clone());
        let done = tokio.block_on(async {
            loop {
                match rx.try_recv() {
                    Ok(input) => break input,
                    Err(_) => tokio::time::sleep(Duration::from_millis(5)).await,
                }
            }
        });
        assert!(
            matches!(done, RuntimeInput::LibraryDone { id, .. } if id == n as u64),
            "{request:?}"
        );
    }
    let want: Vec<_> = table
        .into_iter()
        .map(|(request, size)| (request, size, vec!["live".to_owned()]))
        .collect();
    assert_eq!(library.seen(), want);
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
        Self::build(
            |engine, jobs| PlayerRuntime::new(PlayerConfig::default(), 7, engine, jobs),
            None,
        )
    }

    /// As `main` starts the standalone TUI (spec 0009): the player
    /// restores from `state_dir` first; `open` is the startup items'
    /// `Open`, sent when the client connects.
    fn restored(state_dir: &std::path::Path, open: Option<Command>) -> (Self, Log) {
        let settings = PlayerSettings::default();
        Self::build(
            |engine, jobs| {
                PlayerRuntime::restored(
                    settings.player.clone(),
                    7,
                    &settings,
                    Persister::new(state_dir.to_owned(), settings.remember_playback),
                    engine,
                    jobs,
                )
            },
            open,
        )
    }

    fn build(
        make: impl FnOnce(FakeEngine, FakeJobs) -> PlayerRuntime<FakeEngine, FakeJobs>,
        open: Option<Command>,
    ) -> (Self, Log) {
        use crate::client::{Connector, InProcess, Session};
        let log: Log = Arc::default();
        let (tx, inputs) = mpsc::channel();
        let engine = FakeEngine::new(&log, Script::Plays);
        let mut jobs = FakeJobs::new(&log, &tx, true);
        jobs.metadata = Some(Arc::new(PromptMeta));
        let rt = make(engine, jobs);
        let mut connector = InProcess::new(tx);
        let link = connector.connect().expect("an in-process link");
        let session = Session::new(connector, link, open);
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
                    match effect {
                        ui::Effect::Send(command) => {
                            self.sent.push(command.clone());
                            self.session.send(command);
                        }
                        ui::Effect::Devices { id } => self.session.send_devices(id),
                        _ => {}
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

// --- spec 0009: the remembered playback state ------------------------------

/// Three 4:56 tracks, the second current, shuffle on, repeat `queue`,
/// volume 70, at 1:23 (reached through commands, as a user would).
fn remembered() -> tidal_player_core::SavedPlayback {
    let mut st = PlayerState::new(PlayerConfig::default(), 3);
    for command in [
        Command::LoadQueue {
            tracks: vec![
                track(1, Some(296)),
                track(2, Some(296)),
                track(3, Some(296)),
            ],
            start: 1,
        },
        Command::ToggleShuffle,
        Command::CycleRepeat,
        Command::SetVolume(70),
        Command::SeekTo(Duration::from_secs(83)),
    ] {
        player::update(&mut st, PlayerInput::Command(command));
    }
    st.saved()
}

/// A state dir holding `saved` as `playback.json`.
fn state_dir_with(saved: &tidal_player_core::SavedPlayback) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    crate::player::persist::save(&crate::player::persist::RealFs, dir.path(), saved).unwrap();
    dir
}

/// The restored snapshot `saved` should give.
fn restored_snapshot(saved: &tidal_player_core::SavedPlayback) -> PlayerSnapshot {
    PlayerState::restore(PlayerConfig::default(), 7, saved.clone()).snapshot()
}

fn restored_runtime(
    state_dir: &std::path::Path,
) -> (
    PlayerRuntime<FakeEngine, FakeJobs>,
    Log,
    Receiver<RuntimeInput>,
) {
    let settings = PlayerSettings::default();
    let log: Log = Arc::default();
    let (tx, rx) = mpsc::channel();
    let rt = PlayerRuntime::restored(
        settings.player.clone(),
        7,
        &settings,
        Persister::new(state_dir.to_owned(), true),
        FakeEngine::new(&log, Script::Plays),
        FakeJobs::new(&log, &tx, true),
    );
    (rt, log, rx)
}

/// Spec 0009 AC9: the standalone player restores before its first
/// frame: the `Welcome` the TUI draws first already holds the
/// remembered queue, stopped at its position, and nothing was resolved
/// or sent to the engine but the restored gain (AC15); a corrupt file is the player's message; quit
/// saves what changed.
#[test]
fn ac9_standalone_restores() {
    let saved = remembered();
    let dir = state_dir_with(&saved);
    let (mut client, log) = Client::restored(dir.path(), None);
    let expected = restored_snapshot(&saved);
    assert_eq!(expected.state, PlaybackState::Stopped);
    assert_eq!(expected.position, Duration::from_secs(83));
    assert_eq!(client.ui.player.as_ref(), Some(&expected));
    assert_eq!(client.rt.snapshot(), expected);
    assert_eq!(
        client.ui.current().map(|e| e.id),
        saved.current,
        "the current entry is marked"
    );
    assert_eq!(
        take(&log),
        vec![Call::SetGain(0.7f64.powi(3) as f32)],
        "nothing resolved or played"
    );

    // Quit (as `q` does): the change is saved before the player ends.
    client
        .rt
        .handle(RuntimeInput::Command(Command::ChangeVolume(-10)));
    let h = client.rt.handle(RuntimeInput::Command(Command::Shutdown));
    assert!(h.shutdown);
    let on_disk = crate::player::persist::load(&crate::player::persist::RealFs, dir.path());
    assert_eq!(
        on_disk.saved,
        Some(tidal_player_core::SavedPlayback {
            volume: 60,
            ..saved
        })
    );

    // A corrupt file: an empty start with the player's message.
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("playback.json"), "{").unwrap();
    let (client, _log) = Client::restored(dir.path(), None);
    assert!(client.ui.queue().is_empty());
    let message = client.ui.message().unwrap_or_default();
    assert!(
        message.starts_with("Could not restore the playback state (kept as playback.json.bad): "),
        "{message:?}"
    );
    assert!(dir.path().join("playback.json.bad").exists());
}

/// Spec 0009 AC9: `tidal-player ITEM` over a remembered state: the
/// items replace the queue and play; shuffle, repeat, autoplay and
/// volume stay restored.
#[test]
fn ac9_items_replace_queue() {
    let saved = remembered();
    let dir = state_dir_with(&saved);
    let open = crate::client::startup_open(vec![Item::Track(TrackId(3))], None);
    assert!(open.is_some());
    let (client, _log) = Client::restored(dir.path(), open);
    let snapshot = client.rt.snapshot();
    assert_eq!(client.queue(), vec![3]);
    assert_eq!(snapshot.state, PlaybackState::Playing);
    assert_eq!(
        (
            snapshot.shuffle,
            snapshot.repeat,
            snapshot.autoplay,
            snapshot.volume,
            snapshot.muted
        ),
        (
            true,
            tidal_player_core::protocol::RepeatMode::Queue,
            false,
            70,
            false
        )
    );
    assert_eq!(client.shown(), vec![3]);
}

/// Spec 0009 AC8 (the part the daemon binary cannot observe: it has no
/// device): a restored player sends nothing before `TogglePause`; then
/// it resolves the current entry and the engine's `Play` starts at the
/// saved position, with no seek.
#[test]
fn ac8_play_at_saved_position() {
    let saved = remembered();
    let dir = state_dir_with(&saved);
    let (mut rt, log, rx) = restored_runtime(dir.path());
    assert_eq!(rt.snapshot(), restored_snapshot(&saved));
    // Only the restored gain (AC15): nothing fetched, nothing opened.
    assert_eq!(take(&log), vec![Call::SetGain(0.7f64.powi(3) as f32)]);
    rt.handle(RuntimeInput::Command(Command::TogglePause));
    while let Some(input) = rt.next_input(&rx, Duration::ZERO) {
        rt.handle(input);
    }
    let current = restored_snapshot(&saved)
        .queue
        .iter()
        .find(|e| Some(e.id) == saved.current)
        .map(|e| e.track.id.0)
        .expect("a current entry");
    let calls = take(&log);
    assert!(
        matches!(
            calls[..],
            [Call::Resolve { track, .. }, Call::Play { track: played, start_at, .. }, ..]
                if track == current && played == current && start_at == Duration::from_secs(83)
        ),
        "{calls:?}"
    );
    assert!(
        !calls.iter().any(|c| matches!(c, Call::Seek(_))),
        "{calls:?}"
    );
}

/// Spec 0009 AC15 (issue #41): a restored player's engine starts at the
/// restored gain, sent before any input; playing needs no other.
#[test]
fn ac15_restored_gain_reaches_engine() {
    let muted = tidal_player_core::SavedPlayback {
        volume: 40,
        muted: true,
        ..remembered()
    };
    let rows = [
        ("volume 70", Some(remembered()), 0.7f64.powi(3) as f32),
        ("muted at 40", Some(muted), 0.0),
        ("no file", None, 1.0),
    ];
    for (name, saved, gain) in rows {
        let dir = match &saved {
            Some(saved) => state_dir_with(saved),
            None => tempfile::tempdir().unwrap(),
        };
        let (mut rt, log, rx) = restored_runtime(dir.path());
        assert_eq!(take(&log), vec![Call::SetGain(gain)], "{name}");
        if saved.is_none() {
            continue;
        }
        rt.handle(RuntimeInput::Command(Command::TogglePause));
        while let Some(input) = rt.next_input(&rx, Duration::ZERO) {
            rt.handle(input);
        }
        let calls = take(&log);
        assert!(
            calls.iter().any(|c| matches!(c, Call::Play { .. }))
                && !calls.iter().any(|c| matches!(c, Call::SetGain(_))),
            "{name}: {calls:?}"
        );
    }
}

/// Spec 0009 "Saving", as the runtime wires it: a volume change is
/// written by the tick 2 s later, not at once; a write that fails is
/// the player's message once, broadcast.
#[test]
fn runtime_saves_on_schedule() {
    let saved = remembered();
    let dir = state_dir_with(&saved);
    let (mut rt, _log, _rx) = restored_runtime(dir.path());
    let on_disk = |dir: &std::path::Path| {
        crate::player::persist::load(&crate::player::persist::RealFs, dir)
            .saved
            .expect("a saved state")
    };
    let start = Instant::now();
    rt.handle(RuntimeInput::Command(Command::SetVolume(50)));
    assert_eq!(on_disk(dir.path()).volume, 70, "written at once");
    assert!(rt.tick(start).is_empty());
    assert_eq!(on_disk(dir.path()).volume, 70);
    assert!(rt.tick(start + Duration::from_secs(3)).is_empty());
    assert_eq!(on_disk(dir.path()).volume, 50, "not written after 2 s");

    // Unwritable: the temporary file's name is taken by a directory.
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("playback.json.tmp")).unwrap();
    let (mut rt, _log, _rx) = restored_runtime(dir.path());
    let h = rt.handle(RuntimeInput::Command(Command::ToggleMute));
    assert!(h.events.iter().all(|e| match e {
        Event::Player(s) => s.message.is_none(),
        _ => true,
    }));
    let events = rt.tick(Instant::now() + Duration::from_secs(3));
    let message = match events.last() {
        Some(Event::Player(s)) => s.message.clone().unwrap_or_default(),
        other => panic!("expected a snapshot, got {other:?}"),
    };
    assert!(
        message.starts_with("Could not save the playback state: "),
        "{message:?}"
    );
    assert_eq!(rt.snapshot().message, Some(message));
    // Shown once: the next failure says nothing new.
    rt.handle(RuntimeInput::Command(Command::ToggleMute));
    assert!(rt.tick(Instant::now() + Duration::from_secs(6)).is_empty());
}

/// The `asound` fixtures of the audio crate.
fn asound_fixture(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../audio/tests/fixtures/asound")
        .join(name)
}

/// 0014 AC6: the device the player starts on is the configured one
/// (flag for `play`, then `TIDAL_PLAYER_DEVICE`, `output_device`,
/// `default`), the one `main` starts the engine on, and the snapshot
/// says so, for a fresh and a restored player.
#[test]
fn ac6_snapshot_device_is_configured() {
    use crate::config::AppConfig;
    use crate::settings::{
        DEVICE_VAR, resolve_play_config_with, resolve_player_config_with, resolve_settings_with,
        with_device,
    };

    struct Row {
        name: &'static str,
        file: Option<&'static str>,
        env: Option<&'static str>,
        /// `play --device`; `None` for the TUI and the daemon.
        flag: Option<&'static str>,
        want: &'static str,
    }
    let rows = [
        Row {
            name: "default",
            file: None,
            env: None,
            flag: None,
            want: "default",
        },
        Row {
            name: "app.toml",
            file: Some("hw:0,0"),
            env: None,
            flag: None,
            want: "hw:0,0",
        },
        Row {
            name: "environment over app.toml",
            file: Some("hw:0,0"),
            env: Some("hw:1,0"),
            flag: None,
            want: "hw:1,0",
        },
        Row {
            name: "play's flag over the environment",
            file: Some("hw:0,0"),
            env: Some("hw:1,0"),
            flag: Some("plughw:1,0"),
            want: "plughw:1,0",
        },
    ];
    for row in rows {
        let name = row.name;
        let file = AppConfig {
            output_device: row.file.map(str::to_owned),
            ..AppConfig::default()
        };
        let env = |key: &str| (key == DEVICE_VAR).then(|| row.env.map(str::to_owned))?;
        let settings = resolve_settings_with(&file, None, row.flag, env).unwrap();
        let player = match row.flag {
            Some(_) => resolve_play_config_with(&file, false, env),
            None => resolve_player_config_with(&file, env),
        }
        .unwrap();
        // As `main` (and `play`) build the player and its engine.
        let config = with_device(player.player.clone(), &settings);
        let engine_device = settings.device.clone();
        assert_eq!(engine_device, row.want, "{name}: engine");
        assert_eq!(config.device, engine_device, "{name}: config");

        let log: Log = Arc::default();
        let (tx, _rx) = mpsc::channel();
        let rt = PlayerRuntime::new(
            config.clone(),
            7,
            FakeEngine::new(&log, Script::Plays),
            FakeJobs::new(&log, &tx, false),
        );
        assert_eq!(rt.snapshot().device, row.want, "{name}: fresh");

        let dir = tempfile::tempdir().unwrap();
        let rt = PlayerRuntime::restored(
            config,
            7,
            &player,
            Persister::new(dir.path().to_owned(), true),
            FakeEngine::new(&log, Script::Plays),
            FakeJobs::new(&log, &tx, false),
        );
        assert_eq!(rt.snapshot().device, row.want, "{name}: restored");
    }
}

/// 0014 AC11: with two subscribers (the TUI in-process and a socket
/// client), a `SetDevice` from the TUI reaches the engine as
/// `SetDevice` and both get the snapshot with the new device; the
/// engine's `OutputChanged` reaches both as the new output line, and a
/// `DeviceFallback` as 0003's message with the device put back. An
/// empty name is refused with a reply. The popup lists the player's
/// devices through the in-process link.
#[test]
fn ac11_set_device_reaches_engine() {
    use crate::ipc::server::{OUTBOX, Peer};
    use tidal_player_audio::{OutputKind, SampleFormat};
    use tidal_player_core::protocol::ServerMessage;

    const OTHER: ClientId = ClientId(900);
    let (mut client, log) = Client::build(
        |engine, jobs| {
            PlayerRuntime::new(PlayerConfig::default(), 7, engine, jobs).with_devices(Box::new(
                AsoundDevices {
                    dir: asound_fixture("onboard_usb"),
                },
            ))
        },
        None,
    );
    let (outbox, other) = mpsc::sync_channel(OUTBOX);
    client.rt.handle(RuntimeInput::Client(ClientInput::Attach {
        client: OTHER,
        peer: Peer::new(outbox, None),
    }));
    client
        .rt
        .handle(RuntimeInput::Client(ClientInput::Subscribe(OTHER)));
    let other_snapshots = || -> Vec<PlayerSnapshot> {
        other
            .try_iter()
            .filter_map(|m| match m {
                ServerMessage::Welcome { snapshot, .. }
                | ServerMessage::Event(Event::Player(snapshot)) => Some(snapshot),
                _ => None,
            })
            .collect()
    };
    client.rt.handle(load(&[1, 2]));
    client.act(Vec::new());
    assert_eq!(client.rt.snapshot().state, PlaybackState::Playing);
    assert_eq!(client.rt.snapshot().device, "default");
    other_snapshots();
    take(&log);

    // The TUI's `SetDevice`: the engine's call, both snapshots.
    client.session.send(Command::SetDevice("hw:0,0".into()));
    client.act(Vec::new());
    assert_eq!(take(&log), vec![Call::SetDevice("hw:0,0".into())]);
    let tui = client.ui.player.clone().expect("a snapshot");
    assert_eq!(tui.device, "hw:0,0");
    assert_eq!(
        other_snapshots().last().map(|s| s.device.clone()),
        Some("hw:0,0".to_owned())
    );

    // The engine reopened it: the new output line, to both.
    let onboard = OutputInfo {
        requested: "hw:0,0".into(),
        device: "hw:0,0".into(),
        kind: OutputKind::Exclusive,
        sample_format: SampleFormat::S16Le,
        sample_rate: 44_100,
        channels: 2,
        bit_perfect: true,
        not_bit_perfect_reason: None,
    };
    client
        .rt
        .handle(RuntimeInput::Engine(audio::Event::OutputChanged(onboard)));
    client.act(Vec::new());
    let output = |s: &PlayerSnapshot| s.now_playing.as_ref().map(|np| np.output.clone());
    let want = Some("hw:0,0 (exclusive) S16_LE 44.1 kHz 2 ch".to_owned());
    assert_eq!(client.ui.player.as_ref().and_then(output), want);
    let seen = other_snapshots();
    assert_eq!(seen.last().and_then(output), want);
    assert_eq!(
        seen.last()
            .and_then(|s| s.now_playing.as_ref())
            .map(|np| np.source.clone()),
        Some("FLAC 16-bit 44.1 kHz stereo".to_owned()),
        "the source line is kept"
    );

    // A switch that falls back: 0003's message, the device put back.
    client.session.send(Command::SetDevice("hw:1,0".into()));
    client.act(Vec::new());
    client
        .rt
        .handle(RuntimeInput::Engine(audio::Event::DeviceFallback {
            tried: "hw:1,0".into(),
            error: SinkError::Busy {
                device: "hw:1,0".into(),
                holder: Some("firefox".into()),
            },
            device: "hw:0,0".into(),
        }));
    client.act(Vec::new());
    let message = "Cannot switch to hw:1,0: Output hw:1,0 is busy (used by firefox): \
                       close it, or use --device default; staying on hw:0,0";
    for snapshot in [
        client.ui.player.clone().expect("a snapshot"),
        other_snapshots().pop().expect("a snapshot"),
    ] {
        assert_eq!(snapshot.device, "hw:0,0");
        assert_eq!(snapshot.message.as_deref(), Some(message));
        assert_eq!(snapshot.state, PlaybackState::Playing);
    }
    take(&log);

    // An empty name: refused with the reply, nothing reaches the engine.
    client.rt.handle(RuntimeInput::Client(ClientInput::Request {
        client: OTHER,
        id: 5,
        command: Command::SetDevice(String::new()),
    }));
    let replies: Vec<ServerMessage> = other
        .try_iter()
        .filter(|m| matches!(m, ServerMessage::Reply { .. }))
        .collect();
    assert_eq!(
        replies,
        vec![ServerMessage::Reply {
            id: 5,
            result: Err("Device name is empty".into())
        }]
    );
    assert_eq!(take(&log), vec![]);
    assert_eq!(client.rt.snapshot().device, "hw:0,0");

    // The popup, through the in-process link: the player's list, the
    // cursor on its device; `k` then `Enter` switches to `hw:0,0`'s
    // neighbour above, `default`.
    client.sent.clear();
    client.act(vec![ui::Action::Key(ui::Key::Char('D'))]);
    match &client.ui.popup {
        Some(ui::Popup::Devices {
            list: ui::DeviceList::Loaded(devices),
            cursor,
        }) => {
            assert_eq!(devices.len(), 4, "{devices:?}");
            assert_eq!(devices[*cursor].name, "hw:0,0");
        }
        other => panic!("expected the loaded devices popup, got {other:?}"),
    }
    client.act(vec![
        ui::Action::Key(ui::Key::Char('k')),
        ui::Action::Key(ui::Key::Enter),
    ]);
    assert_eq!(client.sent, vec![Command::SetDevice("default".into())]);
    assert_eq!(take(&log), vec![Call::SetDevice("default".into())]);
}

/// An API that answers lyrics requests from a table and counts its
/// calls (spec 0021 AC4).
#[derive(Default)]
struct CountingApi {
    calls: std::sync::Mutex<Vec<u64>>,
    /// Tracks whose request fails.
    failing: std::sync::Mutex<HashSet<u64>>,
    /// Tracks without lyrics (the rest have some).
    none: HashSet<u64>,
}

impl CountingApi {
    fn calls(&self) -> Vec<u64> {
        self.calls.lock().unwrap().clone()
    }
}

fn lyrics_of(track: u64) -> tidal_player_core::lyrics::Lyrics {
    tidal_player_core::lyrics::Lyrics {
        synced: vec![],
        plain: format!("words of {track}"),
        right_to_left: false,
    }
}

impl Library for CountingApi {
    fn request(
        &self,
        request: LibraryRequest,
        _: u32,
        _: Vec<String>,
    ) -> BoxFuture<'_, Result<LibraryResponse, String>> {
        Box::pin(async move {
            let LibraryRequest::Page(PageRequest::Lyrics(track)) = request else {
                return Ok(LibraryResponse::Done);
            };
            self.calls.lock().unwrap().push(track);
            if self.failing.lock().unwrap().contains(&track) {
                return Err("Tidal answered 429: try again in a moment".into());
            }
            let lyrics = (!self.none.contains(&track)).then(|| lyrics_of(track));
            Ok(LibraryResponse::Page(
                tidal_player_core::library::PageData::Lyrics { track, lyrics },
            ))
        })
    }
}

/// Spec 0021 AC4: the lyrics cache keeps the last 50 tracks asked for,
/// "no lyrics" included, never a failure; a hit makes the track the
/// newest.
#[test]
fn ac4_lyrics_cache() {
    use tidal_player_core::library::PageData;

    enum Step {
        /// Ask the track; the calls made so far are then `.1`.
        Ask(u64, &'static [u64]),
        /// The track's requests fail from now on.
        Fail(u64),
        /// The track's requests work again.
        Heal(u64),
    }
    use Step::*;
    let range = |from: u64, to: u64| (from..=to).collect::<Vec<_>>();
    let leak = |v: Vec<u64>| -> &'static [u64] { Box::leak(v.into_boxed_slice()) };
    let asked = |to: u64| -> Vec<Step> { (1..=to).map(|t| Ask(t, leak(range(1, t)))).collect() };

    let mut eviction = asked(51);
    eviction.extend([
        // 2 is still kept, 1 was the oldest and is gone.
        Ask(2, leak(range(1, 51))),
        Ask(1, leak([range(1, 51), vec![1]].concat())),
        // Asking 1 evicted 3 (2 had just become the newest).
        Ask(3, leak([range(1, 51), vec![1, 3]].concat())),
        Ask(2, leak([range(1, 51), vec![1, 3]].concat())),
    ]);
    let mut recency = asked(50);
    recency.extend([
        Ask(1, leak(range(1, 50))),
        // 51 evicts 2, the least recently asked, not 1.
        Ask(51, leak(range(1, 51))),
        Ask(1, leak(range(1, 51))),
        Ask(2, leak([range(1, 51), vec![2]].concat())),
    ]);
    let mut fifty = asked(50);
    fifty.push(Ask(50, leak(range(1, 50))));
    fifty.push(Ask(1, leak(range(1, 50))));

    let table: Vec<(&str, Vec<Step>)> = vec![
        ("second request: no call", vec![Ask(1, &[1]), Ask(1, &[1])]),
        ("no lyrics is kept", vec![Ask(9, &[9]), Ask(9, &[9])]),
        (
            "a failure is not kept",
            vec![
                Fail(3),
                Ask(3, &[3]),
                Ask(3, &[3, 3]),
                Heal(3),
                Ask(3, &[3, 3, 3]),
                Ask(3, &[3, 3, 3]),
            ],
        ),
        ("fifty are kept", fifty),
        ("the 51st evicts the oldest", eviction),
        ("a kept track becomes the newest", recency),
    ];
    for (name, steps) in table {
        let api = Arc::new(CountingApi {
            none: HashSet::from([9]),
            ..CountingApi::default()
        });
        let cache = LyricsCache::new(api.clone());
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        for (n, step) in steps.into_iter().enumerate() {
            match step {
                Fail(t) => {
                    api.failing.lock().unwrap().insert(t);
                }
                Heal(t) => {
                    api.failing.lock().unwrap().remove(&t);
                }
                Ask(track, calls) => {
                    let got = rt.block_on(cache.request(
                        LibraryRequest::Page(PageRequest::Lyrics(track)),
                        100,
                        vec![],
                    ));
                    if api.failing.lock().unwrap().contains(&track) {
                        assert!(got.is_err(), "{name} step {n}: {got:?}");
                    } else {
                        let lyrics = (track != 9).then(|| lyrics_of(track));
                        assert_eq!(
                            got,
                            Ok(LibraryResponse::Page(PageData::Lyrics { track, lyrics })),
                            "{name} step {n}"
                        );
                    }
                    assert_eq!(api.calls(), calls, "{name} step {n}");
                }
            }
        }
    }
}

/// Records what the runtime hands its hook (spec 0025).
#[derive(Clone, Default)]
struct Recorder {
    lines: Arc<Mutex<Vec<String>>>,
    finished: Arc<AtomicUsize>,
}

impl HookSink for Recorder {
    fn send(&mut self, events: Vec<tidal_player_core::hook::HookEvent>) {
        let mut lines = self.lines.lock().unwrap();
        lines.extend(events.iter().map(|e| e.arguments().join(" ")));
    }

    fn finish(&mut self) {
        self.finished.fetch_add(1, Ordering::SeqCst);
    }
}

struct Hooked {
    rt: PlayerRuntime<FakeEngine, FakeJobs>,
    log: Log,
    rx: Receiver<RuntimeInput>,
    recorder: Recorder,
}

impl Hooked {
    fn new(default: Script) -> Self {
        let (rt, log, rx) = runtime(default, true);
        let recorder = Recorder::default();
        let rt = rt.with_hook(Some(Box::new(recorder.clone())));
        Self {
            rt,
            log,
            rx,
            recorder,
        }
    }

    fn input(&mut self, input: RuntimeInput) {
        self.rt.handle(input);
        while let Some(input) = self.rt.next_input(&self.rx, Duration::ZERO) {
            self.rt.handle(input);
        }
    }

    fn command(&mut self, command: Command) {
        self.input(RuntimeInput::Command(command));
    }

    fn position(&mut self, secs: u64) {
        let position = Duration::from_secs(secs);
        self.input(RuntimeInput::Engine(audio::Event::Position(position)));
    }

    fn lines(&self) -> Vec<String> {
        self.recorder.lines.lock().unwrap().clone()
    }
}

/// Spec 0025 AC13: `play` ends without a `Shutdown` when its queue runs
/// out, and the hook still gets its time, or `Stopped` dies with it.
#[test]
fn ac13_play_waits_for_the_hook() {
    let mut h = Hooked::new(Script::Ends);
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let mut polls = 0;
    let code = crate::cli::play::play_queue(
        &mut h.rt,
        vec![track(1, Some(200))],
        &crate::cli::play::PlayOptions::default(),
        &h.rx,
        Duration::ZERO,
        &mut || {
            polls += 1;
            polls > 10_000
        },
        &mut crate::cli::play::PlayOutput {
            out: &mut out,
            err: &mut err,
            tty: false,
        },
    );
    assert_eq!(code, 0, "the queue ran out");
    assert_eq!(
        h.lines(),
        ["Changed 1", "Playing 1 0", "EndOfTrack 1", "Stopped 1 0"]
    );
    assert_eq!(h.recorder.finished.load(Ordering::SeqCst), 1);
}

/// Spec 0025 AC10: the sequences of a real player, gapless included.
#[test]
fn ac10_real_sequences() {
    let mut ending = Hooked::new(Script::Ends);
    ending.input(load(&[1]));
    assert_eq!(
        ending.lines(),
        ["Changed 1", "Playing 1 0", "EndOfTrack 1", "Stopped 1 0"],
        "the end of the queue"
    );

    let mut skip = Hooked::new(Script::Plays);
    skip.input(load(&[1, 2]));
    skip.command(Command::Next);
    assert_eq!(
        skip.lines(),
        ["Changed 1", "Playing 1 0", "Changed 2", "Playing 2 0"],
        "a skip is not an end"
    );

    // Gapless: the preload armed near the end, then the transition.
    let mut gapless = Hooked::new(Script::Plays);
    gapless.input(load(&[1, 2]));
    gapless.position(190);
    let tag = take(&gapless.log)
        .iter()
        .find_map(|call| match call {
            Call::Preload { tag, track: 2 } => Some(*tag),
            _ => None,
        })
        .expect("the next track was preloaded");
    gapless.input(RuntimeInput::Engine(audio::Event::Transitioned {
        tag,
        source: SOURCE,
        output: output(),
    }));
    assert_eq!(
        gapless.lines(),
        [
            "Changed 1",
            "Playing 1 0",
            "EndOfTrack 1",
            "Changed 2",
            "Playing 2 0"
        ],
        "gapless"
    );

    // An end that is not gapless: track 1 ends, track 2 goes on.
    let log: Log = Arc::default();
    let (tx, rx) = mpsc::channel();
    let engine = FakeEngine::new(&log, Script::Plays).script(1, vec![Script::Ends]);
    let jobs = FakeJobs::new(&log, &tx, true);
    let recorder = Recorder::default();
    let rt = PlayerRuntime::new(PlayerConfig::default(), 7, engine, jobs)
        .with_hook(Some(Box::new(recorder.clone())));
    let mut plain = Hooked {
        rt,
        log,
        rx,
        recorder,
    };
    plain.input(load(&[1, 2]));
    assert_eq!(
        plain.lines(),
        [
            "Changed 1",
            "Playing 1 0",
            "EndOfTrack 1",
            "Changed 2",
            "Playing 2 0"
        ],
        "an end that is not gapless"
    );
}

/// Spec 0025 AC4: seeks and `Previous` as the runtime sees them.
#[test]
fn ac4_seeks_through_the_runtime() {
    let mut h = Hooked::new(Script::Plays);
    h.input(load(&[1, 2]));
    h.position(10);
    h.command(Command::SeekTo(Duration::from_secs(20)));
    h.command(Command::SeekBy(5000));
    h.command(Command::Previous);
    assert_eq!(
        h.lines()[2..],
        ["Seeked 1 20000", "Seeked 1 25000", "Seeked 1 0"]
    );
}

/// Spec 0025 AC6: moving a playing track says so, and nothing else.
#[test]
fn ac6_device_move_runs_only_device_changed() {
    let mut h = Hooked::new(Script::Plays);
    h.input(load(&[1]));
    h.command(Command::SetDevice("hw:9".into()));
    h.input(RuntimeInput::Engine(audio::Event::OutputChanged(output())));
    assert_eq!(
        h.lines(),
        ["Changed 1", "Playing 1 0", "DeviceChanged hw:9"]
    );
}

/// Spec 0025 AC11: a failure is the player's message, playback goes on.
#[test]
fn ac11_failure_is_the_players_message() {
    let mut h = Hooked::new(Script::Plays);
    h.input(load(&[1]));
    let handled = h.rt.handle(RuntimeInput::HookFailed(
        "Player event hook failed: x".into(),
    ));
    let snapshot = last_snapshot(&handled);
    assert_eq!(
        snapshot.message.as_deref(),
        Some("Player event hook failed: x")
    );
    assert_eq!(snapshot.state, PlaybackState::Playing);
}

/// Spec 0025 AC14: a shutdown stops what plays, then gives the hook its
/// time.
#[test]
fn ac14_shutdown_sends_stopped() {
    let shutdown = |h: &mut Hooked| {
        h.rt.handle(RuntimeInput::Command(Command::Shutdown));
        assert_eq!(h.recorder.finished.load(Ordering::SeqCst), 1);
        h.lines().last().cloned()
    };
    let mut h = Hooked::new(Script::Plays);
    h.input(load(&[1]));
    h.position(5);
    assert_eq!(shutdown(&mut h).as_deref(), Some("Stopped 1 5000"));

    let mut h = Hooked::new(Script::Plays);
    h.input(load(&[1]));
    h.position(5);
    h.command(Command::Pause);
    assert_eq!(shutdown(&mut h).as_deref(), Some("Stopped 1 5000"));

    let mut h = Hooked::new(Script::Plays);
    assert_eq!(shutdown(&mut h), None);
}
