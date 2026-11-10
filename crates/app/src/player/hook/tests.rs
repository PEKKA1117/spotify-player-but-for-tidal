use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use super::*;

const LONG: Duration = Duration::from_secs(5);

fn command() -> HookCommand {
    HookCommand {
        command: "on-event".into(),
        args: vec!["--quiet".into()],
    }
}

/// Answers with `outcomes` in turn (then `Done`), telling `ran` each
/// job, and holds each run until `gate` lets it go, when it has one.
struct FakeRunner {
    outcomes: Mutex<VecDeque<Outcome>>,
    ran: Sender<Job>,
    gate: Option<Mutex<Receiver<()>>>,
    running: Arc<AtomicUsize>,
    most: Arc<AtomicUsize>,
}

impl Runner for FakeRunner {
    fn run(&self, job: &Job) -> Outcome {
        let now = self.running.fetch_add(1, Ordering::SeqCst) + 1;
        self.most.fetch_max(now, Ordering::SeqCst);
        let _ = self.ran.send(job.clone());
        if let Some(gate) = &self.gate {
            let _ = gate.lock().unwrap().recv();
        }
        self.running.fetch_sub(1, Ordering::SeqCst);
        self.outcomes
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(Outcome::Done)
    }
}

struct Rig {
    hook: Hook,
    ran: Receiver<Job>,
    reports: Receiver<String>,
    most: Arc<AtomicUsize>,
}

fn rig(outcomes: Vec<Outcome>, gate: Option<Receiver<()>>, wait: Duration) -> Rig {
    let (ran_tx, ran) = mpsc::channel();
    let (report_tx, reports) = mpsc::channel();
    let most = Arc::new(AtomicUsize::new(0));
    let runner = FakeRunner {
        outcomes: Mutex::new(outcomes.into()),
        ran: ran_tx,
        gate: gate.map(Mutex::new),
        running: Arc::default(),
        most: Arc::clone(&most),
    };
    let report = move |message| {
        let _ = report_tx.send(message);
    };
    Rig {
        hook: Hook::new(command(), runner, report, wait),
        ran,
        reports,
        most,
    }
}

fn volume(percent: u8) -> Vec<HookEvent> {
    vec![HookEvent::VolumeChanged(percent)]
}

/// The args of the next job to run.
fn next(ran: &Receiver<Job>) -> Vec<String> {
    ran.recv_timeout(LONG).expect("a job ran").args
}

/// AC8: a job is the command, its `args`, then the event's.
#[test]
fn ac8_job_of_an_event() {
    let mut rig = rig(Vec::new(), None, LONG);
    rig.hook.send(volume(40));
    let job = rig.ran.recv_timeout(LONG).expect("a job ran");
    assert_eq!(job.command, "on-event");
    assert_eq!(job.args, ["--quiet", "VolumeChanged", "40"]);
    assert!(job.env.is_empty());
}

/// AC9: in order, never two at once.
#[test]
fn ac9_one_at_a_time() {
    let (open, gate) = mpsc::channel();
    let mut rig = rig(Vec::new(), Some(gate), LONG);
    rig.hook.send(volume(1));
    rig.hook.send(vec![HookEvent::DeviceChanged("a".into())]);
    rig.hook.send(volume(3));
    let first = next(&rig.ran);
    assert!(
        rig.ran.recv_timeout(Duration::from_millis(50)).is_err(),
        "the second waits for the first"
    );
    open.send(()).unwrap();
    let second = next(&rig.ran);
    open.send(()).unwrap();
    let third = next(&rig.ran);
    open.send(()).unwrap();
    assert_eq!(first[1..], ["VolumeChanged", "1"]);
    assert_eq!(second[1..], ["DeviceChanged", "a"]);
    assert_eq!(third[1..], ["VolumeChanged", "3"]);
    assert_eq!(rig.most.load(Ordering::SeqCst), 1);
}

