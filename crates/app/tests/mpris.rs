//! Spec 0010, end to end with the binary: the daemon on a private session
//! bus (AC10, AC12–AC14) and without one (AC11).
//!
//! Each test starts its own `dbus-daemon` (a session bus on a socket in a
//! temp dir, from a config file written here); a test that needs one fails,
//! never skips, when `dbus-daemon` is missing (install the `dbus` package).
//! The player side is 0005's: a state dir with a session, a runtime dir, a
//! mock API (debug builds honour `TIDAL_PLAYER_API_BASE`, and
//! `TIDAL_PLAYER_IMAGES_BASE` for covers), no audio device. Streams never
//! resolve, so a loaded queue stays loading.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant, SystemTime};

use tidal_player::ipc::client::Connection;
use tidal_player::mpris::model::{self, Value};
use tidal_player::passphrase::{PassphraseEnv, ProcessPassphrase, Prompt};
use tidal_player::session_store::EncryptedFileStore;
use tidal_player_api::auth::{Session, SessionStore};
use tidal_player_core::protocol::{
    ClientMessage, Command as PlayerCommand, Event, PlayerSnapshot, ServerMessage,
};
use tidal_player_core::{AlbumRef, ArtistRef, Track, TrackId};
use zbus::blocking::fdo::{DBusProxy, PropertiesProxy};
use zbus::names::InterfaceName;
use zbus::zvariant::{self, OwnedValue};

const NAME: &str = "org.mpris.MediaPlayer2.tidal_player";
const PATH: &str = "/org/mpris/MediaPlayer2";
const ROOT: &str = "org.mpris.MediaPlayer2";
const PLAYER: &str = "org.mpris.MediaPlayer2.Player";
const COVER: &str = "2e4a5d2d-9a0d-4c3a-a0ba-42b0bd16a6ec";
/// The album `album_page.json` (the API's fixture) answers for.
const ALBUM: u64 = 2001;

// --- the private bus ---------------------------------------------------------------

/// A `dbus-daemon` of this test's own, killed when dropped.
struct PrivateBus {
    child: Child,
    address: String,
    _dir: tempfile::TempDir,
}

impl PrivateBus {
    fn start() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("bus");
        let config = dir.path().join("session.conf");
        std::fs::write(
            &config,
            format!(
                "<!DOCTYPE busconfig PUBLIC \"-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN\" \
                 \"http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd\">\n\
                 <busconfig><type>session</type><listen>unix:path={}</listen>\
                 <auth>EXTERNAL</auth><policy context=\"default\">\
                 <allow send_destination=\"*\" eavesdrop=\"true\"/><allow eavesdrop=\"true\"/>\
                 <allow own=\"*\"/></policy></busconfig>\n",
                socket.display()
            ),
        )
        .unwrap();
        let mut child = Command::new("dbus-daemon")
            .arg(format!("--config-file={}", config.display()))
            .args(["--nofork", "--print-address"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap_or_else(|e| panic!("cannot start dbus-daemon (install the dbus package): {e}"));
        let mut line = String::new();
        BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        let address = line.trim().to_owned();
        assert!(address.starts_with("unix:"), "dbus-daemon said {line:?}");
        Self {
            child,
            address,
            _dir: dir,
        }
    }

    fn connect(&self) -> zbus::blocking::Connection {
        zbus::blocking::connection::Builder::address(self.address.as_str())
            .unwrap()
            .build()
            .unwrap()
    }
}

impl Drop for PrivateBus {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Signals on the bus, as a test sees them.
#[derive(Debug, Clone, PartialEq)]
enum Signal {
    /// `PropertiesChanged` on the player interface: the changed values.
    Changed(HashMap<String, Value>),
    /// `Seeked`, in µs.
    Seeked(i64),
    /// `NameOwnerChanged(name, old, new)`.
    Owner(String, String, String),
}

/// Collects the signals matching `rule` on a connection of its own.
fn listen(bus: &PrivateBus, rule: &str) -> Receiver<Signal> {
    let conn = bus.connect();
    let iterator = zbus::blocking::MessageIterator::for_match_rule(rule, &conn, None).unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _conn = conn;
        for message in iterator {
            let Ok(message) = message else { break };
            let header = message.header();
            let signal = match header.member().map(|m| m.as_str()) {
                Some("PropertiesChanged") => {
                    let (iface, changed, _): (String, HashMap<String, OwnedValue>, Vec<String>) =
                        message.body().deserialize().unwrap();
                    if iface != PLAYER {
                        continue;
                    }
                    Signal::Changed(changed.iter().map(|(k, v)| (k.clone(), value(v))).collect())
                }
                Some("Seeked") => Signal::Seeked(message.body().deserialize::<(i64,)>().unwrap().0),
                Some("NameOwnerChanged") => {
                    let (name, old, new): (String, String, String) =
                        message.body().deserialize().unwrap();
                    Signal::Owner(name, old, new)
                }
                _ => continue,
            };
            if tx.send(signal).is_err() {
                break;
            }
        }
    });
    rx
}

