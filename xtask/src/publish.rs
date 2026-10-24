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
mod tests {
    use super::*;
    use anyhow::anyhow;

    fn krate(name: &str, version: &str, publishable: bool, deps: &[&str]) -> Crate {
        Crate {
            name: name.into(),
            version: version.into(),
            publishable,
            deps: deps.iter().map(|d| d.to_string()).collect(),
        }
    }

    /// The workspace of spec 0018's Context, versions as of `v1.2.0`.
    fn workspace() -> Vec<Crate> {
        vec![
            krate(
                "tidal-player",
                "1.2.0",
                true,
                &[
                    "tidal-player-api",
                    "tidal-player-audio",
                    "tidal-player-core",
                ],
            ),
            krate("tidal-player-core", "1.2.0", true, &[]),
            krate("tidal-player-api", "1.1.0", true, &["tidal-player-core"]),
            krate("tidal-player-audio", "1.1.0", true, &["tidal-player-core"]),
            krate("xtask", "1.2.0", false, &[]),
        ]
    }

    fn v(vers: &str, yanked: bool) -> IndexVersion {
        IndexVersion {
            vers: vers.into(),
            yanked,
        }
    }

    fn published(rows: &[(&str, Vec<IndexVersion>)]) -> BTreeMap<String, Vec<IndexVersion>> {
        rows.iter()
            .map(|(n, vs)| (n.to_string(), vs.clone()))
            .collect()
    }

    fn names(cs: &[Crate]) -> Vec<&str> {
        cs.iter().map(|c| c.name.as_str()).collect()
    }

    const ALL: [&str; 4] = [
        "tidal-player-core",
        "tidal-player-api",
        "tidal-player-audio",
        "tidal-player",
    ];

