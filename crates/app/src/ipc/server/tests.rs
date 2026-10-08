//! The player's server with 0004's fake engine and jobs, clients over
//! `UnixStream::pair()` (spec 0005 Test plan, AC3–AC6, AC8).

use std::collections::HashMap;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc::{self, Sender};
use std::time::{Duration, Instant};

use tidal_player_api::auth::AuthStatus;
use tidal_player_core::library::LibraryResponse;
use tidal_player_core::player::PlayerConfig;
use tidal_player_core::protocol::{
    ClientMessage, Command, Event, InsertAt, PlayerSnapshot, ServerMessage,
};
use tidal_player_core::{EntryId, Track, ui};

use super::*;
use crate::ipc::client::{Connection, RecvError};
use crate::player_runtime::fakes::{
    Call, FakeEngine, FakeJobs, FakeLibrary, Log, Script, delete, track,
};
use crate::player_runtime::{PlayerRuntime, RuntimeHandle, spawn_runtime};

/// The longest any one message may take to arrive.
const WAIT: Duration = Duration::from_secs(5);

/// A command that never changes anything: its reply marks "everything
/// before it has arrived".
const NOOP: Command = Command::RemoveFromQueue(EntryId(u64::MAX));

/// The runtime on its thread, with fakes, and a way in for clients.
struct TestPlayer {
    _handle: RuntimeHandle,
    inputs: Sender<RuntimeInput>,
    log: Log,
}

impl TestPlayer {
    fn start() -> Self {
        Self::start_with(|jobs| jobs)
    }

    /// A player whose library is `library`.
    fn start_with_library(library: &Arc<FakeLibrary>) -> Self {
        Self::start_with(|jobs| jobs.with_library(Arc::clone(library) as _))
    }

    fn start_with(jobs: impl FnOnce(FakeJobs) -> FakeJobs) -> Self {
        let log: Log = Log::default();
        let (tx, rx) = mpsc::channel();
        let engine = FakeEngine::new(&log, Script::Plays);
        let jobs = jobs(FakeJobs::new(&log, &tx, true));
        let runtime = PlayerRuntime::new(PlayerConfig::default(), 7, engine, jobs);
        let handle = spawn_runtime(runtime, rx, tx.clone());
        Self {
            _handle: handle,
            inputs: tx,
            log,
        }
    }

    /// A client connected over a socket pair, past the greeting.
    fn connect(&self) -> Client {
        let (ours, theirs) = UnixStream::pair().unwrap();
        attach_stream(ours, &self.inputs).unwrap();
        let conn = Connection::handshake(theirs, Path::new("<pair>")).expect("greeting");
        Client { conn, next_id: 0 }
    }

    /// The player's snapshot now, as a new subscriber's `Welcome`.
    fn snapshot(&self) -> (PlayerSnapshot, bool) {
        self.connect().subscribe()
    }
}

struct Client {
    conn: Connection,
    next_id: u64,
}

impl Client {
    /// Subscribes; the first message must be the `Welcome`.
    fn subscribe(&mut self) -> (PlayerSnapshot, bool) {
        self.conn.send(&ClientMessage::Subscribe).unwrap();
        match self.recv() {
            ServerMessage::Welcome {
                snapshot,
                login_required,
            } => (snapshot, login_required),
            other => panic!("expected a Welcome first, got {other:?}"),
        }
    }

    fn send(&mut self, command: Command) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.conn
            .send(&ClientMessage::Request { id, command })
            .unwrap();
        id
    }

    /// Sends a library request named `label` (see `delete`).
    fn library(&mut self, label: &str) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.conn
            .send(&ClientMessage::Library {
                id,
                request: delete(label),
            })
            .unwrap();
        id
    }

    /// Sends `request` as a `Library` request; its `id`.
    fn ask_library(&mut self, request: LibraryRequest) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.conn
            .send(&ClientMessage::Library { id, request })
            .unwrap();
        id
    }

    /// The next `LibraryReply` (events before it are skipped).
    fn library_reply(&mut self) -> (u64, Result<LibraryResponse, String>) {
        loop {
            if let ServerMessage::LibraryReply { id, result } = self.recv() {
                return (id, result);
            }
        }
    }

    fn recv(&mut self) -> ServerMessage {
        match self.conn.recv(Some(WAIT)) {
            Ok(Some(message)) => message,
            Ok(None) => panic!("no message within {WAIT:?}"),
            Err(e) => panic!("connection failed: {e}"),
        }
    }

    /// Every message that arrived already.
    fn drain(&mut self) -> Vec<ServerMessage> {
        let mut messages = Vec::new();
        while let Ok(Some(message)) = self.conn.recv(Some(Duration::ZERO)) {
            messages.push(message);
        }
        messages
    }

    /// The messages up to and including the reply to `id` (which must be
    /// `Ok`).
    fn until_reply(&mut self, id: u64) -> Vec<ServerMessage> {
        let mut messages = Vec::new();
        loop {
            let message = self.recv();
            let done = matches!(&message, ServerMessage::Reply { id: got, .. } if *got == id);
            if done {
                assert_eq!(
                    message,
                    ServerMessage::Reply { id, result: Ok(()) },
                    "{messages:?}"
                );
            }
            messages.push(message);
            if done {
                return messages;
            }
        }
    }

    /// Sends `command` and waits for its reply.
    fn request(&mut self, command: Command) -> Vec<ServerMessage> {
        let id = self.send(command);
        self.until_reply(id)
    }

    /// Waits until the player is idle: two no-op round trips in a row,
    /// some time apart, bring no event. Returns what arrived meanwhile.
    fn settle(&mut self) -> Vec<ServerMessage> {
        let mut seen = Vec::new();
        let mut quiet = 0;
        for _ in 0..200 {
            std::thread::sleep(Duration::from_millis(15));
            let got = self.request(NOOP);
            let events = got
                .iter()
                .filter(|m| matches!(m, ServerMessage::Event(_)))
                .count();
            seen.extend(got);
            quiet = if events == 0 { quiet + 1 } else { 0 };
            if quiet == 2 {
                return seen;
            }
        }
        panic!("the player never became idle");
    }
}

