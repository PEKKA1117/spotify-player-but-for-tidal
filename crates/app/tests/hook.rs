//! Spec 0025 AC16: a real process gets what the spec promises.

use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use tidal_player::hook::{Job, Outcome, ProcessRunner, Runner, Stderr};

/// A job running `body` through `/bin/sh`: a script written here and
/// executed directly can fail with `ETXTBSY` when another test forks.
fn job(dir: &Path, body: &str, args: &[&str], env: Vec<(&'static str, String)>) -> Job {
    let path = dir.join("hook.sh");
    fs::write(&path, body).expect("the script is written");
    let mut all = vec![path.display().to_string()];
    all.extend(args.iter().map(|arg| (*arg).to_owned()));
    Job {
        command: "/bin/sh".into(),
        args: all,
        env,
    }
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
    let out_arg = out.display().to_string();
    let job = job(
        dir.path(),
        r#"out="$1"; shift
{ printf 'arg=%s\n' "$@"
  printf 'title=%s\n' "$TIDAL_PLAYER_TRACK_TITLE"
  printf 'stdin=%s\n' "$(cat)"
} > "$out"
echo noise"#,
        &[&out_arg, "Changed", "12 3"],
        vec![("TIDAL_PLAYER_TRACK_TITLE", "Song (Live)".into())],
    );
    assert_eq!(runner(LONG).run(&job), Outcome::Done);
    let got = fs::read_to_string(&out).expect("the script wrote its file");
    assert_eq!(got, "arg=Changed\narg=12 3\ntitle=Song (Live)\nstdin=\n");
}

#[test]
fn ac16_a_failing_script() {
    let dir = tempfile::tempdir().expect("a directory");
    let job = job(dir.path(), "exit 3", &[], Vec::new());
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
    let job = job(dir.path(), "exec sleep 30", &[], Vec::new());
    let limit = Duration::from_millis(100);
    let start = Instant::now();
    assert_eq!(runner(limit).run(&job), Outcome::TimedOut(limit));
    assert!(start.elapsed() < Duration::from_secs(5));
}
