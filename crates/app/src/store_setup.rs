//! Builds the session store chain from the environment (spec 0002, "Storage").

use std::path::PathBuf;
use std::sync::Arc;

use tidal_player_api::auth::SessionStore;

use crate::passphrase::ProcessPassphrase;
use crate::session_store::{EncryptedFileStore, FallbackStore, KeyringStore};

/// Overrides the state directory.
pub const STATE_DIR_VAR: &str = "TIDAL_PLAYER_STATE_DIR";
/// `1` skips the keyring entirely.
pub const NO_KEYRING_VAR: &str = "TIDAL_PLAYER_NO_KEYRING";

/// Where the session lives and whether the keyring is tried first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorePlan {
    pub state_dir: PathBuf,
    pub session_file: PathBuf,
    pub use_keyring: bool,
}

/// Pure resolution: every input is a parameter. `default_state_dir` is the
/// platform state directory for `tidal-player` (`None` when unknown), `home`
/// the last-resort base.
pub fn plan_store(
    state_dir_var: Option<&str>,
    no_keyring_var: Option<&str>,
    default_state_dir: Option<PathBuf>,
    home: Option<PathBuf>,
) -> StorePlan {
    let state_dir = state_dir_var
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or(default_state_dir)
        .or_else(|| home.map(|h| h.join(".local/state/tidal-player")))
        .unwrap_or_else(|| PathBuf::from(".tidal-player"));
    StorePlan {
        session_file: state_dir.join("session.age"),
        state_dir,
        use_keyring: !matches!(no_keyring_var, Some("1" | "true")),
    }
}

impl StorePlan {
    /// The plan for this process's environment.
    pub fn from_env() -> Self {
        let dirs = directories::ProjectDirs::from("", "", "tidal-player");
        plan_store(
            std::env::var(STATE_DIR_VAR).ok().as_deref(),
            std::env::var(NO_KEYRING_VAR).ok().as_deref(),
            dirs.and_then(|d| d.state_dir().map(ToOwned::to_owned)),
            directories::BaseDirs::new().map(|d| d.home_dir().to_owned()),
        )
    }

    /// The store chain: keyring first, then the encrypted file; the file
    /// alone when the keyring is disabled (a `KeyringStore` is then never
    /// constructed).
    pub fn build_store(&self) -> Arc<dyn SessionStore> {
        let file = EncryptedFileStore::new(
            self.session_file.clone(),
            Arc::new(ProcessPassphrase::from_process()),
        );
        if self.use_keyring {
            Arc::new(FallbackStore::new(
                Box::new(KeyringStore::new()),
                Box::new(file),
            ))
        } else {
            Arc::new(file)
        }
    }

    /// Whether a session is stored, decided without ever needing a
    /// passphrase: the session file exists, or the keyring holds one.
    pub fn has_stored_session(&self) -> bool {
        self.session_file.exists()
            || (self.use_keyring && matches!(KeyringStore::new().load(), Ok(Some(_))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn plan_store_table() {
        // (state dir var, no keyring var, default dir, home) -> (dir, keyring)
        #[allow(clippy::type_complexity)]
        let cases: Vec<(
            Option<&str>,
            Option<&str>,
            Option<PathBuf>,
            Option<PathBuf>,
            &str,
            bool,
        )> = vec![
            (
                Some("/tmp/x"),
                None,
                Some(p("/d")),
                Some(p("/h")),
                "/tmp/x",
                true,
            ),
            (None, None, Some(p("/d")), Some(p("/h")), "/d", true),
            (
                None,
                None,
                None,
                Some(p("/h")),
                "/h/.local/state/tidal-player",
                true,
            ),
            (Some(""), None, Some(p("/d")), None, "/d", true),
            (None, Some("1"), Some(p("/d")), None, "/d", false),
            (None, Some("true"), Some(p("/d")), None, "/d", false),
            (None, Some("0"), Some(p("/d")), None, "/d", true),
            (None, Some(""), Some(p("/d")), None, "/d", true),
        ];
        for (var, nk, default, home, dir, keyring) in cases {
            let plan = plan_store(var, nk, default, home);
            assert_eq!(plan.state_dir, p(dir), "dir for {var:?}/{nk:?}");
            assert_eq!(plan.session_file, p(dir).join("session.age"));
            assert_eq!(plan.use_keyring, keyring, "keyring for {var:?}/{nk:?}");
        }
    }
}
