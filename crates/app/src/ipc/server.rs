//! The player's side of the socket (spec 0005 "Messages" and "Sync").
//!
//! Each client gets a reader thread and a writer thread. The reader decodes
//! the client's lines into [`ClientInput`]s on the runtime's one input
//! channel, so subscribing, requests and detaching are ordered with every
//! other input on the player thread. The player thread keeps the [`Hub`]:
//! a `Welcome` is the snapshot at the moment the subscription is handled,
//! and every event after it goes to every subscriber, in order, once. The
//! writer thread drains the client's outbox (at most [`OUTBOX`] messages)
//! into the socket; a client whose outbox is full is disconnected rather
//! than skipped over or waited for.
//!
//! Library requests (spec 0006 AC8) are jobs: the player thread only queues
//! them per client and starts the next one when the last was answered, so a
//! slow request never delays a command or an event.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::Shutdown;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use tidal_player_api::auth::AuthStatus;
use tidal_player_core::library::{LibraryRequest, LibraryResponse};
use tidal_player_core::protocol::{
    ClientMessage, Command, DeviceEntry, Event, PlayerSnapshot, ServerMessage,
};
use tokio::sync::watch;

use super::codec::{Decoder, encode, greeting};
use crate::player_runtime::RuntimeInput;

/// The messages a client may have waiting before it is disconnected.
pub const OUTBOX: usize = 1024;

/// A connected client, numbered in the order of connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ClientId(pub u64);

/// A fresh client ID.
pub fn next_client_id() -> ClientId {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    ClientId(NEXT.fetch_add(1, Ordering::Relaxed))
}

/// How the player thread reaches one client.
#[derive(Debug)]
pub struct Peer {
    outbox: SyncSender<ServerMessage>,
    /// Shut down on disconnect (unblocks a writer stuck on a full socket).
    stream: Option<UnixStream>,
}

impl Peer {
    /// A client fed through `outbox` (whose capacity is the limit), and
    /// whose `stream`, if any, is shut down when it is disconnected.
    pub fn new(outbox: SyncSender<ServerMessage>, stream: Option<UnixStream>) -> Self {
        Self { outbox, stream }
    }

    fn close(&self) {
        if let Some(stream) = &self.stream {
            let _ = stream.shutdown(Shutdown::Both);
        }
    }
}

/// What a client's threads tell the player thread.
#[derive(Debug)]
pub enum ClientInput {
    /// A client connected (sent before anything it reads).
    Attach {
        client: ClientId,
        peer: Peer,
    },
    Subscribe(ClientId),
    Request {
        client: ClientId,
        id: u64,
        command: Command,
    },
    /// A library request (spec 0006 AC8), answered by one `LibraryReply`.
    Library {
        client: ClientId,
        id: u64,
        request: LibraryRequest,
    },
    /// A device list request (spec 0014 AC7), answered by one
    /// `DevicesReply` to this client only.
    Devices {
        client: ClientId,
        id: u64,
    },
    /// The client left, or was dropped for a bad line.
    Detach(ClientId),
}

#[derive(Debug)]
struct Client {
    peer: Peer,
    subscribed: bool,
}

/// The clients, as the player thread keeps them.
#[derive(Debug, Default)]
pub struct Hub {
    clients: BTreeMap<ClientId, Client>,
    login_required: bool,
}

impl Hub {
    pub fn attach(&mut self, client: ClientId, peer: Peer) {
        self.clients.insert(
            client,
            Client {
                peer,
                subscribed: false,
            },
        );
    }

    pub fn detach(&mut self, client: ClientId) {
        if let Some(gone) = self.clients.remove(&client) {
            gone.peer.close();
        }
    }

    /// The connected clients.
    pub fn len(&self) -> usize {
        self.clients.len()
    }

    pub fn is_empty(&self) -> bool {
        self.clients.is_empty()
    }

    /// Answers `Subscribe` with `snapshot` (the state now); every later
    /// event follows.
    pub fn subscribe(&mut self, client: ClientId, snapshot: PlayerSnapshot) {
        let welcome = ServerMessage::Welcome {
            snapshot,
            login_required: self.login_required,
        };
        if self.send(client, welcome)
            && let Some(c) = self.clients.get_mut(&client)
        {
            c.subscribed = true;
        }
    }

