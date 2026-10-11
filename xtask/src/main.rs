//! Workspace dev tooling. See docs/specs/0001-architecture.md.

use std::process::ExitCode;

const USAGE: &str = "usage: cargo xtask layering | cargo xtask wiki <out-dir>";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["layering"] => layering(),
        ["wiki", out] => wiki(std::path::Path::new(out)),
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

/// `cargo xtask layering`: enforce the dependency rules of spec 0001.
fn layering() -> ExitCode {
    match xtask::layering::check_workspace(std::path::Path::new(".")) {
        Ok(violations) if violations.is_empty() => {
            println!("layering ok");
            ExitCode::SUCCESS
        }
        Ok(violations) => {
            for v in violations {
                eprintln!("{v}");
            }
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("layering: {e:#}");
            ExitCode::FAILURE
        }
    }
}

/// `cargo xtask wiki <out-dir>`: write the GitHub wiki from `docs/`
/// (spec 0015).
fn wiki(out: &std::path::Path) -> ExitCode {
    let result = xtask::wiki::build(std::path::Path::new("docs"))
        .and_then(|pages| xtask::wiki::write(&pages, out).map(|()| pages.len()));
    match result {
        Ok(count) => {
            println!("wiki: {count} pages in {}", out.display());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("wiki: {e:#}");
            ExitCode::FAILURE
        }
    }
}
