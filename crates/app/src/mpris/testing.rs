//! The in-process harness of the adapter's tests: a player runtime on its
//! own thread with the fake engine and jobs (0005's harness), the adapter
//! attached to it, and a [`Recorder`] for a bus.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tidal_player_api::auth::BoxFuture;
use tidal_player_api::metadata::MetadataError;
use tidal_player_core::player::PlayerConfig;
use tidal_player_core::protocol::{Command, Event, PlayerSnapshot, ServerMessage};
use tidal_player_core::{Track, TrackId};

use super::hub::{Adapter, Bus, Options};
use super::model::Properties;
use crate::ipc::server::{ClientInput, OUTBOX, Peer, next_client_id};
use crate::player_runtime::fakes::{FakeEngine, FakeJobs, Script, track};
use crate::player_runtime::{Metadata, PlayerRuntime, RuntimeHandle, RuntimeInput, spawn_runtime};

/// What the adapter told the bus, in order.
#[derive(Debug, Clone, PartialEq)]
pub enum Signal {
    Changed(Properties),
    Seeked(Duration),
    Closed,
}

/// A bus that records.
#[derive(Default)]
pub struct Recorder {
    signals: Mutex<Vec<Signal>>,
    closed: AtomicBool,
}

impl Recorder {
    pub fn take(&self) -> Vec<Signal> {
        std::mem::take(&mut *self.signals.lock().unwrap())
    }

    pub fn closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }

    /// Waits at most 5 s until `pred` holds for what was recorded so far
    /// (taken).
    pub fn wait(&self, what: &str, pred: impl Fn(&[Signal]) -> bool) -> Vec<Signal> {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut seen = Vec::new();
        loop {
            seen.extend(self.take());
            if pred(&seen) {
                return seen;
            }
            assert!(Instant::now() < deadline, "no {what} within 5 s: {seen:?}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

impl Bus for Recorder {
    fn properties_changed(&self, changed: &Properties) {
        self.signals
            .lock()
            .unwrap()
            .push(Signal::Changed(changed.clone()));
    }

    fn seeked(&self, position: Duration) {
        self.signals.lock().unwrap().push(Signal::Seeked(position));
    }

    fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
        self.signals.lock().unwrap().push(Signal::Closed);
    }
}

/// Albums: `404` has no tracks; any other has tracks 1 and 2.
pub struct Albums;

impl Metadata for Albums {
    fn track(&self, id: TrackId) -> BoxFuture<'_, Result<Track, MetadataError>> {
        Box::pin(async move { Ok(track(id.0, Some(200))) })
    }

    fn album(&self, id: u64) -> BoxFuture<'_, Result<Vec<Track>, MetadataError>> {
        Box::pin(async move {
            Ok(if id == 404 {
                Vec::new()
            } else {
                vec![track(1, Some(200)), track(2, Some(200))]
            })
        })
    }

    fn playlist(&self, _uuid: String) -> BoxFuture<'_, Result<Vec<Track>, MetadataError>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn suggestions(&self, _seed: TrackId) -> BoxFuture<'_, Result<Vec<Track>, MetadataError>> {
        Box::pin(async { Ok(Vec::new()) })
    }
}

/// A running player with the adapter attached to it.
pub struct Rig {
    pub player: RuntimeHandle,
    pub adapter: Adapter,
    pub bus: Arc<Recorder>,
}

impl Rig {
    /// A player whose tracks keep playing, with `options`' adapter.
    pub fn new(options: Options) -> Self {
        let log = Arc::default();
        let (tx, rx) = mpsc::channel();
        let engine = FakeEngine::new(&log, Script::Plays);
        let mut jobs = FakeJobs::new(&log, &tx, true);
        jobs.metadata = Some(Arc::new(Albums));
        let runtime = PlayerRuntime::new(PlayerConfig::default(), 7, engine, jobs);
        let player = spawn_runtime(runtime, rx, tx);
        let adapter = Adapter::new(player.inputs(), options);
        let bus = Arc::new(Recorder::default());
        adapter.set_bus(Arc::clone(&bus) as Arc<dyn Bus>);
        assert!(adapter.attach(), "the player is gone");
        Self {
            player,
            adapter,
            bus,
        }
    }

    /// Another client of the same player, subscribed: its messages.
    pub fn client(&self) -> Receiver<ServerMessage> {
        let client = next_client_id();
        let (outbox, messages) = mpsc::sync_channel(OUTBOX);
        let inputs = self.player.inputs();
        inputs
            .send(RuntimeInput::Client(ClientInput::Attach {
                client,
                peer: Peer::new(outbox, None),
            }))
            .unwrap();
        inputs
            .send(RuntimeInput::Client(ClientInput::Subscribe(client)))
            .unwrap();
        messages
    }

    /// Sends a command as the in-process TUI would.
    pub fn send(&self, command: Command) {
        self.player.send(command);
    }
}

/// The snapshots in `messages` so far (the `Welcome`'s first).
pub fn snapshots(messages: &Receiver<ServerMessage>) -> Vec<PlayerSnapshot> {
    messages
        .try_iter()
        .filter_map(|m| match m {
            ServerMessage::Welcome { snapshot, .. }
            | ServerMessage::Event(Event::Player(snapshot)) => Some(snapshot),
            _ => None,
        })
        .collect()
}
