//! The cover cache (spec 0010 "The cover cache"): the current entry's
//! album cover downloaded into `<cache dir>/covers/<cover>.jpg`, so every
//! MPRIS consumer gets a `file://` URL. The decisions (what to delete, which
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
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::Mutex;

    const A: &str = "2e4a5d2d-9a0d-4c3a-a0ba-42b0bd16a6ec";
    const B: &str = "8b0c1a3e-1111-4c3a-a0ba-000000000002";
    const C: &str = "8b0c1a3e-2222-4c3a-a0ba-000000000003";

    fn at(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000 + secs)
    }

    /// AC20: least recently used first, never the kept ones, down to `max`.
    #[test]
    fn ac20_eviction() {
        let files = |n: u64| -> Vec<(String, SystemTime)> {
            // Named by age: `c0` is the oldest.
            (0..n).map(|i| (format!("c{i}"), at(i))).collect()
        };
        let shuffled = {
            let mut f = files(21);
            f.reverse();
            f.swap(3, 17);
            f
        };
        let rows: [(&str, Vec<(String, SystemTime)>, usize, &[&str], &[&str]); 7] = [
            ("21 at 20: the oldest", files(21), 20, &[], &["c0"]),
            ("in any order", shuffled, 20, &[], &["c0"]),
            (
                "the current one oldest: the next oldest",
                files(21),
                20,
                &["c0"],
                &["c1"],
            ),
            ("20 at 20: none", files(20), 20, &[], &[]),
            (
                "3 at 1, the newest current",
                files(3),
                1,
                &["c2"],
                &["c0", "c1"],
            ),
            ("two kept at 1", files(3), 1, &["c2", "c0"], &["c1"]),
            ("0: all but the kept", files(3), 0, &["c1"], &["c0", "c2"]),
        ];
        for (name, files, max, keep, want) in rows {
            let mut got = evict(&files, max, keep);
            got.sort();
            assert_eq!(got, want, "{name}");
        }
    }

    /// AC20: the `mpris:artUrl` per state.
    #[test]
    fn ac20_art_url() {
        let https = format!(
            "https://resources.tidal.com/images/{}/640x640.jpg",
            A.replace('-', "/")
        );
        let cached = Art::Cached(PathBuf::from(format!(
            "/home/u/.cache/tidal-player/covers/{A}.jpg"
        )));
        let spaced = Art::Cached(PathBuf::from(format!("/home/a b/c%d/{A}.jpg")));
        let rows: Vec<(&str, Option<&str>, Art, Option<String>)> = vec![
            (
                "cached",
                Some(A),
                cached.clone(),
                Some(format!("file:///home/u/.cache/tidal-player/covers/{A}.jpg")),
            ),
            (
                "cached, a path to escape",
                Some(A),
                spaced,
                Some(format!("file:///home/a%20b/c%25d/{A}.jpg")),
            ),
            ("downloading", Some(A), Art::Downloading, None),
            ("failed", Some(A), Art::Failed, Some(https.clone())),
            ("off", Some(A), Art::Off, Some(https)),
            ("no cover", None, cached, None),
            ("no cover, off", None, Art::Off, None),
        ];
        for (name, cover, art, want) in rows {
            assert_eq!(art_url(cover, &art), want, "{name}");
        }
    }

    /// AC20: at start, anything but `<uuid>.jpg` goes.
    #[test]
    fn ac20_start_cleanup() {
        let names: Vec<String> = [
            format!("{A}.jpg"),
            format!("{A}.jpg.tmp"),
            "notes.txt".into(),
            "x.jpg".into(),
            format!("{}.jpg", A.to_uppercase()),
            format!("{B}.png"),
            format!("{C}.jpg"),
            format!("{}.jpg", &A[..35]),
            format!("{}.jpg", A.replace('-', "")),
        ]
        .into();
        let mut got = start_cleanup(&names);
        got.sort();
        let mut want = vec![
            format!("{A}.jpg.tmp"),
            // Tidal's IDs are lower case: this name is never looked up.
            format!("{}.jpg", A.to_uppercase()),
            "notes.txt".to_owned(),
            "x.jpg".into(),
            format!("{B}.png"),
            format!("{}.jpg", &A[..35]),
            format!("{}.jpg", A.replace('-', "")),
        ];
        want.sort();
        assert_eq!(got, want);
    }

    /// AC20: the cache directory.
    #[test]
    fn ac20_cache_dir() {
        let home = Path::new("/home/u");
        let rows: [(&str, Option<&str>, Option<&str>, Option<&Path>, &str); 6] = [
            ("variable", Some("/v"), Some("/x"), Some(home), "/v"),
            (
                "empty variable",
                Some(""),
                Some("/x"),
                Some(home),
                "/x/tidal-player",
            ),
            (
                "XDG_CACHE_HOME",
                None,
                Some("/x"),
                Some(home),
                "/x/tidal-player",
            ),
            (
                "empty XDG_CACHE_HOME",
                None,
                Some(""),
                Some(home),
                "/home/u/.cache/tidal-player",
            ),
            (
                "HOME",
                None,
                None,
                Some(home),
                "/home/u/.cache/tidal-player",
            ),
            ("nothing", None, None, None, "./.cache/tidal-player"),
        ];
        for (name, var, xdg, home, want) in rows {
            assert_eq!(cache_dir(var, xdg, home), PathBuf::from(want), "{name}");
        }
    }

    async fn image_server() -> wiremock::MockServer {
        wiremock::MockServer::start().await
    }

    fn image_path(cover: &str) -> String {
        format!("/images/{}/640x640.jpg", cover.replace('-', "/"))
    }

    async fn mount(
        server: &wiremock::MockServer,
        cover: &str,
        response: wiremock::ResponseTemplate,
    ) {
        use wiremock::matchers::{method, path};
        wiremock::Mock::given(method("GET"))
            .and(path(image_path(cover)))
            .respond_with(response)
            .mount(server)
            .await;
    }

    fn jpeg(body: Vec<u8>) -> wiremock::ResponseTemplate {
        wiremock::ResponseTemplate::new(200).set_body_raw(body, "image/jpeg")
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .map(|d| {
                d.map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }

    async fn requests(server: &wiremock::MockServer) -> Vec<String> {
        server
            .received_requests()
            .await
            .unwrap_or_default()
            .iter()
            .map(|r| r.url.path().to_owned())
            .collect()
    }

    /// AC21: a `200 image/jpeg` lands in `covers/<cover>.jpg` (`0600` in a
    /// `0700` directory, no `.tmp` left); a cached cover is not downloaded
    /// again, and using it touches it.
    #[tokio::test]
    async fn ac21_download() {
        let server = image_server().await;
        mount(&server, A, jpeg(b"JPEG A".to_vec())).await;
        let dir = tempfile::tempdir().unwrap();
        let cache = CoverCache::new(dir.path(), 20).with_base(&server.uri());

        let path = cache.fetch(A).await.unwrap();
        assert_eq!(path, dir.path().join("covers").join(format!("{A}.jpg")));
        assert_eq!(std::fs::read(&path).unwrap(), b"JPEG A");
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&path), 0o600, "file mode");
        assert_eq!(mode(&dir.path().join("covers")), 0o700, "directory mode");
        assert_eq!(names(cache.dir()), vec![format!("{A}.jpg")]);

        // Used long ago; using it again neither downloads nor keeps the time.
        let old = at(0);
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(old)
            .unwrap();
        assert_eq!(cache.fetch(A).await.unwrap(), path);
        assert_eq!(requests(&server).await.len(), 1, "downloaded again");
        let touched = std::fs::metadata(&path).unwrap().modified().unwrap();
        assert!(touched > old, "not touched: {touched:?}");
    }

    /// AC21: anything but a `200` image under the limit, in time, fails
    /// and leaves no file.
    #[tokio::test]
    async fn ac21_download_failures() {
        let server = image_server().await;
        let over = vec![0u8; LIMIT + 1];
        let rows: Vec<(&str, wiremock::ResponseTemplate)> = vec![
            (
                "404",
                wiremock::ResponseTemplate::new(404).set_body_raw(b"no".to_vec(), "image/jpeg"),
            ),
            (
                "text/html",
                wiremock::ResponseTemplate::new(200).set_body_raw(b"<html>".to_vec(), "text/html"),
            ),
            ("over 5 MB", jpeg(over)),
            (
                "no content type",
                wiremock::ResponseTemplate::new(200).set_body_bytes(b"x".to_vec()),
            ),
        ];
        let covers = [A, B, C, "8b0c1a3e-3333-4c3a-a0ba-000000000004"];
        for ((name, response), cover) in rows.into_iter().zip(covers) {
            mount(&server, cover, response).await;
            let dir = tempfile::tempdir().unwrap();
            let cache = CoverCache::new(dir.path(), 20).with_base(&server.uri());
            let got = cache.fetch(cover).await;
            assert!(got.is_err(), "{name}: {got:?}");
            assert_eq!(names(cache.dir()), Vec::<String>::new(), "{name}");
        }
        // The exact limit is accepted.
        let cover = "8b0c1a3e-5555-4c3a-a0ba-000000000005";
        mount(&server, cover, jpeg(vec![1u8; LIMIT])).await;
        let dir = tempfile::tempdir().unwrap();
        let cache = CoverCache::new(dir.path(), 20).with_base(&server.uri());
        assert!(cache.fetch(cover).await.is_ok(), "exactly 5 MB");
    }

    /// AC21: a response slower than the timeout fails (paused clock), with
    /// no file left.
    #[tokio::test(start_paused = true)]
    async fn ac21_download_timeout() {
        let server = image_server().await;
        mount(
            &server,
            A,
            jpeg(b"late".to_vec()).set_delay(Duration::from_secs(3600)),
        )
        .await;
        let dir = tempfile::tempdir().unwrap();
        let cache = CoverCache::new(dir.path(), 20).with_base(&server.uri());
        let started = tokio::time::Instant::now();
        // A guard on the same (virtual) clock: a download without a deadline
        // fails the test instead of hanging it.
        let got = tokio::time::timeout(TIMEOUT * 3, cache.fetch(A))
            .await
            .expect("still waiting after three times the timeout");
        assert!(got.is_err(), "{got:?}");
        assert!(
            started.elapsed() >= TIMEOUT,
            "gave up after {:?}",
            started.elapsed()
        );
        assert_eq!(names(cache.dir()), Vec::<String>::new());
    }

    /// AC21: covers asked for while one downloads: only the newest is
    /// fetched next.
    #[tokio::test(flavor = "multi_thread")]
    async fn ac21_latest_wins() {
        let server = image_server().await;
        mount(
            &server,
            A,
            jpeg(b"A".to_vec()).set_delay(Duration::from_millis(400)),
        )
        .await;
        mount(&server, B, jpeg(b"B".to_vec())).await;
        mount(&server, C, jpeg(b"C".to_vec())).await;
        let dir = tempfile::tempdir().unwrap();
        let cache = CoverCache::new(dir.path(), 20).with_base(&server.uri());
        let (tx, rx) = std::sync::mpsc::channel();
        let tx = Mutex::new(tx);
        let worker = CoverWorker::spawn(
            cache,
            &tokio::runtime::Handle::current(),
            Arc::new(move |cover, result: Result<PathBuf, String>| {
                let _ = tx.lock().unwrap().send((cover, result.is_ok()));
            }),
        );
        worker.want(A);
        // A is on its way.
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while requests(&server).await.is_empty() {
            assert!(std::time::Instant::now() < deadline, "A never asked for");
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        worker.want(B);
        worker.want(C);
        let done: Vec<(String, bool)> = tokio::task::spawn_blocking(move || {
            let mut done = Vec::new();
            while let Ok(d) = rx.recv_timeout(Duration::from_secs(2)) {
                done.push(d);
            }
            done
        })
        .await
        .unwrap();
        assert_eq!(done, vec![(A.to_owned(), true), (C.to_owned(), true)]);
        assert_eq!(requests(&server).await, vec![image_path(A), image_path(C)]);
    }

    /// AC20 (I/O): at start, strays go and the limit applies; after a
    /// download, the least recently used go, never the current cover.
    #[tokio::test]
    async fn ac20_cache_on_disk() {
        let server = image_server().await;
        mount(&server, C, jpeg(b"C".to_vec())).await;
        let dir = tempfile::tempdir().unwrap();
        let covers = dir.path().join("covers");
        std::fs::create_dir_all(&covers).unwrap();
        for (i, name) in [format!("{A}.jpg"), format!("{B}.jpg")].iter().enumerate() {
            let path = covers.join(name);
            std::fs::write(&path, b"x").unwrap();
            std::fs::File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_modified(at(i as u64))
                .unwrap();
        }
        std::fs::write(covers.join(format!("{C}.jpg.tmp")), b"half").unwrap();
        std::fs::write(covers.join("notes.txt"), b"?").unwrap();

        let cache = CoverCache::new(dir.path(), 2).with_base(&server.uri());
        cache.start().unwrap();
        assert_eq!(names(&covers), vec![format!("{A}.jpg"), format!("{B}.jpg")]);

        // A (the oldest) is current: B goes when C arrives.
        cache.download(C, Some(A)).await.unwrap();
        assert_eq!(names(&covers), vec![format!("{A}.jpg"), format!("{C}.jpg")]);

        // A lowered limit applies at the next start.
        CoverCache::new(dir.path(), 1).start().unwrap();
        assert_eq!(names(&covers), vec![format!("{C}.jpg")]);
    }
}
