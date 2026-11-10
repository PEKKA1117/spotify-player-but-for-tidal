//! The cover cache (spec 0010 "The cover cache", 0016): the current entry's
//! album cover downloaded into `<cache dir>/covers/<cover>.jpg`, so every
//! MPRIS consumer gets a `file://` URL and a TUI can read the image. The decisions (what to delete, which
//! URL, where the directory is) are pure; the download sits behind
//! [`CoverCache`].

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use tidal_player_core::cover_url;
use tokio::sync::watch;

/// Overrides the cache directory.
pub const CACHE_DIR_VAR: &str = "TIDAL_PLAYER_CACHE_DIR";
/// The size the player asks for.
pub const SIZE: &str = "640x640";
/// A download's deadline.
pub const TIMEOUT: Duration = Duration::from_secs(10);
/// The largest cover accepted.
pub const LIMIT: usize = 5 * 1024 * 1024;
/// Where Tidal serves the images (the prefix of [`cover_url`]).
pub const IMAGES_BASE: &str = "https://resources.tidal.com";

/// The cache directory: `$TIDAL_PLAYER_CACHE_DIR`, else
/// `$XDG_CACHE_HOME/tidal-player`, else `~/.cache/tidal-player`; an empty
/// variable counts as unset.
pub fn cache_dir(var: Option<&str>, xdg_cache_home: Option<&str>, home: Option<&Path>) -> PathBuf {
    let set = |v: Option<&str>| v.filter(|v| !v.is_empty()).map(PathBuf::from);
    if let Some(dir) = set(var) {
        return dir;
    }
    if let Some(xdg) = set(xdg_cache_home) {
        return xdg.join("tidal-player");
    }
    home.unwrap_or(Path::new("."))
        .join(".cache")
        .join("tidal-player")
}

/// [`cache_dir`] for this process's environment.
pub fn process_cache_dir() -> PathBuf {
    let home = directories::BaseDirs::new().map(|d| d.home_dir().to_owned());
    cache_dir(
        std::env::var(CACHE_DIR_VAR).ok().as_deref(),
        std::env::var("XDG_CACHE_HOME").ok().as_deref(),
        home.as_deref(),
    )
}

/// The file `cover` has in the cache under `cache_dir`, for a reader that
/// never writes to it (the TUI, spec 0016); `None` for what is not a cover
/// ID, so no other path is ever read.
pub fn cached_file(cache_dir: &Path, cover: &str) -> Option<PathBuf> {
    is_cover_id(cover).then(|| cache_dir.join("covers").join(format!("{cover}.jpg")))
}

/// Where a cover stands for the current entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Art {
    /// The cache is off (`max_cover_arts = 0`).
    Off,
    Downloading,
    /// In the cache, at this path.
    Cached(PathBuf),
    /// The download or the write failed.
    Failed,
}

/// The `mpris:artUrl` for `cover` in `art`'s state (spec 0010): the file
/// once cached, nothing while it downloads, Tidal's URL otherwise.
pub fn art_url(cover: Option<&str>, art: &Art) -> Option<String> {
    let cover = cover?;
    match art {
        Art::Cached(path) => Some(file_url(path)),
        Art::Downloading => None,
        Art::Off | Art::Failed => Some(cover_url(cover, SIZE)),
    }
}

/// `file://` and the path, its bytes outside `[A-Za-z0-9/._~-]` escaped.
fn file_url(path: &Path) -> String {
    use std::fmt::Write;
    use std::os::unix::ffi::OsStrExt;
    let mut url = String::from("file://");
    for &b in path.as_os_str().as_bytes() {
        if b.is_ascii_alphanumeric() || b"/._~-".contains(&b) {
            url.push(char::from(b));
        } else {
            let _ = write!(url, "%{b:02X}");
        }
    }
    url
}

