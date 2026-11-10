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
use crate::player::runtime::RuntimeInput;

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
            ClientMessage::Devices { id } => ClientInput::Devices { client, id },
            ClientMessage::Logs { id, after } => ClientInput::Logs { client, id, after },
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

    /// Asks the player for its log lines after `after` (spec 0027); the
    /// answer comes back as `Action::LogsReply` with `id`.
    pub fn send_logs(&mut self, id: u64, after: Option<u64>) {
        self.write(&ClientMessage::Logs { id, after });
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
                Ok(Some(ServerMessage::DevicesReply { id, result })) => {
                    actions.push(Action::DevicesReply { id, result })
                }
                Ok(Some(ServerMessage::LogsReply { id, result })) => {
                    actions.push(Action::LogsReply { id, result })
                }
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
mod tests;
