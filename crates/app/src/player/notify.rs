//! Desktop notifications on track change (spec 0029), their process side:
//! one thread sends the latest started track's notification, waiting for
//! its cover, so playback never waits for the bus. What is sent is
//! `tidal_player_core::notify`.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use tidal_player_core::Track;
use tidal_player_core::notify::{NotifySettings, Request, supports_markup};

pub use crate::player::mpris::{Level, Log};

/// Log lines through `tracing`, under this module's name: the player's log
/// (spec 0027), a daemon's stderr included.
pub fn tracing_log() -> Log {
    Arc::new(|level, line| match level {
        Level::Info => tracing::info!("{line}"),
        Level::Warn => tracing::warn!("{line}"),
    })
}

/// How long a notification waits for its cover.
pub const COVER_WAIT: Duration = Duration::from_secs(2);
/// How long one call to the server may take.
pub const CALL_LIMIT: Duration = Duration::from_secs(5);

/// A call that did not succeed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallError {
    /// No answer within [`CALL_LIMIT`].
    TimedOut,
    /// The bus's or the server's words.
    Failed(String),
}

impl std::fmt::Display for CallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TimedOut => write!(f, "no answer within {} s", CALL_LIMIT.as_secs()),
            Self::Failed(why) => f.write_str(why),
        }
    }
}

/// The notification server: zbus on the session bus, or a fake in tests.
pub trait Server: Send + 'static {
    /// `GetCapabilities`.
    fn capabilities(&mut self) -> Result<Vec<String>, CallError>;
    /// `Notify`: the ID the server gave the notification.
    fn notify(&mut self, request: &Request) -> Result<u32, CallError>;
}

/// Opens the connection to the server, on the worker's thread.
pub type Connect = Box<dyn FnOnce() -> Result<Box<dyn Server>, String> + Send>;

/// What the player's runtime hands started tracks to.
pub trait NotifySink: Send {
    /// `track` started and notifies: takes it without waiting.
    fn send(&mut self, track: Track);
}

/// What the notifier needs from the player's settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub notify: NotifySettings,
    /// The cover cache's directory (`covers/` is inside); `None` when
    /// there is no cache (`max_cover_arts = 0`).
    pub cover_dir: Option<PathBuf>,
}

impl Settings {
    /// From the player's resolved settings and this process's environment.
    pub fn from_env(player: &crate::settings::PlayerSettings) -> Self {
        Self {
            notify: player.notify.clone(),
            cover_dir: (player.max_cover_arts > 0)
                .then(crate::player::cover_cache::process_cache_dir),
        }
    }
}

/// How often a waiting notification looks for its cover.
const COVER_POLL: Duration = Duration::from_millis(25);

/// The worker: a slot holding the latest started track, and the thread
/// that sends it.
pub struct Notifier {
    shared: Arc<Shared>,
}

struct Shared {
    slot: Mutex<Slot>,
    changed: Condvar,
}

#[derive(Default)]
struct Slot {
    /// The latest track not taken by the thread yet.
    latest: Option<Track>,
    /// No more tracks come: the player is going away, or the thread has
    /// no bus.
    closed: bool,
}

/// What the thread remembers between notifications.
struct Worker {
    settings: Settings,
    server: Box<dyn Server>,
    log: Log,
    cover_wait: Duration,
    /// The ID the server gave the last notification; `0`: none yet.
    previous: u32,
    /// Whether the server reads the body as markup; `None`: to ask.
    markup: Option<bool>,
    /// A failure was logged, and nothing has succeeded since.
    quiet: bool,
}

