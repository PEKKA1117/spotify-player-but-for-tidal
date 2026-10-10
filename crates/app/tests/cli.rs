//! AC4: `--version` and `--help` of the `tidal-player` binary.

use assert_cmd::Command;
use predicates::prelude::*;

fn bin() -> Command {
    Command::new(assert_cmd::cargo::cargo_bin!("tidal-player"))
}

#[test]
fn ac4_version() {
    bin()
        .arg("--version")
        .assert()
        .success()
        .stdout(format!("tidal-player {}\n", env!("CARGO_PKG_VERSION")));
}

#[test]
fn ac4_help() {
    bin()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage:"));
}

// AC12: login-related subcommands, against a temp state dir and no keyring.

use std::path::Path;
use std::time::{Duration, SystemTime};

use tidal_player::passphrase::{PassphraseEnv, ProcessPassphrase, Prompt};
use tidal_player::session_store::EncryptedFileStore;
use tidal_player_api::auth::{Session, SessionStore};
use wiremock::MockServer;

struct NoPrompt;

impl Prompt for NoPrompt {
    fn ask(&mut self, _prompt: &str) -> std::io::Result<String> {
        Err(std::io::Error::other("no prompt in tests"))
    }
}

/// A binary invocation with a controlled login environment.
fn bin_in(state: &Path) -> Command {
    let mut cmd = bin();
    cmd.env("TIDAL_PLAYER_STATE_DIR", state)
        // Each test its own player lock (spec 0005 "Transport").
        .env("TIDAL_PLAYER_RUNTIME_DIR", state.join("run"))
        .env("TIDAL_PLAYER_NO_KEYRING", "1")
        .env_remove("TIDAL_PLAYER_PASSPHRASE_FILE")
        .env_remove("CREDENTIALS_DIRECTORY")
        .write_stdin("");
    cmd
}

/// Writes `<state>/session.age`, encrypted with a known test passphrase.
fn write_session_file(state: &Path) {
    let pass_file = state.join("test-passphrase");
    std::fs::write(&pass_file, "test-passphrase\n").unwrap();
    let source = ProcessPassphrase::new(
        PassphraseEnv {
            passphrase_file: Some(pass_file.clone()),
            credentials_dir: None,
            interactive: false,
        },
        Box::new(NoPrompt),
    );
    EncryptedFileStore::new(state.join("session.age"), std::sync::Arc::new(source))
        .save(&Session {
            access_token: "FAKE-ACCESS".into(),
            refresh_token: "FAKE-REFRESH".into(),
            expires_at: SystemTime::now() + Duration::from_secs(3600),
            user_id: 7,
            country_code: "NO".into(),
        })
        .unwrap();
    std::fs::remove_file(pass_file).unwrap();
}

#[test]
fn ac12_daemon_needs_login() {
    let dir = tempfile::tempdir().unwrap();
    bin_in(dir.path())
        .arg("daemon")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("tidal-player login"));
}

#[test]
fn ac12_logout_when_logged_out() {
    let dir = tempfile::tempdir().unwrap();
    bin_in(dir.path())
        .arg("logout")
        .assert()
        .code(0)
        .stdout(predicate::str::contains("Not logged in"));
}

#[tokio::test(flavor = "multi_thread")]
async fn ac12_logout_deletes_locally() {
    let dir = tempfile::tempdir().unwrap();
    write_session_file(dir.path());
    let mock = MockServer::start().await;
    let state = dir.path().to_owned();
    let proxy = mock.uri();
    tokio::task::spawn_blocking(move || {
        bin_in(&state)
            .env("HTTPS_PROXY", &proxy)
            .env("HTTP_PROXY", &proxy)
            .env("ALL_PROXY", &proxy)
            .arg("logout")
            .assert()
            .code(0)
            .stdout(predicate::str::contains("Logged out"));
    })
    .await
    .unwrap();
    assert!(!dir.path().join("session.age").exists());
    assert!(mock.received_requests().await.unwrap().is_empty());
}

/// Spec 0009 AC13: `logout` also deletes `playback.json` and
/// `playback.json.bad`; its output is unchanged.
#[test]
fn ac13_logout_forgets_playback() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("playback.json"), "{}").unwrap();
    std::fs::write(dir.path().join("playback.json.bad"), "{").unwrap();
    bin_in(dir.path())
        .arg("logout")
        .assert()
        .code(0)
        .stdout("Not logged in\n")
        .stderr("");
    assert!(!dir.path().join("playback.json").exists());
    assert!(!dir.path().join("playback.json.bad").exists());
}

#[test]
fn ac12_daemon_needs_passphrase() {
    let dir = tempfile::tempdir().unwrap();
    write_session_file(dir.path());
    bin_in(dir.path())
        .arg("daemon")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("TIDAL_PLAYER_PASSPHRASE_FILE"));
}

