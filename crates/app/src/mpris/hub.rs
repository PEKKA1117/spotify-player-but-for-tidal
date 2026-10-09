//! The MPRIS adapter as a client of the player's hub (spec 0010, 0005
//! decision 1): an in-process [`Peer`] that subscribes like any client,
//! keeps its own copy of the state (properties are answered from it, never
//! by asking the player), turns each change into `PropertiesChanged` and
//! `Seeked` on a [`Bus`], and sends each MPRIS call to the player as a
//! `Request`, answered once the player's `Reply` came.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::{Duration, Instant};

use tidal_player_core::EntryId;
use tidal_player_core::protocol::{
    Command, Event, PlaybackState, PlayerSnapshot, RepeatMode, ServerMessage,
};
use tokio::sync::oneshot;

use super::covers::{Art, CoverCache, CoverWorker, art_url};
use super::model::{self, Call, CallError, PositionClock, Properties};
use super::{Level, Log};
use crate::ipc::server::{ClientId, ClientInput, OUTBOX, Peer, next_client_id};
use crate::player_runtime::RuntimeInput;

/// How long a call waits for the player (0005's one-shot deadline).
pub const DEADLINE: Duration = Duration::from_secs(5);
/// The error of a call the player can no longer answer.
pub const SHUTTING_DOWN: &str = "The player is shutting down";

/// The D-Bus side, as the adapter drives it: the bus layer, or a recorder
/// in tests.
pub trait Bus: Send + Sync {
    /// One `PropertiesChanged` on `org.mpris.MediaPlayer2.Player`.
    fn properties_changed(&self, changed: &Properties);
    /// `Seeked(position)`.
    fn seeked(&self, position: Duration);
    /// Closes the connection (the name is released with it).
    fn close(&self);
}

/// What the adapter is built with.
pub struct Options {
    /// How long a call waits for the player's reply.
    pub deadline: Duration,
    /// The cover cache and the runtime its downloads run on; `None`: no
    /// cache (Tidal's `https://` URLs).
    pub covers: Option<(CoverCache, tokio::runtime::Handle)>,
    pub log: Log,
}

impl Options {
    /// [`DEADLINE`], no cache, `tracing` for the log.
    pub fn new() -> Self {
        Self {
            deadline: DEADLINE,
            covers: None,
            log: super::tracing_log(),
        }
    }
}

impl Default for Options {
    fn default() -> Self {
        Self::new()
    }
}

/// The adapter: cheap to share (`Arc` inside).
#[derive(Clone)]
pub struct Adapter {
    shared: Arc<Shared>,
}

struct Shared {
    client: ClientId,
    inputs: Mutex<Sender<RuntimeInput>>,
    deadline: Duration,
    next_id: AtomicU64,
    calls: Mutex<Calls>,
    view: Mutex<View>,
    bus: Mutex<Option<Arc<dyn Bus>>>,
    cache: Option<CoverCache>,
    worker: OnceLock<CoverWorker>,
    log: Log,
    /// The outbox, until [`Adapter::attach`] starts reading it.
    messages: Mutex<Option<(mpsc::SyncSender<ServerMessage>, Receiver<ServerMessage>)>>,
}

#[derive(Default)]
struct Calls {
    closed: bool,
    pending: HashMap<u64, oneshot::Sender<Result<(), String>>>,
}

struct View {
    snapshot: PlayerSnapshot,
    welcomed: bool,
    clock: PositionClock,
    /// What the bus was last told (`Position` aside).
    published: Properties,
    /// The covers' states, by cover ID.
    art: HashMap<String, Art>,
    /// Covers whose failure was logged.
    warned: HashSet<String>,
}

/// A request sent to the player, waiting for its reply.
pub struct Pending {
    id: u64,
    reply: oneshot::Receiver<Result<(), String>>,
    shared: Arc<Shared>,
}

fn empty_snapshot() -> PlayerSnapshot {
    PlayerSnapshot {
        queue: Vec::new(),
        current: None,
        state: PlaybackState::Stopped,
        position: Duration::ZERO,
        shuffle: false,
        repeat: RepeatMode::Off,
        autoplay: false,
        volume: 100,
        muted: false,
        now_playing: None,
        message: None,
    }
}

