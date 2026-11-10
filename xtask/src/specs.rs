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
pub fn check(spec: &str, text: &str, read: &dyn Fn(&str) -> Option<String>) -> Vec<Finding> {
    let mut findings = Vec::new();
    for (index, row) in text.lines().enumerate() {
        if !row.trim_start().starts_with('|') {
            continue;
        }
        for message in check_row(row, read) {
            findings.push(Finding {
                spec: spec.to_string(),
                line: index + 1,
                message,
            });
        }
    }
    findings
}

/// A piece of a table cell, outside parentheses.
enum Piece {
    /// Text in backticks.
    Code(String),
    /// Text between two backtick spans.
    Text(String),
    /// A `|`: the next cell starts.
    Cell,
}

/// Splits a row into its pieces, dropping what is in parentheses.
fn pieces(row: &str) -> Vec<Piece> {
    let mut out = Vec::new();
    let mut text = String::new();
    let mut code: Option<String> = None;
    let mut depth = 0usize;
    let mut escaped = false;
    for c in row.chars() {
        if let Some(span) = code.as_mut() {
            if c == '`' {
                if depth == 0 {
                    out.push(Piece::Code(std::mem::take(span)));
                }
                code = None;
            } else {
                span.push(c);
            }
            continue;
        }
        let was_escaped = std::mem::replace(&mut escaped, c == '\\');
        match c {
            '`' => {
                if depth == 0 {
                    out.push(Piece::Text(std::mem::take(&mut text)));
                }
                code = Some(String::new());
            }
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            '|' if !was_escaped => {
                out.push(Piece::Text(std::mem::take(&mut text)));
                out.push(Piece::Cell);
                depth = 0;
            }
            _ if depth == 0 => text.push(c),
            _ => {}
        }
    }
    out
}

/// What one table row is missing.
fn check_row(row: &str, read: &dyn Fn(&str) -> Option<String>) -> Vec<String> {
    let mut missing = Vec::new();
    // The file the names after `::` belong to, with its text and its tests
    // module's.
    let mut file: Option<(String, String)> = None;
    let mut names = false;
    for piece in pieces(row) {
        match piece {
            Piece::Cell => {
                file = None;
                names = false;
            }
            Piece::Text(text) => {
                let text = text.trim();
                if text == "::" {
                    names = file.is_some();
                } else if !is_separator(text) {
                    names = false;
                }
            }
            Piece::Code(code) if is_workspace_file(&code) => {
                names = false;
                file = match read(&code) {
                    Some(mut source) => {
                        if let Some(tests) = tests_module(&code).and_then(|path| read(&path)) {
                            source.push('\n');
                            source.push_str(&tests);
                        }
                        Some((code, source))
                    }
                    None => {
                        missing.push(format!("no file `{code}`"));
                        None
                    }
                };
            }
            Piece::Code(code) if names && is_identifier(&code) => {
                if let Some((path, source)) = &file
                    && !has_function(source, &code)
                {
                    missing.push(format!("no test `{code}` in `{path}`"));
                }
            }
            Piece::Code(_) => {
                file = None;
                names = false;
            }
        }
    }
    missing
}

/// Whether `text` only joins two test names (`,`, `;`, `+`, `and`).
fn is_separator(text: &str) -> bool {
    text.split_whitespace()
        .all(|word| matches!(word, "," | ";" | "+" | "and" | "/"))
}

fn is_workspace_file(code: &str) -> bool {
    (code.starts_with("crates/") || code.starts_with("xtask/"))
        && code.ends_with(".rs")
        && !code.contains(char::is_whitespace)
}

fn is_identifier(code: &str) -> bool {
    !code.is_empty()
        && code
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        && !code.starts_with(|c: char| c.is_ascii_digit())
}

/// The tests module of a source file: `foo/tests.rs` for `foo.rs`,
/// `tests.rs` beside a `mod.rs`.
fn tests_module(path: &str) -> Option<String> {
    let (dir, name) = path.rsplit_once('/')?;
    match name {
        "tests.rs" => None,
        "mod.rs" => Some(format!("{dir}/tests.rs")),
        _ => Some(format!("{dir}/{}/tests.rs", name.strip_suffix(".rs")?)),
    }
}

/// Whether `source` defines a function called `name`.
fn has_function(source: &str, name: &str) -> bool {
    let needle = format!("fn {name}");
    source
        .match_indices(&needle)
        .any(|(at, _)| matches!(source[at + needle.len()..].chars().next(), Some('(' | '<')))
}

/// Checks every `docs/specs/*.md` under `root`.
pub fn check_workspace(root: &Path) -> anyhow::Result<Vec<Finding>> {
    use anyhow::Context;

    let dir = root.join("docs/specs");
    let mut specs: Vec<_> = std::fs::read_dir(&dir)
        .with_context(|| format!("cannot read {}", dir.display()))?
        .filter_map(|entry| Some(entry.ok()?.path()))
        .filter(|path| path.extension().is_some_and(|e| e == "md"))
        .collect();
    specs.sort();
    let read = |path: &str| std::fs::read_to_string(root.join(path)).ok();
    let mut findings = Vec::new();
    for path in specs {
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("cannot read {}", path.display()))?;
        let name = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .display()
            .to_string();
        findings.extend(check(&name, &text, &read));
    }
    Ok(findings)
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

    /// A row of a table: its name, the spec's text and the findings wanted.
    type Case = (&'static str, &'static str, &'static [(usize, &'static str)]);

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
        let cases: &[Case] = &[
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
        let cases: &[Case] = &[
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
