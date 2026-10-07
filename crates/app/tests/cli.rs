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
    bin()
        .arg("devices")
        .env("TIDAL_PLAYER_ASOUND_DIR", asound_fixture("onboard_usb"))
        .env_remove("TIDAL_PLAYER_DEVICE")
        .assert()
        .success()
        .stdout(listing("default"));
    bin()
        .arg("devices")
        .env("TIDAL_PLAYER_ASOUND_DIR", asound_fixture("onboard_usb"))
        .env("TIDAL_PLAYER_DEVICE", "hw:1,0")
        .assert()
        .success()
        .stdout(listing("hw:1,0"));
    bin()
        .arg("devices")
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