/// A client's copy of the player, built only from what it received.
#[derive(Debug, Default, Clone, PartialEq)]
struct View {
    snapshot: Option<PlayerSnapshot>,
    login_required: bool,
}

impl View {
    fn apply(&mut self, message: &ServerMessage) {
        match message {
            ServerMessage::Welcome {
                snapshot,
                login_required,
            } => {
                self.snapshot = Some(snapshot.clone());
                self.login_required = *login_required;
            }
            ServerMessage::Event(Event::Player(snapshot)) => self.snapshot = Some(snapshot.clone()),
            ServerMessage::Event(Event::Position { entry, position }) => {
                if let Some(s) = &mut self.snapshot
                    && s.current == Some(*entry)
                {
                    s.position = *position;
                }
            }
            ServerMessage::Event(Event::LoginRequired) => self.login_required = true,
            ServerMessage::Event(Event::LoginRestored) => self.login_required = false,
            ServerMessage::Event(Event::ShuttingDown)
            | ServerMessage::Reply { .. }
            | ServerMessage::LibraryReply { .. } => {}
        }
    }

    fn applied(mut self, messages: &[ServerMessage]) -> Self {
        for message in messages {
            self.apply(message);
        }
        self
    }
}

fn welcome(snapshot: PlayerSnapshot, login_required: bool) -> View {
    View {
        snapshot: Some(snapshot),
        login_required,
    }
}

fn events(messages: &[ServerMessage]) -> Vec<Event> {
    messages
        .iter()
        .filter_map(|m| match m {
            ServerMessage::Event(e) => Some(e.clone()),
            _ => None,
        })
        .collect()
}

fn tracks(ids: impl IntoIterator<Item = u64>) -> Vec<Track> {
    ids.into_iter().map(|id| track(id, Some(200))).collect()
}

/// 50 commands of every kind, some addressed to entries that may be gone.
fn run_of_50() -> Vec<Command> {
    let mut commands = vec![Command::LoadQueue {
        tracks: tracks(1..=8),
        start: 0,
    }];
    let cycle = |i: u64| match i % 12 {
        0 => Command::Next,
        1 => Command::ToggleShuffle,
        2 => Command::ChangeVolume(-7),
        3 => Command::AddToQueue {
            tracks: tracks([20 + i]),
            at: InsertAt::End,
        },
        4 => Command::CycleRepeat,
        5 => Command::Previous,
        6 => Command::TogglePause,
        7 => Command::RemoveFromQueue(EntryId(i / 2)),
        8 => Command::AddToQueue {
            tracks: tracks([40 + i, 41 + i]),
            at: InsertAt::Next,
        },
        9 => Command::PlayEntry(EntryId(i / 3 + 1)),
        10 => Command::ToggleMute,
        _ => Command::SeekTo(Duration::from_secs(i)),
    };
    commands.extend((0..49).map(cycle));
    commands
}

/// AC3: the player's first line on a new connection is the greeting.
#[test]
fn ac3_server_greets_first() {
    let player = TestPlayer::start();
    let (ours, mut theirs) = UnixStream::pair().unwrap();
    attach_stream(ours, &player.inputs).unwrap();
    theirs.set_read_timeout(Some(WAIT)).unwrap();
    let want = format!("{{\"tidal_player\":\"{}\"}}\n", env!("CARGO_PKG_VERSION"));
    let mut got = vec![0u8; want.len()];
    theirs.read_exact(&mut got).expect("the greeting");
    assert_eq!(String::from_utf8(got).unwrap(), want);
}

