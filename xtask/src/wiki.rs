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
mod tests;
