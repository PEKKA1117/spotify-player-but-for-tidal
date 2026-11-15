//! The TUI's clipboard (spec 0030 "The clipboard"): which one, chosen from
//! the environment, the tools run for it, and the messages. The decisions
//! are pure; processes are run behind [`Runner`], faked in the tests.

use std::time::Duration;

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
/// empty value counts as unset).
pub fn provider(_wayland_display: Option<&str>, _display: Option<&str>) -> Provider {
    Provider::Terminal
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
    /// `stdout` (when it was captured, at most [`MAX_OUTPUT`] bytes).
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

/// Copies `link` with `provider`'s tool; the error is the message row's.
pub fn copy(_runner: &dyn Runner, _provider: Provider, _link: &str) -> Result<Copied, String> {
    Err(String::new())
}

/// Reads the clipboard with `provider`'s tool; the error is the message
/// row's.
pub fn read(_runner: &dyn Runner, _provider: Provider) -> Result<String, String> {
    Err(String::new())
}

/// The OSC 52 sequence that puts `text` on the terminal's clipboard.
pub fn osc52(_text: &str) -> String {
    String::new()
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
    fn run(
        &self,
        _program: &str,
        _args: &[&str],
        _input: Option<&[u8]>,
        _capture: bool,
    ) -> Outcome {
        Outcome::Failed("not implemented".into())
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::time::Instant;

    use super::*;

    const LINK: &str = "https://tidal.com/browse/album/10";

    /// A runner that answers each program with a scripted outcome
    /// (`NotFound` when it has none) and records what it was asked.
    struct Fake {
        answers: Vec<(&'static str, Outcome)>,
        calls: RefCell<Vec<(String, Vec<String>, Option<Vec<u8>>, bool)>>,
    }

    impl Fake {
        fn new(answers: Vec<(&'static str, Outcome)>) -> Self {
            Self {
                answers,
                calls: RefCell::new(Vec::new()),
            }
        }

        fn calls(&self) -> Vec<(String, Vec<String>, Option<Vec<u8>>, bool)> {
            self.calls.borrow().clone()
        }
    }

    impl Runner for Fake {
        fn run(
            &self,
            program: &str,
            args: &[&str],
            input: Option<&[u8]>,
            capture: bool,
        ) -> Outcome {
            self.calls.borrow_mut().push((
                program.to_owned(),
                args.iter().map(|a| (*a).to_owned()).collect(),
                input.map(<[u8]>::to_vec),
                capture,
            ));
            self.answers
                .iter()
                .find(|(p, _)| *p == program)
                .map_or(Outcome::NotFound, |(_, o)| o.clone())
        }
    }

    fn exited(status: i32, stdout: &str) -> Outcome {
        Outcome::Exited {
            status: Some(status),
            stdout: stdout.as_bytes().to_vec(),
        }
    }

    fn call(
        program: &str,
        args: &[&str],
        input: Option<&str>,
        capture: bool,
    ) -> (String, Vec<String>, Option<Vec<u8>>, bool) {
        (
            program.to_owned(),
            args.iter().map(|a| (*a).to_owned()).collect(),
            input.map(|i| i.as_bytes().to_vec()),
            capture,
        )
    }

    /// AC7: Wayland first, then X11, else the terminal.
    #[test]
    fn ac7_provider() {
        let cases = [
            (Some("wayland-1"), Some(":0"), Provider::Wayland),
            (Some("wayland-1"), None, Provider::Wayland),
            (None, Some(":0"), Provider::X11),
            (Some(""), Some(":0"), Provider::X11),
            (None, None, Provider::Terminal),
            (Some(""), Some(""), Provider::Terminal),
        ];
        for (wayland, display, want) in cases {
            assert_eq!(provider(wayland, display), want, "{wayland:?} {display:?}");
        }
    }

    /// AC8: the tool run per provider, with the link on its input, and the
    /// result per outcome.
    #[test]
    fn ac8_copy() {
        let wl = || call("wl-copy", &[], Some(LINK), false);
        let xclip = || call("xclip", &["-selection", "clipboard"], Some(LINK), false);
        let xsel = || call("xsel", &["--clipboard", "--input"], Some(LINK), false);
        type Case = (
            Provider,
            Vec<(&'static str, Outcome)>,
            Vec<(String, Vec<String>, Option<Vec<u8>>, bool)>,
            Result<Copied, String>,
        );
        let cases: Vec<Case> = vec![
            (
                Provider::Wayland,
                vec![("wl-copy", exited(0, ""))],
                vec![wl()],
                Ok(Copied::Tool),
            ),
            (
                Provider::Wayland,
                vec![],
                vec![wl()],
                Err("Copying the link needs wl-copy: install wl-clipboard".into()),
            ),
            (
                Provider::Wayland,
                vec![("wl-copy", exited(1, ""))],
                vec![wl()],
                Err("Could not copy the link: wl-copy exited with status 1".into()),
            ),
            (
                Provider::Wayland,
                vec![("wl-copy", Outcome::TimedOut)],
                vec![wl()],
                Err("Could not copy the link: wl-copy did not answer".into()),
            ),
            (
                Provider::Wayland,
                vec![(
                    "wl-copy",
                    Outcome::Exited {
                        status: None,
                        stdout: Vec::new(),
                    },
                )],
                vec![wl()],
                Err("Could not copy the link: wl-copy was stopped by a signal".into()),
            ),
            (
                Provider::Wayland,
                vec![("wl-copy", Outcome::Failed("Permission denied".into()))],
                vec![wl()],
                Err("Could not copy the link: wl-copy: Permission denied".into()),
            ),
            (
                Provider::X11,
                vec![("xclip", exited(0, ""))],
                vec![xclip()],
                Ok(Copied::Tool),
            ),
            (
                Provider::X11,
                vec![("xsel", exited(0, ""))],
                vec![xclip(), xsel()],
                Ok(Copied::Tool),
            ),
            (
                Provider::X11,
                vec![("xclip", exited(1, "")), ("xsel", exited(0, ""))],
                vec![xclip()],
                Err("Could not copy the link: xclip exited with status 1".into()),
            ),
            (
                Provider::X11,
                vec![],
                vec![xclip(), xsel()],
                Err("Copying the link needs xclip or xsel: install one of them".into()),
            ),
            (
                Provider::Terminal,
                vec![],
                vec![],
                Ok(Copied::Osc52(osc52(LINK))),
            ),
        ];
        for (provider, answers, want_calls, want) in cases {
            let fake = Fake::new(answers);
            assert_eq!(copy(&fake, provider, LINK), want, "{provider:?}");
            assert_eq!(fake.calls(), want_calls, "{provider:?}: {want:?}");
        }
    }

    /// AC9: the tool run per provider, and the result per outcome.
    #[test]
    fn ac9_read() {
        let wl = || call("wl-paste", &["--no-newline"], None, true);
        let xclip = || call("xclip", &["-selection", "clipboard", "-o"], None, true);
        let xsel = || call("xsel", &["--clipboard", "--output"], None, true);
        type Case = (
            Provider,
            Vec<(&'static str, Outcome)>,
            Vec<(String, Vec<String>, Option<Vec<u8>>, bool)>,
            Result<String, String>,
        );
        let no_text = || Err("The clipboard holds no text".to_owned());
        let cases: Vec<Case> = vec![
            (
                Provider::Wayland,
                vec![("wl-paste", exited(0, LINK))],
                vec![wl()],
                Ok(LINK.into()),
            ),
            (
                Provider::Wayland,
                vec![("wl-paste", exited(1, ""))],
                vec![wl()],
                no_text(),
            ),
            (
                Provider::Wayland,
                vec![],
                vec![wl()],
                Err("Reading the clipboard needs wl-paste: install wl-clipboard".into()),
            ),
            (
                Provider::Wayland,
                vec![("wl-paste", Outcome::TimedOut)],
                vec![wl()],
                Err("Could not read the clipboard: wl-paste did not answer".into()),
            ),
            (
                Provider::Wayland,
                vec![("wl-paste", Outcome::Failed("Permission denied".into()))],
                vec![wl()],
                Err("Could not read the clipboard: wl-paste: Permission denied".into()),
            ),
            (
                Provider::Wayland,
                vec![(
                    "wl-paste",
                    Outcome::Exited {
                        status: Some(0),
                        stdout: vec![b'a', 0xff, b'b'],
                    },
                )],
                vec![wl()],
                Ok("a\u{fffd}b".into()),
            ),
            (
                Provider::X11,
                vec![("xclip", exited(0, LINK))],
                vec![xclip()],
                Ok(LINK.into()),
            ),
            (
                Provider::X11,
                vec![("xsel", exited(0, LINK))],
                vec![xclip(), xsel()],
                Ok(LINK.into()),
            ),
            (
                Provider::X11,
                vec![("xclip", exited(1, ""))],
                vec![xclip()],
                no_text(),
            ),
            (
                Provider::X11,
                vec![],
                vec![xclip(), xsel()],
                Err("Reading the clipboard needs xclip or xsel: install one of them".into()),
            ),
            (
                Provider::Terminal,
                vec![],
                vec![],
                Err("Reading the clipboard needs wl-paste (Wayland), xclip or xsel (X11)".into()),
            ),
        ];
        for (provider, answers, want_calls, want) in cases {
            let fake = Fake::new(answers);
            assert_eq!(read(&fake, provider), want, "{provider:?}");
            assert_eq!(fake.calls(), want_calls, "{provider:?}: {want:?}");
        }
    }

    /// AC9: the real runner gives the input, cuts the output, kills what
    /// does not finish and knows a missing program.
    #[test]
    fn ac9_runner_cuts_and_times_out() {
        let runner = Processes {
            timeout: Duration::from_millis(300),
            max_output: 1000,
        };
        assert_eq!(
            runner.run("sh", &["-c", "cat"], Some(b"abc"), true),
            Outcome::Exited {
                status: Some(0),
                stdout: b"abc".to_vec(),
            }
        );
        match runner.run("sh", &["-c", "head -c 100000 /dev/zero"], None, true) {
            Outcome::Exited { stdout, .. } => assert_eq!(stdout.len(), 1000),
            other => panic!("{other:?}"),
        }
        assert_eq!(
            runner.run("sh", &["-c", "exit 3"], None, false),
            Outcome::Exited {
                status: Some(3),
                stdout: Vec::new(),
            }
        );
        // A child that keeps the output open (as `wl-copy` does) is not
        // waited for when the output is not captured.
        let started = Instant::now();
        assert_eq!(
            runner.run("sh", &["-c", "sleep 5 & exit 0"], None, false),
            Outcome::Exited {
                status: Some(0),
                stdout: Vec::new(),
            }
        );
        assert!(started.elapsed() < Duration::from_secs(2));
        let started = Instant::now();
        assert_eq!(
            runner.run("sh", &["-c", "sleep 5"], None, false),
            Outcome::TimedOut
        );
        assert!(started.elapsed() < Duration::from_secs(2));
        let started = Instant::now();
        assert_eq!(
            runner.run("sh", &["-c", "sleep 5 & echo hi"], None, true),
            Outcome::TimedOut
        );
        assert!(started.elapsed() < Duration::from_secs(2));
        assert_eq!(
            runner.run("tidal-player-no-such-tool", &[], None, false),
            Outcome::NotFound
        );
    }

    /// AC10: `ESC ] 52 ; c ; <base64> BEL`.
    #[test]
    fn ac10_osc52() {
        let cases = [
            ("a", "\x1b]52;c;YQ==\x07"),
            (
                LINK,
                "\x1b]52;c;aHR0cHM6Ly90aWRhbC5jb20vYnJvd3NlL2FsYnVtLzEw\x07",
            ),
            ("", "\x1b]52;c;\x07"),
        ];
        for (text, want) in cases {
            assert_eq!(osc52(text), want, "{text:?}");
        }
    }
}