impl Shared {
    fn slot(&self) -> MutexGuard<'_, Slot> {
        self.slot.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The next track to send; `None` once closed.
    fn next(&self) -> Option<Track> {
        let mut slot = self.slot();
        loop {
            if slot.closed {
                return None;
            }
            if let Some(track) = slot.latest.take() {
                return Some(track);
            }
            slot = self
                .changed
                .wait(slot)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }

    /// Waits until `file` exists, at most `wait`: `Some(true)` when it
    /// does, `Some(false)` when it did not come, `None` when a newer track
    /// (or the end) came first.
    fn wait_for(&self, file: &Path, wait: Duration) -> Option<bool> {
        let deadline = Instant::now() + wait;
        let mut slot = self.slot();
        loop {
            if slot.closed || slot.latest.is_some() {
                return None;
            }
            if file.exists() {
                return Some(true);
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Some(false);
            }
            slot = self
                .changed
                .wait_timeout(slot, left.min(COVER_POLL))
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    fn close(&self) {
        self.slot().closed = true;
        self.changed.notify_all();
    }
}

impl Worker {
    /// The thread's loop: the latest track at a time until closed.
    fn work(mut self, shared: &Shared) {
        while let Some(track) = shared.next() {
            let Some(cover) = self.cover(shared, &track) else {
                // A newer track took this one's place.
                continue;
            };
            self.notify(&track, cover.as_deref());
        }
    }

    /// The cover to send with `track`, waiting for it a bounded time;
    /// `None` when a newer track came meanwhile.
    fn cover(&self, shared: &Shared, track: &Track) -> Option<Option<PathBuf>> {
        let file = self.settings.cover_dir.as_deref().and_then(|dir| {
            let cover = track.album.as_ref()?.cover.as_deref()?;
            cover_file(dir, cover)
        });
        let Some(file) = file else {
            return Some(None);
        };
        let came = shared.wait_for(&file, self.cover_wait)?;
        Some(came.then_some(file))
    }

    fn notify(&mut self, track: &Track, cover: Option<&Path>) {
        let markup = match self.markup {
            Some(markup) => markup,
            None => match self.server.capabilities() {
                Ok(capabilities) => *self.markup.insert(supports_markup(&capabilities)),
                Err(e) => return self.failed(&e),
            },
        };
        let request = Request::new(&self.settings.notify, track, cover, self.previous, markup);
        match self.server.notify(&request) {
            Ok(id) => {
                self.previous = id;
                self.quiet = false;
            }
            Err(e) => self.failed(&e),
        }
    }

    /// Says a failure if it is the first since a success; the server may
    /// have been replaced, so its capabilities are asked again.
    fn failed(&mut self, error: &CallError) {
        self.markup = None;
        if !self.quiet {
            self.quiet = true;
            (self.log)(
                Level::Warn,
                &format!("Desktop notification failed: {error}"),
            );
        }
    }
}

impl Notifier {
    /// Starts the thread, which opens its connection with `connect`.
    /// `cover_wait` is how long a notification waits for its cover.
    pub fn new(settings: Settings, connect: Connect, log: Log, cover_wait: Duration) -> Self {
        let shared = Arc::new(Shared {
            slot: Mutex::default(),
            changed: Condvar::new(),
        });
        let worker = Arc::clone(&shared);
        std::thread::spawn(move || match connect() {
            Ok(server) => Worker {
                settings,
                server,
                log,
                cover_wait,
                previous: 0,
                markup: None,
                quiet: false,
            }
            .work(&worker),
            Err(e) => {
                log(
                    Level::Info,
                    &format!("No session bus, desktop notifications are off: {e}"),
                );
                worker.close();
            }
        });
        Self { shared }
    }
}

impl NotifySink for Notifier {
    fn send(&mut self, track: Track) {
        let mut slot = self.shared.slot();
        if !slot.closed {
            slot.latest = Some(track);
        }
        drop(slot);
        self.shared.changed.notify_all();
    }
}

impl Drop for Notifier {
    /// A waiting notification is dropped; a call in flight ends on its own.
    fn drop(&mut self) {
        self.shared.close();
    }
}

/// The cover's file in the cache under `dir`, as an absolute path; `None`
/// for what is not a cover ID.
pub fn cover_file(dir: &Path, cover: &str) -> Option<PathBuf> {
    let file = crate::player::cover_cache::cached_file(dir, cover)?;
    Some(std::path::absolute(&file).unwrap_or(file))
}

/// As [`start`], connecting with `connect`: `None` when `enable_notify` is
/// off, and `connect` is not called then.
pub fn start_with(settings: &Settings, log: Log, connect: Connect) -> Option<Box<dyn NotifySink>> {
    if !settings.notify.enabled {
        return None;
    }
    Some(Box::new(Notifier::new(
        settings.clone(),
        connect,
        log,
        COVER_WAIT,
    )))
}

/// The notifier of a player (standalone, daemon, `play`), on its own
/// connection to the session bus; `None` when `enable_notify` is off.
pub fn start(settings: &Settings, log: Log) -> Option<Box<dyn NotifySink>> {
    start_with(settings, log, Box::new(bus::connect))
}

/// The zbus side: `org.freedesktop.Notifications` on the session bus.
mod bus {
    use std::collections::HashMap;

    use tidal_player_core::notify::{Hint, Request};
    use zbus::zvariant::Value;

    use super::{CALL_LIMIT, CallError, Server};

    const DESTINATION: &str = "org.freedesktop.Notifications";
    const PATH: &str = "/org/freedesktop/Notifications";
    const INTERFACE: &str = "org.freedesktop.Notifications";

    struct Bus {
        connection: zbus::blocking::Connection,
    }

    /// A connection of the notifier's own (spec 0029 decision 7), whose
    /// calls give up after [`CALL_LIMIT`].
    pub(super) fn connect() -> Result<Box<dyn Server>, String> {
        let connection = zbus::blocking::connection::Builder::session()
            .and_then(|builder| builder.method_timeout(CALL_LIMIT).build())
            .map_err(|e| e.to_string())?;
        Ok(Box::new(Bus { connection }))
    }

    fn call_error(e: &zbus::Error) -> CallError {
        match e {
            zbus::Error::InputOutput(io) if io.kind() == std::io::ErrorKind::TimedOut => {
                CallError::TimedOut
            }
            other => CallError::Failed(other.to_string()),
        }
    }

    impl Bus {
        fn call<B>(&self, method: &str, body: &B) -> Result<zbus::Message, CallError>
        where
            B: serde::Serialize + zbus::zvariant::DynamicType,
        {
            self.connection
                .call_method(Some(DESTINATION), PATH, Some(INTERFACE), method, body)
                .map_err(|e| call_error(&e))
        }
    }

    impl Server for Bus {
        fn capabilities(&mut self) -> Result<Vec<String>, CallError> {
            let reply = self.call("GetCapabilities", &())?;
            reply
                .body()
                .deserialize()
                .map_err(|e| CallError::Failed(e.to_string()))
        }

        fn notify(&mut self, request: &Request) -> Result<u32, CallError> {
            let hints: HashMap<&str, Value<'_>> = request
                .hints
                .iter()
                .map(|(key, hint)| {
                    let value = match hint {
                        Hint::Str(text) => Value::from(text.as_str()),
                        Hint::Bool(on) => Value::from(*on),
                    };
                    (key.as_str(), value)
                })
                .collect();
            let body = (
                request.app_name.as_str(),
                request.replaces_id,
                request.app_icon.as_str(),
                request.summary.as_str(),
                request.body.as_str(),
                &request.actions,
                hints,
                request.expire_timeout,
            );
            let reply = self.call("Notify", &body)?;
            reply
                .body()
                .deserialize::<u32>()
                .map_err(|e| CallError::Failed(e.to_string()))
        }
    }
}

#[cfg(test)]
mod tests;