/// AC4: whenever a client subscribes (before, during or after a run of 50
/// commands), its `Welcome` plus the events after it give the player's
/// final state; a later subscriber's sequence is the earlier one's from
/// its `Welcome` on.
#[test]
fn ac4_subscribe_ordering() {
    #[derive(Debug, Clone, Copy, PartialEq)]
    enum When {
        Before,
        During,
        After,
    }
    for when in [When::Before, When::During, When::After] {
        let player = TestPlayer::start();
        let mut first = player.connect();
        let (snapshot, login) = first.subscribe();
        let first_welcome = welcome(snapshot, login);
        let mut sender = player.connect();
        let mut late = player.connect();

        let mut late_welcome = None;
        if when == When::Before {
            let (snapshot, login) = late.subscribe();
            late_welcome = Some(welcome(snapshot, login));
        }
        let mut ids = Vec::new();
        for (i, command) in run_of_50().into_iter().enumerate() {
            ids.push(sender.send(command));
            if i == 25 && when == When::During {
                // Pipelined: the player is still working through them.
                late.conn.send(&ClientMessage::Subscribe).unwrap();
            }
        }
        for id in ids {
            sender.until_reply(id);
        }
        let mut first_seen = first.settle();
        if when == When::After {
            let (snapshot, login) = late.subscribe();
            late_welcome = Some(welcome(snapshot, login));
        }
        let mut late_seen = late.request(NOOP);
        if when == When::During {
            let (snapshot, login) = match late_seen.remove(0) {
                ServerMessage::Welcome {
                    snapshot,
                    login_required,
                } => (snapshot, login_required),
                other => panic!("{when:?}: expected a Welcome first, got {other:?}"),
            };
            late_welcome = Some(welcome(snapshot, login));
        }
        first_seen.extend(first.request(NOOP));
        let late_welcome = late_welcome.expect("a Welcome");

        // Applied, each gives the player's state now.
        let (now, _) = player.snapshot();
        let first_view = first_welcome.clone().applied(&first_seen);
        let late_view = late_welcome.clone().applied(&late_seen);
        assert_eq!(first_view.snapshot.as_ref(), Some(&now), "{when:?}: first");
        assert_eq!(late_view.snapshot.as_ref(), Some(&now), "{when:?}: late");

        // The late sequence is the first one's tail, from the point where
        // the first client's state equals the late `Welcome`.
        let first_events = events(&first_seen);
        let late_events = events(&late_seen);
        assert!(
            first_events.ends_with(&late_events),
            "{when:?}: {} late events are not the tail of {} first ones",
            late_events.len(),
            first_events.len()
        );
        let split = first_events.len() - late_events.len();
        let head: Vec<ServerMessage> = first_events[..split]
            .iter()
            .cloned()
            .map(ServerMessage::Event)
            .collect();
        assert_eq!(
            first_welcome.applied(&head).snapshot,
            late_welcome.snapshot,
            "{when:?}"
        );
        match when {
            When::Before => assert_eq!(first_events, late_events),
            // Where it falls depends on the threads; the checks above hold
            // wherever it is.
            When::During => {}
            When::After => assert!(late_events.is_empty(), "{late_events:?}"),
        }
    }
}

/// splitmix64: a seeded, reproducible sequence.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// A random command of every queue, mode and volume kind, addressed to
/// the entries this client saw (which may be gone by now).
fn random_command(rng: &mut Rng, seen: &[EntryId]) -> Command {
    let some_tracks = |rng: &mut Rng| {
        let n = 1 + rng.below(4);
        tracks((0..n).map(|_| 1 + rng.below(30)).collect::<Vec<_>>())
    };
    let entry = |rng: &mut Rng| {
        if seen.is_empty() {
            EntryId(rng.below(50))
        } else {
            seen[rng.below(seen.len() as u64) as usize]
        }
    };
    match rng.below(17) {
        0 => {
            let tracks = some_tracks(rng);
            let start = rng.below(tracks.len() as u64 + 1) as usize;
            Command::LoadQueue { tracks, start }
        }
        1 => Command::AddToQueue {
            tracks: some_tracks(rng),
            at: InsertAt::End,
        },
        2 => Command::AddToQueue {
            tracks: some_tracks(rng),
            at: InsertAt::Next,
        },
        3 => Command::RemoveFromQueue(entry(rng)),
        4 => Command::ClearQueue,
        5 => Command::PlayEntry(entry(rng)),
        6 => Command::TogglePause,
        7 => Command::Next,
        8 => Command::Previous,
        9 => Command::SeekBy(rng.below(20_000) as i64 - 10_000),
        10 => Command::SeekTo(Duration::from_millis(rng.below(250_000))),
        11 => Command::ToggleShuffle,
        12 => Command::CycleRepeat,
        13 => Command::ToggleAutoplay,
        14 => Command::ChangeVolume(rng.below(61) as i8 - 30),
        15 => Command::SetVolume(rng.below(121) as u8),
        _ => Command::ToggleMute,
    }
}

