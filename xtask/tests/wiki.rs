//! Spec 0015: the wiki built from the real `docs/`.

use std::collections::HashSet;
use std::path::Path;

use xtask::wiki::{PAGES, ZH_SUFFIX, build};

/// AC1–AC5: the real docs build, into every page, and no page links to a
/// relative `.md` file or to a page that is not in the wiki.
#[test]
fn real_docs_build() {
    let docs = Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs");
    let pages = build(&docs).expect("build the wiki");
    assert_eq!(pages.len(), PAGES.len() * 2 + 3, "page count");

    let names: HashSet<&str> = pages.iter().map(|p| p.name.as_str()).collect();
    for (_, name) in PAGES {
        assert!(names.contains(name), "{name}");
        assert!(
            names.contains(format!("{name}{ZH_SUFFIX}").as_str()),
            "{name}{ZH_SUFFIX}"
        );
    }
    for page in &pages {
        for target in page
            .text
            .split("](")
            .skip(1)
            .filter_map(|s| s.split(')').next())
        {
            if target.starts_with('#') || target.contains("://") {
                continue;
            }
            let page_name = target.split('#').next().unwrap();
            assert!(
                names.contains(page_name),
                "{}: link to `{target}`, not a wiki page",
                page.name
            );
        }
    }
}
