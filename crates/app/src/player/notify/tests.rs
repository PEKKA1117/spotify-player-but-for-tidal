//! The worker against a fake server: no test here talks to a bus.

use std::collections::VecDeque;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Instant;

use tidal_player_core::notify::Hint;
use tidal_player_core::{AlbumRef, ArtistRef, TrackId};

use super::*;

const COVER: &str = "2e4a5d2d-9a0d-4c3a-a0ba-42b0bd16a6ec";
/// Long enough to tell waiting from not waiting on a busy machine.
const WAIT: Duration = Duration::from_millis(1000);

fn track(id: u64, cover: Option<&str>) -> Track {
    Track {
        id: TrackId(id),
        title: format!("Title {id}"),
        version: None,
        artists: vec![ArtistRef {
            id: 1,
            name: "Artist".into(),
        }],
        album: Some(AlbumRef {
            id: 1,
            title: "Album".into(),
            cover: cover.map(str::to_owned),
        }),
        duration: Some(Duration::from_secs(200)),
        streamable: true,
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Call {
    Capabilities,
    Notify(Request),
}

#[derive(Default)]
struct FakeState {
    calls: Vec<Call>,
    /// What the next `Notify` calls answer; then IDs from 11 up.
    notify: VecDeque<Result<u32, CallError>>,
    /// What the next `GetCapabilities` calls answer; then `body`.
    capabilities: VecDeque<Result<Vec<String>, CallError>>,
    next_id: u32,
}

/// A recording server; with a gate, each `Notify` says it entered and
/// waits to be let go.
#[derive(Clone, Default)]
struct Fake {
    state: Arc<Mutex<FakeState>>,
    gate: Option<Arc<Mutex<(Sender<()>, Receiver<()>)>>>,
}

impl Fake {
    /// A fake whose `Notify` waits; the receiver hears each call enter,
    /// the sender lets one go.
    fn gated() -> (Self, Receiver<()>, Sender<()>) {
        let (entered_tx, entered) = mpsc::channel();
        let (release, release_rx) = mpsc::channel();
        let fake = Self {
            gate: Some(Arc::new(Mutex::new((entered_tx, release_rx)))),
            ..Self::default()
        };
        (fake, entered, release)
    }

    fn calls(&self) -> Vec<Call> {
        self.state.lock().unwrap().calls.clone()
    }

    fn requests(&self) -> Vec<Request> {
        self.calls()
            .into_iter()
            .filter_map(|call| match call {
                Call::Notify(request) => Some(request),
                Call::Capabilities => None,
            })
            .collect()
    }

    /// Waits until `n` `Notify` calls were made.
    fn wait_requests(&self, n: usize) -> Vec<Request> {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let requests = self.requests();
            if requests.len() >= n {
                return requests;
            }
            assert!(Instant::now() < deadline, "{n} calls: {requests:?}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn connect(&self) -> Connect {
        let fake = self.clone();
        Box::new(move || Ok(Box::new(fake) as Box<dyn Server>))
    }
}

impl Server for Fake {
    fn capabilities(&mut self) -> Result<Vec<String>, CallError> {
        let mut state = self.state.lock().unwrap();
        state.calls.push(Call::Capabilities);
        state
            .capabilities
            .pop_front()
            .unwrap_or_else(|| Ok(vec!["body".into()]))
    }

    fn notify(&mut self, request: &Request) -> Result<u32, CallError> {
        self.state
            .lock()
            .unwrap()
            .calls
            .push(Call::Notify(request.clone()));
        if let Some(gate) = &self.gate {
            let gate = gate.lock().unwrap();
            let _ = gate.0.send(());
            let _ = gate.1.recv();
        }
        let mut state = self.state.lock().unwrap();
        state.notify.pop_front().unwrap_or_else(|| {
            state.next_id += 1;
            Ok(10 + state.next_id)
        })
    }
}

type Lines = Arc<Mutex<Vec<(Level, String)>>>;

fn recording_log() -> (Log, Lines) {
    let lines: Lines = Arc::default();
    let into = Arc::clone(&lines);
    let log: Log = Arc::new(move |level, line: &str| {
        into.lock().unwrap().push((level, line.to_owned()));
    });
    (log, lines)
}

fn settings(cover_dir: Option<&Path>) -> Settings {
    Settings {
        notify: NotifySettings::default(),
        cover_dir: cover_dir.map(Path::to_owned),
    }
}

fn new_notifier(fake: &Fake, cover_dir: Option<&Path>) -> (Notifier, Lines) {
    let (log, lines) = recording_log();
    let notifier = Notifier::new(settings(cover_dir), fake.connect(), log, WAIT);
    (notifier, lines)
}

/// The notifier on a thread of its own, so a test never blocks on a
/// `send` that waits (as a wrong one would).
struct Driver {
    sends: Sender<Vec<Track>>,
    /// How long each batch of sends took.
    took: Receiver<Duration>,
}

impl Driver {
    fn new(mut notifier: Notifier) -> Self {
        let (sends, batches) = mpsc::channel::<Vec<Track>>();
        let (took_tx, took) = mpsc::channel();
        std::thread::spawn(move || {
            for batch in batches {
                let started = Instant::now();
                for track in batch {
                    notifier.send(track);
                }
                let _ = took_tx.send(started.elapsed());
            }
        });
        Self { sends, took }
    }

    /// Has the thread send `tracks`, without waiting for it.
    fn post(&self, tracks: Vec<Track>) {
        self.sends.send(tracks).unwrap();
    }

    /// Sends `tracks` and says how long that took; panics when it does not
    /// return within a second.
    fn send(&self, tracks: Vec<Track>) -> Duration {
        self.post(tracks);
        self.took
            .recv_timeout(Duration::from_secs(1))
            .expect("the send waited")
    }
}

fn image_path(request: &Request) -> Option<String> {
    request.hints.iter().find_map(|(key, value)| match value {
        Hint::Str(path) if key == "image-path" => Some(path.clone()),
        _ => None,
    })
}

fn summaries(requests: &[Request]) -> Vec<String> {
    requests.iter().map(|r| r.summary.clone()).collect()
}

/// AC7: each notification replaces the one before it.
#[test]
fn ac7_replaces_the_previous() {
    let fake = Fake::default();
    let (mut notifier, _) = new_notifier(&fake, None);
    for (n, id) in [1, 2, 3].into_iter().enumerate() {
        notifier.send(track(id, None));
        fake.wait_requests(n + 1);
    }
    let replaced: Vec<u32> = fake.requests().iter().map(|r| r.replaces_id).collect();
    assert_eq!(replaced, [0, 11, 12]);
}

/// AC8: the cover: cached, late within the wait, too late, none, no
/// cache.
#[test]
fn ac8_cover() {
    enum Arrives {
        Before,
        After(Duration),
        Never,
    }
    struct Row {
        name: &'static str,
        cover: Option<&'static str>,
        cache: bool,
        arrives: Arrives,
        image: bool,
        waits: (Duration, Duration),
    }
    let quick = (Duration::ZERO, WAIT / 2);
    let rows = [
        Row {
            name: "cached",
            cover: Some(COVER),
            cache: true,
            arrives: Arrives::Before,
            image: true,
            waits: quick,
        },
        Row {
            name: "late, within the wait",
            cover: Some(COVER),
            cache: true,
            arrives: Arrives::After(WAIT / 4),
            image: true,
            waits: (WAIT / 4, WAIT * 3 / 4),
        },
        Row {
            name: "too late",
            cover: Some(COVER),
            cache: true,
            arrives: Arrives::Never,
            image: false,
            waits: (WAIT, WAIT * 3),
        },
        Row {
            name: "no cover",
            cover: None,
            cache: true,
            arrives: Arrives::Never,
            image: false,
            waits: quick,
        },
        Row {
            name: "no cache",
            cover: Some(COVER),
            cache: false,
            arrives: Arrives::Never,
            image: false,
            waits: quick,
        },
    ];
    for row in rows {
        let dir = tempfile::tempdir().unwrap();
        let covers = dir.path().join("covers");
        std::fs::create_dir_all(&covers).unwrap();
        let file = covers.join(format!("{COVER}.jpg"));
        let fake = Fake::default();
        let (mut notifier, _) = new_notifier(&fake, row.cache.then_some(dir.path()));
        if let Arrives::Before = row.arrives {
            std::fs::write(&file, b"JPEG").unwrap();
        }
        let sent = Instant::now();
        notifier.send(track(1, row.cover));
        if let Arrives::After(delay) = row.arrives {
            std::thread::sleep(delay);
            std::fs::write(&file, b"JPEG").unwrap();
        }
        let requests = fake.wait_requests(1);
        let took = sent.elapsed();
        assert!(
            (row.waits.0..row.waits.1).contains(&took),
            "{}: took {took:?}",
            row.name
        );
        let want = row.image.then(|| file.display().to_string());
        assert_eq!(image_path(&requests[0]), want, "{}", row.name);
        if let Arrives::Never = row.arrives
            && row.cache
            && row.cover.is_some()
        {
            // The cover after the wait changes nothing.
            std::fs::write(&file, b"JPEG").unwrap();
        }
        std::thread::sleep(WAIT / 2);
        assert_eq!(fake.requests().len(), 1, "{}: sent once", row.name);
    }
    // The path given is absolute, whatever the cache's.
    let file = cover_file(Path::new("relative/cache"), COVER).expect("a cover ID");
    assert!(file.is_absolute(), "{}", file.display());
    assert!(file.ends_with(format!("relative/cache/covers/{COVER}.jpg")));
    assert_eq!(cover_file(Path::new("/c"), "../../etc/passwd"), None);
}

/// AC9: the latest track wins, while a call is in flight and while a
/// notification waits for its cover.
#[test]
fn ac9_latest_wins() {
    let (fake, entered, release) = Fake::gated();
    let (notifier, _) = new_notifier(&fake, None);
    let driver = Driver::new(notifier);
    driver.post(vec![track(1, None)]);
    entered.recv_timeout(Duration::from_secs(10)).unwrap();
    driver.post(vec![track(2, None), track(3, None), track(4, None)]);
    release.send(()).unwrap();
    entered.recv_timeout(Duration::from_secs(10)).unwrap();
    release.send(()).unwrap();
    std::thread::sleep(WAIT / 2);
    assert_eq!(
        summaries(&fake.requests()),
        ["Title 1 • Artist", "Title 4 • Artist"],
        "in flight"
    );
    drop(release);

    // Waiting for a cover that never comes: the tracks that start
    // meanwhile take its place, and only the last is sent.
    let dir = tempfile::tempdir().unwrap();
    let fake = Fake::default();
    let (mut notifier, _) = new_notifier(&fake, Some(dir.path()));
    notifier.send(track(1, Some(COVER)));
    std::thread::sleep(WAIT / 4);
    notifier.send(track(2, Some(COVER)));
    notifier.send(track(3, Some(COVER)));
    fake.wait_requests(1);
    std::thread::sleep(WAIT * 3 / 2);
    assert_eq!(
        summaries(&fake.requests()),
        ["Title 3 • Artist"],
        "waiting for the cover"
    );
}

/// AC11: failures are one `warn` line until a success; the next track is
/// tried; the capabilities are asked again after a failure; nothing
/// reaches the player (the notifier has no way to it: it holds no
/// `RuntimeInput` sender).
#[test]
fn ac11_failures() {
    fn failed<T>(why: &str) -> Result<T, CallError> {
        Err(CallError::Failed(why.into()))
    }
    let fake = Fake::default();
    {
        let mut state = fake.state.lock().unwrap();
        state.notify = VecDeque::from([
            failed("org.freedesktop.DBus.Error.ServiceUnknown: no server"),
            failed("a second failure"),
            Ok(5),
            Ok(6),
            Err(CallError::TimedOut),
        ]);
        // Asked before tracks 1, 2, 3 and 6; the one before 3 fails.
        state.capabilities = VecDeque::from([
            Ok(vec!["body".into()]),
            Ok(vec!["body".into()]),
            failed("capabilities failed"),
            Ok(vec!["body".into()]),
        ]);
    }
    let (mut notifier, lines) = new_notifier(&fake, None);
    let warn = |text: &str| (Level::Warn, format!("Desktop notification failed: {text}"));
    // Each track, the calls made so far, and the lines it adds.
    let steps: [(u64, usize, Vec<(Level, String)>); 6] = [
        (
            1,
            2,
            vec![warn("org.freedesktop.DBus.Error.ServiceUnknown: no server")],
        ),
        (2, 4, vec![]),
        // The capabilities fail: no `Notify`, no new line (still quiet).
        (3, 5, vec![]),
        (4, 7, vec![]),
        (5, 8, vec![]),
        (6, 9, vec![warn("no answer within 5 s")]),
    ];
    let mut want_lines = Vec::new();
    for (id, calls, new_lines) in steps {
        notifier.send(track(id, None));
        let deadline = Instant::now() + Duration::from_secs(2);
        while fake.calls().len() < calls && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        std::thread::sleep(Duration::from_millis(50));
        want_lines.extend(new_lines);
        assert_eq!(*lines.lock().unwrap(), want_lines, "track {id}: lines");
        assert_eq!(fake.calls().len(), calls, "track {id}: calls");
    }
    use Call::Capabilities as C;
    let kinds: Vec<&str> = fake
        .calls()
        .iter()
        .map(|call| match call {
            C => "caps",
            Call::Notify(_) => "notify",
        })
        .collect();
    assert_eq!(
        kinds,
        [
            "caps", "notify", // 1 fails
            "caps", "notify", // 2 fails, quiet
            "caps",   // 3: the capabilities fail
            "caps", "notify", // 4 succeeds: re-armed
            "notify", // 5: the capabilities are known
            "notify", // 6 times out: said again
        ]
    );
    // The replaced ID is the last one the server gave.
    assert_eq!(fake.requests()[4].replaces_id, 6);

    // No session bus: one `info` line, and nothing is tried again.
    let (log, lines) = recording_log();
    let connects = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&connects);
    let connect: Connect = Box::new(move || {
        counted.fetch_add(1, Ordering::SeqCst);
        Err("no bus here".to_owned())
    });
    let mut notifier = Notifier::new(settings(None), connect, log, WAIT);
    for id in [1, 2] {
        notifier.send(track(id, None));
    }
    std::thread::sleep(WAIT / 4);
    assert_eq!(connects.load(Ordering::SeqCst), 1);
    assert_eq!(
        *lines.lock().unwrap(),
        [(
            Level::Info,
            "No session bus, desktop notifications are off: no bus here".to_owned()
        )]
    );
}

/// AC12: `send` returns at once while the server holds a call, and while
/// a notification waits for its cover.
#[test]
fn ac12_never_blocks() {
    let (fake, entered, release) = Fake::gated();
    let (notifier, _) = new_notifier(&fake, None);
    let driver = Driver::new(notifier);
    let first = driver.send(vec![track(1, None)]);
    assert!(first < Duration::from_millis(100), "the first send waited");
    entered.recv_timeout(Duration::from_secs(10)).unwrap();
    let held = driver.send(vec![track(2, None), track(3, None)]);
    assert!(held < Duration::from_millis(100), "the send waited");
    drop(release);

    let dir = tempfile::tempdir().unwrap();
    let fake = Fake::default();
    let (notifier, _) = new_notifier(&fake, Some(dir.path()));
    let driver = Driver::new(notifier);
    let waiting = driver.send(vec![track(1, Some(COVER)), track(2, Some(COVER))]);
    assert!(waiting < Duration::from_millis(100), "the send waited");

    // Dropping it (the player shuts down) does not wait either.
    let (fake, entered, _release) = Fake::gated();
    let (mut notifier, _) = new_notifier(&fake, None);
    notifier.send(track(1, None));
    entered.recv_timeout(Duration::from_secs(10)).unwrap();
    let dropping = Instant::now();
    drop(notifier);
    assert!(dropping.elapsed() < Duration::from_millis(100));
}

/// AC13: with `enable_notify = false` nothing starts: no notifier, no
/// connection.
#[test]
fn ac13_off_starts_nothing() {
    for enabled in [false, true] {
        let connects = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&connects);
        let fake = Fake::default();
        let connect: Connect = Box::new(move || {
            counted.fetch_add(1, Ordering::SeqCst);
            Ok(Box::new(fake) as Box<dyn Server>)
        });
        let mut settings = settings(None);
        settings.notify.enabled = enabled;
        let (log, _) = recording_log();
        let notifier = start_with(&settings, log, connect);
        assert_eq!(notifier.is_some(), enabled, "enabled: {enabled}");
        let deadline = Instant::now() + Duration::from_secs(10);
        while enabled && connects.load(Ordering::SeqCst) == 0 {
            assert!(Instant::now() < deadline, "the connection was not opened");
            std::thread::sleep(Duration::from_millis(5));
        }
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(
            connects.load(Ordering::SeqCst),
            usize::from(enabled),
            "enabled: {enabled}"
        );
    }
}