/// The player's signals (`PropertiesChanged` and `Seeked`).
fn player_signals(bus: &PrivateBus) -> Receiver<Signal> {
    listen(bus, &format!("type='signal',path='{PATH}'"))
}

/// Every connection that comes or goes.
fn owner_changes(bus: &PrivateBus) -> Receiver<Signal> {
    listen(
        bus,
        "type='signal',sender='org.freedesktop.DBus',member='NameOwnerChanged'",
    )
}

/// What arrived within `wait`.
fn drain(rx: &Receiver<Signal>, wait: Duration) -> Vec<Signal> {
    let deadline = Instant::now() + wait;
    let mut got = Vec::new();
    while let Ok(signal) = rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
        got.push(signal);
    }
    got
}

/// A D-Bus value as the model's.
fn value(v: &zvariant::Value<'_>) -> Value {
    match v {
        zvariant::Value::Bool(b) => Value::Bool(*b),
        zvariant::Value::F64(d) => Value::Double(*d),
        zvariant::Value::I64(i) => Value::Int64(*i),
        zvariant::Value::Str(s) => Value::Str(s.to_string()),
        zvariant::Value::ObjectPath(p) => Value::ObjectPath(p.to_string()),
        zvariant::Value::Value(inner) => value(inner),
        zvariant::Value::Array(a) => Value::StrList(
            a.iter()
                .map(|item| match item {
                    zvariant::Value::Str(s) => s.to_string(),
                    other => panic!("not a string: {other:?}"),
                })
                .collect(),
        ),
        zvariant::Value::Dict(d) => Value::Metadata(
            d.iter()
                .map(|(k, v)| match k {
                    zvariant::Value::Str(k) => (k.to_string(), value(v)),
                    other => panic!("not a string key: {other:?}"),
                })
                .collect(),
        ),
        other => panic!("unexpected value {other:?}"),
    }
}

fn properties(
    conn: &zbus::blocking::Connection,
    name: &str,
    iface: &str,
) -> HashMap<String, Value> {
    let proxy = PropertiesProxy::builder(conn)
        .destination(name.to_owned())
        .unwrap()
        .path(PATH)
        .unwrap()
        .build()
        .unwrap();
    proxy
        .get_all(InterfaceName::try_from(iface).unwrap())
        .unwrap()
        .iter()
        .map(|(k, v)| (k.clone(), value(v)))
        .collect()
}

fn has_owner(conn: &zbus::blocking::Connection, name: &str) -> bool {
    DBusProxy::new(conn)
        .unwrap()
        .name_has_owner(name.try_into().unwrap())
        .unwrap()
}

