//! The TUI's clipboard (spec 0030 "The clipboard"): which one, chosen from
//! the environment, the tools run for it, and the messages. The decisions
//! are pure; processes are run behind [`Runner`], faked in the tests.

use std::io::{self, Read, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;

/// How long a clipboard tool may take before it is killed.
pub const TIMEOUT: Duration = Duration::from_secs(2);
/// How much of the clipboard is read: past this it is not a link anyway.
pub const MAX_OUTPUT: usize = 64 * 1024;

/// Which clipboard the TUI uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    /// `wl-copy` / `wl-paste`.
    Wayland,
    /// `xclip`, or `xsel`.
    X11,
    /// OSC 52 to the terminal; nothing to read with.
    Terminal,
}

/// The provider for the environment's `WAYLAND_DISPLAY` and `DISPLAY` (an
/// empty value counts as unset): Wayland first, as XWayland sets both.
pub fn provider(wayland_display: Option<&str>, display: Option<&str>) -> Provider {
    let set = |v: Option<&str>| v.is_some_and(|v| !v.is_empty());
    if set(wayland_display) {
        Provider::Wayland
    } else if set(display) {
        Provider::X11
    } else {
        Provider::Terminal
    }
}

/// The provider for this process's environment.
pub fn from_env() -> Provider {
    let var = |name| std::env::var(name).ok();
    provider(var("WAYLAND_DISPLAY").as_deref(), var("DISPLAY").as_deref())
}

/// How a run of a tool ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// It exited (`status`: `None` when a signal ended it), having written
    /// `stdout` (when it was captured, at most the runner's limit).
    Exited {
        status: Option<i32>,
        stdout: Vec<u8>,
    },
    /// The program is not installed.
    NotFound,
    /// It had not finished after the timeout, and was killed.
    TimedOut,
    /// It could not be started or waited for: the reason.
    Failed(String),
}

/// Runs a program: `input` on its standard input (else none), its
/// standard output captured when `capture` (else `/dev/null`, as its
/// standard error always is).
pub trait Runner {
    fn run(&self, program: &str, args: &[&str], input: Option<&[u8]>, capture: bool) -> Outcome;
}

/// How a link was copied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Copied {
    /// A tool has it.
    Tool,
    /// This sequence is to be written to the terminal.
    Osc52(String),
}