/// A TUI-like client: the UI model, fed only by its connection.
struct ModelClient {
    client: Client,
    ui: ui::State,
}

impl ModelClient {
    fn new(player: &TestPlayer) -> Self {
        let mut client = player.connect();
        client.conn.send(&ClientMessage::Subscribe).unwrap();
        Self {
            client,
            ui: ui::State::default(),
        }
    }

    fn apply(&mut self, messages: Vec<ServerMessage>) {
        for message in messages {
            match message {
                ServerMessage::Welcome {
                    snapshot,
                    login_required,
                } => {
                    self.ui = ui::State {
                        login_required,
                        ..ui::State::default()
                    };
                    ui::update(&mut self.ui, ui::Action::Player(Event::Player(snapshot)));
                }
                ServerMessage::Event(event) => {
                    let effects = ui::update(&mut self.ui, ui::Action::Player(event));
                    assert!(effects.is_empty(), "{effects:?}");
                }
                ServerMessage::Reply { result, .. } => assert_eq!(result, Ok(())),
                ServerMessage::LibraryReply { id, .. } => panic!("unasked library reply {id}"),
            }
        }
    }
}

/// Prints the seed of a run that panics.
struct SeedGuard(u64);

impl Drop for SeedGuard {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!("ac4_clients_converge failed with seed {}", self.0);
        }
    }
}

/// AC4: 3 clients send random interleavings of every queue, mode and
/// volume command; one of them disconnects and resubscribes mid-run; once
/// the player is idle, every client's view equals the player's snapshot.
#[test]
fn ac4_clients_converge() {
    for seed in 0..200 {
        let _guard = SeedGuard(seed);
        let mut rng = Rng(seed);
        let player = TestPlayer::start();
        let mut probe = player.connect();
        probe.subscribe();
        let mut clients: Vec<ModelClient> = (0..3).map(|_| ModelClient::new(&player)).collect();
        let steps = 20 + rng.below(40);
        let reconnect_at = rng.below(steps);
        for step in 0..steps {
            if step == reconnect_at {
                // Leaves mid-run, unread messages and all, and comes back.
                clients[2].client.conn.close();
                clients[2] = ModelClient::new(&player);
            }
            let c = rng.below(3) as usize;
            let seen: Vec<EntryId> = clients[c].ui.queue().iter().map(|e| e.id).collect();
            let command = random_command(&mut rng, &seen);
            clients[c].client.send(command);
            for client in &mut clients {
                if rng.below(3) == 0 || step % 8 == 7 {
                    let got = client.client.drain();
                    client.apply(got);
                }
            }
        }
        probe.settle();
        let (now, login) = player.snapshot();
        for (i, client) in clients.iter_mut().enumerate() {
            let got = client.client.request(NOOP);
            client.apply(got);
            assert_eq!(
                client.ui.player.as_ref(),
                Some(&now),
                "seed {seed}: client {i} differs from the player"
            );
            assert_eq!(client.ui.login_required, login, "seed {seed}: client {i}");
        }
    }
}

fn volumes(messages: &[ServerMessage]) -> Vec<u8> {
    events(messages)
        .iter()
        .filter_map(|e| match e {
            Event::Player(s) => Some(s.volume),
            _ => None,
        })
        .collect()
}

