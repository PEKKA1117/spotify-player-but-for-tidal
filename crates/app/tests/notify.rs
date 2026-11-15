//! Spec 0029, end to end with the binary: the daemon notifying on a private
//! session bus with a recording notification server (AC15), on one with no
//! server (AC15) and with no bus at all (AC14).
//!
//! Each test starts its own `dbus-daemon` (as `tests/mpris.rs` does) and
//! fails, never skips, when it is missing. The player side is 0005's: a
//! state dir with a session, a runtime dir and a mock API serving a short
//! FLAC; the daemon plays to ALSA's `null` PCM, which takes no card and
//! makes no sound. No test here reaches the developer's session bus.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use tidal_player::ipc::client::Connection;
use tidal_player::session::passphrase::{PassphraseEnv, ProcessPassphrase, Prompt};
use tidal_player::session::store::EncryptedFileStore;
use tidal_player_api::auth::{Session, SessionStore};
use tidal_player_core::protocol::{
    ClientMessage, Command as PlayerCommand, Event, PlaybackState, PlayerSnapshot, ServerMessage,
};
use tidal_player_core::{AlbumRef, ArtistRef, Track, TrackId};
use zbus::zvariant::OwnedValue;

const COVER: &str = "2e4a5d2d-9a0d-4c3a-a0ba-42b0bd16a6ec";
const LONG: Duration = Duration::from_secs(20);

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
}

impl Drop for PrivateBus {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

// --- the recording notification server ---------------------------------------------

/// One `Notify` call as the server got it.
#[derive(Debug, Clone, PartialEq)]
struct Notified {
    app_name: String,
    replaces_id: u32,
    app_icon: String,
    summary: String,
    body: String,
    actions: Vec<String>,
    image_path: Option<String>,
    transient: Option<bool>,
    other_hints: Vec<String>,
    expire_timeout: i32,
    /// The ID it answered.
    id: u32,
}

#[derive(Clone, Default)]
struct Recorder {
    calls: Arc<Mutex<Vec<Notified>>>,
}

#[zbus::interface(name = "org.freedesktop.Notifications")]
impl Recorder {
    fn get_capabilities(&self) -> Vec<String> {
        vec!["body".into(), "body-markup".into()]
    }

    #[allow(clippy::too_many_arguments)]
    fn notify(
        &self,
        app_name: String,
        replaces_id: u32,
        app_icon: String,
        summary: String,
        body: String,
        actions: Vec<String>,
        hints: HashMap<String, OwnedValue>,
        expire_timeout: i32,
    ) -> u32 {
        let mut calls = self.calls.lock().unwrap();
        let id = 100 + calls.len() as u32;
        calls.push(Notified {
            app_name,
            replaces_id,
            app_icon,
            summary,
            body,
            actions,
            image_path: hints
                .get("image-path")
                .map(|v| String::try_from(v.try_clone().unwrap()).unwrap()),
            transient: hints
                .get("transient")
                .map(|v| bool::try_from(v.try_clone().unwrap()).unwrap()),
            other_hints: hints
                .keys()
                .filter(|k| !matches!(k.as_str(), "image-path" | "transient"))
                .cloned()
                .collect(),
            expire_timeout,
            id,
        });
        id
    }

    fn get_server_information(&self) -> (String, String, String, String) {
        ("recorder".into(), "test".into(), "0".into(), "1.2".into())
    }
}

/// Serves a [`Recorder`] as `org.freedesktop.Notifications` on `bus`.
fn serve_recorder(bus: &PrivateBus) -> (zbus::blocking::Connection, Recorder) {
    let recorder = Recorder::default();
    let conn = zbus::blocking::connection::Builder::address(bus.address.as_str())
        .unwrap()
        .name("org.freedesktop.Notifications")
        .unwrap()
        .serve_at("/org/freedesktop/Notifications", recorder.clone())
        .unwrap()
        .build()
        .unwrap();
    (conn, recorder)
}

// --- the player's machine ----------------------------------------------------------

struct NoPrompt;

impl Prompt for NoPrompt {
    fn ask(&mut self, _prompt: &str) -> std::io::Result<String> {
        Err(std::io::Error::other("no prompt in tests"))
    }
}

/// Serves a file with `Range: bytes=N-` support.
struct RangeFile(Vec<u8>);

impl wiremock::Respond for RangeFile {
    fn respond(&self, request: &wiremock::Request) -> wiremock::ResponseTemplate {
        let len = self.0.len();
        let from = request
            .headers
            .get("range")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("bytes="))
            .and_then(|v| v.trim_end_matches('-').parse::<usize>().ok());
        match from {
            Some(from) if from >= len => wiremock::ResponseTemplate::new(416),
            Some(from) => wiremock::ResponseTemplate::new(206)
                .insert_header(
                    "content-range",
                    format!("bytes {from}-{}/{len}", len - 1).as_str(),
                )
                .set_body_bytes(self.0[from..].to_vec()),
            None => wiremock::ResponseTemplate::new(200).set_body_bytes(self.0.clone()),
        }
    }
}

