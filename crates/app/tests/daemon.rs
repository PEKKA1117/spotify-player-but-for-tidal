//! Spec 0005, end to end with the binary: one player per user (AC10),
//! clients (AC11, AC14–AC16) and the daemon (AC17).
//! Each test has its own `TIDAL_PLAYER_RUNTIME_DIR`, a session in a temp
//! state dir, a mock API (debug builds honour `TIDAL_PLAYER_API_BASE`), no
//! session bus and no audio device: the daemon opens none before
//! something plays.

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime};

use tidal_player::client::NO_PLAYER;
use tidal_player::ipc::client::{Connection, RecvError};
use tidal_player::ipc::lock::{Probe, probe};
use tidal_player::passphrase::{PassphraseEnv, ProcessPassphrase, Prompt};
use tidal_player::session_store::EncryptedFileStore;
use tidal_player_api::auth::{Session, SessionStore};
use tidal_player_core::library::{LibraryRequest, PageRequest};
use tidal_player_core::protocol::{
    ClientMessage, Command as PlayerCommand, Event, PlayerSnapshot, ServerMessage,
};

/// Metadata for the clients' `Open`s: track 1001 exists, album 404 does
/// not (the API's own fixtures).
async fn mount_metadata(api: &wiremock::MockServer) {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, ResponseTemplate};
    let fixture = |name: &str| {
        let file = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../api/tests/fixtures/metadata")
            .join(name);
        serde_json::from_slice::<serde_json::Value>(&std::fs::read(file).unwrap()).unwrap()
    };
    Mock::given(method("GET"))
        .and(path("/tracks/1001"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("track.json")))
        .mount(api)
        .await;
    // 0007 AC13: any search answers the probe's "pierce the veil".
    let search =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../api/tests/fixtures/search/search_page.json");
    let search: serde_json::Value =
        serde_json::from_slice(&std::fs::read(search).unwrap()).unwrap();
    Mock::given(method("GET"))
        .and(path("/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(search))
        .mount(api)
        .await;
    Mock::given(method("GET"))
        .and(path("/albums/404/tracks"))
        .respond_with(
            ResponseTemplate::new(404).set_body_json(fixture("error_album_not_found_2001.json")),
        )
        .mount(api)
        .await;
}

struct NoPrompt;

impl Prompt for NoPrompt {
    fn ask(&mut self, _prompt: &str) -> std::io::Result<String> {
        Err(std::io::Error::other("no prompt in tests"))
    }
}

/// A user's machine: state dir with a session, runtime dir, mock API.
struct Machine {
    state: tempfile::TempDir,
    /// A state dir with nothing in it, for clients (AC14).
    empty: tempfile::TempDir,
    run: PathBuf,
    pass_file: PathBuf,
    _tokio: tokio::runtime::Runtime,
    api: String,
    /// The mock API, for the requests it received (0008 AC15).
    server: std::sync::Arc<wiremock::MockServer>,
}

impl Machine {
    fn new() -> Self {
        let state = tempfile::tempdir().unwrap();
        let pass_file = state.path().join("test-passphrase");
        std::fs::write(&pass_file, "test-passphrase\n").unwrap();
        let source = ProcessPassphrase::new(
            PassphraseEnv {
                passphrase_file: Some(pass_file.clone()),
                credentials_dir: None,
                interactive: false,
            },
            Box::new(NoPrompt),
        );
        EncryptedFileStore::new(
            state.path().join("session.age"),
            std::sync::Arc::new(source),
        )
        .save(&Session {
            access_token: "FAKE-ACCESS".into(),
            refresh_token: "FAKE-REFRESH".into(),
            expires_at: SystemTime::now() + Duration::from_secs(3600),
            user_id: 7,
            country_code: "NO".into(),
        })
        .unwrap();
        let tokio = tokio::runtime::Runtime::new().unwrap();
        let api = std::sync::Arc::new(tokio.block_on(wiremock::MockServer::start()));
        tokio.block_on(mount_metadata(&api));
        let uri = api.uri();
        let server = std::sync::Arc::clone(&api);
        // The server lives as long as the runtime's task keeps it.
        tokio.spawn(async move {
            let _api = api;
            std::future::pending::<()>().await;
        });
        let run = state.path().join("run");
        Self {
            state,
            empty: tempfile::tempdir().unwrap(),
            run,
            pass_file,
            _tokio: tokio,
            api: uri,
            server,
        }
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_tidal-player"));
        cmd.args(args)
            .env("TIDAL_PLAYER_STATE_DIR", self.state.path())
            .env("TIDAL_PLAYER_RUNTIME_DIR", &self.run)
            .env("TIDAL_PLAYER_NO_KEYRING", "1")
            .env("TIDAL_PLAYER_PASSPHRASE_FILE", &self.pass_file)
            .env("TIDAL_PLAYER_API_BASE", &self.api)
            .env("DBUS_SESSION_BUS_ADDRESS", "unix:path=/nonexistent/bus")
            .env_remove("CREDENTIALS_DIRECTORY")
            .env_remove("TIDAL_PLAYER_QUALITY")
            .env_remove("TIDAL_PLAYER_DEVICE")
            .env_remove("NOTIFY_SOCKET")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        cmd
    }

    fn spawn(&self, args: &[&str]) -> Running {
        Running(self.command(args).spawn().unwrap())
    }

    /// A client's invocation: the same runtime dir, but no session, no
    /// keyring and no passphrase (AC14).
    fn client(&self, args: &[&str]) -> Command {
        let mut cmd = self.command(args);
        cmd.env("TIDAL_PLAYER_STATE_DIR", self.empty.path())
            .env_remove("TIDAL_PLAYER_PASSPHRASE_FILE")
            .env_remove("TIDAL_PLAYER_API_BASE");
        cmd
    }

    /// Runs a client to the end: exit code, stdout and stderr.
    fn run_client(&self, args: &[&str]) -> (Option<i32>, String, String) {
        Running(self.client(args).spawn().unwrap()).output(Duration::from_secs(20))
    }

    /// Runs to the end: exit code and stderr.
    fn run(&self, args: &[&str]) -> (Option<i32>, String) {
        let mut running = self.spawn(args);
        running.finish(Duration::from_secs(20))
    }

    /// Every request the mock API has received so far.
    fn requests(&self) -> Vec<wiremock::Request> {
        self._tokio
            .block_on(self.server.received_requests())
            .unwrap_or_default()
    }

    /// Waits at most 10 s for a request to the mock API that `pred`
    /// accepts.
    fn wait_request(
        &self,
        what: &str,
        pred: impl Fn(&wiremock::Request) -> bool,
    ) -> wiremock::Request {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(request) = self.requests().into_iter().find(|r| pred(r)) {
                return request;
            }
            assert!(
                Instant::now() < deadline,
                "no request for {what} within 10 s: {:?}",
                self.requests()
                    .iter()
                    .map(|r| r.url.to_string())
                    .collect::<Vec<_>>()
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn socket(&self) -> PathBuf {
        self.run.join("player.sock")
    }

    fn create_run_dir(&self) {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&self.run)
            .unwrap();
    }
}

/// A child killed when dropped.
struct Running(Child);

impl Running {
    fn pid(&self) -> u32 {
        self.0.id()
    }

    /// Waits at most `limit` for it to exit: its code and stderr.
    fn finish(&mut self, limit: Duration) -> (Option<i32>, String) {
        let deadline = Instant::now() + limit;
        let status = loop {
            if let Some(status) = self.0.try_wait().unwrap() {
                break status;
            }
            assert!(Instant::now() < deadline, "still running after {limit:?}");
            std::thread::sleep(Duration::from_millis(20));
        };
        let mut stderr = String::new();
        self.0
            .stderr
            .take()
            .unwrap()
            .read_to_string(&mut stderr)
            .unwrap();
        (status.code(), stderr)
    }

    /// Waits at most `limit` for it to exit: its code, stdout and stderr.
    fn output(&mut self, limit: Duration) -> (Option<i32>, String, String) {
        let mut stdout = self.0.stdout.take().unwrap();
        let reader = std::thread::spawn(move || {
            let mut text = String::new();
            stdout.read_to_string(&mut text).unwrap();
            text
        });
        let (code, stderr) = self.finish(limit);
        (code, reader.join().unwrap(), stderr)
    }

    fn running(&mut self) -> bool {
        self.0.try_wait().unwrap().is_none()
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Connects to the player at `socket` (retrying while it starts) and
/// subscribes: the `Welcome` must come.
fn attach(socket: &Path) -> Connection {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut conn = loop {
        match Connection::connect(socket) {
            Ok(conn) => break conn,
            Err(e) => {
                assert!(Instant::now() < deadline, "cannot connect: {e}");
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    };
    conn.send(&ClientMessage::Subscribe).unwrap();
    match conn.recv(Some(Duration::from_secs(5))) {
        Ok(Some(ServerMessage::Welcome { snapshot, .. })) => {
            assert!(snapshot.queue.is_empty(), "{snapshot:?}");
        }
        other => panic!("expected a Welcome, got {other:?}"),
    }
    conn
}

/// Holds `player.lock` the way a player does: locked, with a pid in it.
fn hold_lock(dir: &Path, pid: u32) -> File {
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(dir.join("player.lock"))
        .unwrap();
    file.try_lock().unwrap();
    writeln!(file, "{pid}").unwrap();
    file
}

const PLAY_HINT: &str = ": use \"tidal-player playback load\"";

/// AC10: with the lock held by another process, `daemon` and `play` exit
/// 3 and say who holds it (`play` with its hint).
#[test]
fn ac10_second_player_exits_3() {
    // Held by this test process.
    let machine = Machine::new();
    machine.create_run_dir();
    let me = std::process::id();
    let lock = hold_lock(&machine.run, me);
    let held = format!("Another player is running (pid {me})");
    assert_eq!(machine.run(&["daemon"]), (Some(3), format!("{held}\n")));
    assert_eq!(
        machine.run(&["play", "123"]),
        (Some(3), format!("{held}{PLAY_HINT}\n"))
    );
    drop(lock);

    // Held by a running daemon.
    let machine = Machine::new();
    let first = machine.spawn(&["daemon"]);
    let _client = attach(&machine.socket());
    let held = format!("Another player is running (pid {})", first.pid());
    assert_eq!(machine.run(&["daemon"]), (Some(3), format!("{held}\n")));
    assert_eq!(
        machine.run(&["play", "123"]),
        (Some(3), format!("{held}{PLAY_HINT}\n"))
    );
    // The first one is unaffected.
    attach(&machine.socket());
}

/// AC10: a crashed player's `player.sock`, with the lock free, is removed
/// and the new player listens.
#[test]
fn ac10_stale_socket_removed() {
    let machine = Machine::new();
    machine.create_run_dir();
    drop(UnixListener::bind(machine.socket()).unwrap());
    assert!(machine.socket().exists());
    assert!(
        std::os::unix::net::UnixStream::connect(machine.socket()).is_err(),
        "the leftover socket answers"
    );
    let mut daemon = machine.spawn(&["daemon"]);
    attach(&machine.socket());
    assert!(daemon.running());
}

/// AC10: two daemons started at once: exactly one keeps running, the other
/// exits 3 naming it.
#[test]
fn ac10_race_one_wins() {
    for _ in 0..3 {
        let machine = Machine::new();
        let mut a = machine.spawn(&["daemon"]);
        let mut b = machine.spawn(&["daemon"]);
        let deadline = Instant::now() + Duration::from_secs(20);
        let (loser, mut winner) = loop {
            match (a.running(), b.running()) {
                (true, true) => {
                    assert!(Instant::now() < deadline, "neither exited");
                    std::thread::sleep(Duration::from_millis(20));
                }
                (false, true) => break (a, b),
                (true, false) => break (b, a),
                (false, false) => panic!(
                    "both exited: {:?}, {:?}",
                    a.finish(Duration::ZERO),
                    b.finish(Duration::ZERO)
                ),
            }
        };
        let mut loser = loser;
        assert_eq!(
            loser.finish(Duration::ZERO),
            (
                Some(3),
                format!("Another player is running (pid {})\n", winner.pid())
            )
        );
        attach(&machine.socket());
        assert!(winner.running());
    }
}

/// The next snapshot event `pred` accepts (others are skipped); panics
/// after 10 s.
fn wait_snapshot(
    conn: &mut Connection,
    what: &str,
    pred: impl Fn(&PlayerSnapshot) -> bool,
) -> PlayerSnapshot {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match conn.recv(Some(left)) {
            Ok(Some(ServerMessage::Event(Event::Player(s)))) if pred(&s) => return s,
            Ok(Some(_)) => {}
            Ok(None) => panic!("no snapshot with {what} within 10 s"),
            Err(e) => panic!("waiting for {what}: {e}"),
        }
    }
}

/// Waits for `ShuttingDown`; panics after 10 s.
fn wait_shutting_down(conn: &mut Connection) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match conn.recv(Some(left)) {
            Ok(Some(ServerMessage::Event(Event::ShuttingDown))) => return,
            Ok(Some(_)) => {}
            Ok(None) => panic!("no ShuttingDown within 10 s"),
            Err(e) => panic!("the connection ended before ShuttingDown: {e}"),
        }
    }
}

fn has_track(snapshot: &PlayerSnapshot, track: u64) -> bool {
    snapshot.queue.iter().any(|e| e.track.id.0 == track)
}

/// The pid in the lock file.
fn lock_pid(dir: &Path) -> String {
    std::fs::read_to_string(dir.join("player.lock"))
        .unwrap()
        .trim()
        .to_owned()
}

/// AC11: `tidal-player [ITEM]...` with a player running attaches to it
/// (no lock, no second player) and sends `Open` first; while the player
/// does not answer (lock held, no socket) it tries for 2 s, then says so.
#[test]
fn ac11_tui_attaches() {
    let machine = Machine::new();
    let mut daemon = machine.spawn(&["daemon"]);
    let mut watcher = attach(&machine.socket());
    // No terminal here: the client attaches and sends its `Open`, then
    // cannot enter the TUI.
    let (code, _, stderr) = machine.run_client(&["--add-to-queue", "1001"]);
    assert_ne!(code, Some(3), "started a second player: {stderr}");
    assert!(
        stderr.contains("terminal") && !stderr.contains("Another player"),
        "{code:?}: {stderr}"
    );
    let snapshot = wait_snapshot(&mut watcher, "track 1001", |s| has_track(s, 1001));
    assert_eq!(snapshot.queue.len(), 1);
    assert!(daemon.running());
    assert_eq!(lock_pid(&machine.run), daemon.pid().to_string());

    // The lock is held, the socket never answers.
    let machine = Machine::new();
    machine.create_run_dir();
    let me = std::process::id();
    let _lock = hold_lock(&machine.run, me);
    let started = Instant::now();
    let (code, stdout, stderr) = machine.run_client(&[]);
    assert_eq!(
        (code, stdout.as_str(), stderr),
        (
            Some(1),
            "",
            format!(
                "A player is running (pid {me}) but not answering on {}\n",
                machine.socket().display()
            )
        )
    );
    assert!(
        started.elapsed() >= Duration::from_millis(1900),
        "gave up after {:?}",
        started.elapsed()
    );
}

/// AC14: clients open no session store, keyring or passphrase source:
/// with an empty state dir, `playback status` and a TUI client attach
/// work (they would otherwise say `Not logged in`).
#[test]
fn ac14_client_needs_no_session() {
    let machine = Machine::new();
    let _daemon = machine.spawn(&["daemon"]);
    let mut watcher = attach(&machine.socket());
    assert_eq!(
        machine.run_client(&["playback", "status"]),
        (Some(0), "Nothing playing\n".into(), String::new())
    );
    let (code, stdout, stderr) = machine.run_client(&["--play-next", "1001"]);
    assert!(
        !stdout.contains("Not logged in") && !stderr.contains("Not logged in"),
        "{code:?}: {stdout}{stderr}"
    );
    assert!(stderr.contains("terminal"), "{code:?}: {stderr}");
    wait_snapshot(&mut watcher, "track 1001", |s| has_track(s, 1001));
    // 0006 AC19: a client's library request is answered by the player (with
    // no library connected yet: an error), and still opens no session.
    watcher
        .send(&ClientMessage::Library {
            id: 77,
            request: LibraryRequest::Page(PageRequest::Library),
        })
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        assert!(Instant::now() < deadline, "no library reply");
        match watcher.recv(Some(Duration::from_millis(200))) {
            Ok(Some(ServerMessage::LibraryReply { id, result })) => {
                assert_eq!(id, 77);
                assert!(result.is_ok() || result.is_err());
                break;
            }
            Ok(_) => {}
            Err(e) => panic!("connection failed: {e:?}"),
        }
    }
    // 0007 AC13: a client's search, as the TUI makes it (`g s`, a typed
    // query, `Enter`), is answered from the player's API and fills the
    // search page; still no session.
    search_from_a_client(&machine.socket());
    // Nothing was written to the empty state dir.
    assert_eq!(std::fs::read_dir(machine.empty.path()).unwrap().count(), 0);
}

/// 0007 AC13: the TUI's model and session, attached to the player at
/// `socket`, without a terminal: `g s`, `pierce the veil`, `Enter`; the
/// search goes to the player, which asks the mock API (`search_page.json`),
/// and the reply fills the page.
fn search_from_a_client(socket: &Path) {
    use tidal_player::client::{Session, SocketConnector};
    use tidal_player_core::library::TopHit;
    use tidal_player_core::ui::{Action, Effect, Key, SearchFocus, State, update};

    let link = Connection::connect(socket).unwrap();
    let mut session = Session::new(SocketConnector::new(socket.to_owned()), link, None);
    let mut state = State::default();
    let mut effects = Vec::new();
    // Until the player's `Welcome` connects the model.
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        assert!(Instant::now() < deadline, "no Welcome");
        let actions = session.poll(Instant::now());
        let welcomed = actions.iter().any(|a| matches!(a, Action::Welcome { .. }));
        for action in actions {
            effects.extend(update(&mut state, action));
        }
        if welcomed {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let keys = [Key::Char('g'), Key::Char('s')]
        .into_iter()
        .chain("pierce the veil".chars().map(Key::Char))
        .chain([Key::Enter]);
    for key in keys {
        effects.extend(update(&mut state, Action::Key(key)));
    }
    let mut asked = 0;
    for effect in effects.drain(..) {
        match effect {
            Effect::Library { id, request } => {
                assert_eq!(
                    request,
                    LibraryRequest::Page(PageRequest::Search("pierce the veil".into()))
                );
                asked += 1;
                session.send_library(id, request);
            }
            other => panic!("unexpected effect: {other:?}"),
        }
    }
    assert_eq!(asked, 1);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        assert!(Instant::now() < deadline, "no search reply");
        let actions = session.poll(Instant::now());
        let replied = actions
            .iter()
            .any(|a| matches!(a, Action::LibraryReply { .. }));
        for action in actions {
            if let Action::LibraryReply { result: Err(e), .. } = &action {
                panic!("the search failed: {e}");
            }
            update(&mut state, action);
        }
        if replied {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let page = state.page();
    assert_eq!(page.title(), "Search · \"pierce the veil\"");
    let search = page.search.as_ref().expect("a search page");
    assert!(
        matches!(&search.top_hit, Some(TopHit::Artist(a)) if a.name == "Pierce The Veil"),
        "{:?}",
        search.top_hit
    );
    assert_eq!(search.focus, SearchFocus::TopHit);
    let titles: Vec<String> = page.windows.iter().map(|w| w.title()).collect();
    assert_eq!(
        titles,
        [
            "Tracks (223)",
            "Albums (55)",
            "Artists (7)",
            "Playlists (3)"
        ]
    );
}

/// AC15: the exit codes of `playback`: bad arguments 2 (nothing sent),
/// no player 1, `Ok` 0, an `Err` reply its message and 1; `status` and
/// `--json`.
#[test]
fn ac15_exit_codes() {
    let machine = Machine::new();
    let bad: [&[&str]; 3] = [
        &["playback", "volume", "101"],
        &["playback", "seek", "x"],
        &["playback", "load", "https://tidal.com/browse/artist/1"],
    ];
    // Refused before looking for a player (there is none).
    for args in bad {
        let (code, stdout, stderr) = machine.run_client(args);
        assert_eq!((code, stdout.as_str()), (Some(2), ""), "{args:?}: {stderr}");
        assert!(!stderr.is_empty(), "{args:?}");
    }
    assert_eq!(
        machine.run_client(&["playback", "next"]),
        (Some(1), String::new(), format!("{NO_PLAYER}\n"))
    );

    let _daemon = machine.spawn(&["daemon"]);
    let mut watcher = attach(&machine.socket());
    assert_eq!(
        machine.run_client(&["playback", "volume", "80"]),
        (Some(0), String::new(), String::new())
    );
    wait_snapshot(&mut watcher, "volume 80", |s| s.volume == 80);
    // Nothing is sent for a bad argument: the next change is the next
    // good command's.
    for args in bad {
        assert_eq!(machine.run_client(args).0, Some(2), "{args:?}");
    }
    assert_eq!(
        machine.run_client(&["playback", "volume", "-10"]).0,
        Some(0)
    );
    let next = wait_snapshot(&mut watcher, "any change", |_| true);
    assert_eq!(next.volume, 70);

    assert_eq!(
        machine.run_client(&["playback", "load", "https://tidal.com/browse/album/404"]),
        (Some(1), String::new(), "Album 404 was not found\n".into())
    );
    assert_eq!(
        machine.run_client(&["playback", "status"]),
        (Some(0), "Nothing playing\n".into(), String::new())
    );
    let (code, stdout, stderr) = machine.run_client(&["playback", "status", "--json"]);
    assert_eq!((code, stderr.as_str()), (Some(0), ""));
    assert_eq!(stdout.lines().count(), 1, "{stdout}");
    match serde_json::from_str::<ServerMessage>(stdout.trim_end()) {
        Ok(ServerMessage::Welcome {
            snapshot,
            login_required,
        }) => {
            assert_eq!((snapshot.volume, login_required), (70, false));
        }
        other => panic!("not a Welcome: {other:?} from {stdout}"),
    }
}

/// AC16: `daemon stop` asks the player to shut down and returns once the
/// lock is free; without a player, exit 1.
#[test]
fn ac16_daemon_stop() {
    let machine = Machine::new();
    assert_eq!(
        machine.run_client(&["daemon", "stop"]),
        (Some(1), String::new(), format!("{NO_PLAYER}\n"))
    );
    let mut daemon = machine.spawn(&["daemon"]);
    let mut watcher = attach(&machine.socket());
    assert_eq!(
        machine.run_client(&["daemon", "stop"]),
        (Some(0), String::new(), String::new())
    );
    assert_eq!(
        probe(&machine.run).unwrap(),
        Probe::Free,
        "returned before the lock was free"
    );
    wait_shutting_down(&mut watcher);
    assert_eq!(daemon.finish(Duration::from_secs(5)).0, Some(0));
}

/// AC17: `READY=1` on `$NOTIFY_SOCKET` (a path, or an abstract `@name`)
/// once the socket accepts; on `SIGTERM`, and on a client's `Shutdown`,
/// every subscriber gets `ShuttingDown`, the socket file goes and the
/// daemon exits 0.
#[test]
fn ac17_ready_and_sigterm() {
    use std::os::linux::net::SocketAddrExt;
    use std::os::unix::net::{SocketAddr, UnixDatagram};

    for abstract_name in [false, true] {
        let machine = Machine::new();
        let (notify, address) = if abstract_name {
            let name = format!(
                "tidal-player-test-{}-{:?}",
                std::process::id(),
                Instant::now()
            );
            let addr = SocketAddr::from_abstract_name(name.as_bytes()).unwrap();
            (UnixDatagram::bind_addr(&addr).unwrap(), format!("@{name}"))
        } else {
            let path = machine.state.path().join("notify.sock");
            let socket = UnixDatagram::bind(&path).unwrap();
            (socket, path.display().to_string())
        };
        notify
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let mut cmd = machine.command(&["daemon"]);
        cmd.env("NOTIFY_SOCKET", &address);
        let mut daemon = Running(cmd.spawn().unwrap());
        let mut buf = [0u8; 256];
        let n = notify
            .recv(&mut buf)
            .unwrap_or_else(|e| panic!("no READY=1 on {address}: {e}"));
        assert_eq!(String::from_utf8_lossy(&buf[..n]).trim_end(), "READY=1");
        // Ready means accepting: no retry needed.
        let mut conn =
            Connection::connect(&machine.socket()).expect("the socket accepts after READY=1");
        conn.send(&ClientMessage::Subscribe).unwrap();
        assert!(matches!(
            conn.recv(Some(Duration::from_secs(5))),
            Ok(Some(ServerMessage::Welcome { .. }))
        ));

        let killed = Command::new("kill")
            .args(["-TERM", &daemon.pid().to_string()])
            .status()
            .unwrap();
        assert!(killed.success());
        wait_shutting_down(&mut conn);
        assert_eq!(
            daemon.finish(Duration::from_secs(10)).0,
            Some(0),
            "{address}"
        );
        assert!(!machine.socket().exists(), "the socket file is left");
        assert!(matches!(
            conn.recv(Some(Duration::from_secs(5))),
            Err(RecvError::Closed) | Ok(None)
        ));
    }

    // A client's `Shutdown`: every subscriber is told, the sender answered.
    let machine = Machine::new();
    let mut daemon = machine.spawn(&["daemon"]);
    let mut watcher = attach(&machine.socket());
    let mut asker = attach(&machine.socket());
    asker
        .send(&ClientMessage::Request {
            id: 1,
            command: PlayerCommand::Shutdown,
        })
        .unwrap();
    wait_shutting_down(&mut watcher);
    let replied = loop {
        match asker.recv(Some(Duration::from_secs(5))) {
            Ok(Some(ServerMessage::Reply { id: 1, result })) => break result,
            Ok(Some(_)) => {}
            other => panic!("no reply to Shutdown: {other:?}"),
        }
    };
    assert_eq!(replied, Ok(()));
    assert_eq!(daemon.finish(Duration::from_secs(10)).0, Some(0));
    assert!(!machine.socket().exists(), "the socket file is left");

    // 0002 AC12 still holds: no session → exit 1.
    let machine = Machine::new();
    let (code, _, stderr) =
        Running(machine.client(&["daemon"]).spawn().unwrap()).output(Duration::from_secs(20));
    assert_eq!(
        (code, stderr.as_str()),
        (Some(1), "Not logged in: run \"tidal-player login\"\n")
    );
}

/// 0008 AC15: the daemon's player settings come from `app.toml` in the
/// config folder given with `-c`: its page size sizes the library's
/// requests, its quality is the one the stream is asked for, and
/// `release_paused_secs = "never"` is accepted (the release itself needs an
/// audio device; it is covered by `config.rs` :: `ac11_precedence`).
#[test]
fn ac15_daemon_reads_app_toml() {
    let machine = Machine::new();
    let config = tempfile::tempdir().unwrap();
    std::fs::write(
        config.path().join("app.toml"),
        "quality = \"lossless\"\nrelease_paused_secs = \"never\"\npage_size = 7\n",
    )
    .unwrap();
    let dir = config.path().to_str().unwrap();
    let mut daemon = machine.spawn(&["daemon", "-c", dir]);
    let mut watcher = attach(&machine.socket());

    // The page size: the library's first requests ask for 7 rows.
    watcher
        .send(&ClientMessage::Library {
            id: 5,
            request: LibraryRequest::Page(PageRequest::Library),
        })
        .unwrap();
    let request = machine.wait_request("a library page", |r| {
        r.url.query_pairs().any(|(k, _)| k == "limit")
    });
    let limits: Vec<String> = machine
        .requests()
        .iter()
        .flat_map(|r| {
            r.url
                .query_pairs()
                .filter(|(k, _)| k == "limit")
                .map(|(_, v)| v.into_owned())
                .collect::<Vec<_>>()
        })
        .collect();
    assert!(
        !limits.is_empty() && limits.iter().all(|l| l == "7"),
        "{limits:?} ({})",
        request.url
    );

    // The quality: playing track 1001 asks for its stream at LOSSLESS.
    assert_eq!(machine.run_client(&["playback", "load", "1001"]).0, Some(0));
    let request = machine.wait_request("the stream of 1001", |r| {
        r.url
            .path()
            .ends_with("/tracks/1001/playbackinfopostpaywall")
    });
    let quality: Vec<String> = request
        .url
        .query_pairs()
        .filter(|(k, _)| k == "audioquality")
        .map(|(_, v)| v.into_owned())
        .collect();
    assert_eq!(quality, ["LOSSLESS"], "{}", request.url);
    assert!(daemon.running());
}

/// The TUI's model and session attached to the player at `socket`,
/// without a terminal, with the keymap of `config` (its `keymap.toml`, read
/// as the TUI reads it); returned once the player's `Welcome` is in.
fn keyed_client(
    socket: &Path,
    config: &Path,
) -> (
    tidal_player::client::Session<tidal_player::client::SocketConnector>,
    tidal_player_core::ui::State,
) {
    use tidal_player::client::{Session, SocketConnector};
    use tidal_player_core::ui::{Action, State, apply_keymap, update};

    let keymap = tidal_player::config::load_keymap_toml(config).unwrap();
    let link = Connection::connect(socket).unwrap();
    let mut session = Session::new(SocketConnector::new(socket.to_owned()), link, None);
    let mut state = State::default();
    apply_keymap(&mut state, keymap);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        assert!(Instant::now() < deadline, "no Welcome");
        let actions = session.poll(Instant::now());
        let welcomed = actions.iter().any(|a| matches!(a, Action::Welcome { .. }));
        for action in actions {
            update(&mut state, action);
        }
        if welcomed {
            return (session, state);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// 0008 AC16: two clients of one player, each with its own `keymap.toml`:
/// `n` lowers the volume in the one that rebinds it and still skips in the
/// other. The keys never reach the player; only the commands do.
#[test]
fn ac16_keymap_is_per_client() {
    use tidal_player_core::ui::{Action, Effect, Key, update};

    let machine = Machine::new();
    let _daemon = machine.spawn(&["daemon"]);
    let mut watcher = attach(&machine.socket());
    // A queue, so that `NextTrack` has something to skip.
    machine.run_client(&["--add-to-queue", "1001"]);
    wait_snapshot(&mut watcher, "track 1001", |s| has_track(s, 1001));
    let rebound = tempfile::tempdir().unwrap();
    std::fs::write(
        rebound.path().join("keymap.toml"),
        "[[keymaps]]\ncommand = { VolumeChange = { offset = -10 } }\nkey_sequence = \"n\"\n",
    )
    .unwrap();
    let plain = tempfile::tempdir().unwrap();
    let (mut a, mut a_state) = keyed_client(&machine.socket(), rebound.path());
    let (mut b, mut b_state) = keyed_client(&machine.socket(), plain.path());

    let effects = update(&mut a_state, Action::Key(Key::Char('n')));
    assert_eq!(
        effects,
        vec![Effect::Send(PlayerCommand::ChangeVolume(-10))],
        "the rebinding client"
    );
    for effect in effects {
        if let Effect::Send(command) = effect {
            a.send(command);
        }
    }
    wait_snapshot(&mut watcher, "volume 90", |s| s.volume == 90);

    let effects = update(&mut b_state, Action::Key(Key::Char('n')));
    assert_eq!(
        effects,
        vec![Effect::Send(PlayerCommand::Next)],
        "the other client"
    );
    for effect in effects {
        if let Effect::Send(command) = effect {
            b.send(command);
        }
    }
    // The first client's keymap is untouched by the second's.
    assert_eq!(
        update(&mut a_state, Action::Key(Key::Char('n'))),
        vec![Effect::Send(PlayerCommand::ChangeVolume(-10))]
    );
}

// --- spec 0009: the remembered playback state ----------------------------------

fn track(id: u64) -> tidal_player_core::Track {
    use tidal_player_core::{AlbumRef, ArtistRef, Track, TrackId};
    Track {
        id: TrackId(id),
        title: format!("Title {id}"),
        version: None,
        artists: vec![ArtistRef {
            id: 1,
            name: "Artist".into(),
        }],
        album: Some(AlbumRef {
            id: 2,
            title: "Album".into(),
            cover: None,
        }),
        duration: Some(Duration::from_secs(296)),
        streamable: true,
    }
}

/// Connects and subscribes (retrying while the player starts): the
/// `Welcome`'s snapshot.
fn subscribe(socket: &Path) -> (Connection, PlayerSnapshot) {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut conn = loop {
        match Connection::connect(socket) {
            Ok(conn) => break conn,
            Err(e) => {
                assert!(Instant::now() < deadline, "cannot connect: {e}");
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    };
    conn.send(&ClientMessage::Subscribe).unwrap();
    match conn.recv(Some(Duration::from_secs(5))) {
        Ok(Some(ServerMessage::Welcome { snapshot, .. })) => (conn, snapshot),
        other => panic!("expected a Welcome, got {other:?}"),
    }
}

/// Sends `command` and waits for its reply: the last snapshot it caused.
fn request(conn: &mut Connection, id: u64, command: PlayerCommand) -> Option<PlayerSnapshot> {
    conn.send(&ClientMessage::Request { id, command }).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut last = None;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match conn.recv(Some(left)) {
            Ok(Some(ServerMessage::Event(Event::Player(s)))) => last = Some(s),
            Ok(Some(ServerMessage::Reply { id: got, result })) if got == id => {
                assert_eq!(result, Ok(()));
                return last;
            }
            Ok(Some(_)) => {}
            Ok(None) => panic!("no reply to request {id} within 10 s"),
            Err(e) => panic!("waiting for the reply to {id}: {e}"),
        }
    }
}

fn is_stream_request(request: &wiremock::Request) -> bool {
    request.url.path().ends_with("/playbackinfopostpaywall")
}

/// AC8: load a queue, shuffle on, repeat `queue`, volume 70, seek to 1:23,
/// `daemon stop`; a new daemon's first `Welcome` is that state, stopped,
/// and nothing reaches the API before the client's `TogglePause`, which
/// resolves the current track. (`Play` at the saved position: the daemon
/// has no device here; `player_runtime.rs` :: `ac8_play_at_saved_position`
/// proves it with the fake engine.)
#[test]
fn ac8_daemon_resumes() {
    use wiremock::matchers::{method, path_regex};
    use wiremock::{Mock, ResponseTemplate};

    let machine = Machine::new();
    // Streams never resolve in time: the first daemon stays loading, so
    // the seek sets the position it starts at, without a device.
    machine._tokio.block_on(
        Mock::given(method("GET"))
            .and(path_regex(r"^/tracks/\d+/playbackinfopostpaywall$"))
            .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(60)))
            .mount(&machine.server),
    );
    let mut daemon = machine.spawn(&["daemon"]);
    let (mut conn, welcome) = subscribe(&machine.socket());
    assert!(welcome.queue.is_empty(), "{welcome:?}");
    let mut last = None;
    for (id, command) in [
        PlayerCommand::LoadQueue {
            tracks: vec![track(11), track(12), track(13)],
            start: 1,
        },
        PlayerCommand::ToggleShuffle,
        PlayerCommand::CycleRepeat,
        PlayerCommand::SetVolume(70),
        PlayerCommand::SeekTo(Duration::from_secs(83)),
    ]
    .into_iter()
    .enumerate()
    {
        last = request(&mut conn, id as u64 + 1, command).or(last);
    }
    let saved = last.expect("snapshots");
    assert_eq!(
        (
            saved.shuffle,
            saved.repeat,
            saved.volume,
            saved.position,
            saved.queue.len()
        ),
        (
            true,
            tidal_player_core::protocol::RepeatMode::Queue,
            70,
            Duration::from_secs(83),
            3
        ),
        "{saved:?}"
    );
    assert_eq!(
        machine.run_client(&["daemon", "stop"]),
        (Some(0), String::new(), String::new())
    );
    assert_eq!(daemon.finish(Duration::from_secs(5)).0, Some(0));

    let before = machine.requests().len();
    let _daemon = machine.spawn(&["daemon"]);
    let (mut conn, welcome) = subscribe(&machine.socket());
    assert_eq!(
        welcome,
        PlayerSnapshot {
            state: tidal_player_core::protocol::PlaybackState::Stopped,
            ..saved.clone()
        }
    );
    // Restoring fetches nothing.
    std::thread::sleep(Duration::from_millis(300));
    let urls = |from: usize| {
        machine.requests()[from..]
            .iter()
            .map(|r| r.url.path().to_owned())
            .collect::<Vec<_>>()
    };
    assert_eq!(urls(before), Vec::<String>::new(), "requests before play");

    request(&mut conn, 100, PlayerCommand::TogglePause);
    let current = saved
        .queue
        .iter()
        .find(|e| Some(e.id) == saved.current)
        .map(|e| e.track.id.0)
        .expect("a current entry");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let streams: Vec<String> = machine.requests()[before..]
            .iter()
            .filter(|r| is_stream_request(r))
            .map(|r| r.url.path().to_owned())
            .collect();
        if !streams.is_empty() {
            assert_eq!(
                streams[0],
                format!("/tracks/{current}/playbackinfopostpaywall")
            );
            break;
        }
        assert!(
            Instant::now() < deadline,
            "no stream request after play: {:?}",
            urls(before)
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// AC10: only the player touches `playback.json`: clients sharing the
/// player's state dir (as on one machine) leave it byte-for-byte as it
/// was, and so does the player when nothing it remembers changed.
#[test]
fn ac10_client_leaves_playback_file() {
    use tidal_player_core::protocol::QueueEntry;
    use tidal_player_core::{EntryId, SavedPlayback};

    let machine = Machine::new();
    let saved = SavedPlayback {
        entries: vec![QueueEntry {
            id: EntryId(4),
            track: track(11),
            suggested: false,
        }],
        play_order: vec![EntryId(4)],
        current: Some(EntryId(4)),
        position_ms: 83_000,
        volume: 70,
        ..SavedPlayback::default()
    };
    let file = machine.state.path().join("playback.json");
    std::fs::write(&file, serde_json::to_vec_pretty(&saved).unwrap()).unwrap();
    let bytes = std::fs::read(&file).unwrap();

    let mut daemon = machine.spawn(&["daemon"]);
    let (_conn, welcome) = subscribe(&machine.socket());
    assert_eq!(
        (welcome.current, welcome.position, welcome.volume),
        (Some(EntryId(4)), Duration::from_secs(83), 70),
        "the player did not restore the file"
    );
    let shared = |args: &[&str]| {
        let mut cmd = machine.client(args);
        cmd.env("TIDAL_PLAYER_STATE_DIR", machine.state.path());
        Running(cmd.spawn().unwrap()).output(Duration::from_secs(20))
    };
    let (code, _, stderr) = shared(&["playback", "status"]);
    assert_eq!(code, Some(0), "{stderr}");
    let (code, _, stderr) = shared(&[]);
    assert!(stderr.contains("terminal"), "{code:?}: {stderr}");
    assert_eq!(std::fs::read(&file).unwrap(), bytes, "a client wrote it");

    assert_eq!(
        machine.run_client(&["daemon", "stop"]),
        (Some(0), String::new(), String::new())
    );
    assert_eq!(daemon.finish(Duration::from_secs(5)).0, Some(0));
    assert_eq!(std::fs::read(&file).unwrap(), bytes, "nothing changed");
}