/// A tool and its arguments.
type Tool = (&'static str, &'static [&'static str]);

const COPY_WAYLAND: [Tool; 1] = [("wl-copy", &[])];
const COPY_X11: [Tool; 2] = [
    ("xclip", &["-selection", "clipboard"]),
    ("xsel", &["--clipboard", "--input"]),
];
const READ_WAYLAND: [Tool; 1] = [("wl-paste", &["--no-newline"])];
const READ_X11: [Tool; 2] = [
    ("xclip", &["-selection", "clipboard", "-o"]),
    ("xsel", &["--clipboard", "--output"]),
];

/// Runs the first of `tools` that is installed: its name and how it
/// ended, or `None` when none is.
fn first_installed(
    runner: &dyn Runner,
    tools: &[Tool],
    input: Option<&[u8]>,
    capture: bool,
) -> Option<(&'static str, Outcome)> {
    tools.iter().find_map(
        |(tool, args)| match runner.run(tool, args, input, capture) {
            Outcome::NotFound => None,
            outcome => Some((*tool, outcome)),
        },
    )
}

/// What `what` needs when none of `provider`'s tools is installed.
fn missing(what: &str, provider: Provider, wayland_tool: &str) -> String {
    match provider {
        Provider::Wayland => format!("{what} needs {wayland_tool}: install wl-clipboard"),
        _ => format!("{what} needs xclip or xsel: install one of them"),
    }
}

/// Why a tool that ran did not do it.
fn failure(tool: &str, outcome: Outcome) -> String {
    match outcome {
        Outcome::Exited {
            status: Some(n), ..
        } => format!("{tool} exited with status {n}"),
        Outcome::Exited { status: None, .. } => format!("{tool} was stopped by a signal"),
        Outcome::TimedOut => format!("{tool} did not answer"),
        Outcome::Failed(reason) => format!("{tool}: {reason}"),
        Outcome::NotFound => format!("{tool} is not installed"),
    }
}

/// Copies `link` with `provider`'s tool; the error is the message row's.
pub fn copy(runner: &dyn Runner, provider: Provider, link: &str) -> Result<Copied, String> {
    let tools: &[Tool] = match provider {
        Provider::Wayland => &COPY_WAYLAND,
        Provider::X11 => &COPY_X11,
        Provider::Terminal => return Ok(Copied::Osc52(osc52(link))),
    };
    match first_installed(runner, tools, Some(link.as_bytes()), false) {
        None => Err(missing("Copying the link", provider, "wl-copy")),
        Some((
            _,
            Outcome::Exited {
                status: Some(0), ..
            },
        )) => Ok(Copied::Tool),
        Some((tool, outcome)) => Err(format!(
            "Could not copy the link: {}",
            failure(tool, outcome)
        )),
    }
}

/// Reads the clipboard with `provider`'s tool; the error is the message
/// row's. A tool that exits with an error found nothing it could give as
/// text (spec 0030 "Facts").
pub fn read(runner: &dyn Runner, provider: Provider) -> Result<String, String> {
    let tools: &[Tool] = match provider {
        Provider::Wayland => &READ_WAYLAND,
        Provider::X11 => &READ_X11,
        Provider::Terminal => {
            return Err(
                "Reading the clipboard needs wl-paste (Wayland), xclip or xsel (X11)".into(),
            );
        }
    };
    match first_installed(runner, tools, None, true) {
        None => Err(missing("Reading the clipboard", provider, "wl-paste")),
        Some((
            _,
            Outcome::Exited {
                status: Some(0),
                stdout,
            },
        )) => Ok(String::from_utf8_lossy(&stdout).into_owned()),
        Some((
            _,
            Outcome::Exited {
                status: Some(_), ..
            },
        )) => Err("The clipboard holds no text".into()),
        Some((tool, outcome)) => Err(format!(
            "Could not read the clipboard: {}",
            failure(tool, outcome)
        )),
    }
}

/// The OSC 52 sequence that puts `text` on the terminal's clipboard.
pub fn osc52(text: &str) -> String {
    format!("\x1b]52;c;{}\x07", BASE64.encode(text))
}

/// Runs real processes, killed after `timeout`, reading at most
/// `max_output` bytes.
#[derive(Debug, Clone, Copy)]
pub struct Processes {
    pub timeout: Duration,
    pub max_output: usize,
}

impl Default for Processes {
    fn default() -> Self {
        Self {
            timeout: TIMEOUT,
            max_output: MAX_OUTPUT,
        }
    }
}

impl Runner for Processes {
    fn run(&self, program: &str, args: &[&str], input: Option<&[u8]>, capture: bool) -> Outcome {
        let pipe = |yes: bool| if yes { Stdio::piped() } else { Stdio::null() };
        let mut child = match Command::new(program)
            .args(args)
            .stdin(pipe(input.is_some()))
            .stdout(pipe(capture))
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(child) => child,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Outcome::NotFound,
            Err(e) => return Outcome::Failed(e.to_string()),
        };
        let deadline = Instant::now() + self.timeout;
        // A link is far below a pipe's buffer, so this does not block; a
        // tool that exits without reading it fails below by its status.
        if let (Some(input), Some(mut stdin)) = (input, child.stdin.take())
            && let Err(e) = stdin.write_all(input)
            && e.kind() != io::ErrorKind::BrokenPipe
        {
            let _ = child.kill();
            let _ = child.wait();
            return Outcome::Failed(e.to_string());
        }
        // The output is read on its own thread, up to the limit; dropping
        // the pipe there ends a tool that writes more.
        let output = child.stdout.take().map(|stdout| {
            let (send, receive) = mpsc::channel();
            let limit = u64::try_from(self.max_output).unwrap_or(u64::MAX);
            thread::spawn(move || {
                let mut out = Vec::new();
                let read = stdout.take(limit).read_to_end(&mut out).map(|_| out);
                let _ = send.send(read);
            });
            receive
        });
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if Instant::now() >= deadline => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Outcome::TimedOut;
                }
                Ok(None) => thread::sleep(Duration::from_millis(5)),
                Err(e) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Outcome::Failed(e.to_string());
                }
            }
        };
        let stdout = match output {
            None => Vec::new(),
            // A child of the tool may still hold the pipe: the deadline
            // holds for the output too.
            Some(receive) => {
                match receive.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                    Ok(Ok(out)) => out,
                    Ok(Err(e)) => return Outcome::Failed(e.to_string()),
                    Err(_) => return Outcome::TimedOut,
                }
            }
        };
        Outcome::Exited {
            status: status.code(),
            stdout,
        }
    }
}

#[cfg(test)]
mod tests;