impl Adapter {
    /// An adapter for the player reading `inputs`; it joins the hub on
    /// [`Self::attach`].
    pub fn new(inputs: Sender<RuntimeInput>, options: Options) -> Self {
        let now = Instant::now();
        let (cache, runtime) = match options.covers {
            Some((cache, runtime)) if cache.max() > 0 => (Some(cache), Some(runtime)),
            _ => (None, None),
        };
        let snapshot = empty_snapshot();
        let mut view = View {
            snapshot,
            welcomed: false,
            clock: PositionClock::new(now),
            published: Properties::new(),
            art: HashMap::new(),
            warned: HashSet::new(),
        };
        view.published = properties(&view, now);
        let shared = Arc::new(Shared {
            client: next_client_id(),
            inputs: Mutex::new(inputs),
            deadline: options.deadline,
            next_id: AtomicU64::new(1),
            calls: Mutex::new(Calls::default()),
            view: Mutex::new(view),
            bus: Mutex::new(None),
            cache: cache.clone(),
            worker: OnceLock::new(),
            log: options.log,
            messages: Mutex::new(Some(mpsc::sync_channel(OUTBOX))),
        });
        if let (Some(cache), Some(runtime)) = (cache, runtime) {
            let weak: Weak<Shared> = Arc::downgrade(&shared);
            let worker = CoverWorker::spawn(
                cache,
                &runtime,
                Arc::new(move |cover, result| {
                    if let Some(shared) = weak.upgrade() {
                        shared.cover_done(&cover, result);
                    }
                }),
            );
            let _ = shared.worker.set(worker);
        }
        Self { shared }
    }

    /// Joins the hub (attach, subscribe) and starts reading what the
    /// player sends. `false` when the player is gone.
    pub fn attach(&self) -> bool {
        let Some((outbox, messages)) = lock(&self.shared.messages).take() else {
            return false;
        };
        let client = self.shared.client;
        let attached = {
            let inputs = lock(&self.shared.inputs);
            inputs
                .send(RuntimeInput::Client(ClientInput::Attach {
                    client,
                    peer: Peer::new(outbox, None),
                }))
                .is_ok()
                && inputs
                    .send(RuntimeInput::Client(ClientInput::Subscribe(client)))
                    .is_ok()
        };
        if !attached {
            self.shared.shut();
            return false;
        }
        let shared = Arc::clone(&self.shared);
        let spawned = std::thread::Builder::new()
            .name("mpris".into())
            .spawn(move || {
                // Drained at once, so the hub never finds the outbox full.
                for message in messages {
                    shared.on_message(message);
                }
                shared.shut();
            });
        if spawned.is_err() {
            self.shared.shut();
            return false;
        }
        true
    }

    /// Where changes are signalled from now on.
    pub fn set_bus(&self, bus: Arc<dyn Bus>) {
        *lock(&self.shared.bus) = Some(bus);
    }

    /// The properties now (`Position` extrapolated).
    pub fn properties(&self) -> Properties {
        properties(&lock(&self.shared.view), Instant::now())
    }

    /// One property now.
    pub fn property(&self, name: &str) -> Option<model::Value> {
        self.properties().remove(name)
    }

    /// Sends `command` to the player as a request.
    pub fn request(&self, command: Command) -> Result<Pending, CallError> {
        let shared = &self.shared;
        let id = shared.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, reply) = oneshot::channel();
        {
            let mut calls = lock(&shared.calls);
            if calls.closed {
                return Err(CallError::Failed(SHUTTING_DOWN.into()));
            }
            calls.pending.insert(id, tx);
        }
        let sent = lock(&shared.inputs).send(RuntimeInput::Client(ClientInput::Request {
            client: shared.client,
            id,
            command,
        }));
        if sent.is_err() {
            lock(&shared.calls).pending.remove(&id);
            return Err(CallError::Failed(SHUTTING_DOWN.into()));
        }
        Ok(Pending {
            id,
            reply,
            shared: Arc::clone(shared),
        })
    }

    /// Applies an MPRIS call: once the player has handled it.
    pub async fn call(&self, call: Call) -> Result<(), CallError> {
        match model::command_for(&call)? {
            Some(command) => self.request(command)?.wait().await,
            None => {
                if lock(&self.shared.calls).closed {
                    Err(CallError::Failed(SHUTTING_DOWN.into()))
                } else {
                    Ok(())
                }
            }
        }
    }

    /// Stops taking calls, leaves the hub and closes the bus (after the
    /// player's other clients were told it shut down: the caller closes
    /// it after the socket server).
    pub fn close(&self) {
        self.shared.shut();
        let _ = lock(&self.shared.inputs).send(RuntimeInput::Client(ClientInput::Detach(
            self.shared.client,
        )));
        let bus = lock(&self.shared.bus).take();
        if let Some(bus) = bus {
            bus.close();
        }
    }
}

