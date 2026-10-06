//! Enforcement of the workspace dependency rules (spec 0001, AC3).
//!
//! The rules are checked on an in-memory [`Graph`], so they are unit-tested
//! without cargo; [`check`] is pure.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;

/// Kind of a dependency edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DepKind {
    /// A normal `[dependencies]` edge.
    Normal,
    /// A `[dev-dependencies]` edge.
    Dev,
    /// A `[build-dependencies]` edge.
    Build,
}

/// A resolved dependency graph: package name to its dependencies.
#[derive(Clone, Debug, Default)]
pub struct Graph {
    /// Names of the workspace member packages.
    pub members: BTreeSet<String>,
    /// Outgoing edges per package name.
    pub deps: BTreeMap<String, Vec<(String, DepKind)>>,
}

impl Graph {
    /// Creates a graph with the given workspace members and no edges.
    pub fn new<I, S>(members: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Graph {
            members: members.into_iter().map(Into::into).collect(),
            deps: BTreeMap::new(),
        }
    }

    /// Adds an edge `from -> to` of the given kind.
    pub fn add_edge(&mut self, from: &str, to: &str, kind: DepKind) {
        self.deps
            .entry(from.to_string())
            .or_default()
            .push((to.to_string(), kind));
    }
}

/// A broken layering rule.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Violation {
    /// The offending package.
    pub from: String,
    /// The package it must not depend on.
    pub to: String,
    /// For transitive violations, the full path from `from` to `to`.
    pub path: Vec<String>,
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} -> {}", self.from, self.to)?;
        if self.path.len() > 2 {
            write!(f, " (via {})", self.path.join(" -> "))?;
        }
        Ok(())
    }
}

const CORE: &str = "tidal-player-core";
const API: &str = "tidal-player-api";
const AUDIO: &str = "tidal-player-audio";
const APP: &str = "tidal-player";

/// External crates that `tidal-player-core` must never reach through normal dependencies.
const CORE_FORBIDDEN: [&str; 9] = [
    "tokio",
    "reqwest",
    "crossterm",
    "ratatui",
    "alsa",
    "symphonia",
    "zbus",
    "keyring",
    "serde_json",
];

/// Workspace crates a workspace member may depend on.
fn allowed_members(from: &str) -> &'static [&'static str] {
    match from {
        API => &[CORE],
        APP => &[CORE, API, AUDIO],
        _ => &[],
    }
}

/// Checks `graph` against the layering rules and returns every violation.
///
/// Edges between workspace members must appear in the allowed table. For
/// `tidal-player-core`, the normal-dependency closure must not contain any of
/// [`CORE_FORBIDDEN`]. Edges to external crates are otherwise unrestricted.
pub fn check(graph: &Graph) -> Vec<Violation> {
    let mut out = Vec::new();

    for member in &graph.members {
        for (to, _kind) in graph.deps.get(member).into_iter().flatten() {
            if graph.members.contains(to)
                && member != to
                && !allowed_members(member).contains(&to.as_str())
            {
                out.push(Violation {
                    from: member.clone(),
                    to: to.clone(),
                    path: vec![member.clone(), to.clone()],
                });
            }
        }
    }

    if graph.members.contains(CORE) {
        out.extend(core_forbidden(graph));
    }
    out
}

/// Breadth-first walk of core's normal edges, reporting each forbidden crate once.
fn core_forbidden(graph: &Graph) -> Vec<Violation> {
    let mut out = Vec::new();
    let mut seen: BTreeSet<&str> = BTreeSet::from([CORE]);
    let mut queue: VecDeque<Vec<&str>> = VecDeque::from([vec![CORE]]);
    while let Some(path) = queue.pop_front() {
        let node = path[path.len() - 1];
        for (to, kind) in graph.deps.get(node).into_iter().flatten() {
            if *kind != DepKind::Normal || !seen.insert(to) {
                continue;
            }
            let mut next = path.clone();
            next.push(to);
            if CORE_FORBIDDEN.contains(&to.as_str()) {
                out.push(Violation {
                    from: CORE.to_string(),
                    to: to.clone(),
                    path: next.iter().map(|s| s.to_string()).collect(),
                });
            }
            queue.push_back(next);
        }
    }
    out
}

