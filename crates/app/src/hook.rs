//! The player event hook (spec 0025), its process side: one thread runs
//! the configured command for each event, one at a time, so playback never
//! waits for it. The events themselves are `tidal_player_core::hook`.

use std::os::unix::process::ExitStatusExt;
use std::process::{Command, ExitStatus, Stdio};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use tidal_player_core::hook::{HookEvent, HookQueue};

use crate::config::HookCommand;
use crate::player_runtime::RuntimeInput;

/// How long one command may run.
pub const TIME_LIMIT: Duration = Duration::from_secs(30);
/// How long a shutting-down player waits for what is left.
pub const SHUTDOWN_WAIT: Duration = Duration::from_secs(2);

/// How often a running command is checked on.
const POLL: Duration = Duration::from_millis(20);

/// One run of the command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    pub command: String,
    pub args: Vec<String>,
    pub env: Vec<(&'static str, String)>,
}

/// How a run went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Done,
    /// It did not start: the system's words.
    CouldNotStart(String),
    /// A non-zero exit, in words (`exited with status 3`).
    Failed(String),
    /// Killed at its time limit.
    TimedOut(Duration),
}

/// Runs one job to its end: the process, or a fake in tests.
pub trait Runner: Send + 'static {
    fn run(&self, job: &Job) -> Outcome;
}

/// Where the command's stderr goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stderr {
    /// The player's (the journal under systemd).
    Inherit,
    /// Nowhere: it would draw over the TUI.
    Drop,
}

/// Runs jobs as processes.
#[derive(Debug, Clone, Copy)]
pub struct ProcessRunner {
    pub stderr: Stderr,
    pub limit: Duration,
}

impl Runner for ProcessRunner {
    fn run(&self, job: &Job) -> Outcome {
        let stderr = match self.stderr {
            Stderr::Inherit => Stdio::inherit(),
            Stderr::Drop => Stdio::null(),
        };
        let spawned = Command::new(&job.command)
            .args(&job.args)
            .envs(job.env.iter().map(|(key, value)| (*key, value)))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(stderr)
            .spawn();
        let mut child = match spawned {
            Ok(child) => child,
            Err(e) => return Outcome::CouldNotStart(e.to_string()),
        };
        let deadline = Instant::now() + self.limit;
        loop {
            match child.try_wait() {
                Ok(Some(status)) => return outcome_of(status),
                Ok(None) if Instant::now() >= deadline => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Outcome::TimedOut(self.limit);
                }
                Ok(None) => std::thread::sleep(POLL),
                Err(e) => return Outcome::Failed(format!("could not be waited for: {e}")),
            }
        }
    }
}

fn outcome_of(status: ExitStatus) -> Outcome {
    if status.success() {
        return Outcome::Done;
    }
    Outcome::Failed(match (status.code(), status.signal()) {
        (Some(code), _) => format!("exited with status {code}"),
        (None, Some(signal)) => format!("was killed by signal {signal}"),
        (None, None) => "failed".to_owned(),
    })
}

/// What a run has to say, if anything.
fn failure(command: &str, outcome: &Outcome) -> Option<String> {
    let what = match outcome {
        Outcome::Done => return None,
        Outcome::CouldNotStart(error) => format!("{command}: {error}"),
        Outcome::Failed(how) => format!("{command} {how}"),
        Outcome::TimedOut(limit) => format!("{command} was killed after {} s", limit.as_secs()),
    };
    Some(format!("Player event hook failed: {what}"))
}

/// What the player's runtime hands events to.
pub trait HookSink: Send {
    /// Takes `events` without waiting for any command.
    fn send(&mut self, events: Vec<HookEvent>);
    /// The player is going away: waits a bounded time for what is left.
    fn finish(&mut self);
}

/// The worker: a line of events and the thread that runs them.
pub struct Hook {
    shared: Arc<Shared>,
    wait: Duration,
}