    /// Sends `events` to every subscriber, in order.
    pub fn broadcast(&mut self, events: &[Event]) {
        if events.is_empty() {
            return;
        }
        let subscribers: Vec<ClientId> = self
            .clients
            .iter()
            .filter(|(_, c)| c.subscribed)
            .map(|(id, _)| *id)
            .collect();
        for client in subscribers {
            for event in events {
                if !self.send(client, ServerMessage::Event(event.clone())) {
                    break;
                }
            }
        }
    }

    /// Answers request `id` of `client`.
    pub fn reply(&mut self, client: ClientId, id: u64, result: Result<(), String>) {
        self.send(client, ServerMessage::Reply { id, result });
    }

    /// Answers library request `id` of `client` (it alone gets the reply; a
    /// client that left gets nothing).
    pub fn reply_library(
        &mut self,
        client: ClientId,
        id: u64,
        result: Result<LibraryResponse, String>,
    ) {
        self.send(client, ServerMessage::LibraryReply { id, result });
    }

    /// Answers device list request `id` of `client` (it alone gets it).
    pub fn reply_devices(
        &mut self,
        client: ClientId,
        id: u64,
        result: Result<Vec<DeviceEntry>, String>,
    ) {
        self.send(client, ServerMessage::DevicesReply { id, result });
    }

    /// Whether `client` is connected.
    pub fn contains(&self, client: ClientId) -> bool {
        self.clients.contains_key(&client)
    }

    /// The login status `Welcome` carries.
    pub fn login_required(&self) -> bool {
        self.login_required
    }

    /// Records the authenticator's status; the event to broadcast when it
    /// changed.
    pub fn set_login_required(&mut self, required: bool) -> Option<Event> {
        if required == self.login_required {
            return None;
        }
        self.login_required = required;
        Some(if required {
            Event::LoginRequired
        } else {
            Event::LoginRestored
        })
    }

    /// Queues `message` for `client`; a full or closed outbox disconnects
    /// it (`false`).
    fn send(&mut self, client: ClientId, message: ServerMessage) -> bool {
        let Some(c) = self.clients.get(&client) else {
            return false;
        };
        match c.peer.outbox.try_send(message) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) => {
                tracing::warn!(client = client.0, "client not reading: disconnected");
                self.detach(client);
                false
            }
            Err(TrySendError::Disconnected(_)) => {
                self.detach(client);
                false
            }
        }
    }
}

/// Serves one connected client: sends the greeting, then its outbox, and
/// feeds what it sends to the player's `inputs`.
pub fn attach_stream(
    stream: UnixStream,
    inputs: &Sender<RuntimeInput>,
) -> std::io::Result<ClientId> {
    attach(stream, inputs).map(|(client, _)| client)
}

/// [`attach_stream`], with the client's writer thread: it ends once the
/// outbox is closed (the client detached, or the player is gone) and
/// everything in it was written.
fn attach(
    stream: UnixStream,
    inputs: &Sender<RuntimeInput>,
) -> std::io::Result<(ClientId, JoinHandle<()>)> {
    let client = next_client_id();
    let (outbox, messages) = mpsc::sync_channel::<ServerMessage>(OUTBOX);
    let mut writer = stream.try_clone()?;
    let mut reader = stream.try_clone()?;
    let peer = Peer::new(outbox, Some(stream));

    let writing = std::thread::Builder::new()
        .name(format!("client-{}-out", client.0))
        .spawn(move || {
            if writer.write_all(&greeting()).is_err() {
                return;
            }
            for message in messages {
                if writer.write_all(&encode(&message)).is_err() {
                    break;
                }
            }
            let _ = writer.shutdown(Shutdown::Write);
        })?;

    // Attached before anything it sends can arrive.
    let _ = inputs.send(RuntimeInput::Client(ClientInput::Attach { client, peer }));
    let inputs = inputs.clone();
    std::thread::Builder::new()
        .name(format!("client-{}-in", client.0))
        .spawn(move || {
            let mut decoder = Decoder::<ClientMessage>::new();
            let mut buf = vec![0u8; 64 * 1024];
            'read: loop {
                let n = match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => n,
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => break,
                };
                for message in decoder.feed(&buf[..n]) {
                    let input = match message {
                        Ok(ClientMessage::Subscribe) => ClientInput::Subscribe(client),
                        Ok(ClientMessage::Request { id, command }) => ClientInput::Request {
                            client,
                            id,
                            command,
                        },
                        Ok(ClientMessage::Library { id, request }) => ClientInput::Library {
                            client,
                            id,
                            request,
                        },
                        Ok(ClientMessage::Devices { id }) => ClientInput::Devices { client, id },
                        Err(e) => {
                            tracing::warn!(client = client.0, "dropped: {e}");
                            break 'read;
                        }
                    };
                    if inputs.send(RuntimeInput::Client(input)).is_err() {
                        break 'read;
                    }
                }
            }
            let _ = inputs.send(RuntimeInput::Client(ClientInput::Detach(client)));
        })?;
    Ok((client, writing))
}

