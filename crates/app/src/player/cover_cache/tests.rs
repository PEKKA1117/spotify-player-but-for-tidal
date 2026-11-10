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
    // name, files, max, kept, deleted
    type Row<'a> = (
        &'a str,
        Vec<(String, SystemTime)>,
        usize,
        &'a [&'a str],
        &'a [&'a str],
    );
    let rows: [Row; 7] = [
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
    // name, variable, XDG_CACHE_HOME, home, want
    type Row<'a> = (
        &'a str,
        Option<&'a str>,
        Option<&'a str>,
        Option<&'a Path>,
        &'a str,
    );
    let rows: [Row; 6] = [
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

async fn mount(server: &wiremock::MockServer, cover: &str, response: wiremock::ResponseTemplate) {
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

fn with_cover(id: u64, cover: Option<&str>) -> tidal_player_core::Track {
    let mut t = crate::player::runtime::fakes::track(id, Some(200));
    t.album.as_mut().unwrap().cover = cover.map(str::to_owned);
    t
}

/// The `Metadata` signals recorded, in order.
fn metadata_signals(
    signals: &[crate::player::mpris::testing::Signal],
) -> Vec<(Vec<&'static str>, crate::player::mpris::model::Metadata)> {
    use crate::player::mpris::model::Value;
    use crate::player::mpris::testing::Signal;
    signals
        .iter()
        .filter_map(|s| match s {
            Signal::Changed(p) => match p.get("Metadata") {
                Some(Value::Metadata(m)) => Some((p.keys().copied().collect(), m.clone())),
                _ => None,
            },
            _ => None,
        })
        .collect()
}

/// AC21: on the adapter, an entry's `Metadata` comes at once without
/// `mpris:artUrl`, then once more (one `PropertiesChanged`, `Metadata`
/// alone) with the `file://` URL; nothing when the entry changed
/// while its cover downloaded.
#[tokio::test(flavor = "multi_thread")]
async fn ac21_metadata_sequence() {
    use crate::player::mpris::hub::{DEADLINE, Options};
    use crate::player::mpris::model::Value;
    use crate::player::mpris::testing::Rig;
    use tidal_player_core::protocol::Command;

    let server = image_server().await;
    let slow = |body: &[u8]| jpeg(body.to_vec()).set_delay(Duration::from_millis(300));
    mount(&server, A, slow(b"A")).await;
    mount(&server, B, slow(b"B")).await;
    let dir = tempfile::tempdir().unwrap();
    let cache = CoverCache::new(dir.path(), 20).with_base(&server.uri());
    let rig = Rig::new(Options {
        deadline: DEADLINE,
        covers: Some((cache, tokio::runtime::Handle::current())),
        log: crate::player::mpris::tracing_log(),
    });
    rig.send(Command::LoadQueue {
        tracks: vec![
            with_cover(1, Some(A)),
            with_cover(2, Some(B)),
            with_cover(3, None),
        ],
        start: 0,
    });
    let file = format!(
        "file://{}",
        dir.path().join("covers").join(format!("{A}.jpg")).display()
    );
    let art = |m: &crate::player::mpris::model::Metadata| m.get("mpris:artUrl").cloned();
    let signals = rig.bus.wait("A's file", |signals| {
        metadata_signals(signals)
            .iter()
            .any(|(_, m)| art(m) == Some(Value::Str(file.clone())))
    });
    let metadata = metadata_signals(&signals);
    assert_eq!(metadata.len(), 2, "{metadata:?}");
    assert_eq!(art(&metadata[0].1), None, "art before the download");
    assert_eq!(
        metadata[1].0,
        vec!["Metadata"],
        "one signal, Metadata alone"
    );
    assert_eq!(
        metadata[1].1,
        {
            let mut m = metadata[0].1.clone();
            m.insert("mpris:artUrl".into(), Value::Str(file.clone()));
            m
        },
        "the same metadata with the file"
    );

    // Entry 2 (B, downloading), then at once entry 3 (no cover): B's
    // file changes nothing.
    rig.send(Command::Next);
    rig.send(Command::Next);
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !names(&dir.path().join("covers")).contains(&format!("{B}.jpg")) {
        assert!(std::time::Instant::now() < deadline, "B never cached");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    tokio::time::sleep(Duration::from_millis(100)).await;
    let metadata = metadata_signals(&rig.bus.take());
    let last = metadata.last().expect("metadata for the next entries");
    assert_eq!(
        last.1.get("mpris:trackid"),
        Some(&Value::ObjectPath("/tidal_player/entry/3".into())),
        "{metadata:?}"
    );
    assert!(
        metadata.iter().all(|(_, m)| art(m).is_none()),
        "{metadata:?}"
    );
}