impl Pending {
    /// The player's answer, within the deadline.
    pub async fn wait(self) -> Result<(), CallError> {
        let deadline = self.shared.deadline;
        match tokio::time::timeout(deadline, self.reply).await {
            Ok(Ok(Ok(()))) => Ok(()),
            Ok(Ok(Err(message))) => Err(CallError::Failed(message)),
            Ok(Err(_)) => Err(CallError::Failed(SHUTTING_DOWN.into())),
            Err(_) => {
                lock(&self.shared.calls).pending.remove(&self.id);
                Err(CallError::Timeout(format!(
                    "The player did not answer within {} s",
                    deadline.as_secs()
                )))
            }
        }
    }
}

impl Shared {
    fn on_message(&self, message: ServerMessage) {
        match message {
            ServerMessage::Welcome { snapshot, .. } => self.on_snapshot(snapshot, true),
            ServerMessage::Event(Event::Player(snapshot)) => self.on_snapshot(snapshot, false),
            ServerMessage::Event(Event::Position { entry, position }) => {
                self.on_position(entry, position);
            }
            ServerMessage::Event(Event::ShuttingDown) => self.shut(),
            ServerMessage::Event(_) | ServerMessage::LibraryReply { .. } => {}
            ServerMessage::Reply { id, result } => {
                if let Some(tx) = lock(&self.calls).pending.remove(&id) {
                    let _ = tx.send(result);
                }
            }
        }
    }

    /// No more calls; every waiting one fails.
    fn shut(&self) {
        let mut calls = lock(&self.calls);
        calls.closed = true;
        for (_, tx) in calls.pending.drain() {
            let _ = tx.send(Err(SHUTTING_DOWN.into()));
        }
    }

    fn bus(&self) -> Option<Arc<dyn Bus>> {
        lock(&self.bus).clone()
    }

    fn on_snapshot(&self, snapshot: PlayerSnapshot, welcome: bool) {
        let now = Instant::now();
        let mut view = lock(&self.view);
        let new = snapshot.current.map(|e| (e, snapshot.position));
        let seek = (view.welcomed && !welcome)
            .then(|| model::seeked(view.clock.at(), new, view.clock.elapsed(now)))
            .flatten();
        let entry_changed = !view.welcomed || view.snapshot.current != snapshot.current;
        view.clock.report(
            snapshot.current,
            snapshot.position,
            snapshot.state,
            duration(&snapshot),
            now,
        );
        view.snapshot = snapshot;
        view.welcomed = true;
        if entry_changed {
            self.cover_for_current(&mut view);
        }
        self.publish(&mut view, now);
        if let (Some(position), Some(bus)) = (seek, self.bus()) {
            bus.seeked(position);
        }
    }

    fn on_position(&self, entry: EntryId, position: std::time::Duration) {
        let now = Instant::now();
        let mut view = lock(&self.view);
        if view.snapshot.current != Some(entry) {
            return;
        }
        let seek = model::seeked(
            view.clock.at(),
            Some((entry, position)),
            view.clock.elapsed(now),
        );
        view.snapshot.position = position;
        let state = view.snapshot.state;
        let length = duration(&view.snapshot);
        view.clock.report(Some(entry), position, state, length, now);
        if let (Some(position), Some(bus)) = (seek, self.bus()) {
            bus.seeked(position);
        }
    }

