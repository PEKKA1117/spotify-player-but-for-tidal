//! A client of the player (spec 0005 "Roles", "The TUI as a client"):
//! finding the player, the connection a TUI keeps to it (reconnecting
//! every second), and the in-process connection a standalone TUI uses to
//! its own player, with the same messages.
//!
//! "Sync": a client holds no playback state of its own. [`Session`] only
//! carries messages: commands go out as requests, and the screen changes
//! when the player's `Welcome`/events come back (as [`Action`]s for the
//! UI model).

use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::time::{Duration, Instant};

use tidal_player_core::Item;
use tidal_player_core::library::LibraryRequest;
use tidal_player_core::protocol::{ClientMessage, Command, Event, InsertAt, ServerMessage};
use tidal_player_core::ui::Action;

use crate::ipc::client::{ConnectError, Connection, RecvError};
use crate::ipc::lock::{self, Probe};
use crate::ipc::paths::{
    DirMeta, RuntimeDirError, SOCKET_NAME, check_private, current_uid, runtime_dir,
};
use crate::ipc::server::{ClientId, ClientInput, OUTBOX, Peer, next_client_id};
use crate::player_runtime::RuntimeInput;

/// How long a lost connection waits before the next attempt.
pub const RECONNECT: Duration = Duration::from_secs(1);
/// How long a client keeps trying a player that holds the lock but does
/// not answer yet (it may still be starting).
pub const CONNECT_RETRY: Duration = Duration::from_secs(2);
/// The pause between two of those attempts.
const RETRY_STEP: Duration = Duration::from_millis(50);
/// The most messages one [`Session::poll`] takes (a frame stays short).
const POLL_BATCH: usize = 4096;

/// What a client says when no player is running.
pub const NO_PLAYER: &str =
    "No player is running: start \"tidal-player\" or \"tidal-player daemon\"";

// --- connections -------------------------------------------------------------------

/// One connection to a player, past its greeting.
pub trait Link {
    fn send(&mut self, message: &ClientMessage) -> io::Result<()>;
    /// The next message, waiting at most `timeout` (`None`: no limit; zero:
    /// only what has arrived). `Ok(None)`: none in time.
    fn recv(&mut self, timeout: Option<Duration>) -> Result<Option<ServerMessage>, RecvError>;
    /// Leaves the player (it detaches this client).
    fn close(&mut self);
}

impl Link for Connection {
    fn send(&mut self, message: &ClientMessage) -> io::Result<()> {
        Connection::send(self, message)
    }

    fn recv(&mut self, timeout: Option<Duration>) -> Result<Option<ServerMessage>, RecvError> {
        Connection::recv(self, timeout)
    }

    fn close(&mut self) {
        Connection::close(self);
    }
}

/// Why no connection was made.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConnectFailure {
    /// The player refused this client (a version mismatch, or not a
    /// tidal-player socket): never retried.
    #[error("{0}")]
    Refused(String),
    /// Nothing answered (yet).
    #[error("{0}")]
    Unavailable(String),
}

impl From<ConnectError> for ConnectFailure {
    fn from(error: ConnectError) -> Self {
        match error {
            ConnectError::Greeting(e) => Self::Refused(e.to_string()),
            other => Self::Unavailable(other.to_string()),
        }
    }
}

/// Makes connections to one player.
pub trait Connector {
    type Link: Link;
    fn connect(&mut self) -> Result<Self::Link, ConnectFailure>;
}

/// Connects to the socket at a path.
#[derive(Debug, Clone)]
pub struct SocketConnector {
    path: PathBuf,
}

impl SocketConnector {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl Connector for SocketConnector {
    type Link = Connection;

    fn connect(&mut self) -> Result<Connection, ConnectFailure> {
        Connection::connect(&self.path).map_err(ConnectFailure::from)
    }
}

/// A standalone TUI's connection to its own player (0001's in-process
/// join): the same messages as over the socket, through the player's input
/// channel and an outbox of [`OUTBOX`] messages.
#[derive(Debug, Clone)]
pub struct InProcess {
    inputs: Sender<RuntimeInput>,
}

impl InProcess {
    pub fn new(inputs: Sender<RuntimeInput>) -> Self {
        Self { inputs }
    }
}

/// One in-process connection.
#[derive(Debug)]
pub struct InProcessLink {
    client: ClientId,
    inputs: Sender<RuntimeInput>,
    messages: Receiver<ServerMessage>,
}

impl Connector for InProcess {
    type Link = InProcessLink;

