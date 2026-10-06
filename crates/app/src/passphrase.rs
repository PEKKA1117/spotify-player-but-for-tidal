//! Where the passphrase for the encrypted session file comes from. See
//! docs/specs/0002-auth.md ("Passphrase sources", AC16).

use std::fmt;
use std::io::{self, IsTerminal};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use zeroize::Zeroizing;

/// Environment variable naming a file whose first line is the passphrase.
pub const PASSPHRASE_FILE_VAR: &str = "TIDAL_PLAYER_PASSPHRASE_FILE";
/// File name of the systemd credential inside `$CREDENTIALS_DIRECTORY`.
pub const CREDENTIAL_NAME: &str = "tidal-player-passphrase";
/// Wrong interactive attempts before giving up.
pub const MAX_ATTEMPTS: usize = 3;

const ASK: &str = "Passphrase for the tidal-player session:";
const CONFIRM: &str = "Confirm passphrase:";

/// A passphrase, zeroised on drop and never printed.
#[derive(Clone, PartialEq, Eq)]
pub struct Passphrase(Zeroizing<String>);

impl Passphrase {
    pub fn new(value: String) -> Self {
        Self(Zeroizing::new(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Passphrase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Passphrase(<redacted>)")
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PassphraseError {
    #[error(
        "Session file is encrypted and no passphrase is available: set \
         TIDAL_PLAYER_PASSPHRASE_FILE or a systemd credential (see docs/login.md)"
    )]
    Unavailable,
    #[error("Wrong passphrase for the session file")]
    WrongPassphrase,
    #[error("the passphrase is empty")]
    Empty,
    #[error("cannot read the passphrase: {0}")]
    Io(String),
}

/// What the caller needs the passphrase for.
pub enum Purpose<'a> {
    /// A new file is being created: ask twice.
    Create,
    /// An existing file is being opened. `verify` tells whether a candidate
    /// opens it, so an interactive prompt can retry.
    Unlock(&'a dyn Fn(&Passphrase) -> bool),
}

/// Hands out the passphrase when the encrypted store needs it (lazily).
pub trait PassphraseSource: Send + Sync {
    fn passphrase(&self, purpose: Purpose<'_>) -> Result<Passphrase, PassphraseError>;
}

/// The inputs of [`resolve_passphrase`], injected so tests never read the
/// real process environment or terminal.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PassphraseEnv {
    /// Value of `TIDAL_PLAYER_PASSPHRASE_FILE`.
    pub passphrase_file: Option<PathBuf>,
    /// Value of `CREDENTIALS_DIRECTORY`.
    pub credentials_dir: Option<PathBuf>,
    /// Whether stdin and stderr are both terminals.
    pub interactive: bool,
}

impl PassphraseEnv {
    pub fn from_process() -> Self {
        let var = |name: &str| {
            std::env::var_os(name)
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
        };
        Self {
            passphrase_file: var(PASSPHRASE_FILE_VAR),
            credentials_dir: var("CREDENTIALS_DIRECTORY"),
            interactive: io::stdin().is_terminal() && io::stderr().is_terminal(),
        }
    }
}

/// Echo-off question asked on the terminal.
pub trait Prompt {
    fn ask(&mut self, prompt: &str) -> io::Result<String>;
    /// A short message to the user (mismatch, wrong passphrase).
    fn notice(&mut self, _message: &str) {}
}

/// The real prompt: `rpassword` on the controlling terminal.
pub struct TerminalPrompt;

impl Prompt for TerminalPrompt {
    fn ask(&mut self, prompt: &str) -> io::Result<String> {
        rpassword::prompt_password(format!("{prompt} "))
    }

    fn notice(&mut self, message: &str) {
        eprintln!("{message}");
    }
}

/// Picks the passphrase source in spec order: passphrase file, systemd
/// credential, interactive prompt, else [`PassphraseError::Unavailable`].
pub fn resolve_passphrase(
    env: &PassphraseEnv,
    prompt: &mut dyn Prompt,
    purpose: Purpose<'_>,
) -> Result<Passphrase, PassphraseError> {
    // stub (red)
    let _ = (env, prompt, purpose);
    Err(PassphraseError::Unavailable)
}

fn read_passphrase_file(path: &Path) -> Result<Passphrase, PassphraseError> {
    let io_err = |e: io::Error| PassphraseError::Io(format!("{}: {e}", path.display()));
    let mode = std::fs::metadata(path)
        .map_err(io_err)?
        .permissions()
        .mode();
    if mode & 0o077 != 0 {
        tracing::warn!(path = %path.display(), "passphrase file is readable by other users");
    }
    let content = Zeroizing::new(std::fs::read_to_string(path).map_err(io_err)?);
    let line = content.lines().next().unwrap_or("");
    if line.is_empty() {
        return Err(PassphraseError::Empty);
    }
    Ok(Passphrase::new(line.to_owned()))
}

fn ask(prompt: &mut dyn Prompt, question: &str) -> Result<Zeroizing<String>, PassphraseError> {
    prompt
        .ask(question)
        .map(Zeroizing::new)
        .map_err(|e| PassphraseError::Io(e.to_string()))
}

fn prompt_passphrase(
    prompt: &mut dyn Prompt,
    purpose: Purpose<'_>,
) -> Result<Passphrase, PassphraseError> {
    match purpose {
        Purpose::Unlock(verify) => {
            for _ in 0..MAX_ATTEMPTS {
                let answer = ask(prompt, ASK)?;
                if answer.is_empty() {
                    prompt.notice("The passphrase cannot be empty.");
                    continue;
                }
                let candidate = Passphrase::new(answer.to_string());
                if verify(&candidate) {
                    return Ok(candidate);
                }
                prompt.notice("Wrong passphrase.");
            }
            Err(PassphraseError::WrongPassphrase)
        }
        Purpose::Create => loop {
            let first = ask(prompt, ASK)?;
            if first.is_empty() {
                prompt.notice("The passphrase cannot be empty.");
                continue;
            }
            let second = ask(prompt, CONFIRM)?;
            if *first == *second {
                return Ok(Passphrase::new(first.to_string()));
            }
            prompt.notice("Passphrases do not match, try again.");
        },
    }
}

/// The per-process source: resolves on first use, then keeps the passphrase
/// in memory so refreshed sessions can be re-encrypted without asking again.
pub struct ProcessPassphrase {
    env: PassphraseEnv,
    prompt: Mutex<Box<dyn Prompt + Send>>,
    cached: Mutex<Option<Passphrase>>,
}

impl ProcessPassphrase {
    pub fn new(env: PassphraseEnv, prompt: Box<dyn Prompt + Send>) -> Self {
        Self {
            env,
            prompt: Mutex::new(prompt),
            cached: Mutex::new(None),
        }
    }

    /// Environment and terminal of this process, real prompt.
    pub fn from_process() -> Self {
        Self::new(PassphraseEnv::from_process(), Box::new(TerminalPrompt))
    }
}

impl PassphraseSource for ProcessPassphrase {
    fn passphrase(&self, purpose: Purpose<'_>) -> Result<Passphrase, PassphraseError> {
        let mut cached = self.cached.lock().expect("poisoned");
        if let Some(p) = cached.as_ref() {
            return Ok(p.clone());
        }
        let mut prompt = self.prompt.lock().expect("poisoned");
        let p = resolve_passphrase(&self.env, prompt.as_mut(), purpose)?;
        *cached = Some(p.clone());
        Ok(p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    struct FakePrompt {
        answers: VecDeque<String>,
        asked: Vec<String>,
    }

    impl FakePrompt {
        fn new(answers: &[&str]) -> Self {
            Self {
                answers: answers.iter().map(|s| (*s).to_owned()).collect(),
                asked: Vec::new(),
            }
        }
    }

    impl Prompt for FakePrompt {
        fn ask(&mut self, prompt: &str) -> io::Result<String> {
            self.asked.push(prompt.to_owned());
            self.answers
                .pop_front()
                .ok_or_else(|| io::Error::other("no more answers"))
        }
    }

    fn write(dir: &Path, name: &str, content: &str, mode: u32) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, content).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        path
    }

    fn ok(s: &str) -> Result<Passphrase, PassphraseError> {
        Ok(Passphrase::new(s.to_owned()))
    }

    #[test]
    fn ac16_resolve_passphrase() {
        let dir = tempfile::tempdir().unwrap();
        let file = write(dir.path(), "pass", "from-file\nsecond line\n", 0o600);
        let creds = dir.path().join("creds");
        std::fs::create_dir(&creds).unwrap();
        write(&creds, CREDENTIAL_NAME, "from-credential\n", 0o600);
        let empty = write(dir.path(), "empty", "\n", 0o600);
        let crlf = write(dir.path(), "crlf", "windows\r\n", 0o600);
        let loose = write(dir.path(), "loose", "loose-pass\n", 0o644);
        let missing_creds = dir.path().join("no-creds");
        std::fs::create_dir(&missing_creds).unwrap();

        let env = |f: Option<&Path>, c: Option<&Path>, interactive: bool| PassphraseEnv {
            passphrase_file: f.map(Path::to_path_buf),
            credentials_dir: c.map(Path::to_path_buf),
            interactive,
        };

        #[allow(clippy::type_complexity)]
        let table: Vec<(
            &str,
            PassphraseEnv,
            Result<Passphrase, PassphraseError>,
            usize,
        )> = vec![
            (
                "file var set",
                env(Some(&file), None, false),
                ok("from-file"),
                0,
            ),
            (
                "credentials dir set",
                env(None, Some(&creds), false),
                ok("from-credential"),
                0,
            ),
            (
                "both set: file var wins",
                env(Some(&file), Some(&creds), true),
                ok("from-file"),
                0,
            ),
            (
                "crlf stripped",
                env(Some(&crlf), None, false),
                ok("windows"),
                0,
            ),
            (
                "group-readable file is still used",
                env(Some(&loose), None, false),
                ok("loose-pass"),
                0,
            ),
            (
                "empty passphrase rejected",
                env(Some(&empty), None, true),
                Err(PassphraseError::Empty),
                0,
            ),
            (
                "neither + terminal prompts",
                env(None, None, true),
                ok("typed"),
                1,
            ),
            (
                "credentials dir without the credential + terminal prompts",
                env(None, Some(&missing_creds), true),
                ok("typed"),
                1,
            ),
            (
                "neither + no terminal",
                env(None, None, false),
                Err(PassphraseError::Unavailable),
                0,
            ),
        ];

        for (name, env, expected, prompts) in table {
            let mut prompt = FakePrompt::new(&["typed"]);
            let got = resolve_passphrase(&env, &mut prompt, Purpose::Unlock(&|_| true));
            assert_eq!(got, expected, "{name}");
            assert_eq!(prompt.asked.len(), prompts, "{name}: prompt calls");
        }

        let message = PassphraseError::Unavailable.to_string();
        assert!(message.contains("TIDAL_PLAYER_PASSPHRASE_FILE"));
    }

    #[test]
    fn ac16_prompt_retries() {
        let env = PassphraseEnv {
            interactive: true,
            ..PassphraseEnv::default()
        };
        let verify = |p: &Passphrase| p.as_str() == "right";

        // wrong then right: two prompt calls
        let mut prompt = FakePrompt::new(&["wrong", "right"]);
        let got = resolve_passphrase(&env, &mut prompt, Purpose::Unlock(&verify));
        assert_eq!(got, ok("right"));
        assert_eq!(prompt.asked, [ASK, ASK]);

        // three wrong: WrongPassphrase after exactly three asks
        let mut prompt = FakePrompt::new(&["a", "b", "c", "right"]);
        let got = resolve_passphrase(&env, &mut prompt, Purpose::Unlock(&verify));
        assert_eq!(got, Err(PassphraseError::WrongPassphrase));
        assert_eq!(prompt.asked.len(), 3);

        // creation asks twice
        let mut prompt = FakePrompt::new(&["new", "new"]);
        let got = resolve_passphrase(&env, &mut prompt, Purpose::Create);
        assert_eq!(got, ok("new"));
        assert_eq!(prompt.asked, [ASK, CONFIRM]);

        // mismatch asks again
        let mut prompt = FakePrompt::new(&["one", "two", "three", "three"]);
        let got = resolve_passphrase(&env, &mut prompt, Purpose::Create);
        assert_eq!(got, ok("three"));
        assert_eq!(prompt.asked, [ASK, CONFIRM, ASK, CONFIRM]);
    }

    #[test]
    fn process_passphrase_asks_once() {
        let env = PassphraseEnv {
            interactive: true,
            ..PassphraseEnv::default()
        };
        let source = ProcessPassphrase::new(env, Box::new(FakePrompt::new(&["pw", "pw"])));
        assert_eq!(source.passphrase(Purpose::Create), ok("pw"));
        assert_eq!(source.passphrase(Purpose::Unlock(&|_| false)), ok("pw"));
    }
}