    /// The current entry's cover: cached (touched), or downloading.
    fn cover_for_current(&self, view: &mut View) {
        let Some(cover) = current_cover(&view.snapshot) else {
            return;
        };
        let art = match &self.cache {
            None => Art::Off,
            Some(cache) => match cache.lookup(&cover) {
                Some(path) => Art::Cached(path),
                None => match self.worker.get() {
                    Some(worker) => {
                        worker.want(&cover);
                        Art::Downloading
                    }
                    None => Art::Off,
                },
            },
        };
        view.art.insert(cover, art);
    }

    /// A download finished: the cover's state, and `Metadata` again if it
    /// is still the current entry's.
    fn cover_done(&self, cover: &str, result: Result<std::path::PathBuf, String>) {
        let mut view = lock(&self.view);
        let art = match result {
            Ok(path) => Art::Cached(path),
            Err(e) => {
                if view.warned.insert(cover.to_owned()) {
                    (self.log)(Level::Warn, &format!("Cannot cache the cover {cover}: {e}"));
                }
                Art::Failed
            }
        };
        view.art.insert(cover.to_owned(), art);
        self.publish(&mut view, Instant::now());
    }

    /// Signals what changed since the bus was last told.
    fn publish(&self, view: &mut View, now: Instant) {
        let new = properties(view, now);
        let changed = model::changes(&view.published, &new);
        view.published = new;
        if changed.is_empty() {
            return;
        }
        if let Some(bus) = self.bus() {
            bus.properties_changed(&changed);
        }
    }
}

fn properties(view: &View, now: Instant) -> Properties {
    let art = |cover: &str| {
        let state = view.art.get(cover).cloned().unwrap_or(Art::Off);
        art_url(Some(cover), &state)
    };
    model::player_properties(&view.snapshot, view.clock.position(now), &art)
}

fn current_cover(snapshot: &PlayerSnapshot) -> Option<String> {
    let id = snapshot.current?;
    let entry = snapshot.queue.iter().find(|e| e.id == id)?;
    entry.track.album.as_ref()?.cover.clone()
}