/// AC11: each failure's words, said once until a success.
#[test]
fn ac11_failures() {
    let enoent = "No such file or directory (os error 2)".to_owned();
    let rows: Vec<(&str, Outcome, String)> = vec![
        (
            "cannot start",
            Outcome::CouldNotStart(enoent.clone()),
            format!("Player event hook failed: on-event: {enoent}"),
        ),
        (
            "non-zero exit",
            Outcome::Failed("exited with status 3".into()),
            "Player event hook failed: on-event exited with status 3".into(),
        ),
        (
            "time limit",
            Outcome::TimedOut(TIME_LIMIT),
            "Player event hook failed: on-event was killed after 30 s".into(),
        ),
    ];
    for (name, outcome, message) in rows {
        let outcomes = vec![
            outcome.clone(),
            outcome.clone(),
            Outcome::Done,
            outcome,
            Outcome::Done,
        ];
        let mut rig = rig(outcomes, None, LONG);
        for percent in 0..5 {
            rig.hook.send(volume(percent * 10));
            next(&rig.ran);
        }
        rig.hook.finish();
        let got: Vec<String> = rig.reports.try_iter().collect();
        assert_eq!(got, [message.clone(), message], "{name}");
    }
}

/// AC9, AC11: events dropped from a full line are said, once.
#[test]
fn ac11_too_slow() {
    let (open, gate) = mpsc::channel();
    let mut rig = rig(Vec::new(), Some(gate), Duration::ZERO);
    rig.hook.send(volume(1));
    next(&rig.ran);
    let limit = tidal_player_core::hook::MAX_WAITING as u64;
    for id in 0..limit + 3 {
        rig.hook
            .send(vec![HookEvent::DeviceChanged(id.to_string())]);
    }
    drop(open);
    let got: Vec<String> = rig.reports.try_iter().collect();
    assert_eq!(got, ["Player event hook is too slow: events were dropped"]);
}

/// AC12: a command that runs does not hold up the sender.
#[test]
fn ac12_never_blocks() {
    let (open, gate) = mpsc::channel();
    let mut rig = rig(Vec::new(), Some(gate), LONG);
    rig.hook.send(volume(1));
    next(&rig.ran);
    let start = Instant::now();
    rig.hook.send(volume(2));
    rig.hook.send(vec![HookEvent::DeviceChanged("a".into())]);
    assert!(start.elapsed() < Duration::from_secs(1));
    drop(open);
}

/// AC14: what is waiting runs before the player goes, for as long as
/// the wait lasts; a stuck command does not hold it up.
#[test]
fn ac14_shutting_down() {
    let mut rig1 = rig(Vec::new(), None, LONG);
    rig1.hook.send(volume(1));
    rig1.hook.send(vec![HookEvent::DeviceChanged("a".into())]);
    rig1.hook.finish();
    assert_eq!(rig1.ran.try_iter().count(), 2, "all of it ran");

    let (open, gate) = mpsc::channel();
    let mut stuck = rig(Vec::new(), Some(gate), Duration::from_millis(50));
    stuck.hook.send(volume(1));
    next(&stuck.ran);
    stuck.hook.send(vec![HookEvent::DeviceChanged("a".into())]);
    let start = Instant::now();
    stuck.hook.finish();
    assert!(start.elapsed() < Duration::from_secs(1));
    drop(open);
    assert!(
        stuck.ran.recv_timeout(Duration::from_millis(100)).is_err(),
        "what was left is dropped"
    );
}

/// AC1: no setting, no hook (and no thread).
#[test]
fn ac1_no_setting_no_hook() {
    let (inputs, _) = mpsc::channel();
    assert!(start(None, inputs.clone(), Stderr::Drop).is_none());
    assert!(start(Some(&command()), inputs, Stderr::Drop).is_some());
}

/// AC11: `start`'s failures reach the player as an input.
#[test]
fn ac11_failure_reaches_the_player() {
    let (inputs, received) = mpsc::channel();
    let missing = HookCommand {
        command: "/nonexistent/on-event".into(),
        args: Vec::new(),
    };
    let mut hook = start(Some(&missing), inputs, Stderr::Drop).expect("a hook");
    hook.send(volume(1));
    hook.finish();
    match received.recv_timeout(LONG) {
        Ok(RuntimeInput::HookFailed(message)) => assert!(
            message.starts_with("Player event hook failed: /nonexistent/on-event: No such file"),
            "{message}"
        ),
        other => panic!("expected a failure, got {other:?}"),
    }
}