    fn connect(&mut self) -> Result<InProcessLink, ConnectFailure> {
        let client = next_client_id();
        let (outbox, messages) = mpsc::sync_channel::<ServerMessage>(OUTBOX);
        let attach = ClientInput::Attach {
            client,
            peer: Peer::new(outbox, None),
        };
        self.inputs
            .send(RuntimeInput::Client(attach))
            .map_err(|_| ConnectFailure::Unavailable(STOPPED.to_owned()))?;
        Ok(InProcessLink {
            client,
            inputs: self.inputs.clone(),
            messages,
        })
    }
}

impl Link for InProcessLink {
    fn send(&mut self, message: &ClientMessage) -> io::Result<()> {
        let client = self.client;
        let input = match message.clone() {
            ClientMessage::Subscribe => ClientInput::Subscribe(client),
            ClientMessage::Request { id, command } => ClientInput::Request {
                client,
                id,
                command,
            },
            ClientMessage::Library { id, request } => ClientInput::Library {
                client,
                id,
                request,
            },
            // 0014 slice C
            ClientMessage::Devices { .. } => return Ok(()),
        };
        self.inputs
            .send(RuntimeInput::Client(input))
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, STOPPED))
    }

    fn recv(&mut self, timeout: Option<Duration>) -> Result<Option<ServerMessage>, RecvError> {
        match timeout {
            None => self
                .messages
                .recv()
                .map(Some)
                .map_err(|_| RecvError::Closed),
            Some(t) if t.is_zero() => match self.messages.try_recv() {
                Ok(message) => Ok(Some(message)),
                Err(TryRecvError::Empty) => Ok(None),
                Err(TryRecvError::Disconnected) => Err(RecvError::Closed),
            },
            Some(t) => match self.messages.recv_timeout(t) {
                Ok(message) => Ok(Some(message)),
                Err(RecvTimeoutError::Timeout) => Ok(None),
                Err(RecvTimeoutError::Disconnected) => Err(RecvError::Closed),
            },
        }
    }

    fn close(&mut self) {
        let detach = ClientInput::Detach(self.client);
        let _ = self.inputs.send(RuntimeInput::Client(detach));
    }
}

/// Why an in-process link failed: the player thread has ended.
const STOPPED: &str = "The player has stopped";

// --- finding the player --------------------------------------------------------------

/// Why a client found no player to talk to (exit 1).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FindError {
    #[error("{NO_PLAYER}")]
    NoPlayer,
    #[error("A player is running{} but not answering on {}", pid_suffix(*.pid), .socket.display())]
    NotAnswering { pid: Option<u32>, socket: PathBuf },
    #[error("{0}")]
    Refused(String),
    #[error("{0}")]
    Other(String),
}

fn pid_suffix(pid: Option<u32>) -> String {
    pid.map(|pid| format!(" (pid {pid})")).unwrap_or_default()
}

/// Time, for the retries (a fake in tests).
pub trait Clock {
    fn now(&self) -> Instant;
    fn sleep(&mut self, duration: Duration);
}

/// The real clock.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }

    fn sleep(&mut self, duration: Duration) {
        std::thread::sleep(duration);
    }
}

/// Connects to a player that holds the lock (with `pid`, when known),
/// trying again for [`CONNECT_RETRY`] while it does not answer.
pub fn retry_connect<L>(
    connect: &mut dyn FnMut() -> Result<L, ConnectFailure>,
    clock: &mut dyn Clock,
    pid: Option<u32>,
    socket: &Path,
) -> Result<L, FindError> {
    let deadline = clock.now() + CONNECT_RETRY;
    loop {
        match connect() {
            Ok(link) => return Ok(link),
            Err(ConnectFailure::Refused(message)) => return Err(FindError::Refused(message)),
            Err(ConnectFailure::Unavailable(_)) => {}
        }
        if clock.now() >= deadline {
            return Err(FindError::NotAnswering {
                pid,
                socket: socket.to_owned(),
            });
        }
        clock.sleep(RETRY_STEP);
    }
}

