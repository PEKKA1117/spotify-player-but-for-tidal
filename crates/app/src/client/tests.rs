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
    crate::player::runtime::fakes::delete(label)
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
    use crate::player::runtime::fakes::{FakeEngine, FakeJobs, FakeLibrary, Log, Script};
    use crate::player::runtime::{PlayerRuntime, spawn_runtime};
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
