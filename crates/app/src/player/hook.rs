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
use crate::player::runtime::RuntimeInput;

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
mod tests;
