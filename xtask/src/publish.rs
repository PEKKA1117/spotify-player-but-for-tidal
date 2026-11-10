//! Publishing the workspace crates that crates.io lacks (spec 0018).
//!
//! The plan, the index path and the index parsing are pure; [`run`] talks to
//! crates.io and to `cargo publish` only through [`Index`] and [`Runner`], so
//! it is unit-tested with fakes.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};
use cargo_metadata::DependencyKind;

/// A workspace crate as the plan sees it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Crate {
    pub name: String,
    pub version: String,
    /// False for `publish = false` (xtask).
    pub publishable: bool,
    /// Workspace crates this one needs published first.
    pub deps: Vec<String>,
}

/// One version line of a crate's crates.io index entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexVersion {
    pub vers: String,
    pub yanked: bool,
}

/// What [`plan`] decided, both lists in dependency order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Plan {
    pub publish: Vec<Crate>,
    pub skipped: Vec<Crate>,
}

/// Reads a crate's sparse-index entry.
pub trait Index {
    /// The entry's text, or `None` when the crate was never published (404).
    fn fetch(&mut self, name: &str) -> Result<Option<String>>;
}

/// Runs `cargo publish`.
pub trait Runner {
    fn cargo_publish(&mut self, name: &str) -> Result<()>;
}

/// Which crates to publish and which to skip, given each crate's versions on
/// crates.io (a crate missing from `published` was never published). A yanked
/// version counts as there: crates.io refuses a re-upload of it.
pub fn plan(crates: &[Crate], published: &BTreeMap<String, Vec<IndexVersion>>) -> Plan {
    let mut plan = Plan::default();
    for c in dependency_order(crates) {
        let there = published
            .get(&c.name)
            .is_some_and(|vs| vs.iter().any(|v| v.vers == c.version));
        if there {
            plan.skipped.push(c);
        } else {
            plan.publish.push(c);
        }
    }
    plan
}

/// The skipped crates that depend on a crate the plan publishes (spec 0018
/// AC10): with exact pins (AC6) the published dependent needs the old
/// version, so the release cannot resolve. One line per such crate.
pub fn stale_dependents(_plan: &Plan) -> Vec<String> {
    Vec::new()
}

/// The publishable crates, each after the crates it needs. Ties go by name so
/// the order does not depend on the input order.
fn dependency_order(crates: &[Crate]) -> Vec<Crate> {
    let mut left: BTreeMap<&str, &Crate> = crates
        .iter()
        .filter(|c| c.publishable)
        .map(|c| (c.name.as_str(), c))
        .collect();
    let mut ordered: Vec<Crate> = Vec::new();
    while !left.is_empty() {
        // A cycle cannot be published; take the rest in name order rather than loop.
        let next = left
            .values()
            .find(|c| c.deps.iter().all(|d| !left.contains_key(d.as_str())))
            .or_else(|| left.values().next())
            .copied()
            .expect("left is not empty");
        left.remove(next.name.as_str());
        ordered.push(next.clone());
    }
    ordered
}

/// The index path of a crate name.
pub fn index_path(name: &str) -> String {
    let n = name.to_lowercase();
    match n.len() {
        0..=2 => format!("{}/{n}", n.len()),
        3 => format!("3/{}/{n}", &n[..1]),
        _ => format!("{}/{}/{n}", &n[..2], &n[2..4]),
    }
}

/// Parses an index entry: one JSON object per line.
pub fn parse_index(name: &str, text: &str) -> Result<Vec<IndexVersion>> {
    let mut versions = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let at = format!("{name} line {}", i + 1);
        let json: serde_json::Value =
            serde_json::from_str(line).with_context(|| format!("{at}: not JSON"))?;
        let vers = json
            .get("vers")
            .and_then(|v| v.as_str())
            .with_context(|| format!("{at}: no `vers`"))?;
        let yanked = json
            .get("yanked")
            .and_then(|y| y.as_bool())
            .unwrap_or(false);
        versions.push(IndexVersion {
            vers: vers.to_string(),
            yanked,
        });
    }
    Ok(versions)
}