// Spec 0003 AC25, AC26: `devices` and `play`.

/// A fixture directory of `/proc/asound` contents (crates/audio/tests/fixtures/asound).
fn asound_fixture(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../audio/tests/fixtures/asound")
        .join(name)
}

#[test]
fn ac25_devices_marks_configured() {
    let listing = |configured: &str| {
        let mark = |name: &str| if name == configured { "*" } else { " " };
        format!(
            "{} default  shared, through the system mixer\n\
             {} hw:0,0   HDA Intel PCH: ALC892 Analog\n\
             {} hw:0,1   HDA Intel PCH: ALC892 Digital\n\
             {} hw:1,0   E30 II: USB Audio\n",
            mark("default"),
            mark("hw:0,0"),
            mark("hw:0,1"),
            mark("hw:1,0"),
        )
    };
    // No player: an empty runtime dir of its own, so a player running on
    // this machine is not asked (spec 0014 AC13).
    let run = tempfile::tempdir().unwrap();
    bin()
        .arg("devices")
        .env("TIDAL_PLAYER_RUNTIME_DIR", run.path())
        .env("TIDAL_PLAYER_ASOUND_DIR", asound_fixture("onboard_usb"))
        .env_remove("TIDAL_PLAYER_DEVICE")
        .assert()
        .success()
        .stdout(listing("default"));
    bin()
        .arg("devices")
        .env("TIDAL_PLAYER_RUNTIME_DIR", run.path())
        .env("TIDAL_PLAYER_ASOUND_DIR", asound_fixture("onboard_usb"))
        .env("TIDAL_PLAYER_DEVICE", "hw:1,0")
        .assert()
        .success()
        .stdout(listing("hw:1,0"));
    bin()
        .arg("devices")
        .env("TIDAL_PLAYER_RUNTIME_DIR", run.path())
        .env("TIDAL_PLAYER_ASOUND_DIR", asound_fixture("no_cards"))
        .env_remove("TIDAL_PLAYER_DEVICE")
        .assert()
        .success()
        .stdout("* default  shared, through the system mixer\n");
}

#[test]
fn ac26_bad_quality() {
    let state = tempfile::tempdir().unwrap();
    write_session_file(state.path());
    bin_in(state.path())
        .args(["play", "123", "--quality", "low"])
        .env_remove("TIDAL_PLAYER_QUALITY")
        .assert()
        .code(2)
        .stderr(predicate::str::contains("--quality").and(predicate::str::contains("HE-AAC")));
    bin_in(state.path())
        .args(["play", "123"])
        .env("TIDAL_PLAYER_QUALITY", "ultra")
        .assert()
        .code(2)
        .stderr(predicate::str::contains("TIDAL_PLAYER_QUALITY"));
}

#[test]
fn ac26_play_needs_login() {
    let state = tempfile::tempdir().unwrap();
    bin_in(state.path())
        .args(["play", "123"])
        .env_remove("TIDAL_PLAYER_QUALITY")
        .assert()
        .code(1)
        .stdout("")
        .stderr("Not logged in: run \"tidal-player login\"\n");
}

// AC26, end to end against a mock API (debug builds honour
// TIDAL_PLAYER_API_BASE), a mock stream server, no session bus and an ALSA
// device that does not exist: resolution, fetch, decode and the error
// lines, without sound.

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
        "trackId": 123,
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

