//! Spec 0025 AC16: a real process gets what the spec promises.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::{Duration, Instant};

use tidal_player::hook::{Job, Outcome, ProcessRunner, Runner, Stderr};

fn script(dir: &Path, body: &str) -> String {
    let path = dir.join("hook.sh");
    fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("the script is written");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("it is executable");
    path.display().to_string()
}

fn runner(limit: Duration) -> ProcessRunner {
    ProcessRunner {
        stderr: Stderr::Drop,
        limit,
    }
}

const LONG: Duration = Duration::from_secs(20);

#[test]
fn ac16_real_process() {
    let dir = tempfile::tempdir().expect("a directory");
    let out = dir.path().join("out");
    let job = Job {
        command: script(
            dir.path(),
            r#"out="$1"; shift
{ printf 'arg=%s\n' "$@"
  printf 'title=%s\n' "$TIDAL_PLAYER_TRACK_TITLE"
  printf 'stdin=%s\n' "$(cat)"
} > "$out"
echo noise"#,
        ),
        args: vec![out.display().to_string(), "Changed".into(), "12 3".into()],
        env: vec![("TIDAL_PLAYER_TRACK_TITLE", "Song (Live)".into())],
    };
    assert_eq!(runner(LONG).run(&job), Outcome::Done);
    let got = fs::read_to_string(&out).expect("the script wrote its file");
    assert_eq!(got, "arg=Changed\narg=12 3\ntitle=Song (Live)\nstdin=\n");
}

#[test]
fn ac16_a_failing_script() {
    let dir = tempfile::tempdir().expect("a directory");
    let job = Job {
        command: script(dir.path(), "exit 3"),
        args: Vec::new(),
        env: Vec::new(),
    };
    assert_eq!(
        runner(LONG).run(&job),
        Outcome::Failed("exited with status 3".into())
    );
}

#[test]
fn ac16_a_missing_command() {
    let job = Job {
        command: "/nonexistent/hook".into(),
        args: Vec::new(),
        env: Vec::new(),
    };
    match runner(LONG).run(&job) {
        Outcome::CouldNotStart(message) => assert!(message.contains("No such file"), "{message}"),
        other => panic!("expected CouldNotStart, got {other:?}"),
    }
}

#[test]
fn ac16_killed_at_the_limit() {
    let dir = tempfile::tempdir().expect("a directory");
    let job = Job {
        command: script(dir.path(), "exec sleep 30"),
        args: Vec::new(),
        env: Vec::new(),
    };
    let limit = Duration::from_millis(100);
    let start = Instant::now();
    assert_eq!(runner(limit).run(&job), Outcome::TimedOut(limit));
    assert!(start.elapsed() < Duration::from_secs(5));
}