    /// AC1: the plan.
    #[test]
    fn ac1_plan() {
        let on_index = |n: &str, vs: &[&str]| {
            (
                n.to_string(),
                vs.iter().map(|s| v(s, false)).collect::<Vec<_>>(),
            )
        };
        let today: BTreeMap<_, _> = [
            on_index("tidal-player", &["1.0.0", "1.1.0"]),
            on_index("tidal-player-core", &["1.0.0", "1.1.0"]),
            on_index("tidal-player-api", &["1.0.0", "1.1.0"]),
            on_index("tidal-player-audio", &["1.0.0", "1.1.0"]),
        ]
        .into();
        let everything: BTreeMap<_, _> = [
            on_index("tidal-player", &["1.2.0"]),
            on_index("tidal-player-core", &["1.2.0"]),
            on_index("tidal-player-api", &["1.1.0"]),
            on_index("tidal-player-audio", &["1.1.0"]),
        ]
        .into();
        let mut shuffled = workspace();
        shuffled.reverse();
        let mut new_crate = workspace();
        new_crate.push(krate(
            "tidal-player-new",
            "0.1.0",
            true,
            &["tidal-player-core"],
        ));
        let mut with_new_deps = new_crate.clone();
        with_new_deps[0].deps.push("tidal-player-new".into());

        // (name, crates, published, planned, skipped)
        type Case<'a> = (
            &'a str,
            Vec<Crate>,
            BTreeMap<String, Vec<IndexVersion>>,
            Vec<&'a str>,
            Vec<&'a str>,
        );
        let cases: Vec<Case> = vec![
            (
                "every version new",
                workspace(),
                published(&[]),
                ALL.to_vec(),
                vec![],
            ),
            (
                "today's case",
                workspace(),
                today,
                vec!["tidal-player-core", "tidal-player"],
                vec!["tidal-player-api", "tidal-player-audio"],
            ),
            (
                "none new",
                workspace(),
                everything.clone(),
                vec![],
                ALL.to_vec(),
            ),
            (
                "a crate never published",
                new_crate,
                everything.clone(),
                vec!["tidal-player-new"],
                ALL.to_vec(),
            ),
            (
                "a yanked version counts as there",
                workspace(),
                published(&[
                    ("tidal-player", vec![v("1.2.0", true)]),
                    ("tidal-player-core", vec![v("1.2.0", false)]),
                    ("tidal-player-api", vec![v("1.1.0", false)]),
                    ("tidal-player-audio", vec![v("1.1.0", false)]),
                ]),
                vec![],
                ALL.to_vec(),
            ),
            (
                "input order shuffled",
                shuffled,
                published(&[]),
                ALL.to_vec(),
                vec![],
            ),
            (
                "a new crate that tidal-player needs goes before it",
                with_new_deps,
                everything,
                vec!["tidal-player-new"],
                ALL.to_vec(),
            ),
        ];
        for (name, crates, published, planned, skipped) in cases {
            let got = plan(&crates, &published);
            assert_eq!(names(&got.publish), planned, "{name}: planned");
            assert_eq!(names(&got.skipped), skipped, "{name}: skipped");
        }
    }

    /// AC2: the index path by name length.
    #[test]
    fn ac2_index_path() {
        let cases = [
            ("a", "1/a"),
            ("ab", "2/ab"),
            ("abc", "3/a/abc"),
            ("abcd", "ab/cd/abcd"),
            ("tidal-player-api", "ti/da/tidal-player-api"),
            ("Serde_JSON", "se/rd/serde_json"),
            ("XYZ", "3/x/xyz"),
        ];
        for (name, want) in cases {
            assert_eq!(index_path(name), want, "{name}");
        }
    }

    /// AC2: parsing index lines.
    #[test]
    fn ac2_parse_index() {
        let line = |vers: &str, yanked: bool| {
            format!(
                r#"{{"name":"tidal-player-api","vers":"{vers}","deps":[],"cksum":"00","features":{{}},"yanked":{yanked},"rust_version":"1.97","v":2}}"#
            )
        };
        let two = format!("{}\n{}\n", line("1.0.0", true), line("1.1.0", false));
        let blanks = format!(
            "\n{}\n\n  \n{}\n",
            line("1.0.0", true),
            line("1.1.0", false)
        );
        let no_vers = format!("{}\n{{\"name\":\"x\"}}\n", line("1.0.0", false));
        let not_json = format!("{}\nnot json\n", line("1.0.0", false));
        type Case<'a> = (&'a str, String, Result<Vec<IndexVersion>, &'a str>);
        let cases: Vec<Case> = vec![
            (
                "two versions, one yanked",
                two.clone(),
                Ok(vec![v("1.0.0", true), v("1.1.0", false)]),
            ),
            (
                "blank lines ignored",
                blanks,
                Ok(vec![v("1.0.0", true), v("1.1.0", false)]),
            ),
            ("empty", String::new(), Ok(vec![])),
            ("no vers", no_vers, Err("tidal-player-api line 2")),
            ("not json", not_json, Err("tidal-player-api line 2")),
        ];
        for (name, text, want) in cases {
            match (parse_index("tidal-player-api", &text), want) {
                (Ok(got), Ok(want)) => assert_eq!(got, want, "{name}"),
                (Err(e), Err(want)) => {
                    assert!(format!("{e:#}").contains(want), "{name}: {e:#}")
                }
                (got, want) => panic!("{name}: got {got:?}, want {want:?}"),
            }
        }
    }

    struct FakeIndex {
        /// name to the text, `None` for 404, `Err` for a failed fetch
        answers: BTreeMap<String, std::result::Result<Option<String>, String>>,
        fetched: Vec<String>,
    }

    impl Index for FakeIndex {
        fn fetch(&mut self, name: &str) -> Result<Option<String>> {
            self.fetched.push(name.to_string());
            match self.answers.get(name) {
                Some(Ok(text)) => Ok(text.clone()),
                Some(Err(e)) => Err(anyhow!("{e}")),
                None => Ok(None),
            }
        }
    }

    struct FakeRunner {
        fail: Option<&'static str>,
        ran: Vec<String>,
    }

    impl Runner for FakeRunner {
        fn cargo_publish(&mut self, name: &str) -> Result<()> {
            self.ran.push(name.to_string());
            if self.fail == Some(name) {
                return Err(anyhow!("upload refused"));
            }
            Ok(())
        }
    }

    fn entry(name: &str, vers: &[&str]) -> String {
        vers.iter()
            .map(|v| format!("{{\"name\":\"{name}\",\"vers\":\"{v}\",\"yanked\":false}}\n"))
            .collect()
    }

    /// AC3: the command over fakes.
    #[test]
    fn ac3_publish_command() {
        let today = || {
            [
                (
                    "tidal-player-api",
                    Ok(Some(entry("tidal-player-api", &["1.0.0", "1.1.0"]))),
                ),
                (
                    "tidal-player-audio",
                    Ok(Some(entry("tidal-player-audio", &["1.0.0", "1.1.0"]))),
                ),
                (
                    "tidal-player-core",
                    Ok(Some(entry("tidal-player-core", &["1.0.0", "1.1.0"]))),
                ),
                (
                    "tidal-player",
                    Ok(Some(entry("tidal-player", &["1.0.0", "1.1.0"]))),
                ),
            ]
        };
        let all_there = || {
            [
                (
                    "tidal-player-api",
                    Ok(Some(entry("tidal-player-api", &["1.1.0"]))),
                ),
                (
                    "tidal-player-audio",
                    Ok(Some(entry("tidal-player-audio", &["1.1.0"]))),
                ),
                (
                    "tidal-player-core",
                    Ok(Some(entry("tidal-player-core", &["1.2.0"]))),
                ),
                ("tidal-player", Ok(Some(entry("tidal-player", &["1.2.0"])))),
            ]
        };
        type Answers = Vec<(&'static str, std::result::Result<Option<String>, String>)>;
        // (name, answers, dry_run, failing crate, expected runs, output lines, error part)
        struct Case {
            name: &'static str,
            answers: Answers,
            dry_run: bool,
            fail: Option<&'static str>,
            ran: Vec<&'static str>,
            out: Vec<&'static str>,
            err: Option<&'static str>,
        }
        let cases = vec![
            Case {
                name: "today's case",
                answers: today().into(),
                dry_run: false,
                fail: None,
                ran: vec!["tidal-player-core", "tidal-player"],
                out: vec![
                    "tidal-player-api 1.1.0: already on crates.io",
                    "tidal-player-audio 1.1.0: already on crates.io",
                    "publishing tidal-player-core 1.2.0",
                    "publishing tidal-player 1.2.0",
                ],
                err: None,
            },
            Case {
                name: "404 means never published",
                answers: vec![],
                dry_run: false,
                fail: None,
                ran: ALL.to_vec(),
                out: vec![
                    "publishing tidal-player-core 1.2.0",
                    "publishing tidal-player-api 1.1.0",
                    "publishing tidal-player-audio 1.1.0",
                    "publishing tidal-player 1.2.0",
                ],
                err: None,
            },
            Case {
                name: "fetch failure publishes nothing",
                answers: {
                    let mut a: Answers = today().into();
                    a.push(("tidal-player-audio", Err("HTTP 503".into())));
                    a
                },
                dry_run: false,
                fail: None,
                ran: vec![],
                out: vec![],
                err: Some("HTTP 503"),
            },
            Case {
                name: "failing cargo publish stops there",
                answers: vec![],
                dry_run: false,
                fail: Some("tidal-player-api"),
                ran: vec!["tidal-player-core", "tidal-player-api"],
                out: vec![
                    "publishing tidal-player-core 1.2.0",
                    "publishing tidal-player-api 1.1.0",
                ],
                err: Some("tidal-player-api"),
            },
            Case {
                name: "dry run runs nothing",
                answers: today().into(),
                dry_run: true,
                fail: None,
                ran: vec![],
                out: vec![
                    "tidal-player-api 1.1.0: already on crates.io",
                    "tidal-player-audio 1.1.0: already on crates.io",
                    "would publish tidal-player-core 1.2.0",
                    "would publish tidal-player 1.2.0",
                ],
                err: None,
            },
            Case {
                name: "empty plan",
                answers: all_there().into(),
                dry_run: false,
                fail: None,
                ran: vec![],
                out: vec![
                    "tidal-player-core 1.2.0: already on crates.io",
                    "tidal-player-api 1.1.0: already on crates.io",
                    "tidal-player-audio 1.1.0: already on crates.io",
                    "tidal-player 1.2.0: already on crates.io",
                    "nothing to publish",
                ],
                err: None,
            },
        ];
        for c in cases {
            let mut index = FakeIndex {
                answers: c
                    .answers
                    .into_iter()
                    .map(|(n, a)| (n.to_string(), a))
                    .collect(),
                fetched: Vec::new(),
            };
            let mut runner = FakeRunner {
                fail: c.fail,
                ran: Vec::new(),
            };
            let mut out = Vec::new();
            let result = run(&workspace(), &mut index, &mut runner, c.dry_run, &mut out);
            let name = c.name;
            let mut fetched = index.fetched.clone();
            fetched.sort();
            let mut want: Vec<_> = ALL.iter().map(|s| s.to_string()).collect();
            want.sort();
            if name != "fetch failure publishes nothing" {
                assert_eq!(fetched, want, "{name}: each publishable crate fetched once");
            }
            assert_eq!(runner.ran, c.ran, "{name}: cargo publish calls");
            let text = String::from_utf8(out).unwrap();
            assert_eq!(text.lines().collect::<Vec<_>>(), c.out, "{name}: output");
            match (result, c.err) {
                (Ok(()), None) => {}
                (Err(e), Some(part)) => assert!(format!("{e:#}").contains(part), "{name}: {e:#}"),
                (got, want) => panic!("{name}: got {got:?}, want error {want:?}"),
            }
        }
    }
}