/// Finds the running player: connects first, and only when that fails
/// looks at the lock (a probe can be in the way of a starting player, so
/// it is never the first step).
pub fn find_player<L>(
    connect: &mut dyn FnMut() -> Result<L, ConnectFailure>,
    probe: &mut dyn FnMut() -> io::Result<Probe>,
    clock: &mut dyn Clock,
    socket: &Path,
) -> Result<L, FindError> {
    match connect() {
        Ok(link) => return Ok(link),
        Err(ConnectFailure::Refused(message)) => return Err(FindError::Refused(message)),
        Err(ConnectFailure::Unavailable(_)) => {}
    }
    match probe() {
        Ok(Probe::Free) => Err(FindError::NoPlayer),
        Ok(Probe::Held { pid }) => retry_connect(connect, clock, pid, socket),
        Err(e) => Err(FindError::Other(e.to_string())),
    }
}

/// The runtime directory and the socket in it, for a client. An existing
/// directory must be private (spec 0005 "Transport"), or a client would
/// talk to whoever made it; a missing one means no player.
pub fn locate(env: impl Fn(&str) -> Option<String>) -> Result<(PathBuf, PathBuf), FindError> {
    let uid =
        current_uid().map_err(|e| FindError::Other(format!("Cannot tell this user's ID: {e}")))?;
    let dir = runtime_dir(env, uid);
    match DirMeta::of(&dir) {
        Ok(meta) => check_private(meta, uid).map_err(|reason| {
            FindError::Other(
                RuntimeDirError::NotPrivate {
                    path: dir.clone(),
                    reason,
                }
                .to_string(),
            )
        })?,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(FindError::Other(format!("{}: {e}", dir.display()))),
    }
    let socket = dir.join(SOCKET_NAME);
    Ok((dir, socket))
}

/// Connects to this user's player (one-shot commands): the connection
/// and the runtime directory.
pub fn find(env: impl Fn(&str) -> Option<String>) -> Result<(Connection, PathBuf), FindError> {
    let (dir, socket) = locate(env)?;
    let connection = find_player(
        &mut || Connection::connect(&socket).map_err(ConnectFailure::from),
        &mut || lock::probe(&dir),
        &mut SystemClock,
        &socket,
    )?;
    Ok((connection, dir))
}

/// The `Open` a client sends first: the command-line items, at the
/// place the flags say (`None`: replace the queue); nothing without items.
pub fn startup_open(items: Vec<Item>, at: Option<InsertAt>) -> Option<Command> {
    (!items.is_empty()).then_some(Command::Open { items, at })
}

// --- the session ----------------------------------------------------------------------

/// A TUI's connection to the player, kept across losses: it reconnects
/// every [`RECONNECT`] and subscribes again; the `Welcome` then replaces
/// the client's state.
pub struct Session<C: Connector> {
    connector: C,
    link: Option<C::Link>,
    /// A send failed: the link is gone (reported by the next poll).
    lost: bool,
    next_id: u64,
    /// When to try again while disconnected.
    retry_at: Option<Instant>,
    /// Refused by the player: never retried.
    refused: bool,
}

impl<C: Connector> Session<C> {
    /// Joined over `link`: sends `open` first (when given), then
    /// subscribes.
    pub fn new(connector: C, link: C::Link, open: Option<Command>) -> Self {
        let mut session = Self {
            connector,
            link: Some(link),
            lost: false,
            next_id: 0,
            retry_at: None,
            refused: false,
        };
        if let Some(command) = open {
            session.send(command);
        }
        session.write(&ClientMessage::Subscribe);
        session
    }

    /// Sends `command` to the player (nothing while disconnected: the UI
    /// model sends none then).
    pub fn send(&mut self, command: Command) {
        let id = self.next_id;
        self.next_id += 1;
        self.write(&ClientMessage::Request { id, command });
    }

    /// Sends a library request (spec 0006 AC19); its answer comes back from
    /// [`Self::poll`] as `Action::LibraryReply` with the same `id`.
    pub fn send_library(&mut self, id: u64, request: LibraryRequest) {
        self.write(&ClientMessage::Library { id, request });
    }