fn duration(snapshot: &PlayerSnapshot) -> Option<Duration> {
    let id = snapshot.current?;
    snapshot
        .queue
        .iter()
        .find(|e| e.id == id)
        .and_then(|e| e.track.duration)
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    use super::super::testing::{Rig, snapshots};
    use super::*;
    use crate::player_runtime::fakes::track;
    use model::Value;

    fn load(rig: &Rig, n: u64) {
        rig.send(Command::LoadQueue {
            tracks: (1..=n).map(|i| track(i, Some(200))).collect(),
            start: 0,
        });
    }

    fn status(adapter: &Adapter) -> Option<Value> {
        adapter.property("PlaybackStatus")
    }

    fn s(v: &str) -> Option<Value> {
        Some(Value::Str(v.into()))
    }

    /// AC9: a call's command reaches the player as a request and the call
    /// completes after the player's reply (the adapter's copy already
    /// shows its effect); setters are decided by the player.
    #[tokio::test(flavor = "multi_thread")]
    async fn ac9_calls_reach_player() {
        let rig = Rig::new(Options::new());
        let other = rig.client();
        load(&rig, 3);

        rig.adapter.call(Call::SetVolume(0.5)).await.unwrap();
        assert_eq!(rig.adapter.property("Volume"), Some(Value::Double(0.5)));
        assert!(
            snapshots(&other).iter().any(|s| s.volume == 50),
            "the other client did not see the volume"
        );

        rig.adapter.call(Call::Pause).await.unwrap();
        assert_eq!(status(&rig.adapter), s("Paused"));
        // tidalt's `Pause` toggled: a paused player resumed.
        rig.adapter.call(Call::Pause).await.unwrap();
        assert_eq!(status(&rig.adapter), s("Paused"));
        rig.adapter.call(Call::Play).await.unwrap();
        assert_eq!(status(&rig.adapter), s("Playing"));
        rig.adapter.call(Call::Play).await.unwrap();
        assert_eq!(status(&rig.adapter), s("Playing"));

        rig.adapter
            .call(Call::OpenUri("tidal://album/7".into()))
            .await
            .unwrap();
        let last = snapshots(&other).pop().expect("a snapshot");
        assert_eq!(
            last.queue.iter().map(|e| e.track.id.0).collect::<Vec<_>>(),
            vec![1, 2]
        );
        // The bus heard of the volume.
        let changed = rig.bus.take();
        assert!(
            changed.iter().any(|signal| matches!(
                signal,
                super::super::testing::Signal::Changed(p)
                    if p.get("Volume") == Some(&Value::Double(0.5))
            )),
            "{changed:?}"
        );
    }

    /// AC9: the player's `Err` is `Failed` with its message; a bad argument
    /// is `InvalidArgs`; a player that does not answer gives `Timeout`
    /// after the deadline (paused clock).
    #[test]
    fn ac9_error_and_timeout() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let rig = Rig::new(Options::new());
            assert_eq!(
                rig.adapter
                    .call(Call::OpenUri("tidal://album/404".into()))
                    .await,
                Err(CallError::Failed("Album 404 has no tracks".into()))
            );
            assert!(matches!(
                rig.adapter
                    .call(Call::SetLoopStatus("Shuffle".into()))
                    .await,
                Err(CallError::InvalidArgs(_))
            ));
        });

        let paused = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .start_paused(true)
            .build()
            .unwrap();
        paused.block_on(async {
            // A player that never reads its inputs.
            let (inputs, _held) = mpsc::channel();
            let adapter = Adapter::new(inputs, Options::new());
            assert!(adapter.attach());
            let started = tokio::time::Instant::now();
            assert_eq!(
                adapter.call(Call::Play).await,
                Err(CallError::Timeout(
                    "The player did not answer within 5 s".into()
                ))
            );
            assert!(started.elapsed() >= DEADLINE, "{:?}", started.elapsed());
        });
    }

    /// AC9: 100 calls in a burst are all applied, in order (none dropped).
    #[tokio::test(flavor = "multi_thread")]
    async fn ac9_burst_in_order() {
        let rig = Rig::new(Options::new());
        let other = rig.client();
        load(&rig, 1);
        let pending: Vec<Pending> = (1..=100u32)
            .map(|v| {
                let command = model::command_for(&Call::SetVolume(f64::from(v) / 100.0))
                    .unwrap()
                    .unwrap();
                rig.adapter.request(command).unwrap()
            })
            .collect();
        for p in pending {
            assert_eq!(p.wait().await, Ok(()));
        }
        let mut volumes: Vec<u8> = snapshots(&other).iter().map(|s| s.volume).collect();
        volumes.dedup();
        let applied: Vec<u8> = volumes.into_iter().skip_while(|v| *v == 100).collect();
        assert_eq!(applied, (1..=100).collect::<Vec<u8>>());
        assert_eq!(rig.adapter.property("Volume"), Some(Value::Double(1.0)));
    }

    /// AC16: after `Shutdown` the adapter refuses calls with the
    /// shutting-down error; the bus is closed only when the adapter is
    /// closed, after every client got `ShuttingDown`.
    #[tokio::test(flavor = "multi_thread")]
    async fn ac16_shutdown_order() {
        let rig = Rig::new(Options::new());
        let other = rig.client();
        load(&rig, 1);
        rig.adapter.call(Call::Play).await.unwrap();
        rig.send(Command::Shutdown);
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match rig.adapter.call(Call::Next).await {
                Err(CallError::Failed(m)) if m == SHUTTING_DOWN => break,
                other => {
                    assert!(Instant::now() < deadline, "still taking calls: {other:?}");
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            }
        }
        assert!(
            other
                .try_iter()
                .any(|m| m == ServerMessage::Event(Event::ShuttingDown)),
            "the other client got no ShuttingDown"
        );
        assert!(!rig.bus.closed(), "closed before the owner closed it");
        rig.adapter.close();
        assert!(rig.bus.closed());
        assert_eq!(
            rig.adapter.call(Call::Play).await,
            Err(CallError::Failed(SHUTTING_DOWN.into()))
        );
        // Calls that need no command are refused too.
        assert_eq!(
            rig.adapter.call(Call::SetRate(1.0)).await,
            Err(CallError::Failed(SHUTTING_DOWN.into()))
        );
    }
}
