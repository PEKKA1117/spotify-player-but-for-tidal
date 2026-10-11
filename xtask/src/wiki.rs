//! `cargo xtask wiki`: the GitHub wiki built from the user docs in `docs/`.
//! See docs/specs/0015-wiki.md.

use std::path::Path;

use anyhow::{Context, Result, bail};

const REPO: &str = "https://github.com/PEKKA1117/spotify-player-but-for-tidal";

/// Where links to repository files that are not user docs point.
pub const BLOB: &str = "https://github.com/PEKKA1117/spotify-player-but-for-tidal/blob/master/";

/// Each user doc (a file in `docs/`, with its copy in `docs/zh-TW/`) and
/// its wiki page; the zh-TW copy's page adds [`ZH_SUFFIX`].
pub const PAGES: &[(&str, &str)] = &[
    ("tui.md", "TUI"),
    ("config.md", "Configuration"),
    ("playback.md", "Playback"),
    ("login.md", "Login"),
    ("daemon.md", "Daemon"),
    ("mpris.md", "MPRIS"),
];

pub const ZH_SUFFIX: &str = "-zh-TW";

/// One wiki page: `name` is its file name without `.md`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    pub name: String,
    pub text: String,
}

/// The link targets of `text` (a file in the repository directory `dir`,
/// e.g. `docs` or `docs/zh-TW`) rewritten for the wiki.
pub fn rewrite_links(text: &str, dir: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("](") {
        let (before, after) = rest.split_at(start + 2);
        out.push_str(before);
        match after.find(')') {
            Some(end) => {
                out.push_str(&rewrite_target(&after[..end], dir));
                rest = &after[end..];
            }
            None => rest = after,
        }
    }
    out.push_str(rest);
    out
}

/// One link target: a user doc becomes its page, another repository file
/// its `blob/master` URL. Anchors, absolute URLs and targets with a title
/// or spaces are left alone.
fn rewrite_target(target: &str, dir: &str) -> String {
    if target.is_empty()
        || target.starts_with('#')
        || target.contains(':')
        || target.contains(char::is_whitespace)
    {
        return target.to_string();
    }
    let (path, anchor) = match target.split_once('#') {
        Some((path, anchor)) => (path, format!("#{anchor}")),
        None => (target, String::new()),
    };
    let resolved = resolve(dir, path);
    let page = PAGES.iter().find_map(|(file, name)| {
        if resolved == format!("docs/{file}") {
            Some(name.to_string())
        } else if resolved == format!("docs/zh-TW/{file}") {
            Some(format!("{name}{ZH_SUFFIX}"))
        } else {
            None
        }
    });
    match page {
        Some(page) => format!("{page}{anchor}"),
        None => format!("{BLOB}{resolved}{anchor}"),
    }
}

/// `path` relative to the repository directory `dir`, as a
/// repository-relative path.
fn resolve(dir: &str, path: &str) -> String {
    let mut parts: Vec<&str> = dir.split('/').filter(|p| !p.is_empty()).collect();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            part => parts.push(part),
        }
    }
    parts.join("/")
}

/// The text of the first `# ` heading.
fn title(text: &str) -> Option<&str> {
    text.lines()
        .find_map(|line| line.strip_prefix("# "))
        .map(str::trim)
}

/// The `.md` file names directly in `dir`.
fn md_files(dir: &Path) -> Result<Vec<String>> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir).with_context(|| dir.display().to_string())? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if entry.file_type()?.is_file() && name.ends_with(".md") {
            files.push(name);
        }
    }
    Ok(files)
}

/// Every wiki page, from the docs directory `docs`.
pub fn build(docs: &Path) -> Result<Vec<Page>> {
    for (sub, label) in [("", "docs/"), ("zh-TW", "docs/zh-TW/")] {
        for file in md_files(&docs.join(sub))? {
            if !PAGES.iter().any(|(known, _)| *known == file) {
                bail!("{label}{file} is not a wiki page: add it to PAGES in xtask/src/wiki.rs");
            }
        }
    }

    let mut pages = Vec::new();
    let mut en_links = Vec::new();
    let mut zh_links = Vec::new();
    for (file, name) in PAGES {
        for (sub, dir, page, links) in [
            ("", "docs", name.to_string(), &mut en_links),
            (
                "zh-TW",
                "docs/zh-TW",
                format!("{name}{ZH_SUFFIX}"),
                &mut zh_links,
            ),
        ] {
            let path = docs.join(sub).join(file);
            let text = std::fs::read_to_string(&path)
                .with_context(|| format!("{dir}/{file}: {}", path.display()))?;
            let heading = title(&text)
                .with_context(|| format!("{dir}/{file}: no `# ` heading"))?
                .to_string();
            links.push(format!("- [{heading}]({page})"));
            pages.push(Page {
                name: page,
                text: rewrite_links(&text, dir),
            });
        }
    }

    let en_links = en_links.join("\n");
    let zh_links = zh_links.join("\n");
    pages.push(Page {
        name: "Home".into(),
        text: format!(
            "English | [正體中文](Home{ZH_SUFFIX})\n\n# tidal-player\n\n\
             A terminal Tidal player. Source, issues and design specs: \
             [the repository]({REPO}).\n\n{en_links}\n"
        ),
    });
    pages.push(Page {
        name: format!("Home{ZH_SUFFIX}"),
        text: format!(
            "[English](Home) | 正體中文\n\n# tidal-player\n\n\
             終端機 Tidal 播放器。原始碼、issue 與設計規格：[repository]({REPO})。\n\n{zh_links}\n"
        ),
    });
    pages.push(Page {
        name: "_Sidebar".into(),
        text: format!(
            "**[English](Home)**\n\n{en_links}\n\n**[正體中文](Home{ZH_SUFFIX})**\n\n{zh_links}\n"
        ),
    });
    Ok(pages)
}