/// AC5: one reply per request, after the events it caused; commands from
/// two clients applied in arrival order; both subscribers see the same
/// events.
#[test]
fn ac5_requests_replied_in_order() {
    let player = TestPlayer::start();
    let mut a = player.connect();
    let mut b = player.connect();
    let (start_a, _) = a.subscribe();
    let (start_b, _) = b.subscribe();
    assert_eq!(start_a, start_b);

    // Alternating, each waiting for its reply: applied exactly in that
    // order, the event before the reply.
    let (mut seen_a, mut seen_b) = (Vec::new(), Vec::new());
    let mut want = Vec::new();
    for i in 0..10u8 {
        for (from_a, volume) in [(true, 10 + i), (false, 50 + i)] {
            let (me, seen) = if from_a {
                (&mut a, &mut seen_a)
            } else {
                (&mut b, &mut seen_b)
            };
            let got = me.request(Command::SetVolume(volume));
            // The other client's event may come first; ours is last.
            assert_eq!(volumes(&got).last(), Some(&volume), "{got:?}");
            seen.extend(got);
            want.push(volume);
        }
    }
    seen_a.extend(a.request(NOOP));
    seen_b.extend(b.request(NOOP));
    assert_eq!(volumes(&seen_a), want);
    assert_eq!(events(&seen_a), events(&seen_b));

    // Pipelined from two threads: per client in its order, one reply per
    // id, each after its own event; both see the same events.
    let run = |mut client: Client, values: Vec<u8>| {
        std::thread::spawn(move || {
            let ids: Vec<(u64, u8)> = values
                .iter()
                .map(|v| (client.send(Command::SetVolume(*v)), *v))
                .collect();
            let mut seen = Vec::new();
            let mut replies: HashMap<u64, usize> = HashMap::new();
            while replies.len() < ids.len() {
                let message = client.recv();
                if let ServerMessage::Reply { id, result } = &message {
                    assert_eq!(result, &Ok(()));
                    *replies.entry(*id).or_default() += 1;
                    let volume = ids.iter().find(|(i, _)| i == id).expect("our id").1;
                    assert!(
                        volumes(&seen).contains(&volume),
                        "reply {id} before its event (volume {volume})"
                    );
                }
                seen.push(message);
            }
            (client, seen)
        })
    };
    let ta = run(a, (1..=30).collect());
    let tb = run(b, (31..=60).collect());
    let (mut a, mut seen_a) = ta.join().unwrap();
    let (mut b, mut seen_b) = tb.join().unwrap();
    seen_a.extend(a.request(NOOP));
    seen_b.extend(b.request(NOOP));
    let replies = |seen: &[ServerMessage]| {
        seen.iter()
            .filter(|m| matches!(m, ServerMessage::Reply { .. }))
            .count()
    };
    assert_eq!(replies(&seen_a), 31);
    assert_eq!(replies(&seen_b), 31);
    let order = volumes(&seen_a);
    assert_eq!(events(&seen_a), events(&seen_b));
    let mine = |range: std::ops::RangeInclusive<u8>| -> Vec<u8> {
        order
            .iter()
            .copied()
            .filter(|v| range.contains(v))
            .collect()
    };
    assert_eq!(mine(1..=30), (1..=30).collect::<Vec<u8>>());
    assert_eq!(mine(31..=60), (31..=60).collect::<Vec<u8>>());
    assert_eq!(order.len(), 60);
}

/// AC5: 1000 requests from 4 clients: 1000 replies, and the player saw
/// 1000 commands.
#[test]
fn ac5_nothing_dropped() {
    let player = TestPlayer::start();
    let mut watcher = player.connect();
    watcher.subscribe();
    let senders: Vec<_> = (0..4)
        .map(|_| {
            let mut client = player.connect();
            std::thread::spawn(move || {
                let ids: Vec<u64> = (0..250).map(|_| client.send(Command::ToggleMute)).collect();
                let mut replies: HashMap<u64, usize> = HashMap::new();
                for _ in 0..250 {
                    match client.recv() {
                        ServerMessage::Reply { id, result } => {
                            assert_eq!(result, Ok(()));
                            *replies.entry(id).or_default() += 1;
                        }
                        other => panic!("not subscribed, got {other:?}"),
                    }
                }
                assert!(
                    ids.iter().all(|id| replies.get(id) == Some(&1)),
                    "{replies:?}"
                );
                replies.len()
            })
        })
        .collect();
    let replied: usize = senders.into_iter().map(|t| t.join().unwrap()).sum();
    assert_eq!(replied, 1000);
    let seen = watcher.request(NOOP);
    let mutes: Vec<bool> = events(&seen)
        .iter()
        .filter_map(|e| match e {
            Event::Player(s) => Some(s.muted),
            _ => None,
        })
        .collect();
    assert_eq!(mutes.len(), 1000);
    assert!(mutes.iter().enumerate().all(|(i, m)| *m == (i % 2 == 0)));
    let gains = player
        .log
        .lock()
        .unwrap()
        .iter()
        .filter(|c| matches!(c, Call::SetGain(_)))
        .count();
    assert_eq!(gains, 1000);
}