fn playback_info(stream_url: &str) -> serde_json::Value {
    use base64::Engine as _;
    let manifest = serde_json::json!({
        "mimeType": "audio/flac",
        "codecs": "flac",
        "encryptionType": "NONE",
        "urls": [stream_url],
    });
    serde_json::json!({
        "trackId": 1,
        "assetPresentation": "FULL",
        "audioMode": "STEREO",
        "audioQuality": "LOSSLESS",
        "manifestMimeType": "application/vnd.tidal.bts",
        "manifestHash": "FAKE-HASH",
        "manifest": base64::engine::general_purpose::STANDARD.encode(manifest.to_string()),
        "bitDepth": 16,
        "sampleRate": 44100,
    })
}

/// A state dir with a session, runtime, config and cache dirs, and a mock
/// API that streams a one-second FLAC for every track and serves the cover.
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
        EncryptedFileStore::new(state.path().join("session.age"), Arc::new(source))
            .save(&Session {
                access_token: "FAKE-ACCESS".into(),
                refresh_token: "FAKE-REFRESH".into(),
                expires_at: SystemTime::now() + Duration::from_secs(3600),
                user_id: 7,
                country_code: "NO".into(),
            })
            .unwrap();
        let flac = std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../audio/tests/fixtures/flac16_44.flac"),
        )
        .unwrap();
        let tokio = tokio::runtime::Runtime::new().unwrap();
        let api = tokio.block_on(async {
            let api = MockServer::start().await;
            Mock::given(method("GET"))
                .and(path_regex(r"^/tracks/\d+/playbackinfopostpaywall$"))
                .respond_with(
                    ResponseTemplate::new(200)
                        .set_body_json(playback_info(&format!("{}/t.flac", api.uri()))),
                )
                .mount(&api)
                .await;
            Mock::given(method("GET"))
                .and(path("/t.flac"))
                .respond_with(RangeFile(flac))
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
            // Silent, and no card taken.
            .env("TIDAL_PLAYER_DEVICE", "null")
            .env("TIDAL_PLAYER_QUALITY", "lossless")
            .env("TIDAL_PLAYER_REMEMBER_PLAYBACK", "off")
            .env_remove("TIDAL_PLAYER_ENABLE_NOTIFY")
            .env_remove("TIDAL_PLAYER_NOTIFY_TIMEOUT_IN_SECS")
            .env_remove("TIDAL_PLAYER_NOTIFY_TRANSIENT")
            .env_remove("TIDAL_PLAYER_MAX_COVER_ARTS")
            .env_remove("TIDAL_PLAYER_LOG_LEVEL")
            .env_remove("CREDENTIALS_DIRECTORY")
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
        Running(cmd.spawn().unwrap()).output(LONG)
    }

    fn socket(&self) -> PathBuf {
        self.run.join("player.sock")
    }

    /// The daemon's log lines that contain `text`.
    fn log_lines(&self, text: &str) -> Vec<String> {
        std::fs::read_to_string(self.state.path().join("tidal-player.log"))
            .unwrap_or_default()
            .lines()
            .filter(|line| line.contains(text))
            .map(str::to_owned)
            .collect()
    }
}

/// A child killed when dropped.
struct Running(Child);

impl Running {
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
fn subscribe(socket: &Path) -> Connection {
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
        Ok(Some(ServerMessage::Welcome { .. })) => conn,
        other => panic!("expected a Welcome, got {other:?}"),
    }
}

/// Loads `tracks` and waits until the player has played through them:
/// the snapshots in between, the last one stopped.
fn play_through(conn: &mut Connection, tracks: Vec<Track>) -> Vec<PlayerSnapshot> {
    let last = tracks.last().map(|t| t.id).unwrap();
    conn.send(&ClientMessage::Request {
        id: 1,
        command: PlayerCommand::LoadQueue { tracks, start: 0 },
    })
    .unwrap();
    let deadline = Instant::now() + LONG;
    let mut seen = Vec::new();
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match conn.recv(Some(left)) {
            Ok(Some(ServerMessage::Event(Event::Player(s)))) => {
                let done = s.state == PlaybackState::Stopped
                    && seen.iter().any(|seen: &PlayerSnapshot| {
                        seen.state == PlaybackState::Playing && playing_id(seen) == Some(last)
                    });
                assert_eq!(s.message, None, "the player's message");
                seen.push(s);
                if done {
                    return seen;
                }
            }
            Ok(Some(ServerMessage::Reply { result, .. })) => assert_eq!(result, Ok(())),
            Ok(Some(_)) => {}
            Ok(None) => panic!("the queue did not play through within {LONG:?}: {seen:?}"),
            Err(e) => panic!("waiting for the queue: {e}"),
        }
    }
}