    /// Asks the player for its output devices (spec 0014); the answer
    /// comes back as `Action::DevicesReply` with `id`.
    pub fn send_devices(&mut self, id: u64) {
        self.write(&ClientMessage::Devices { id });
    }

    fn write(&mut self, message: &ClientMessage) {
        if let Some(link) = self.link.as_mut()
            && link.send(message).is_err()
        {
            self.lost = true;
        }
    }

    /// Leaves the player: the next attempt is at `now` + [`RECONNECT`].
    fn drop_link(&mut self, now: Instant) {
        if let Some(mut link) = self.link.take() {
            link.close();
        }
        self.lost = false;
        self.retry_at = Some(now + RECONNECT);
    }

    /// What arrived since the last poll, as UI actions; while
    /// disconnected, tries to reconnect when it is time.
    pub fn poll(&mut self, now: Instant) -> Vec<Action> {
        let mut actions = Vec::new();
        if self.link.is_none() {
            self.reconnect(now, &mut actions);
            return actions;
        }
        for _ in 0..POLL_BATCH {
            let Some(link) = self.link.as_mut() else {
                break;
            };
            if self.lost {
                self.drop_link(now);
                actions.push(Action::Disconnected { shut_down: false });
                break;
            }
            match link.recv(Some(Duration::ZERO)) {
                Ok(None) => break,
                Ok(Some(ServerMessage::Welcome {
                    snapshot,
                    login_required,
                })) => actions.push(Action::Welcome {
                    snapshot,
                    login_required,
                }),
                Ok(Some(ServerMessage::Event(Event::ShuttingDown))) => {
                    self.drop_link(now);
                    actions.push(Action::Disconnected { shut_down: true });
                    break;
                }
                Ok(Some(ServerMessage::Event(event))) => actions.push(Action::Player(event)),
                Ok(Some(ServerMessage::Reply { result, .. })) => {
                    actions.push(Action::Reply(result))
                }
                Ok(Some(ServerMessage::LibraryReply { id, result })) => {
                    actions.push(Action::LibraryReply { id, result })
                }
                // 0014 slice C
                Ok(Some(ServerMessage::DevicesReply { .. })) => {}
                Err(_) => self.lost = true,
            }
        }
        actions
    }