/// How long a stopping server waits for its clients' last messages (the
/// `ShuttingDown`) to be written.
pub const FLUSH: Duration = Duration::from_secs(1);

/// Accepts clients on the player's socket until dropped; then the socket
/// file is removed. Drop it after the player: it waits (at most
/// [`FLUSH`]) until each client got what the player sent it last.
#[derive(Debug)]
pub struct Server {
    path: PathBuf,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    writers: Arc<Mutex<Vec<JoinHandle<()>>>>,
}

/// Serves `listener` (bound at `path`) for the player reading `inputs`.
pub fn serve(listener: UnixListener, path: PathBuf, inputs: Sender<RuntimeInput>) -> Server {
    let stop = Arc::new(AtomicBool::new(false));
    let stopping = Arc::clone(&stop);
    let writers: Arc<Mutex<Vec<JoinHandle<()>>>> = Arc::default();
    let serving = Arc::clone(&writers);
    let thread = std::thread::Builder::new()
        .name("socket".into())
        .spawn(move || {
            for stream in listener.incoming() {
                if stopping.load(Ordering::SeqCst) {
                    break;
                }
                match stream {
                    Ok(stream) => match attach(stream, &inputs) {
                        Ok((_, writer)) => {
                            let mut writers = serving.lock().unwrap_or_else(|e| e.into_inner());
                            writers.retain(|w| !w.is_finished());
                            writers.push(writer);
                        }
                        Err(e) => tracing::warn!("cannot serve a client: {e}"),
                    },
                    Err(e) => tracing::warn!("accept failed: {e}"),
                }
            }
        })
        .expect("spawn the socket thread");
    Server {
        path,
        stop,
        thread: Some(thread),
        writers,
    }
}

impl Server {
    /// The socket file.
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Wakes the accept loop, which then sees `stop`.
        let _ = UnixStream::connect(&self.path);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        let writers = std::mem::take(&mut *self.writers.lock().unwrap_or_else(|e| e.into_inner()));
        let deadline = Instant::now() + FLUSH;
        while Instant::now() < deadline && !writers.iter().all(JoinHandle::is_finished) {
            std::thread::sleep(Duration::from_millis(5));
        }
        for writer in writers.into_iter().filter(JoinHandle::is_finished) {
            let _ = writer.join();
        }
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Forwards the authenticator's status to the player (spec 0005 "The
/// daemon": `LoginRequired`/`LoginRestored` to every subscriber), starting
/// with the current one.
pub fn forward_login(
    mut status: watch::Receiver<AuthStatus>,
    inputs: Sender<RuntimeInput>,
    runtime: &tokio::runtime::Handle,
) {
    let send = move |status: AuthStatus| {
        inputs
            .send(RuntimeInput::Login {
                required: status == AuthStatus::LoginRequired,
            })
            .is_ok()
    };
    let first = *status.borrow_and_update();
    if !send(first) {
        return;
    }
    runtime.spawn(async move {
        while status.changed().await.is_ok() {
            let now = *status.borrow_and_update();
            if !send(now) {
                break;
            }
        }
    });
}

#[cfg(test)]
mod tests;
