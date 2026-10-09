//! MPRIS2 (spec 0010): the player published on the session bus as
//! `org.mpris.MediaPlayer2.tidal_player`, for desktop widgets, media keys
//! and `playerctl`. [`model`] is the pure view, [`covers`] the cover cache,
//! [`hub`] the adapter as a client of the player, [`bus`] the zbus layer.
//!
//! [`start`] after the player is spawned and before its socket serves;
//! [`Mpris::close`] after the socket server is dropped.

pub mod bus;
pub mod covers;
pub mod hub;
pub mod model;
#[cfg(test)]
pub(crate) mod testing;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::Sender;

use crate::player_runtime::RuntimeInput;

/// Test-only: in debug builds, cover images come from this base URL
/// instead of `https://resources.tidal.com` when it is set.
pub const IMAGES_BASE_VAR: &str = "TIDAL_PLAYER_IMAGES_BASE";

/// How loud a log line is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Info,
    Warn,
}

/// Where the adapter's log lines go.
pub type Log = Arc<dyn Fn(Level, &str) + Send + Sync>;

/// Log lines through `tracing` (the TUI owns the terminal; `play`'s stderr
/// is for its own messages).
pub fn tracing_log() -> Log {
    Arc::new(|level, line| match level {
        Level::Info => tracing::info!("{line}"),
        Level::Warn => tracing::warn!("{line}"),
    })
}

/// Log lines on stderr: the daemon's log (the journal under systemd).
pub fn stderr_log() -> Log {
    Arc::new(|_, line| eprintln!("{line}"))
}

/// What the player's MPRIS needs from the settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// `mpris` / `TIDAL_PLAYER_MPRIS`.
    pub enabled: bool,
    /// `max_cover_arts`; `0`: no cache.
    pub max_cover_arts: u16,
    /// The cache directory (`covers/` is inside).
    pub cache_dir: PathBuf,
    /// Where the images come from instead of Tidal (tests).
    pub images_base: Option<String>,
}

impl Settings {
    /// From the player's resolved settings and this process's environment.
    pub fn from_env(player: &crate::play::PlayerSettings) -> Self {
        let images_base = if cfg!(debug_assertions) {
            std::env::var(IMAGES_BASE_VAR)
                .ok()
                .filter(|b| !b.is_empty())
        } else {
            None
        };
        Self {
            enabled: player.mpris,
            max_cover_arts: player.max_cover_arts,
            cache_dir: covers::process_cache_dir(),
            images_base,
        }
    }
}

/// The player on the session bus.
pub struct Mpris {
    adapter: hub::Adapter,
    name: String,
}

impl Mpris {
    /// The bus name it owns.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Stops taking calls and closes the bus connection (releasing the
    /// name). Call it after the player's clients were told it shut down.
    pub fn close(self) {
        self.adapter.close();
    }
}

/// Publishes the player reading `inputs` on the session bus; `None` when
/// it is off, or when there is no bus (one `info` line, never retried).
pub fn start(
    settings: &Settings,
    inputs: Sender<RuntimeInput>,
    runtime: &tokio::runtime::Handle,
    log: Log,
) -> Option<Mpris> {
    if !settings.enabled {
        return None;
    }
    let cache = (settings.max_cover_arts > 0).then(|| {
        let cache = covers::CoverCache::new(&settings.cache_dir, settings.max_cover_arts.into());
        match &settings.images_base {
            Some(base) => cache.with_base(base),
            None => cache,
        }
    });
    let adapter = hub::Adapter::new(
        inputs,
        hub::Options {
            deadline: hub::DEADLINE,
            covers: cache.clone().map(|c| (c, runtime.clone())),
            log: Arc::clone(&log),
        },
    );
    let connected = runtime.block_on(bus::connect(
        adapter.clone(),
        runtime.clone(),
        Arc::clone(&log),
    ));
    match connected {
        Ok((emitter, name)) => {
            if let Some(cache) = cache
                && let Err(e) = cache.start()
            {
                log(
                    Level::Warn,
                    &format!(
                        "Cannot clean the cover cache {}: {e}",
                        cache.dir().display()
                    ),
                );
            }
            adapter.set_bus(emitter);
            adapter.attach();
            Some(Mpris { adapter, name })
        }
        Err(e) => {
            log(Level::Info, &format!("MPRIS is not available: {e}"));
            None
        }
    }
}