/// Builds a [`Graph`] from `cargo metadata` output, using the resolved
/// dependency graph (so features and optional dependencies are accounted for).
pub fn graph_from_metadata(metadata: &cargo_metadata::Metadata) -> anyhow::Result<Graph> {
    use cargo_metadata::DependencyKind;

    let resolve = metadata
        .resolve
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("cargo metadata returned no resolved dependency graph"))?;
    let names: BTreeMap<_, _> = metadata
        .packages
        .iter()
        .map(|p| (&p.id, p.name.to_string()))
        .collect();
    let name_of = |id: &cargo_metadata::PackageId| {
        names
            .get(id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("unknown package id {id}"))
    };

    let mut graph = Graph::new(
        metadata
            .workspace_members
            .iter()
            .map(&name_of)
            .collect::<anyhow::Result<Vec<_>>>()?,
    );
    for node in &resolve.nodes {
        let from = name_of(&node.id)?;
        for dep in &node.deps {
            let to = name_of(&dep.pkg)?;
            for info in &dep.dep_kinds {
                let kind = match info.kind {
                    DependencyKind::Normal => DepKind::Normal,
                    DependencyKind::Development => DepKind::Dev,
                    DependencyKind::Build => DepKind::Build,
                    _ => anyhow::bail!("unknown dependency kind on {from} -> {to}"),
                };
                graph.add_edge(&from, &to, kind);
            }
        }
    }
    Ok(graph)
}

/// Runs `cargo metadata` in `dir` and checks the layering rules.
pub fn check_workspace(dir: &std::path::Path) -> anyhow::Result<Vec<Violation>> {
    let metadata = cargo_metadata::MetadataCommand::new()
        .current_dir(dir)
        .exec()?;
    Ok(check(&graph_from_metadata(&metadata)?))
}

#[cfg(test)]
mod tests {
    use super::DepKind::{Dev, Normal};
    use super::*;

    const MEMBERS: [&str; 5] = [
        "tidal-player-core",
        "tidal-player-api",
        "tidal-player-audio",
        "tidal-player",
        "xtask",
    ];

    /// (name, edges, expected `(from, to)` violations)
    type Case<'a> = (
        &'a str,
        Vec<(&'a str, &'a str, DepKind)>,
        Vec<(String, String)>,
    );

    fn graph(edges: &[(&str, &str, DepKind)]) -> Graph {
        let mut g = Graph::new(MEMBERS);
        for (from, to, kind) in edges {
            g.add_edge(from, to, *kind);
        }
        g
    }

    fn pairs(v: &[Violation]) -> Vec<(String, String)> {
        let mut out: Vec<_> = v.iter().map(|v| (v.from.clone(), v.to.clone())).collect();
        out.sort();
        out
    }

    #[test]
    fn ac3_rules() {
        let c = "tidal-player-core";
        let a = "tidal-player-api";
        let u = "tidal-player-audio";
        let p = "tidal-player";
        let pair = |f: &str, t: &str| (f.to_string(), t.to_string());

        // (name, edges, expected (from, to) violations)
        let cases: Vec<Case> = vec![
            (
                "allowed graph",
                vec![
                    (c, "serde", Normal),
                    (c, "thiserror", Normal),
                    (a, c, Normal),
                    (a, "reqwest", Normal),
                    (a, "tokio", Normal),
                    (u, "symphonia", Normal),
                    (u, "alsa", Normal),
                    (p, c, Normal),
                    (p, a, Normal),
                    (p, u, Normal),
                    (p, "tokio", Normal),
                    ("xtask", "cargo_metadata", Normal),
                    ("serde", "serde_derive", Normal),
                ],
                vec![],
            ),
            (
                "core -> tokio",
                vec![(c, "tokio", Normal)],
                vec![pair(c, "tokio")],
            ),
            (
                "core -> serde_x -> tokio (transitive)",
                vec![(c, "serde_x", Normal), ("serde_x", "tokio", Normal)],
                vec![pair(c, "tokio")],
            ),
            (
                "core -> serde_json",
                vec![(c, "serde_json", Normal)],
                vec![pair(c, "serde_json")],
            ),
            ("audio -> core", vec![(u, c, Normal)], vec![pair(u, c)]),
            ("api -> audio", vec![(a, u, Normal)], vec![pair(a, u)]),
            ("core -> api", vec![(c, a, Normal)], vec![pair(c, a)]),
            (
                "xtask -> core",
                vec![("xtask", c, Normal)],
                vec![pair("xtask", c)],
            ),
            (
                "core with tokio as dev-dependency only",
                vec![(c, "tokio", Dev)],
                vec![],
            ),
            (
                "core dev-dependency whose own deps reach tokio",
                vec![(c, "x", Dev), ("x", "tokio", Normal)],
                vec![],
            ),
        ];

        for (name, edges, expected) in cases {
            let got = pairs(&check(&graph(&edges)));
            assert_eq!(got, expected, "case: {name}");
        }
    }

    #[test]
    fn ac3_violation_display_names_the_edge() {
        let g = graph(&[("tidal-player-audio", "tidal-player-core", Normal)]);
        let v = check(&g);
        assert_eq!(v.len(), 1, "expected one violation");
        assert_eq!(v[0].to_string(), "tidal-player-audio -> tidal-player-core");
    }
}
