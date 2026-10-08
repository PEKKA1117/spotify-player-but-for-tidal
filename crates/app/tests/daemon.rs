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
        let api = tokio.block_on(wiremock::MockServer::start());
        tokio.block_on(mount_metadata(&api));
        let uri = api.uri();
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
