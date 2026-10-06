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