/// AC6: a subscriber that never reads is disconnected once 1024 messages
/// wait for it; the others keep receiving, and the player keeps answering.
#[test]
fn ac6_stalled_client_disconnected() {
    // In the hub: exactly at the limit.
    let mut hub = Hub::default();
    let (stalled_tx, stalled_rx) = mpsc::sync_channel(OUTBOX);
    let (reader_tx, reader_rx) = mpsc::sync_channel(OUTBOX);
    hub.attach(ClientId(1), Peer::new(stalled_tx, None));
    hub.attach(ClientId(2), Peer::new(reader_tx, None));
    let (snapshot, _) = TestPlayer::start().snapshot();
    hub.subscribe(ClientId(1), snapshot.clone());
    hub.subscribe(ClientId(2), snapshot);
    let mut read = reader_rx.try_iter().count();
    for _ in 0..OUTBOX - 1 {
        hub.broadcast(&[Event::LoginRequired]);
        read += reader_rx.try_iter().count();
    }
    assert_eq!(hub.len(), 2, "1024 waiting: still connected");
    hub.broadcast(&[Event::LoginRestored]);
    read += reader_rx.try_iter().count();
    assert_eq!(hub.len(), 1, "the 1025th: disconnected");
    assert_eq!(stalled_rx.try_iter().count(), OUTBOX);
    assert!(matches!(
        stalled_rx.try_recv(),
        Err(mpsc::TryRecvError::Disconnected)
    ));
    assert_eq!(read, OUTBOX + 1);

    // Over sockets: the player answers another client within 1 s while
    // the stalled one is pending, then drops it.
    let player = TestPlayer::start();
    let mut control = player.connect();
    let titles: Vec<Track> = (1..=60)
        .map(|id| Track {
            title: format!("{id} {}", "a long title ".repeat(10)),
            ..track(id, Some(200))
        })
        .collect();
    control.request(Command::LoadQueue {
        tracks: titles,
        start: 0,
    });
    control.settle();
    let mut stalled = player.connect();
    stalled.subscribe();
    let mut watcher = player.connect();
    watcher.subscribe();
    const TOTAL: usize = 3000;
    let seen = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = std::sync::Arc::clone(&seen);
    let watching = std::thread::spawn(move || {
        let mut got = 0;
        while got < TOTAL {
            if let ServerMessage::Event(Event::Player(_)) = watcher.recv() {
                got += 1;
                counted.store(got, std::sync::atomic::Ordering::SeqCst);
            }
        }
        got
    });
    for batch in 0..TOTAL / 50 {
        // The watcher reads as it can; it is kept within reach of the
        // limit, which only the stalled client hits.
        let deadline = Instant::now() + WAIT;
        while seen.load(std::sync::atomic::Ordering::SeqCst) + 300 < batch * 50 {
            assert!(Instant::now() < deadline, "the watcher stopped reading");
            std::thread::sleep(Duration::from_millis(1));
        }
        let started = Instant::now();
        let ids: Vec<u64> = (0..50).map(|_| control.send(Command::ToggleMute)).collect();
        for id in ids {
            control.until_reply(id);
        }
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "batch {batch} took {:?}",
            started.elapsed()
        );
    }
    assert_eq!(watching.join().unwrap(), TOTAL);
    // The stalled client finds its connection closed after what reached
    // its socket.
    let mut got = 0;
    let closed = loop {
        match stalled.conn.recv(Some(WAIT)) {
            Ok(Some(_)) => got += 1,
            Ok(None) => break false,
            Err(RecvError::Closed) => break true,
            Err(e) => panic!("{e}"),
        }
    };
    assert!(closed, "still connected after {got} messages");
    assert!(got < TOTAL, "{got}");
}

/// AC6: a client that disconnects mid-line, sends invalid JSON, an
/// unknown message or an oversized line is dropped; the player and the
/// others carry on.
#[test]
fn ac6_bad_clients_dropped() {
    type Misbehave = fn(&mut UnixStream);
    let rows: [(&str, Misbehave, bool); 4] = [
        (
            "disconnects mid-line",
            |s| {
                s.write_all(b"{\"Request\":{\"id\":1,").unwrap();
                s.shutdown(std::net::Shutdown::Both).unwrap();
            },
            false,
        ),
        ("invalid JSON", |s| s.write_all(b"{]\n").unwrap(), true),
        (
            "unknown message",
            |s| s.write_all(b"\"Unsubscribe\"\n").unwrap(),
            true,
        ),
        (
            "oversized line",
            |s| {
                let mut s = s.try_clone().unwrap();
                // Refused once past 16 MiB; the player then closes it.
                std::thread::spawn(move || {
                    let chunk = vec![b' '; 1024 * 1024];
                    for _ in 0..17 {
                        if s.write_all(&chunk).is_err() {
                            break;
                        }
                    }
                })
                .join()
                .unwrap();
            },
            true,
        ),
    ];
    for (name, misbehave, dropped_by_player) in rows {
        let player = TestPlayer::start();
        let mut good = player.connect();
        good.subscribe();
        let mut bad = player.connect();
        bad.subscribe();
        let mut raw = bad.conn.stream().try_clone().unwrap();
        misbehave(&mut raw);
        if dropped_by_player {
            let closed = loop {
                match bad.conn.recv(Some(WAIT)) {
                    Ok(Some(_)) => {}
                    Ok(None) => break false,
                    Err(_) => break true,
                }
            };
            assert!(closed, "{name}: not dropped");
        }
        let got = good.request(Command::SetVolume(42));
        assert_eq!(volumes(&got), vec![42], "{name}: {got:?}");
        let (now, _) = player.snapshot();
        assert_eq!(now.volume, 42, "{name}");
    }
}

