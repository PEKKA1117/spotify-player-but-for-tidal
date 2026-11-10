//! `cargo xtask specs`: every spec's test plan names files and tests that
//! exist (spec 0026). [`check`] is pure: it reads one spec's text and asks
//! `read` for the workspace files it names.

use std::fmt;
use std::path::Path;

/// A reference in a spec that the code does not have.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    /// The spec's path, as given to [`check`].
    pub spec: String,
    /// 1-based line of the table row.
    pub line: usize,
    /// What is missing.
    pub message: String,
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}: {}", self.spec, self.line, self.message)
    }
}

/// The findings of one spec. `read` returns a workspace file's text, or
/// `None` when there is no such file.
pub fn check(_spec: &str, _text: &str, _read: &dyn Fn(&str) -> Option<String>) -> Vec<Finding> {
    Vec::new()
}

/// Checks every `docs/specs/*.md` under `root`.
pub fn check_workspace(_root: &Path) -> anyhow::Result<Vec<Finding>> {
    anyhow::bail!("not implemented")
}

#[cfg(test)]
mod tests {
    use super::*;

    const FOO: &str = "fn helper() {}\n#[test]\nfn ac1_here() {}\n";
    const FOO_TESTS: &str = "#[test]\nfn ac2_in_tests_file() {}\nasync fn ac2_async() {}\n";
    const UI_TESTS: &str = "fn ac5_quit() {}\n";

    fn read(path: &str) -> Option<String> {
        match path {
            "crates/a/src/foo.rs" => Some(FOO.into()),
            "crates/a/src/foo/tests.rs" => Some(FOO_TESTS.into()),
            "crates/a/src/ui/mod.rs" => Some("mod tests;\n".into()),
            "crates/a/src/ui/tests.rs" => Some(UI_TESTS.into()),
            "xtask/src/bar.rs" => Some("fn ac3_bar() {}\n".into()),
            _ => None,
        }
    }

    fn messages(text: &str) -> Vec<(usize, String)> {
        check("docs/specs/0000-x.md", text, &read)
            .into_iter()
            .map(|f| {
                assert_eq!(f.spec, "docs/specs/0000-x.md");
                (f.line, f.message)
            })
            .collect()
    }

    /// Spec 0026 AC1.
    #[test]
    fn ac1_reports_missing_files_and_tests() {
        let cases: &[(&str, &str, &[(usize, &str)])] = &[
            (
                "clean",
                "| AC1 | `crates/a/src/foo.rs` :: `ac1_here` | x | y |\n",
                &[],
            ),
            (
                "missing file",
                "intro\n\n| AC1 | `crates/a/src/gone.rs` :: `ac1_here` | x | y |\n",
                &[(3, "no file `crates/a/src/gone.rs`")],
            ),
            (
                "missing test",
                "| AC1 | `crates/a/src/foo.rs` :: `ac1_gone` | x | y |\n",
                &[(1, "no test `ac1_gone` in `crates/a/src/foo.rs`")],
            ),
            (
                "two in one row, one in the next",
                "| AC1 | `crates/a/src/foo.rs` :: `ac1_gone`, `ac1_here`, `ac1_too` | x | y |\n\
                 | AC2 | `xtask/src/bar.rs` :: `ac3_nope` | x | y |\n",
                &[
                    (1, "no test `ac1_gone` in `crates/a/src/foo.rs`"),
                    (1, "no test `ac1_too` in `crates/a/src/foo.rs`"),
                    (2, "no test `ac3_nope` in `xtask/src/bar.rs`"),
                ],
            ),
            (
                "a helper is not a prefix match",
                "| AC1 | `crates/a/src/foo.rs` :: `help` | x | y |\n",
                &[(1, "no test `help` in `crates/a/src/foo.rs`")],
            ),
        ];
        for (name, text, want) in cases {
            let want: Vec<(usize, String)> =
                want.iter().map(|(l, m)| (*l, m.to_string())).collect();
            assert_eq!(messages(text), want, "{name}");
        }
        let finding = &check(
            "docs/specs/0000-x.md",
            "| AC1 | `crates/a/src/gone.rs` | x | y |\n",
            &read,
        )[0];
        assert_eq!(
            finding.to_string(),
            "docs/specs/0000-x.md:1: no file `crates/a/src/gone.rs`"
        );
    }

    /// Spec 0026 AC2.
    #[test]
    fn ac2_what_is_read() {
        let cases: &[(&str, &str, &[(usize, &str)])] = &[
            (
                "tests module of a file",
                "| AC2 | `crates/a/src/foo.rs` :: `ac2_in_tests_file`, `ac2_async` | x | y |\n",
                &[],
            ),
            (
                "tests module of a mod.rs",
                "| AC5 | `crates/a/src/ui/mod.rs` :: `ac5_quit` and `ac5_gone` | x | y |\n",
                &[(1, "no test `ac5_gone` in `crates/a/src/ui/mod.rs`")],
            ),
            (
                "the tests file named directly",
                "| AC2 | `crates/a/src/foo/tests.rs` :: `ac2_in_tests_file`, `ac1_here` | x | y |\n",
                &[(1, "no test `ac1_here` in `crates/a/src/foo/tests.rs`")],
            ),
            (
                "names in parentheses are notes",
                "| AC1 | `crates/a/src/foo.rs` :: `ac1_here` (renamed from `ac1_old`; see (`ac1_older`)) | x | y |\n",
                &[],
            ),
            (
                "a path without names is checked for existence",
                "| AC1 | `crates/a/src/foo.rs`; `crates/a/src/gone.rs` | x | y |\n",
                &[(1, "no file `crates/a/src/gone.rs`")],
            ),
            (
                "each file owns the names after it",
                "| AC1 | `crates/a/src/foo.rs` :: `ac1_here`; `xtask/src/bar.rs` :: `ac3_bar`, `ac1_here` | x | y |\n",
                &[(1, "no test `ac1_here` in `xtask/src/bar.rs`")],
            ),
            (
                "a name in a later cell is not a test",
                "| AC1 | `crates/a/src/foo.rs` :: `ac1_here` | calls `update` | stub `render` |\n",
                &[],
            ),
            (
                "prose is ignored",
                "See `crates/a/src/gone.rs` :: `ac1_gone`.\n- `crates/a/src/gone.rs`\n",
                &[],
            ),
            (
                "paths of other trees are ignored",
                "| AC1 | `docs/gone.md`, `scripts/gone.sh`, `src/gone.rs` :: `ac1_gone` | x | y |\n",
                &[],
            ),
            (
                "a command check names nothing",
                "| AC1 | — command check | `cargo build` exits 0 | — |\n",
                &[],
            ),
        ];
        for (name, text, want) in cases {
            let want: Vec<(usize, String)> =
                want.iter().map(|(l, m)| (*l, m.to_string())).collect();
            assert_eq!(messages(text), want, "{name}");
        }
    }
}
