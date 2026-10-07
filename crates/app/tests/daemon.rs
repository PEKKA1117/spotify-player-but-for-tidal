//! Spec 0005, end to end with the binary: one player per user (AC10).
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

use tidal_player::ipc::client::Connection;
use tidal_player::passphrase::{PassphraseEnv, ProcessPassphrase, Prompt};
use tidal_player::session_store::EncryptedFileStore;
use tidal_player_api::auth::{Session, SessionStore};
use tidal_player_core::protocol::{ClientMessage, ServerMessage};

struct NoPrompt;

impl Prompt for NoPrompt {
    fn ask(&mut self, _prompt: &str) -> std::io::Result<String> {
        Err(std::io::Error::other("no prompt in tests"))
    }
}

/// A user's machine: state dir with a session, runtime dir, mock API.
struct Machine {
    state: tempfile::TempDir,
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
        let uri = api.uri();
        // The server lives as long as the runtime's task keeps it.
        tokio.spawn(async move {
            let _api = api;
            std::future::pending::<()>().await;
        });
        let run = state.path().join("run");
        Self {
            state,
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