struct Shared {
    state: Mutex<State>,
    changed: Condvar,
    report: Box<dyn Fn(String) + Send + Sync>,
}

#[derive(Default)]
struct State {
    queue: HookQueue,
    running: bool,
    /// No more events come: the thread ends once the line is empty.
    closing: bool,
    /// A failure was said, and no command has succeeded since.
    quiet: bool,
}

impl Shared {
    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The thread's loop: one event at a time until closed.
    fn work(&self, command: &HookCommand, runner: &impl Runner) {
        while let Some(event) = self.next_event() {
            let mut args = command.args.clone();
            args.extend(event.arguments());
            let job = Job {
                command: command.command.clone(),
                args,
                env: event.environment(),
            };
            let outcome = runner.run(&job);
            self.finished(failure(&command.command, &outcome));
        }
    }

    fn next_event(&self) -> Option<HookEvent> {
        let mut state = self.state();
        loop {
            if let Some(event) = state.queue.pop() {
                state.running = true;
                return Some(event);
            }
            if state.closing {
                return None;
            }
            state = self
                .changed
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }

    /// A run ended: says its failure if it is the first since a success.
    fn finished(&self, failure: Option<String>) {
        let say = {
            let mut state = self.state();
            let first = !state.quiet;
            state.quiet = failure.is_some();
            failure.filter(|_| first)
        };
        if let Some(message) = say {
            (self.report)(message);
        }
        self.state().running = false;
        self.changed.notify_all();
    }
}

impl Hook {
    /// Starts the thread. `report` gets each failure to say, `wait` is how
    /// long [`HookSink::finish`] waits.
    pub fn new(
        command: HookCommand,
        runner: impl Runner,
        report: impl Fn(String) + Send + Sync + 'static,
        wait: Duration,
    ) -> Self {
        let shared = Arc::new(Shared {
            state: Mutex::default(),
            changed: Condvar::new(),
            report: Box::new(report),
        });
        let worker = Arc::clone(&shared);
        std::thread::spawn(move || worker.work(&command, &runner));
        Self { shared, wait }
    }
}

impl HookSink for Hook {
    fn send(&mut self, events: Vec<HookEvent>) {
        if events.is_empty() {
            return;
        }
        let say = {
            let mut state = self.shared.state();
            let mut dropped = false;
            for event in events {
                dropped |= state.queue.push(event);
            }
            let first = dropped && !state.quiet;
            state.quiet |= dropped;
            first
        };
        self.shared.changed.notify_all();
        if say {
            (self.shared.report)("Player event hook is too slow: events were dropped".into());
        }
    }

    fn finish(&mut self) {
        let deadline = Instant::now() + self.wait;
        let mut state = self.shared.state();
        state.closing = true;
        self.shared.changed.notify_all();
        while state.running || !state.queue.is_empty() {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            state = self
                .shared
                .changed
                .wait_timeout(state, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        // A command still running is left to finish, on its own.
        state.queue.clear();
    }
}

impl Drop for Hook {
    fn drop(&mut self) {
        self.shared.state().closing = true;
        self.shared.changed.notify_all();
    }
}

/// The hook of a player reading `inputs`, when `app.toml` sets one.
pub fn start(
    command: Option<&HookCommand>,
    inputs: Sender<RuntimeInput>,
    stderr: Stderr,
) -> Option<Box<dyn HookSink>> {
    let runner = ProcessRunner {
        stderr,
        limit: TIME_LIMIT,
    };
    let report = move |message: String| {
        if stderr == Stderr::Inherit {
            eprintln!("{message}");
        }
        let _ = inputs.send(RuntimeInput::HookFailed(message));
    };
    let hook = Hook::new(command?.clone(), runner, report, SHUTDOWN_WAIT);
    Some(Box::new(hook))
}

#[cfg(test)]
mod tests {
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
                message
                    .starts_with("Player event hook failed: /nonexistent/on-event: No such file"),
                "{message}"
            ),
            other => panic!("expected a failure, got {other:?}"),
        }
    }
}