fn playing_id(snapshot: &PlayerSnapshot) -> Option<TrackId> {
    let current = snapshot.current?;
    let entry = snapshot.queue.iter().find(|e| e.id == current)?;
    Some(entry.track.id)
}

fn track(id: u64, title: &str, album: &str, cover: Option<&str>) -> Track {
    Track {
        id: TrackId(id),
        title: title.to_owned(),
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
            title: album.to_owned(),
            cover: cover.map(str::to_owned),
        }),
        duration: Some(Duration::from_secs(1)),
        streamable: true,
    }
}

/// `daemon stop`, then the daemon's exit: its code and stderr.
fn stop(machine: &Machine, daemon: &mut Running) -> (Option<i32>, String) {
    assert_eq!(machine.run_client(&["daemon", "stop"]).0, Some(0));
    let (code, _, stderr) = daemon.output(LONG);
    (code, stderr)
}

const FAILED: &str = "Desktop notification failed:";
const NO_BUS: &str = "No session bus, desktop notifications are off";

/// AC15: two tracks, two `Notify` calls with the expected arguments, the
/// second replacing the first.
#[test]
fn ac15_real_bus() {
    let bus = PrivateBus::start();
    let (_server, recorder) = serve_recorder(&bus);
    let machine = Machine::new(Some(&bus));
    let mut daemon = machine.spawn(&["daemon"]);
    let mut client = subscribe(&machine.socket());
    play_through(
        &mut client,
        vec![
            track(1, "Rock & Roll <Live>", "A & B", Some(COVER)),
            track(2, "Second", "Two", None),
        ],
    );
    let deadline = Instant::now() + LONG;
    while recorder.calls.lock().unwrap().len() < 2 {
        assert!(
            Instant::now() < deadline,
            "two notifications within {LONG:?}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    std::thread::sleep(Duration::from_millis(300));
    let calls = recorder.calls.lock().unwrap().clone();
    let cover = std::fs::canonicalize(machine.cache.path())
        .unwrap()
        .join("covers")
        .join(format!("{COVER}.jpg"));
    let want = |replaces_id, summary: &str, body: &str, image: Option<String>, id| Notified {
        app_name: "tidal-player".into(),
        replaces_id,
        app_icon: String::new(),
        summary: summary.into(),
        body: body.into(),
        actions: Vec::new(),
        image_path: image,
        transient: None,
        other_hints: Vec::new(),
        expire_timeout: -1,
        id,
    };
    assert_eq!(
        calls,
        [
            want(
                0,
                "Rock & Roll <Live> • Artist, Other",
                // The recorder reads markup: the body is escaped.
                "A &amp; B",
                Some(cover.display().to_string()),
                100
            ),
            want(100, "Second • Artist, Other", "Two", None, 101),
        ]
    );
    let (code, stderr) = stop(&machine, &mut daemon);
    assert_eq!(code, Some(0), "{stderr}");
    assert_eq!(machine.log_lines(FAILED), Vec::<String>::new());
}

/// AC15: a bus without a notification server: the daemon plays on and
/// logs one `warn` line.
#[test]
fn ac15_no_server() {
    let bus = PrivateBus::start();
    let machine = Machine::new(Some(&bus));
    let mut daemon = machine.spawn(&["daemon"]);
    let mut client = subscribe(&machine.socket());
    play_through(
        &mut client,
        vec![track(1, "One", "A", None), track(2, "Two", "B", None)],
    );
    let (code, stdout, stderr) = machine.run_client(&["playback", "status"]);
    assert_eq!(code, Some(0), "{stdout}{stderr}");
    let (code, stderr) = stop(&machine, &mut daemon);
    assert_eq!(code, Some(0), "{stderr}");
    let lines = machine.log_lines(FAILED);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(lines[0].contains("WARN"), "{lines:?}");
}

/// AC14: no session bus at all: the daemon starts, plays and serves a
/// client, and logs one `info` line; nothing is tried again.
#[test]
fn ac14_no_bus() {
    let machine = Machine::new(None);
    let mut daemon = machine.spawn(&["daemon"]);
    let mut client = subscribe(&machine.socket());
    play_through(
        &mut client,
        vec![track(1, "One", "A", None), track(2, "Two", "B", None)],
    );
    let (code, stdout, stderr) = machine.run_client(&["playback", "status"]);
    assert_eq!(code, Some(0), "{stdout}{stderr}");
    let (code, stderr) = stop(&machine, &mut daemon);
    assert_eq!(code, Some(0), "{stderr}");
    let lines = machine.log_lines(NO_BUS);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(lines[0].contains("INFO"), "{lines:?}");
    assert_eq!(machine.log_lines(FAILED), Vec::<String>::new());
}
