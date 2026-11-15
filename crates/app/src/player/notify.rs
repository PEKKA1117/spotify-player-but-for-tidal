//! Desktop notifications on track change (spec 0029), their process side:
//! one thread sends the latest started track's notification, waiting for
//! its cover, so playback never waits for the bus. What is sent is
//! `tidal_player_core::notify`.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tidal_player_core::Track;
use tidal_player_core::notify::{NotifySettings, Request};

pub use crate::player::mpris::{Level, Log};

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

/// The worker.
pub struct Notifier {
    settings: Settings,
    server: Option<Arc<Mutex<Box<dyn Server>>>>,
}

impl Notifier {
    /// Starts the thread, which opens its connection with `connect`.
    /// `cover_wait` is how long a notification waits for its cover.
    pub fn new(settings: Settings, connect: Connect, log: Log, cover_wait: Duration) -> Self {
        let _ = (log, cover_wait);
        Self {
            settings,
            server: connect().ok().map(|s| Arc::new(Mutex::new(s))),
        }
    }
}

impl NotifySink for Notifier {
    fn send(&mut self, track: Track) {
        if let Some(server) = &self.server {
            let request = Request::new(&self.settings.notify, &track, None, 0, false);
            let _ = server.lock().unwrap().notify(&request);
        }
    }
}

/// The cover's file in the cache under `dir`, as an absolute path; `None`
/// for what is not a cover ID.
pub fn cover_file(dir: &std::path::Path, cover: &str) -> Option<PathBuf> {
    crate::player::cover_cache::cached_file(dir, cover)
}

/// As [`start`], connecting with `connect`.
pub fn start_with(settings: &Settings, log: Log, connect: Connect) -> Option<Box<dyn NotifySink>> {
    Some(Box::new(Notifier::new(
        settings.clone(),
        connect,
        log,
        COVER_WAIT,
    )))
}

/// The notifier of a player, when `enable_notify` is on.
pub fn start(settings: &Settings, log: Log) -> Option<Box<dyn NotifySink>> {
    start_with(
        settings,
        log,
        Box::new(|| Err("no notifications yet".to_owned())),
    )
}

#[cfg(test)]
mod tests;