/// AC8: the authenticator's status changes reach every subscriber as
/// `LoginRequired`/`LoginRestored`; `Welcome` carries the current status.
#[test]
fn ac8_login_status() {
    let tokio = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let event = |s: AuthStatus| match s {
        AuthStatus::Active => Event::LoginRestored,
        AuthStatus::LoginRequired => Event::LoginRequired,
    };
    // The forwarding task runs on the test's runtime, driven here.
    let wait_for = |client: &mut Client, want: Event| {
        let deadline = Instant::now() + WAIT;
        loop {
            tokio.block_on(async { tokio::time::sleep(Duration::from_millis(5)).await });
            if events(&client.drain()).contains(&want) {
                return;
            }
            assert!(Instant::now() < deadline, "no {want:?}");
        }
    };

    for initial in [AuthStatus::Active, AuthStatus::LoginRequired] {
        let other = match initial {
            AuthStatus::Active => AuthStatus::LoginRequired,
            AuthStatus::LoginRequired => AuthStatus::Active,
        };
        let player = TestPlayer::start();
        let (status, status_rx) = tokio::sync::watch::channel(initial);
        forward_login(status_rx, player.inputs.clone(), tokio.handle());
        let mut a = player.connect();
        let (_, login) = a.subscribe();
        assert_eq!(login, initial == AuthStatus::LoginRequired, "{initial:?}");

        status.send(other).unwrap();
        wait_for(&mut a, event(other));
        let mut b = player.connect();
        let (_, login) = b.subscribe();
        assert_eq!(login, other == AuthStatus::LoginRequired, "{initial:?}");

        status.send(initial).unwrap();
        wait_for(&mut a, event(initial));
        wait_for(&mut b, event(initial));
        // Unchanged: nothing is sent.
        status.send(initial).unwrap();
        tokio.block_on(async { tokio::time::sleep(Duration::from_millis(30)).await });
        assert_eq!(events(&a.request(NOOP)), vec![], "{initial:?}");
    }
}

fn library_replies(messages: &[ServerMessage]) -> Vec<&ServerMessage> {
    messages
        .iter()
        .filter(|m| matches!(m, ServerMessage::LibraryReply { .. }))
        .collect()
}

/// AC8: the reply goes to the sender only, once, with the request's `id`;
/// the player passes its settings along.
#[test]
fn ac8_reply_to_sender() {
    let library = Arc::new(FakeLibrary::default());
    let player = TestPlayer::start_with_library(&library);
    let mut a = player.connect();
    let mut b = player.connect();
    a.subscribe();
    b.subscribe();

    let first = a.library("one");
    let second = a.library("two");
    assert_eq!(a.library_reply(), (first, Ok(LibraryResponse::Done)));
    assert_eq!(a.library_reply(), (second, Ok(LibraryResponse::Done)));
    // Everything the player sent before this reply has arrived at `b`:
    // no library reply among it, nor after.
    let got = b.request(NOOP);
    assert_eq!(
        library_replies(&got),
        Vec::<&ServerMessage>::new(),
        "{got:?}"
    );
    assert_eq!(library_replies(&b.drain()).len(), 0);
    // Exactly one reply each: nothing more for `a` either.
    let got = a.request(NOOP);
    assert_eq!(library_replies(&got).len(), 0, "{got:?}");

    let settings = crate::player_runtime::LibrarySettings::default();
    let want = |label: &str| {
        (
            delete(label),
            settings.page_size,
            settings.hidden_words.clone(),
        )
    };
    assert_eq!(library.seen(), vec![want("one"), want("two")]);
}

/// Spec 0007 AC4: a search page and a `More` on each search list are
/// answered like any 0006 request: one reply, to the sender only; the
/// search page gets the search page size, everything else the page size.
#[test]
fn ac4_search_reply_to_sender() {
    use tidal_player_core::library::{ListRef, PageRequest};

    let library = Arc::new(FakeLibrary::default());
    let player = TestPlayer::start_with_library(&library);
    let mut a = player.connect();
    let mut b = player.connect();
    a.subscribe();
    b.subscribe();

    let q = "pierce the veil".to_owned();
    let more = |list: ListRef| LibraryRequest::More {
        list,
        offset: 20,
        limit: 20,
    };
    let requests = vec![
        LibraryRequest::Page(PageRequest::Search(q.clone())),
        more(ListRef::SearchTracks(q.clone())),
        more(ListRef::SearchAlbums(q.clone())),
        more(ListRef::SearchArtists(q.clone())),
        more(ListRef::SearchPlaylists(q.clone())),
        LibraryRequest::Page(PageRequest::Library),
    ];
    for request in &requests {
        let id = a.ask_library(request.clone());
        assert_eq!(
            a.library_reply(),
            (id, Ok(LibraryResponse::Done)),
            "{request:?}"
        );
    }
    // Nothing for `b`, nothing more for `a`.
    let got = b.request(NOOP);
    assert_eq!(
        library_replies(&got),
        Vec::<&ServerMessage>::new(),
        "{got:?}"
    );
    assert_eq!(library_replies(&b.drain()).len(), 0);
    let got = a.request(NOOP);
    assert_eq!(library_replies(&got).len(), 0, "{got:?}");

    let settings = crate::player_runtime::LibrarySettings::default();
    assert_ne!(settings.page_size, settings.search_page_size);
    let want: Vec<_> = requests
        .iter()
        .map(|request| {
            let size = match request {
                LibraryRequest::Page(PageRequest::Search(_)) => settings.search_page_size,
                _ => settings.page_size,
            };
            (request.clone(), size, settings.hidden_words.clone())
        })
        .collect();
    assert_eq!(library.seen(), want);
}