/// The covers to delete so that at most `max` remain, least recently used
/// (oldest `SystemTime`) first, never one in `keep`.
pub fn evict(files: &[(String, SystemTime)], max: usize, keep: &[&str]) -> Vec<String> {
    let mut by_age: Vec<&(String, SystemTime)> = files.iter().collect();
    by_age.sort_by_key(|(_, used)| *used);
    let mut left = files.len();
    let mut gone = Vec::new();
    for (name, _) in by_age {
        if left <= max {
            break;
        }
        if keep.contains(&name.as_str()) {
            continue;
        }
        gone.push(name.clone());
        left -= 1;
    }
    gone
}

/// Whether `name` is `<uuid>.jpg` (lower-case hex, as Tidal gives them).
fn is_cover_file(name: &str) -> bool {
    name.strip_suffix(".jpg").is_some_and(is_cover_id)
}

/// A UUID in lower-case hex: `8-4-4-4-12`.
fn is_cover_id(id: &str) -> bool {
    let groups: Vec<&str> = id.split('-').collect();
    groups.len() == 5
        && groups
            .iter()
            .zip([8, 4, 4, 4, 12])
            .all(|(g, n)| g.len() == n && g.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')))
}

/// The file names in `covers/` to delete at start: anything that is not
/// `<uuid>.jpg`.
pub fn start_cleanup(names: &[String]) -> Vec<String> {
    names
        .iter()
        .filter(|name| !is_cover_file(name))
        .cloned()
        .collect()
}

/// The cache on disk and its downloads.
#[derive(Debug, Clone)]
pub struct CoverCache {
    covers: PathBuf,
    max: usize,
    client: reqwest::Client,
    base: String,
    timeout: Duration,
}

impl CoverCache {
    /// The cache in `cache_dir` (its `covers/`), holding at most `max`.
    pub fn new(cache_dir: &Path, max: usize) -> Self {
        Self {
            covers: cache_dir.join("covers"),
            max,
            // No credentials: covers are public (spec 0010 "Download").
            client: reqwest::Client::new(),
            base: IMAGES_BASE.to_owned(),
            timeout: TIMEOUT,
        }
    }

    /// Images from `base` instead of Tidal's (tests).
    #[must_use]
    pub fn with_base(mut self, base: &str) -> Self {
        self.base = base.trim_end_matches('/').to_owned();
        self
    }

    /// The most covers kept; `0`: off.
    pub fn max(&self) -> usize {
        self.max
    }

    /// `covers/`.
    pub fn dir(&self) -> &Path {
        &self.covers
    }

    /// The file of `cover`.
    pub fn path(&self, cover: &str) -> PathBuf {
        self.covers.join(format!("{cover}.jpg"))
    }

    /// The URL a cover is downloaded from.
    pub fn url(&self, cover: &str) -> String {
        cover_url(cover, SIZE).replacen(IMAGES_BASE, &self.base, 1)
    }

    /// At start: deletes what [`start_cleanup`] names and applies the
    /// limit. A missing directory is an empty cache.
    pub fn start(&self) -> std::io::Result<()> {
        let names: Vec<String> = match std::fs::read_dir(&self.covers) {
            Ok(entries) => entries
                .filter_map(Result::ok)
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e),
        };
        for name in start_cleanup(&names) {
            remove(&self.covers.join(name));
        }
        self.apply_limit(&[]);
        Ok(())
    }

    /// Deletes the least recently used covers beyond the limit.
    fn apply_limit(&self, keep: &[&str]) {
        let Ok(entries) = std::fs::read_dir(&self.covers) else {
            return;
        };
        let files: Vec<(String, SystemTime)> = entries
            .filter_map(Result::ok)
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                let id = name
                    .strip_suffix(".jpg")
                    .filter(|id| is_cover_id(id))?
                    .to_owned();
                let used = e.metadata().and_then(|m| m.modified()).ok()?;
                Some((id, used))
            })
            .collect();
        for id in evict(&files, self.max, keep) {
            remove(&self.path(&id));
        }
    }

    /// The cached file of `cover`, made the most recently used; `None`
    /// when it is not cached.
    pub fn lookup(&self, cover: &str) -> Option<PathBuf> {
        if self.max == 0 || !is_cover_id(cover) {
            return None;
        }
        let path = self.path(cover);
        let file = std::fs::File::options().write(true).open(&path).ok()?;
        let _ = file.set_modified(SystemTime::now());
        Some(path)
    }

    /// Downloads `cover` into the cache, then applies the limit, keeping
    /// it and `current`: its file, or why not.
    pub async fn download(&self, cover: &str, current: Option<&str>) -> Result<PathBuf, String> {
        if !is_cover_id(cover) {
            return Err(format!("not a cover ID: {cover}"));
        }
        let bytes = tokio::time::timeout(self.timeout, self.get(cover))
            .await
            .map_err(|_| format!("no answer within {} s", self.timeout.as_secs()))??;
        let path = self.path(cover);
        self.write(&path, &bytes)
            .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
        let mut keep = vec![cover];
        keep.extend(current);
        self.apply_limit(&keep);
        Ok(path)
    }

    /// The image: a `200` with an `image/*` body of at most [`LIMIT`].
    async fn get(&self, cover: &str) -> Result<Vec<u8>, String> {
        let mut response = self
            .client
            .get(self.url(cover))
            .send()
            .await
            .map_err(|e| e.without_url().to_string())?;
        if response.status() != reqwest::StatusCode::OK {
            return Err(format!("HTTP {}", response.status().as_u16()));
        }
        let kind = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_owned();
        if !kind.starts_with("image/") {
            return Err(format!("not an image: \"{kind}\""));
        }
        let too_big = || format!("larger than {} bytes", LIMIT);
        if response.content_length().is_some_and(|n| n > LIMIT as u64) {
            return Err(too_big());
        }
        let mut body = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|e| e.without_url().to_string())?
        {
            if body.len() + chunk.len() > LIMIT {
                return Err(too_big());
            }
            body.extend_from_slice(&chunk);
        }
        Ok(body)
    }

    /// Writes `<path>.tmp` (`0600`, in a `0700` `covers/`), then renames it,
    /// so the cache never holds half a file.
    fn write(&self, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        use std::io::Write;
        use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&self.covers)?;
        let tmp = path.with_extension("jpg.tmp");
        let written = std::fs::File::options()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)
            .and_then(|mut file| file.write_all(bytes))
            .and_then(|()| std::fs::rename(&tmp, path));
        if written.is_err() {
            remove(&tmp);
        }
        written
    }

    /// [`Self::lookup`], else [`Self::download`].
    pub async fn fetch(&self, cover: &str) -> Result<PathBuf, String> {
        match self.lookup(cover) {
            Some(path) => Ok(path),
            None => self.download(cover, None).await,
        }
    }
}

