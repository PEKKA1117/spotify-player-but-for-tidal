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

/// AC10: a skipped crate that depends on a published one stops the run.
#[test]
fn ac10_stale_dependents() {
    // `v1.5.0` as it was tagged: core bumped, api left at its published version.
    let v150 = || {
        vec![
            krate(
                "tidal-player",
                "1.5.0",
                true,
                &[
                    "tidal-player-api",
                    "tidal-player-audio",
                    "tidal-player-core",
                ],
            ),
            krate("tidal-player-core", "1.5.0", true, &[]),
            krate("tidal-player-api", "1.3.4", true, &["tidal-player-core"]),
            krate("tidal-player-audio", "1.1.1", true, &[]),
            krate("xtask", "1.1.0", false, &[]),
        ]
    };
    let fixed = || {
        let mut crates = v150();
        crates[2].version = "1.3.5".into();
        crates
    };
    let on_crates_io = |core: &[&str]| {
        vec![
            (
                "tidal-player".to_string(),
                Ok(Some(entry("tidal-player", &["1.4.6"]))),
            ),
            (
                "tidal-player-core".to_string(),
                Ok(Some(entry("tidal-player-core", core))),
            ),
            (
                "tidal-player-api".to_string(),
                Ok(Some(entry("tidal-player-api", &["1.3.4"]))),
            ),
            (
                "tidal-player-audio".to_string(),
                Ok(Some(entry("tidal-player-audio", &["1.1.1"]))),
            ),
        ]
    };
    const STALE: &str = "tidal-player-api 1.3.4 is on crates.io and needs the tidal-player-core it was released with, but tidal-player-core 1.5.0 is new: bump tidal-player-api";
    struct Case {
        name: &'static str,
        crates: Vec<Crate>,
        core: &'static [&'static str],
        dry_run: bool,
        ran: &'static [&'static str],
        err: Option<&'static str>,
    }
    let cases = [
        Case {
            name: "a stale dependent publishes nothing",
            crates: v150(),
            core: &["1.4.4"],
            dry_run: false,
            ran: &[],
            err: Some(STALE),
        },
        Case {
            name: "the dry run fails too",
            crates: v150(),
            core: &["1.4.4"],
            dry_run: true,
            ran: &[],
            err: Some(STALE),
        },
        Case {
            name: "the dependent bumped: published in order",
            crates: fixed(),
            core: &["1.4.4"],
            dry_run: false,
            ran: &["tidal-player-core", "tidal-player-api", "tidal-player"],
            err: None,
        },
        Case {
            name: "the recovery: core already there, its dependents new",
            crates: fixed(),
            core: &["1.4.4", "1.5.0"],
            dry_run: false,
            ran: &["tidal-player-api", "tidal-player"],
            err: None,
        },
    ];
    for c in cases {
        let mut index = FakeIndex {
            answers: on_crates_io(c.core).into_iter().collect(),
            fetched: Vec::new(),
        };
        let mut runner = FakeRunner {
            fail: None,
            ran: Vec::new(),
        };
        let mut out = Vec::new();
        let result = run(&c.crates, &mut index, &mut runner, c.dry_run, &mut out);
        let name = c.name;
        assert_eq!(runner.ran, c.ran, "{name}: cargo publish calls");
        match (result, c.err) {
            (Ok(()), None) => {}
            (Err(e), Some(line)) => assert_eq!(format!("{e:#}"), line, "{name}"),
            (got, want) => panic!("{name}: got {got:?}, want error {want:?}"),
        }
    }
}