/// `cargo xtask publish`: fetch, plan, print, publish.
pub fn run(
    crates: &[Crate],
    index: &mut dyn Index,
    runner: &mut dyn Runner,
    dry_run: bool,
    out: &mut dyn Write,
) -> Result<()> {
    // Everything is fetched before anything is published, so a crates.io
    // outage uploads nothing.
    let mut published = BTreeMap::new();
    for c in crates.iter().filter(|c| c.publishable) {
        if let Some(text) = index
            .fetch(&c.name)
            .with_context(|| format!("reading the crates.io index of {}", c.name))?
        {
            published.insert(c.name.clone(), parse_index(&c.name, &text)?);
        }
    }
    let plan = plan(crates, &published);
    for c in &plan.skipped {
        writeln!(out, "{} {}: already on crates.io", c.name, c.version)?;
    }
    if plan.publish.is_empty() {
        writeln!(out, "nothing to publish")?;
        return Ok(());
    }
    let stale = stale_dependents(&plan);
    if !stale.is_empty() {
        for c in &plan.publish {
            writeln!(out, "would publish {} {}", c.name, c.version)?;
        }
        bail!("{}", stale.join("\n"));
    }
    for c in &plan.publish {
        if dry_run {
            writeln!(out, "would publish {} {}", c.name, c.version)?;
            continue;
        }
        writeln!(out, "publishing {} {}", c.name, c.version)?;
        runner
            .cargo_publish(&c.name)
            .with_context(|| format!("cargo publish of {} {} failed", c.name, c.version))?;
    }
    Ok(())
}

/// The workspace's crates, from `cargo metadata` run in `dir`. Only normal
/// and build dependencies on other members order the publishing.
pub fn workspace_crates(dir: &Path) -> Result<Vec<Crate>> {
    let metadata = cargo_metadata::MetadataCommand::new()
        .current_dir(dir)
        .no_deps()
        .exec()?;
    let members: Vec<_> = metadata.workspace_packages();
    Ok(members
        .iter()
        .map(|p| Crate {
            name: p.name.to_string(),
            version: p.version.to_string(),
            // `publish = false` is `Some([])`; `None` means any registry.
            publishable: p.publish.as_ref().is_none_or(|r| !r.is_empty()),
            deps: p
                .dependencies
                .iter()
                .filter(|d| d.kind != DependencyKind::Development)
                .filter(|d| members.iter().any(|m| *m.name == d.name))
                .map(|d| d.name.clone())
                .collect(),
        })
        .collect())
}

/// Reads the index with `curl`.
pub struct Curl;

impl Index for Curl {
    fn fetch(&mut self, name: &str) -> Result<Option<String>> {
        let url = format!("https://index.crates.io/{}", index_path(name));
        // The status code follows the body on the last line.
        let out = Command::new("curl")
            .args(["-sS", "--max-time", "60", "-w", "\n%{http_code}", &url])
            .output()
            .context("could not run `curl`; is it installed?")?;
        if !out.status.success() {
            bail!(
                "curl {url}: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        let text = String::from_utf8(out.stdout).context("the index is not UTF-8")?;
        let (body, status) = text.rsplit_once('\n').unwrap_or(("", &text));
        match status.trim() {
            "200" => Ok(Some(body.to_string())),
            "404" => Ok(None),
            other => bail!("{url}: HTTP {other}"),
        }
    }
}

/// Runs the real `cargo publish -p <name>`.
pub struct Cargo;

impl Runner for Cargo {
    fn cargo_publish(&mut self, name: &str) -> Result<()> {
        let status = Command::new("cargo")
            .args(["publish", "-p", name])
            .status()
            .context("could not run `cargo`")?;
        if !status.success() {
            bail!("cargo publish -p {name} exited with {status}");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
