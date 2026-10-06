//! Builds the session store chain from the environment (spec 0002, "Storage").

use std::path::PathBuf;

/// Where the session lives and whether the keyring is tried first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorePlan {
    pub state_dir: PathBuf,
    pub session_file: PathBuf,
    pub use_keyring: bool,
}

/// Pure resolution: every input is a parameter.
pub fn plan_store(
    _state_dir_var: Option<&str>,
    _no_keyring_var: Option<&str>,
    _default_state_dir: Option<PathBuf>,
    _home: Option<PathBuf>,
) -> StorePlan {
    StorePlan {
        state_dir: PathBuf::new(),
        session_file: PathBuf::new(),
        use_keyring: true,
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
