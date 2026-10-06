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
