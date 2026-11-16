use std::cell::RefCell;
use std::time::Instant;

use super::*;

const LINK: &str = "https://tidal.com/browse/album/10";

/// What a run was asked: the program, its arguments, its input and
/// whether its output was captured.
type Call = (String, Vec<String>, Option<Vec<u8>>, bool);

/// A runner that answers each program with a scripted outcome
/// (`NotFound` when it has none) and records what it was asked.
struct Fake {
    answers: Vec<(&'static str, Outcome)>,
    calls: RefCell<Vec<Call>>,
}

impl Fake {
    fn new(answers: Vec<(&'static str, Outcome)>) -> Self {
        Self {
            answers,
            calls: RefCell::new(Vec::new()),
        }
    }

    fn calls(&self) -> Vec<Call> {
        self.calls.borrow().clone()
    }
}

impl Runner for Fake {
    fn run(&self, program: &str, args: &[&str], input: Option<&[u8]>, capture: bool) -> Outcome {
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

fn call(program: &str, args: &[&str], input: Option<&str>, capture: bool) -> Call {
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
        Vec<Call>,
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
        Vec<Call>,
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