fn wait_owner(conn: &zbus::blocking::Connection, name: &str, owned: bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while has_owner(conn, name) != owned {
        assert!(
            Instant::now() < deadline,
            "{name} {} within 10 s",
            if owned { "not owned" } else { "still owned" }
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn call(
    conn: &zbus::blocking::Connection,
    method: &str,
    body: &(impl serde::Serialize + zvariant::DynamicType),
) {
    conn.call_method(Some(NAME), PATH, Some(PLAYER), method, body)
        .unwrap_or_else(|e| panic!("{method}: {e}"));
}

// --- the player's machine ----------------------------------------------------------

struct NoPrompt;

impl Prompt for NoPrompt {
    fn ask(&mut self, _prompt: &str) -> std::io::Result<String> {
        Err(std::io::Error::other("no prompt in tests"))
    }
}

/// A state dir with a session, a runtime, config and cache dir, a mock API.
struct Machine {
    state: tempfile::TempDir,
    empty: tempfile::TempDir,
    config: tempfile::TempDir,
    cache: tempfile::TempDir,
    run: PathBuf,
    pass_file: PathBuf,
    /// Keeps the mock API running.
    _tokio: tokio::runtime::Runtime,
    api: String,
    bus: String,
}

impl Machine {
    /// On `bus` (`None`: no session bus at all).
    fn new(bus: Option<&PrivateBus>) -> Self {
        use wiremock::matchers::{method, path, path_regex};
        use wiremock::{Mock, MockServer, ResponseTemplate};
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
        let fixture = |name: &str| {
            let file = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../api/tests/fixtures/metadata")
                .join(name);
            serde_json::from_slice::<serde_json::Value>(&std::fs::read(file).unwrap()).unwrap()
        };
        let api = tokio.block_on(async {
            let api = MockServer::start().await;
            Mock::given(method("GET"))
                .and(path("/tracks/1001"))
                .respond_with(ResponseTemplate::new(200).set_body_json(fixture("track.json")))
                .mount(&api)
                .await;
            Mock::given(method("GET"))
                .and(path(format!("/albums/{ALBUM}/tracks")))
                .respond_with(ResponseTemplate::new(200).set_body_json(fixture("album_page.json")))
                .mount(&api)
                .await;
            // Streams never resolve: a loaded queue stays loading.
            Mock::given(method("GET"))
                .and(path_regex("/playbackinfopostpaywall$"))
                .respond_with(ResponseTemplate::new(500).set_delay(Duration::from_secs(600)))
                .mount(&api)
                .await;
            Mock::given(method("GET"))
                .and(path(format!(
                    "/images/{}/640x640.jpg",
                    COVER.replace('-', "/")
                )))
                .respond_with(
                    ResponseTemplate::new(200).set_body_raw(b"JPEG".to_vec(), "image/jpeg"),
                )
                .mount(&api)
                .await;
            api
        });
        let uri = api.uri();
        tokio.spawn(async move {
            let _api = api;
            std::future::pending::<()>().await;
        });
        let run = state.path().join("run");
        Self {
            state,
            empty: tempfile::tempdir().unwrap(),
            config: tempfile::tempdir().unwrap(),
            cache: tempfile::tempdir().unwrap(),
            run,
            pass_file,
            _tokio: tokio,
            api: uri,
            bus: bus.map_or_else(
                || "unix:path=/nonexistent/bus".to_owned(),
                |b| b.address.clone(),
            ),
        }
    }

    /// Writes `app.toml`.
    fn app_toml(&self, text: &str) {
        std::fs::write(self.config.path().join("app.toml"), text).unwrap();
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_tidal-player"));
        cmd.args(args)
            .env("TIDAL_PLAYER_STATE_DIR", self.state.path())
            .env("TIDAL_PLAYER_RUNTIME_DIR", &self.run)
            .env("TIDAL_PLAYER_CONFIG_DIR", self.config.path())
            .env("TIDAL_PLAYER_CACHE_DIR", self.cache.path())
            .env("TIDAL_PLAYER_NO_KEYRING", "1")
            .env("TIDAL_PLAYER_PASSPHRASE_FILE", &self.pass_file)
            .env("TIDAL_PLAYER_API_BASE", &self.api)
            .env("TIDAL_PLAYER_IMAGES_BASE", &self.api)
            .env("DBUS_SESSION_BUS_ADDRESS", &self.bus)
            .env_remove("TIDAL_PLAYER_MPRIS")
            .env_remove("TIDAL_PLAYER_MAX_COVER_ARTS")
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

    /// A client: no session, no passphrase, no API.
    fn run_client(&self, args: &[&str]) -> (Option<i32>, String, String) {
        let mut cmd = self.command(args);
        cmd.env("TIDAL_PLAYER_STATE_DIR", self.empty.path())
            .env_remove("TIDAL_PLAYER_PASSPHRASE_FILE")
            .env_remove("TIDAL_PLAYER_API_BASE");
        Running(cmd.spawn().unwrap()).output(Duration::from_secs(20))
    }

    fn socket(&self) -> PathBuf {
        self.run.join("player.sock")
    }
}

/// A child killed when dropped.
struct Running(Child);

impl Running {
    fn pid(&self) -> u32 {
        self.0.id()
    }

    fn running(&mut self) -> bool {
        self.0.try_wait().unwrap().is_none()
    }

    /// Waits at most `limit` for it to exit: its code, stdout and stderr.
    fn output(&mut self, limit: Duration) -> (Option<i32>, String, String) {
        let mut stdout = self.0.stdout.take().unwrap();
        let reader = std::thread::spawn(move || {
            let mut text = String::new();
            stdout.read_to_string(&mut text).unwrap();
            text
        });
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
        (status.code(), reader.join().unwrap(), stderr)
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Connects (retrying while the player starts) and subscribes.
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

/// Sends `command`, waits for its reply: the last snapshot before it.
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

/// The next snapshot `pred` accepts; panics after 10 s.
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

fn track(id: u64, cover: Option<&str>) -> Track {
    Track {
        id: TrackId(id),
        title: format!("Title {id}"),
        version: None,
        artists: vec![
            ArtistRef {
                id: 1,
                name: "Artist".into(),
            },
            ArtistRef {
                id: 3,
                name: "Other".into(),
            },
        ],
        album: Some(AlbumRef {
            id: 2,
            title: "Album".into(),
            cover: cover.map(str::to_owned),
        }),
        duration: Some(Duration::from_secs(296)),
        streamable: true,
    }
}

/// `daemon stop`, then the daemon's exit: its code and stderr.
fn stop(machine: &Machine, daemon: &mut Running) -> (Option<i32>, String) {
    assert_eq!(machine.run_client(&["daemon", "stop"]).0, Some(0));
    let (code, _, stderr) = daemon.output(Duration::from_secs(10));
    (code, stderr)
}

const UNAVAILABLE: &str = "MPRIS is not available:";

/// AC10: on the bus, the daemon owns the name; `GetAll` matches AC3/AC4;
/// `Pause()` twice stays paused; `Set(Volume)` reaches clients; a client's
/// toggle is one `PropertiesChanged` with `Shuffle` alone; a seek is
/// `Seeked`; `OpenUri` loads an album; `Quit()` leaves the daemon running;
/// `daemon stop` releases the name.
#[test]
fn ac10_on_the_bus() {
    let bus = PrivateBus::start();
    let me = bus.connect();
    let signals = player_signals(&bus);
    let machine = Machine::new(Some(&bus));
    let mut daemon = machine.spawn(&["daemon"]);
    let (mut client, _) = subscribe(&machine.socket());
    wait_owner(&me, NAME, true);

    // A freshly loaded queue: it is loading until its stream resolves, which
    // never happens here, but the resolver gives up after 10 s
    // (`RESOLVE_TIMEOUT`). Each step that needs a loading track loads a
    // fresh queue first: the new load makes the earlier one stale, so its
    // time-out changes nothing, and no step depends on how long the test
    // has been running.
    let mut loads = 0;
    let mut fresh = |client: &mut Connection| {
        loads += 1;
        request(
            client,
            loads,
            PlayerCommand::LoadQueue {
                tracks: vec![track(1, Some(COVER)), track(2, None), track(3, None)],
                start: 0,
            },
        )
        .expect("the queue")
    };
    fresh(&mut client);
    // The cover lands in the cache, then `Metadata` names the file.
    let file = format!(
        "file://{}/covers/{COVER}.jpg",
        machine.cache.path().display()
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let art = match &properties(&me, NAME, PLAYER)["Metadata"] {
            Value::Metadata(m) => m.get("mpris:artUrl").cloned(),
            other => panic!("Metadata is {other:?}"),
        };
        if art == Some(Value::Str(file.clone())) {
            break;
        }
        assert!(Instant::now() < deadline, "no cover file: {art:?}");
        std::thread::sleep(Duration::from_millis(20));
    }
    // `GetAll` of a fresh load (the cover cached now) is AC3/AC4's view of
    // its snapshot, once the adapter has seen it.
    let snapshot = fresh(&mut client);
    let art = |cover: &str| (cover == COVER).then(|| file.clone());
    let want = model::player_properties(&snapshot, snapshot.position, &art);
    let want: HashMap<String, Value> = want.into_iter().map(|(k, v)| (k.to_owned(), v)).collect();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let player = properties(&me, NAME, PLAYER);
        if player == want {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "GetAll: {player:?}\nwant: {want:?}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let root = properties(&me, NAME, ROOT);
    let s = |v: &str| Value::Str(v.into());
    let want_root: HashMap<String, Value> = [
        ("Identity", s("tidal-player")),
        ("CanQuit", Value::Bool(false)),
        ("CanRaise", Value::Bool(false)),
        ("HasTrackList", Value::Bool(false)),
        (
            "SupportedUriSchemes",
            Value::StrList(vec!["tidal".into(), "https".into()]),
        ),
        ("SupportedMimeTypes", Value::StrList(vec![])),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v))
    .collect();
    assert_eq!(root, want_root);

    // `Pause()` on a paused player leaves it paused (tidalt toggled).
    fresh(&mut client);
    call(&me, "Pause", &());
    wait_snapshot(&mut client, "paused", |s| {
        s.state == tidal_player_core::protocol::PlaybackState::Paused
    });
    call(&me, "Pause", &());
    assert_eq!(properties(&me, NAME, PLAYER)["PlaybackStatus"], s("Paused"));

    // A volume written over MPRIS reaches the clients.
    let props = PropertiesProxy::builder(&me)
        .destination(NAME)
        .unwrap()
        .path(PATH)
        .unwrap()
        .build()
        .unwrap();
    props
        .set(
            InterfaceName::try_from(PLAYER).unwrap(),
            "Volume",
            zvariant::Value::from(0.5),
        )
        .unwrap();
    wait_snapshot(&mut client, "volume 50", |s| s.volume == 50);

    // A client's toggle: one `PropertiesChanged`, `Shuffle` alone.
    fresh(&mut client);
    drain(&signals, Duration::from_millis(300));
    request(&mut client, 100, PlayerCommand::ToggleShuffle);
    let got = drain(&signals, Duration::from_millis(500));
    assert_eq!(
        got,
        vec![Signal::Changed(
            [("Shuffle".to_owned(), Value::Bool(true))].into()
        )]
    );

    // A client's seek: `Seeked`.
    fresh(&mut client);
    drain(&signals, Duration::from_millis(300));
    request(
        &mut client,
        101,
        PlayerCommand::SeekTo(Duration::from_secs(60)),
    );
    let got = drain(&signals, Duration::from_millis(500));
    assert!(got.contains(&Signal::Seeked(60_000_000)), "{got:?}");

    // `OpenUri` of an album loads it.
    call(&me, "OpenUri", &(format!("tidal://album/{ALBUM}"),));
    let loaded = wait_snapshot(&mut client, "the album", |s| {
        s.queue.iter().any(|e| e.track.id.0 == 1001)
    });
    assert_eq!(loaded.queue.len(), 3);

    // `Quit()` does nothing.
    conn_call_root(&me, "Quit");
    std::thread::sleep(Duration::from_millis(200));
    assert!(daemon.running(), "Quit stopped the daemon");
    assert!(has_owner(&me, NAME));

    // `daemon stop`: the name goes with the daemon.
    let (code, stderr) = stop(&machine, &mut daemon);
    assert_eq!(code, Some(0), "{stderr}");
    assert!(!stderr.contains(UNAVAILABLE), "{stderr}");
    wait_owner(&me, NAME, false);
}

fn conn_call_root(conn: &zbus::blocking::Connection, method: &str) {
    conn.call_method(Some(NAME), PATH, Some(ROOT), method, &())
        .unwrap_or_else(|e| panic!("{method}: {e}"));
}

/// AC11: no session bus: the daemon serves and plays as before, logs
/// `MPRIS is not available:` once, and the player's message is empty.
#[test]
fn ac11_no_bus() {
    let machine = Machine::new(None);
    let mut daemon = machine.spawn(&["daemon"]);
    let (mut client, welcome) = subscribe(&machine.socket());
    assert_eq!(welcome.message, None);
    let snapshot = request(
        &mut client,
        1,
        PlayerCommand::LoadQueue {
            tracks: vec![track(1, Some(COVER))],
            start: 0,
        },
    )
    .expect("the queue");
    assert_eq!(snapshot.queue.len(), 1);
    assert_eq!(snapshot.message, None);
    let (code, stdout, stderr) = machine.run_client(&["playback", "status"]);
    assert_eq!(code, Some(0), "{stdout}{stderr}");
    assert!(daemon.running());
    let (code, stderr) = stop(&machine, &mut daemon);
    assert_eq!(code, Some(0), "{stderr}");
    assert_eq!(stderr.matches(UNAVAILABLE).count(), 1, "{stderr}");
    // Nothing was cached without MPRIS.
    assert!(!machine.cache.path().join("covers").exists());
}

/// AC12: with the name taken, the daemon takes `…tidal_player.instance<pid>`
/// and serves there.
#[test]
fn ac12_name_taken() {
    let bus = PrivateBus::start();
    let me = bus.connect();
    me.request_name(NAME).unwrap();
    let machine = Machine::new(Some(&bus));
    let mut daemon = machine.spawn(&["daemon"]);
    let instance = format!("{NAME}.instance{}", daemon.pid());
    let _ = subscribe(&machine.socket());
    wait_owner(&me, &instance, true);
    let root = properties(&me, &instance, ROOT);
    assert_eq!(root["Identity"], Value::Str("tidal-player".into()));
    let player = properties(&me, &instance, PLAYER);
    assert_eq!(player["PlaybackStatus"], Value::Str("Stopped".into()));
    let (code, stderr) = stop(&machine, &mut daemon);
    assert_eq!(code, Some(0), "{stderr}");
    wait_owner(&me, &instance, false);
}

/// New connections to the bus seen in `changes` (a unique name gaining an
/// owner), other than `known`.
fn new_connections(changes: &[Signal], known: &[String]) -> Vec<String> {
    changes
        .iter()
        .filter_map(|s| match s {
            Signal::Owner(name, old, new)
                if name.starts_with(':') && old.is_empty() && !new.is_empty() =>
            {
                Some(name.clone())
            }
            _ => None,
        })
        .filter(|name| !known.contains(name))
        .collect()
}

fn names(conn: &zbus::blocking::Connection) -> Vec<String> {
    let mut names: Vec<String> = DBusProxy::new(conn)
        .unwrap()
        .list_names()
        .unwrap()
        .into_iter()
        .map(|n| n.to_string())
        .collect();
    names.sort();
    names
}

/// AC13: `mpris = false`: the daemon never connects to the bus.
#[test]
fn ac13_mpris_off() {
    let bus = PrivateBus::start();
    let me = bus.connect();
    let changes = owner_changes(&bus);
    // The listener's own connection.
    std::thread::sleep(Duration::from_millis(200));
    let before = names(&me);
    let machine = Machine::new(Some(&bus));
    machine.app_toml("mpris = false\n");
    let mut daemon = machine.spawn(&["daemon"]);
    let (mut client, _) = subscribe(&machine.socket());
    request(
        &mut client,
        1,
        PlayerCommand::LoadQueue {
            tracks: vec![track(1, Some(COVER))],
            start: 0,
        },
    );
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(names(&me), before);
    let (code, stderr) = stop(&machine, &mut daemon);
    assert_eq!(code, Some(0), "{stderr}");
    assert!(!stderr.contains(UNAVAILABLE), "{stderr}");
    let seen = drain(&changes, Duration::from_millis(300));
    assert_eq!(
        new_connections(&seen, &before),
        Vec::<String>::new(),
        "{seen:?}"
    );
    assert!(!machine.cache.path().join("covers").exists());

    // The variable over the file: `TIDAL_PLAYER_MPRIS=off`.
    let machine = Machine::new(Some(&bus));
    machine.app_toml("mpris = true\n");
    let mut cmd = machine.command(&["daemon"]);
    cmd.env("TIDAL_PLAYER_MPRIS", "off");
    let mut daemon = Running(cmd.spawn().unwrap());
    let _ = subscribe(&machine.socket());
    std::thread::sleep(Duration::from_millis(300));
    let (code, stderr) = stop(&machine, &mut daemon);
    assert_eq!(code, Some(0), "{stderr}");
    let seen = drain(&changes, Duration::from_millis(300));
    assert_eq!(
        new_connections(&seen, &before),
        Vec::<String>::new(),
        "{seen:?}"
    );
}

/// AC14: clients never touch the bus: an attached TUI and `playback
/// status`, against a daemon with `mpris = false`.
#[test]
fn ac14_clients_stay_off_the_bus() {
    let bus = PrivateBus::start();
    let me = bus.connect();
    let changes = owner_changes(&bus);
    std::thread::sleep(Duration::from_millis(200));
    let before = names(&me);
    let machine = Machine::new(Some(&bus));
    machine.app_toml("mpris = false\n");
    let mut daemon = machine.spawn(&["daemon"]);
    let (mut watcher, _) = subscribe(&machine.socket());
    // No terminal here: the TUI attaches and sends its `Open`, then cannot
    // enter the TUI.
    let (_, _, stderr) = machine.run_client(&["--play-next", "1001"]);
    assert!(stderr.contains("terminal"), "{stderr}");
    wait_snapshot(&mut watcher, "track 1001", |s| {
        s.queue.iter().any(|e| e.track.id.0 == 1001)
    });
    let (code, stdout, stderr) = machine.run_client(&["playback", "status"]);
    assert_eq!(code, Some(0), "{stdout}{stderr}");
    let (code, stderr) = stop(&machine, &mut daemon);
    assert_eq!(code, Some(0), "{stderr}");
    let seen = drain(&changes, Duration::from_millis(300));
    assert_eq!(
        new_connections(&seen, &before),
        Vec::<String>::new(),
        "{seen:?}"
    );
    assert_eq!(names(&me), before);
}