/// AC8: a failing library's message is the `Err`, unchanged.
#[test]
fn ac8_errors() {
    let library = Arc::new(FakeLibrary::default());
    let table = [
        ("plain", "Playlist not found"),
        ("empty", ""),
        (
            "long",
            "Tidal said: 429 Too Many Requests; try again in a minute",
        ),
        ("unicode", "Kein Zugriff – bitte erneut anmelden ✓"),
    ];
    for (label, message) in table {
        library.fail(label, message);
    }
    let player = TestPlayer::start_with_library(&library);
    let mut client = player.connect();
    client.subscribe();
    for (label, message) in table {
        let id = client.library(label);
        assert_eq!(
            client.library_reply(),
            (id, Err(message.to_owned())),
            "{label}"
        );
    }
    // Success is not an error.
    let id = client.library("fine");
    assert_eq!(client.library_reply(), (id, Ok(LibraryResponse::Done)));
}

/// AC8: a held request delays neither commands nor events nor another
/// client; one client's requests run one at a time, in order.
#[test]
fn ac8_does_not_block() {
    let library = Arc::new(FakeLibrary::default());
    let release_a1 = library.hold("a1");
    let player = TestPlayer::start_with_library(&library);
    let mut a = player.connect();
    let mut b = player.connect();
    a.subscribe();
    b.subscribe();

    let a1 = a.library("a1");
    let a2 = a.library("a2");
    // Commands are answered (and events flow) while a1 is held.
    let before = Instant::now();
    let got = a.request(Command::LoadQueue {
        tracks: tracks(1..=2),
        start: 0,
    });
    let got_toggle = a.request(Command::TogglePause);
    assert!(
        before.elapsed() < Duration::from_secs(1),
        "{:?}",
        before.elapsed()
    );
    assert!(
        !events(&got).is_empty() && !events(&got_toggle).is_empty(),
        "{got:?} {got_toggle:?}"
    );
    assert!(library_replies(&got).is_empty() && library_replies(&got_toggle).is_empty());
    // The other client's request completes meanwhile.
    let b1 = b.library("b1");
    assert_eq!(b.library_reply(), (b1, Ok(LibraryResponse::Done)));
    // a2 has not started: it waits for a1.
    assert_eq!(library.started(), vec!["a1", "b1"]);

    release_a1.send(()).unwrap();
    assert_eq!(a.library_reply(), (a1, Ok(LibraryResponse::Done)));
    assert_eq!(a.library_reply(), (a2, Ok(LibraryResponse::Done)));
    assert_eq!(library.started(), vec!["a1", "b1", "a2"]);
}

/// AC8: a client that leaves with a request out leaves the player (and the
/// others) unaffected; its reply is dropped.
#[test]
fn ac8_disconnect_mid_request() {
    let library = Arc::new(FakeLibrary::default());
    let release = library.hold("gone");
    let player = TestPlayer::start_with_library(&library);
    let mut a = player.connect();
    let mut b = player.connect();
    a.subscribe();
    b.subscribe();

    a.library("gone");
    a.library("queued");
    // Wait until the player has the first one out.
    let deadline = Instant::now() + WAIT;
    while library.started().is_empty() {
        assert!(Instant::now() < deadline, "the request never started");
        std::thread::sleep(Duration::from_millis(5));
    }
    a.conn.close();
    drop(a);
    // The player handles the detach (the socket close is noticed on
    // another thread, with no reply to wait for).
    std::thread::sleep(Duration::from_millis(150));
    b.request(NOOP);

    release.send(()).unwrap();
    // The player still serves everyone.
    let id = b.library("after");
    assert_eq!(b.library_reply(), (id, Ok(LibraryResponse::Done)));
    b.request(Command::TogglePause);
    // The leaver's second request never ran.
    assert_eq!(library.started(), vec!["gone", "after"]);
    let _ = LibraryRequest::DeletePlaylist {
        uuid: String::new(),
    };
}