#[test]
fn ac26_play_end_to_end_errors() {
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, ResponseTemplate};

    let rt = tokio::runtime::Runtime::new().unwrap();
    let server = rt.block_on(MockServer::start());
    let flac = std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../audio/tests/fixtures/flac16_44.flac"),
    )
    .unwrap();
    rt.block_on(async {
        Mock::given(method("GET"))
            .and(path("/tracks/123/playbackinfopostpaywall"))
            .and(query_param("audioquality", "LOSSLESS"))
            .and(query_param("countryCode", "NO"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(playback_info(&format!("{}/t.flac", server.uri()))),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/tracks/404/playbackinfopostpaywall"))
            .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({
                "status": 401, "subStatus": 4005, "userMessage": "Asset is not ready for playback"
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/t.flac"))
            .respond_with(RangeFile(flac))
            .mount(&server)
            .await;
    });
    let state = tempfile::tempdir().unwrap();
    write_session_file(state.path());
    let pass_file = state.path().join("pass");
    std::fs::write(&pass_file, "test-passphrase\n").unwrap();
    let play = |args: &[&str]| {
        let mut cmd = bin_in(state.path());
        cmd.arg("play")
            .args(args)
            .env("TIDAL_PLAYER_PASSPHRASE_FILE", &pass_file)
            .env("TIDAL_PLAYER_API_BASE", server.uri())
            .env("DBUS_SESSION_BUS_ADDRESS", "unix:path=/nonexistent/bus")
            .env_remove("TIDAL_PLAYER_QUALITY")
            .env_remove("TIDAL_PLAYER_DEVICE")
            .timeout(Duration::from_secs(30));
        cmd
    };

    // AC28 (spec 0003 Bugs): exactly one line, for a missing card and for
    // an unknown PCM name; alsa-lib's own diagnostics must not leak.
    for device in ["hw:99,0", "tidal_player_no_such_pcm"] {
        play(&["123", "--quality", "lossless", "--device", device])
            .assert()
            .code(1)
            .stdout("")
            .stderr(format!(
                "No such output device {device}: see \"tidal-player devices\"\n"
            ));
    }
    play(&["404"])
        .assert()
        .code(1)
        .stderr("Track 404 is not available in NO\n");
}

// Spec 0004 AC18: a bad item is refused before anything plays (exit 2),
// before the session is even looked at.

#[test]
fn ac18_bad_item() {
    let state = tempfile::tempdir().unwrap();
    let artist = "https://tidal.com/browse/artist/1";
    let refused = format!("Not a Tidal track, album or playlist: {artist}\n");
    bin_in(state.path())
        .args(["play", "123", artist])
        .env_remove("TIDAL_PLAYER_QUALITY")
        .assert()
        .code(2)
        .stdout("")
        .stderr(refused.clone());
    // `tidal-player [ITEM]...` too, before a login prompt or the terminal.
    bin_in(state.path())
        .args(["123", artist])
        .assert()
        .code(2)
        .stdout("")
        .stderr(refused);
}

// Spec 0004 AC28: `--add-to-queue` and `--play-next` are mutually exclusive
// and need an item (exit 2). The invalid quality makes sure no case gets as
// far as a login prompt: the flags are refused before any setting is read.

#[test]
fn ac28_queue_flags() {
    let state = tempfile::tempdir().unwrap();
    let run = |args: &[&str]| {
        bin_in(state.path())
            .args(args)
            .env("TIDAL_PLAYER_QUALITY", "bogus")
            .assert()
            .code(2)
            .stdout("")
    };
    run(&["--add-to-queue", "--play-next", "123"])
        .stderr(predicate::str::contains("cannot be used with"));
    for flag in ["--add-to-queue", "--play-next"] {
        run(&[flag]).stderr(predicate::str::contains("required"));
        // With an item, the flag is accepted and the item is checked.
        let artist = "https://tidal.com/browse/artist/1";
        run(&[flag, artist]).stderr(format!("Not a Tidal track, album or playlist: {artist}\n"));
    }
}

/// Spec 0009 AC9: `tidal-player play ITEM` neither reads nor writes the
/// remembered state: an existing `playback.json` is left byte-for-byte as
/// it was, and an empty state dir gets none.
#[test]
fn ac9_play_leaves_state_alone() {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, ResponseTemplate};

    let rt = tokio::runtime::Runtime::new().unwrap();
    let server = rt.block_on(MockServer::start());
    rt.block_on(
        Mock::given(method("GET"))
            .and(path("/tracks/404/playbackinfopostpaywall"))
            .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({
                "status": 401, "subStatus": 4005, "userMessage": "Asset is not ready for playback"
            })))
            .mount(&server),
    );
    let remembered = serde_json::to_vec(&tidal_player_core::SavedPlayback {
        volume: 40,
        shuffle: true,
        ..tidal_player_core::SavedPlayback::default()
    })
    .unwrap();
    for existing in [Some(remembered), None] {
        let state = tempfile::tempdir().unwrap();
        write_session_file(state.path());
        let file = state.path().join("playback.json");
        if let Some(bytes) = &existing {
            std::fs::write(&file, bytes).unwrap();
        }
        let pass_file = state.path().join("pass");
        std::fs::write(&pass_file, "test-passphrase\n").unwrap();
        bin_in(state.path())
            .args(["play", "404"])
            .env("TIDAL_PLAYER_PASSPHRASE_FILE", &pass_file)
            .env("TIDAL_PLAYER_API_BASE", server.uri())
            .env("DBUS_SESSION_BUS_ADDRESS", "unix:path=/nonexistent/bus")
            .env_remove("TIDAL_PLAYER_QUALITY")
            .env_remove("TIDAL_PLAYER_DEVICE")
            .env_remove("TIDAL_PLAYER_REMEMBER_PLAYBACK")
            .timeout(Duration::from_secs(30))
            .assert()
            .code(1)
            .stderr("Track 404 is not available in NO\n");
        match &existing {
            Some(bytes) => assert_eq!(&std::fs::read(&file).unwrap(), bytes),
            None => assert!(!file.exists(), "play created playback.json"),
        }
    }
}