    fn reconnect(&mut self, now: Instant, actions: &mut Vec<Action>) {
        if self.refused || self.retry_at.is_some_and(|at| now < at) {
            return;
        }
        match self.connector.connect() {
            Ok(link) => {
                self.link = Some(link);
                self.retry_at = None;
                self.write(&ClientMessage::Subscribe);
            }
            Err(ConnectFailure::Refused(message)) => {
                self.refused = true;
                actions.push(Action::Refused(message));
            }
            Err(ConnectFailure::Unavailable(_)) => self.retry_at = Some(now + RECONNECT),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::rc::Rc;

    use super::*;
    use tidal_player_core::protocol::{PlaybackState, PlayerSnapshot, QueueEntry, RepeatMode};
    use tidal_player_core::ui::{self, Key, State};
    use tidal_player_core::{AlbumRef, ArtistRef, EntryId, Track, TrackId};

    /// What a fake link was sent, and what it will receive.
    #[derive(Default)]
    struct Wire {
        sent: Vec<ClientMessage>,
        incoming: VecDeque<Result<ServerMessage, RecvError>>,
        closed: bool,
    }

    type Shared = Rc<RefCell<Wire>>;

    struct FakeLink(Shared);

    impl Link for FakeLink {
        fn send(&mut self, message: &ClientMessage) -> io::Result<()> {
            let mut wire = self.0.borrow_mut();
            if wire.closed {
                return Err(io::Error::from(io::ErrorKind::BrokenPipe));
            }
            wire.sent.push(message.clone());
            Ok(())
        }

        fn recv(&mut self, _: Option<Duration>) -> Result<Option<ServerMessage>, RecvError> {
            match self.0.borrow_mut().incoming.pop_front() {
                Some(Ok(message)) => Ok(Some(message)),
                Some(Err(e)) => Err(e),
                None => Ok(None),
            }
        }

        fn close(&mut self) {
            self.0.borrow_mut().closed = true;
        }
    }

    /// Answers each attempt from a script (then: unavailable), counting.
    #[derive(Default)]
    struct FakeConnector {
        script: VecDeque<Result<Shared, ConnectFailure>>,
        attempts: Rc<RefCell<usize>>,
    }

    impl Connector for FakeConnector {
        type Link = FakeLink;

        fn connect(&mut self) -> Result<FakeLink, ConnectFailure> {
            *self.attempts.borrow_mut() += 1;
            match self.script.pop_front() {
                Some(Ok(wire)) => Ok(FakeLink(wire)),
                Some(Err(e)) => Err(e),
                None => Err(ConnectFailure::Unavailable("no socket".into())),
            }
        }
    }

    fn track(id: u64) -> Track {
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
            duration: Some(Duration::from_secs(200)),
            streamable: true,
        }
    }

    fn snapshot(ids: &[u64]) -> PlayerSnapshot {
        PlayerSnapshot {
            queue: ids
                .iter()
                .map(|id| QueueEntry {
                    id: EntryId(*id),
                    track: track(id + 100),
                    suggested: false,
                })
                .collect(),
            current: ids.first().copied().map(EntryId),
            state: PlaybackState::Playing,
            position: Duration::ZERO,
            shuffle: false,
            repeat: RepeatMode::Off,
            autoplay: false,
            volume: 100,
            muted: false,
            now_playing: None,
            message: None,
            device: "default".into(),
        }
    }

    fn welcome(ids: &[u64]) -> ServerMessage {
        ServerMessage::Welcome {
            snapshot: snapshot(ids),
            login_required: false,
        }
    }

    fn ids(state: &State) -> Vec<u64> {
        state.queue().iter().map(|e| e.id.0).collect()
    }

    /// AC11: with items, the client sends `Open` first (at the flags'
    /// place), then subscribes; without items, it only subscribes.
    #[test]
    fn ac11_startup_open() {
        let items = vec![Item::Album(10), Item::Track(TrackId(3))];
        for at in [None, Some(InsertAt::End), Some(InsertAt::Next)] {
            let wire = Shared::default();
            let open = startup_open(items.clone(), at);
            Session::new(FakeConnector::default(), FakeLink(Rc::clone(&wire)), open);
            assert_eq!(
                wire.borrow().sent,
                vec![
                    ClientMessage::Request {
                        id: 0,
                        command: Command::Open {
                            items: items.clone(),
                            at,
                        },
                    },
                    ClientMessage::Subscribe,
                ],
                "{at:?}"
            );
        }
        let wire = Shared::default();
        let open = startup_open(Vec::new(), Some(InsertAt::End));
        assert_eq!(open, None);
        Session::new(FakeConnector::default(), FakeLink(Rc::clone(&wire)), open);
        assert_eq!(wire.borrow().sent, vec![ClientMessage::Subscribe]);
    }

    /// A clock that only moves when slept on (shared, so a fake connect
    /// can read it).
    #[derive(Clone)]
    struct FakeClock {
        now: Rc<std::cell::Cell<Instant>>,
        start: Instant,
    }

    impl FakeClock {
        fn new() -> Self {
            let start = Instant::now();
            Self {
                now: Rc::new(std::cell::Cell::new(start)),
                start,
            }
        }

        fn elapsed(&self) -> Duration {
            self.now.get() - self.start
        }
    }

    impl Clock for FakeClock {
        fn now(&self) -> Instant {
            self.now.get()
        }

        fn sleep(&mut self, duration: Duration) {
            self.now.set(self.now.get() + duration);
        }
    }

    /// AC11: a player that holds the lock but does not answer yet is tried
    /// for 2 s, then `A player is running (pid N) but not answering`; one
    /// that answers within them is joined; a refusal is not retried. And
    /// `find_player` connects before it ever probes the lock.
    #[test]
    fn ac11_connect_retry() {
        let socket = Path::new("/run/user/1000/tidal-player/player.sock");

        // Never answers.
        let mut clock = FakeClock::new();
        let mut attempts = 0;
        let got = retry_connect::<()>(
            &mut || {
                attempts += 1;
                Err(ConnectFailure::Unavailable("refused".into()))
            },
            &mut clock,
            Some(42),
            socket,
        );
        let error = got.expect_err("joined a player that never answered");
        assert_eq!(
            error.to_string(),
            "A player is running (pid 42) but not answering on \
             /run/user/1000/tidal-player/player.sock"
        );
        assert!(
            clock.elapsed() >= CONNECT_RETRY,
            "gave up after {:?}",
            clock.elapsed()
        );
        assert!(clock.elapsed() < CONNECT_RETRY + Duration::from_millis(200));
        assert!(attempts > 10, "{attempts} attempts");

        // Answers after a second.
        let mut clock = FakeClock::new();
        let seen = clock.clone();
        let got = retry_connect(
            &mut || {
                if seen.elapsed() >= Duration::from_secs(1) {
                    Ok("joined")
                } else {
                    Err(ConnectFailure::Unavailable("refused".into()))
                }
            },
            &mut clock,
            Some(42),
            socket,
        );
        assert_eq!(got, Ok("joined"));
        let took = clock.elapsed();
        assert!(
            took >= Duration::from_secs(1) && took < CONNECT_RETRY,
            "{took:?}"
        );

        // Refused: at once.
        let mut clock = FakeClock::new();
        let got = retry_connect::<()>(
            &mut || Err(ConnectFailure::Refused("The running player is …".into())),
            &mut clock,
            Some(42),
            socket,
        );
        assert_eq!(
            got,
            Err(FindError::Refused("The running player is …".into()))
        );
        assert_eq!(clock.elapsed(), Duration::ZERO);

        // `find_player`: a socket that answers is joined without a probe;
        // a free lock is "no player"; a held one is retried.
        let mut clock = FakeClock::new();
        let mut probes = 0;
        let got = find_player(
            &mut || Ok("joined"),
            &mut || {
                probes += 1;
                Ok(Probe::Free)
            },
            &mut clock,
            socket,
        );
        assert_eq!((got, probes), (Ok("joined"), 0));
        let got = find_player::<()>(
            &mut || Err(ConnectFailure::Unavailable("refused".into())),
            &mut || Ok(Probe::Free),
            &mut clock,
            socket,
        );
        assert_eq!(got, Err(FindError::NoPlayer));
        assert_eq!(
            FindError::NoPlayer.to_string(),
            "No player is running: start \"tidal-player\" or \"tidal-player daemon\""
        );
        let mut clock = FakeClock::new();
        let got = find_player::<()>(
            &mut || Err(ConnectFailure::Unavailable("refused".into())),
            &mut || Ok(Probe::Held { pid: Some(7) }),
            &mut clock,
            socket,
        );
        assert_eq!(
            got,
            Err(FindError::NotAnswering {
                pid: Some(7),
                socket: socket.to_owned(),
            })
        );
        assert!(clock.elapsed() >= CONNECT_RETRY);
    }

    /// Feeds a session's actions to the UI model.
    fn apply(state: &mut State, actions: Vec<Action>) {
        for action in actions {
            ui::update(state, action);
        }
    }

    /// AC13: a lost connection is reported, tried again every second,
    /// subscribed on success, and the next `Welcome` replaces the state.
    #[test]
    fn ac13_reconnect() {
        let first = Shared::default();
        first.borrow_mut().incoming.push_back(Ok(welcome(&[1, 2])));
        let second = Shared::default();
        let mut connector = FakeConnector::default();
        connector
            .script
            .push_back(Err(ConnectFailure::Unavailable("refused".into())));
        connector.script.push_back(Ok(Rc::clone(&second)));
        let attempts = Rc::clone(&connector.attempts);
        let mut session = Session::new(connector, FakeLink(Rc::clone(&first)), None);
        let mut state = State::default();
        let t0 = Instant::now();
        apply(&mut state, session.poll(t0));
        assert_eq!(ids(&state), vec![1, 2]);

        // The connection drops: disconnected, the snapshot kept.
        first
            .borrow_mut()
            .incoming
            .push_back(Err(RecvError::Closed));
        apply(&mut state, session.poll(t0));
        assert!(state.reconnecting(), "{:?}", state.connection);
        assert_eq!(ids(&state), vec![1, 2]);
        assert_eq!(*attempts.borrow(), 0, "tried again at once");

        // Not before a second; then once per second.
        let ms = |n| t0 + Duration::from_millis(n);
        apply(&mut state, session.poll(ms(500)));
        assert_eq!(*attempts.borrow(), 0, "tried before a second");
        apply(&mut state, session.poll(ms(1000)));
        assert_eq!(*attempts.borrow(), 1);
        apply(&mut state, session.poll(ms(1500)));
        assert_eq!(*attempts.borrow(), 1, "tried twice within a second");
        apply(&mut state, session.poll(ms(2000)));
        assert_eq!(*attempts.borrow(), 2);
        // Joined: subscribed, and the `Welcome` replaces everything.
        assert_eq!(second.borrow().sent, vec![ClientMessage::Subscribe]);
        assert!(state.reconnecting(), "connected before the Welcome");
        second.borrow_mut().incoming.push_back(Ok(welcome(&[9])));
        apply(&mut state, session.poll(ms(2100)));
        assert_eq!(ids(&state), vec![9]);
        assert!(!state.reconnecting());
        assert_eq!(state.message(), None);

        // `ShuttingDown`: "the player shut down", then reconnecting.
        second
            .borrow_mut()
            .incoming
            .push_back(Ok(ServerMessage::Event(Event::ShuttingDown)));
        apply(&mut state, session.poll(ms(3000)));
        assert_eq!(state.message(), Some(tidal_player_core::ui::SHUT_DOWN));
        assert!(second.borrow().closed, "the old connection stays open");
        apply(&mut state, session.poll(ms(4000)));
        assert_eq!(*attempts.borrow(), 3);
        assert_eq!(state.message(), Some(tidal_player_core::ui::SHUT_DOWN));

        // A refusal (version mismatch): shown, never tried again.
        let mut connector = FakeConnector::default();
        connector
            .script
            .push_back(Err(ConnectFailure::Refused("mismatch".into())));
        let attempts = Rc::clone(&connector.attempts);
        let wire = Shared::default();
        wire.borrow_mut().incoming.push_back(Err(RecvError::Closed));
        let mut session = Session::new(connector, FakeLink(wire), None);
        let mut state = State::default();
        apply(&mut state, session.poll(t0));
        apply(&mut state, session.poll(ms(1000)));
        assert_eq!(state.message(), Some("mismatch"));
        for n in 2..10 {
            apply(&mut state, session.poll(ms(n * 1000)));
        }
        assert_eq!(*attempts.borrow(), 1, "a refusal was retried");
    }

    /// AC13: the client never shows a queue edit the player did not send
    /// (tidalt #6): an `o` add goes out as `Open`, and the queue changes
    /// only with the player's snapshot.
    #[test]
    fn ac13_no_local_edits() {
        let wire = Shared::default();
        wire.borrow_mut().incoming.push_back(Ok(welcome(&[1])));
        let mut session = Session::new(FakeConnector::default(), FakeLink(Rc::clone(&wire)), None);
        let mut state = State::default();
        apply(&mut state, session.poll(Instant::now()));
        wire.borrow_mut().sent.clear();

        let mut effects = Vec::new();
        for key in [Key::Char('o'), Key::Char('3'), Key::Enter] {
            effects.extend(ui::update(&mut state, Action::Key(key)));
        }
        for effect in effects {
            if let ui::Effect::Send(command) = effect {
                session.send(command);
            }
        }
        assert_eq!(
            wire.borrow().sent,
            vec![ClientMessage::Request {
                id: 0,
                command: Command::Open {
                    items: vec![Item::Track(TrackId(3))],
                    at: Some(InsertAt::End),
                },
            }]
        );
        // Nothing local: the queue is the player's until it says more.
        apply(&mut state, session.poll(Instant::now()));
        assert_eq!(ids(&state), vec![1]);
        // The reply alone does not add it either.
        wire.borrow_mut()
            .incoming
            .push_back(Ok(ServerMessage::Reply {
                id: 0,
                result: Ok(()),
            }));
        apply(&mut state, session.poll(Instant::now()));
        assert_eq!(ids(&state), vec![1]);
        // The player's snapshot does.
        wire.borrow_mut()
            .incoming
            .push_back(Ok(ServerMessage::Event(Event::Player(snapshot(&[1, 2])))));
        apply(&mut state, session.poll(Instant::now()));
        assert_eq!(ids(&state), vec![1, 2]);
    }

    /// A request the fakes answer: `Done`, or `Err` for `fail`.
    fn deleting(label: &str) -> LibraryRequest {
        crate::player_runtime::fakes::delete(label)
    }

    use tidal_player_core::library::LibraryResponse;

    /// The player's answer to a library request, as the session hands it on.
    #[derive(Debug, Clone, PartialEq, Eq)]
    struct LibraryReply {
        id: u64,
        result: Result<LibraryResponse, String>,
    }

    /// Polls `session` until `want` library replies came (or time is up).
    fn library_replies<C: Connector>(session: &mut Session<C>, want: usize) -> Vec<LibraryReply> {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut got = Vec::new();
        while got.len() < want && Instant::now() < deadline {
            for action in session.poll(Instant::now()) {
                if let Action::LibraryReply { id, result } = action {
                    got.push(LibraryReply { id, result });
                }
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        got
    }

    /// AC19: `send_library` sends `ClientMessage::Library`; a
    /// `LibraryReply` comes out of the session as `Action::LibraryReply`.
    #[test]
    fn ac19_library_on_the_link() {
        let wire = Shared::default();
        let mut session = Session::new(FakeConnector::default(), FakeLink(Rc::clone(&wire)), None);
        session.send_library(4, deleting("x"));
        assert_eq!(
            wire.borrow().sent.last(),
            Some(&ClientMessage::Library {
                id: 4,
                request: deleting("x"),
            })
        );
        wire.borrow_mut()
            .incoming
            .push_back(Ok(ServerMessage::LibraryReply {
                id: 4,
                result: Err("no".into()),
            }));
        assert_eq!(
            session.poll(Instant::now()),
            vec![Action::LibraryReply {
                id: 4,
                result: Err("no".into()),
            }]
        );
        assert!(session.poll(Instant::now()).is_empty());
    }

    /// AC19: a request goes to the player and its reply comes back with
    /// its `id`, over the socket and in-process.
    #[test]
    fn ac19_library_round_trip() {
        use crate::ipc::client::Connection;
        use crate::ipc::server::attach_stream;
        use crate::player_runtime::fakes::{FakeEngine, FakeJobs, FakeLibrary, Log, Script};
        use crate::player_runtime::{PlayerRuntime, spawn_runtime};
        use std::sync::Arc;
        use tidal_player_core::player::PlayerConfig;

        let library = Arc::new(FakeLibrary::default());
        library.fail("bad", "Playlist not found");
        let log: Log = Log::default();
        let (tx, rx) = mpsc::channel();
        let engine = FakeEngine::new(&log, Script::Plays);
        let jobs = FakeJobs::new(&log, &tx, true).with_library(library.clone());
        let runtime = PlayerRuntime::new(PlayerConfig::default(), 7, engine, jobs);
        let _player = spawn_runtime(runtime, rx, tx.clone());

        // Over a socket pair.
        let (ours, theirs) = std::os::unix::net::UnixStream::pair().unwrap();
        attach_stream(ours, &tx).unwrap();
        let link = Connection::handshake(theirs, Path::new("<pair>")).unwrap();
        let connector = SocketConnector::new(PathBuf::from("<pair>"));
        let mut socket = Session::new(connector, link, None);
        socket.send_library(1, deleting("ok"));
        socket.send_library(2, deleting("bad"));
        assert_eq!(
            library_replies(&mut socket, 2),
            vec![
                LibraryReply {
                    id: 1,
                    result: Ok(LibraryResponse::Done),
                },
                LibraryReply {
                    id: 2,
                    result: Err("Playlist not found".into()),
                },
            ]
        );

        // In-process.
        let mut connector = InProcess::new(tx.clone());
        let link = connector.connect().unwrap();
        let mut inner = Session::new(connector, link, None);
        inner.send_library(9, deleting("bad"));
        inner.send_library(10, deleting("ok"));
        assert_eq!(
            library_replies(&mut inner, 2),
            vec![
                LibraryReply {
                    id: 9,
                    result: Err("Playlist not found".into()),
                },
                LibraryReply {
                    id: 10,
                    result: Ok(LibraryResponse::Done),
                },
            ]
        );
        // Each client got only its own.
        assert!(library_replies(&mut socket, 1).is_empty());
    }
}