/// Replace the `.md` files of `out` with `pages`.
pub fn write(pages: &[Page], out: &Path) -> Result<()> {
    std::fs::create_dir_all(out).with_context(|| out.display().to_string())?;
    for file in md_files(out)? {
        std::fs::remove_file(out.join(&file)).with_context(|| file.clone())?;
    }
    for page in pages {
        let path = out.join(format!("{}.md", page.name));
        std::fs::write(&path, &page.text).with_context(|| path.display().to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (path, text) in files {
            let path = dir.path().join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        dir
    }

    /// A docs tree with every page in both languages, each `# Title N`.
    fn full_docs() -> Vec<(String, String)> {
        let mut files = Vec::new();
        for (i, (file, _)) in PAGES.iter().enumerate() {
            files.push((
                (*file).to_string(),
                format!("English | [正體中文](zh-TW/{file})\n\n# Title {i}\n\nSee [specs](specs/0001-architecture.md).\n"),
            ));
            files.push((
                format!("zh-TW/{file}"),
                format!("[English](../{file}) | 正體中文\n\n<a id=\"t\"></a>\n# 標題 {i}\n"),
            ));
        }
        files.push((
            "specs/0001-architecture.md".to_string(),
            "# not a user doc\n".to_string(),
        ));
        files
    }

    fn full_fixture() -> tempfile::TempDir {
        let files = full_docs();
        let refs: Vec<(&str, &str)> = files
            .iter()
            .map(|(p, t)| (p.as_str(), t.as_str()))
            .collect();
        fixture(&refs)
    }

    fn page<'a>(pages: &'a [Page], name: &str) -> &'a Page {
        pages
            .iter()
            .find(|p| p.name == name)
            .unwrap_or_else(|| panic!("no page {name}: {:?}", names(pages)))
    }

    fn names(pages: &[Page]) -> Vec<&str> {
        pages.iter().map(|p| p.name.as_str()).collect()
    }

    /// AC1: each source becomes its page; the text is the source's with its
    /// links rewritten.
    #[test]
    fn ac1_pages_from_sources() {
        let dir = full_fixture();
        let pages = build(dir.path()).unwrap();
        let mut expected: Vec<String> = PAGES
            .iter()
            .flat_map(|(_, name)| [name.to_string(), format!("{name}{ZH_SUFFIX}")])
            .chain(["Home", "Home-zh-TW", "_Sidebar"].map(String::from))
            .collect();
        expected.sort();
        let mut got: Vec<String> = names(&pages).into_iter().map(String::from).collect();
        got.sort();
        assert_eq!(got, expected);

        assert_eq!(
            page(&pages, "TUI").text,
            format!(
                "English | [正體中文](TUI-zh-TW)\n\n# Title 0\n\nSee [specs]({BLOB}docs/specs/0001-architecture.md).\n"
            )
        );
        assert_eq!(
            page(&pages, "MPRIS-zh-TW").text,
            "[English](MPRIS) | 正體中文\n\n<a id=\"t\"></a>\n# 標題 5\n"
        );
    }

    /// AC2: links between user docs, in both directions and languages,
    /// become wiki page links with their anchor kept.
    #[test]
    fn ac2_doc_links() {
        for (dir, text, expected) in [
            ("docs", "[a](tui.md)", "[a](TUI)"),
            (
                "docs",
                "[a](playback.md#settings)",
                "[a](Playback#settings)",
            ),
            ("docs", "[a](zh-TW/config.md)", "[a](Configuration-zh-TW)"),
            ("docs/zh-TW", "[a](daemon.md#x-y)", "[a](Daemon-zh-TW#x-y)"),
            ("docs/zh-TW", "[a](../login.md)", "[a](Login)"),
            ("docs/zh-TW", "[a](../mpris.md#keys)", "[a](MPRIS#keys)"),
            (
                "docs",
                "x [a](tui.md) y [b](config.md#k) z",
                "x [a](TUI) y [b](Configuration#k) z",
            ),
        ] {
            assert_eq!(rewrite_links(text, dir), expected, "{dir}: {text}");
        }
    }

    /// AC3: links to other repository files become `blob/master` URLs;
    /// in-page anchors and absolute URLs stay.
    #[test]
    fn ac3_repo_links() {
        for (dir, text, expected) in [
            (
                "docs",
                "[s](specs/0004-queue-and-controls.md)",
                format!("[s]({BLOB}docs/specs/0004-queue-and-controls.md)"),
            ),
            (
                "docs/zh-TW",
                "[s](../specs/0002-auth.md#storage)",
                format!("[s]({BLOB}docs/specs/0002-auth.md#storage)"),
            ),
            (
                "docs",
                "[e](../examples/app.toml)",
                format!("[e]({BLOB}examples/app.toml)"),
            ),
            (
                "docs/zh-TW",
                "[e](../../examples/keymap.toml)",
                format!("[e]({BLOB}examples/keymap.toml)"),
            ),
            ("docs", "[h](#settings)", "[h](#settings)".to_string()),
            (
                "docs",
                "[x](https://github.com/aome510/spotify-player)",
                "[x](https://github.com/aome510/spotify-player)".to_string(),
            ),
            (
                "docs",
                "`tui.md` and (tui.md)",
                "`tui.md` and (tui.md)".to_string(),
            ),
        ] {
            assert_eq!(rewrite_links(text, dir), expected, "{dir}: {text}");
        }
    }

    /// AC4: `Home`, `Home-zh-TW` and `_Sidebar` link every page of their
    /// language(s), titled with the page's first heading.
    #[test]
    fn ac4_home_and_sidebar() {
        let dir = full_fixture();
        let pages = build(dir.path()).unwrap();
        let home = &page(&pages, "Home").text;
        let home_zh = &page(&pages, "Home-zh-TW").text;
        let sidebar = &page(&pages, "_Sidebar").text;
        assert!(home.contains("[正體中文](Home-zh-TW)"), "{home}");
        assert!(home_zh.contains("[English](Home)"), "{home_zh}");
        for (i, (_, name)) in PAGES.iter().enumerate() {
            let en = format!("[Title {i}]({name})");
            let zh = format!("[標題 {i}]({name}{ZH_SUFFIX})");
            assert_eq!(home.matches(&en).count(), 1, "Home: {en}\n{home}");
            assert_eq!(
                home_zh.matches(&zh).count(),
                1,
                "Home-zh-TW: {zh}\n{home_zh}"
            );
            assert!(sidebar.contains(&en), "_Sidebar: {en}\n{sidebar}");
            assert!(sidebar.contains(&zh), "_Sidebar: {zh}\n{sidebar}");
        }
    }

    /// AC5: a user doc not in [`PAGES`], or a missing source, fails and
    /// names the file.
    #[test]
    fn ac5_unknown_or_missing_doc() {
        let full = full_docs();
        let with_extra = |extra: &str| {
            let mut files = full.clone();
            files.push((extra.to_string(), "# New\n".to_string()));
            files
        };
        let without = |gone: &str| {
            full.iter()
                .filter(|(p, _)| p != gone)
                .cloned()
                .collect::<Vec<_>>()
        };
        for (files, named) in [
            (with_extra("lyrics.md"), "lyrics.md"),
            (with_extra("zh-TW/lyrics.md"), "lyrics.md"),
            (without("login.md"), "login.md"),
            (without("zh-TW/daemon.md"), "daemon.md"),
        ] {
            let refs: Vec<(&str, &str)> = files
                .iter()
                .map(|(p, t)| (p.as_str(), t.as_str()))
                .collect();
            let dir = fixture(&refs);
            let err = build(dir.path()).expect_err(named);
            assert!(format!("{err:#}").contains(named), "{named}: {err:#}");
        }
    }

    /// `write` replaces the old `.md` files and keeps everything else.
    #[test]
    fn write_replaces_pages() {
        let out = fixture(&[("Old.md", "old"), (".git/HEAD", "ref"), ("TUI.md", "stale")]);
        let pages = vec![Page {
            name: "TUI".into(),
            text: "new".into(),
        }];
        write(&pages, out.path()).unwrap();
        assert!(!out.path().join("Old.md").exists());
        assert!(out.path().join(".git/HEAD").exists());
        assert_eq!(
            std::fs::read_to_string(out.path().join("TUI.md")).unwrap(),
            "new"
        );
    }
}