fn remove(path: &Path) {
    if let Err(e) = std::fs::remove_file(path)
        && e.kind() != std::io::ErrorKind::NotFound
    {
        tracing::warn!("cannot delete {}: {e}", path.display());
    }
}

/// What a finished fetch reports: the cover and its file, or why not.
pub type Done = Arc<dyn Fn(String, Result<PathBuf, String>) + Send + Sync>;

/// Fetches covers one at a time; a cover asked for while another
/// downloads replaces any waiting one (spec 0010 "When").
#[derive(Debug)]
pub struct CoverWorker {
    wanted: watch::Sender<Option<String>>,
}

impl CoverWorker {
    /// Starts the worker on `runtime`; `done` hears every fetch. It ends
    /// when the worker is dropped.
    pub fn spawn(cache: CoverCache, runtime: &tokio::runtime::Handle, done: Done) -> Self {
        let (wanted, mut rx) = watch::channel::<Option<String>>(None);
        runtime.spawn(async move {
            while rx.changed().await.is_ok() {
                // The newest wish only: the ones it replaced are skipped.
                let cover = rx.borrow_and_update().clone();
                if let Some(cover) = cover {
                    let result = cache.fetch(&cover).await;
                    done(cover, result);
                }
            }
        });
        Self { wanted }
    }

    /// Fetch `cover` next (replacing any cover still waiting).
    pub fn want(&self, cover: &str) {
        self.wanted.send_replace(Some(cover.to_owned()));
    }
}

#[cfg(test)]
mod tests;
