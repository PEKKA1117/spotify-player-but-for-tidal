//! The remembered playback state on disk (spec 0009 "Saving", "Where", "A
//! missing, unreadable or corrupt file", "Turning it off"): the file's
//! paths, loading it (with the `.bad` rename), writing it atomically behind
//! a small filesystem seam, and the save schedule as a pure function of the
//! saved state, the playing flag and a monotonic instant.
//!
//! How the player runtime wires it (only the player process, never `play`
//! and never a client):
//!
//! 1. At start, before the first input: `let p = Persister::new(state_dir,
//!    settings.remember_playback)`, then `let Loaded { saved, message } =
//!    p.load()`; restore from `saved` (with `autoplay` set by
//!    [`crate::cli::play::start_autoplay`]), put `message` in the player's
//!    message, and make `SaveSchedule::new(state.saved(), now)`.
//! 2. After every input the player handles: `schedule.on_change(now,
//!    state.saved(), playing, urgent)`, where `urgent` is a pause, a stop, a
//!    seek (`SeekBy`/`SeekTo`) or a track change (the current entry changed,
//!    or a track started). A `Some(saved)` is written with
//!    [`Persister::save`].
//! 3. Wake at [`SaveSchedule::next_deadline`] and call
//!    [`SaveSchedule::on_tick`]; a `Some(saved)` is written the same way.
//! 4. On `Shutdown`: [`SaveSchedule::on_exit`], and write what it returns.
//!    Nothing is written after it.
//!
//! [`Persister::save`] returns the message for the player (once per run of
//! failures); after a failure, call [`SaveSchedule::write_failed`] so the
//! next change writes again.

use std::io::{self, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use tidal_player_core::SavedPlayback;
use tidal_player_core::player::SAVED_PLAYBACK_VERSION;

/// The file's name in the state directory.
pub const PLAYBACK_FILE: &str = "playback.json";
/// Where a save is written before the rename.
pub const PLAYBACK_TMP: &str = "playback.json.tmp";
/// Where a corrupt file is kept.
pub const PLAYBACK_BAD: &str = "playback.json.bad";
/// Changes other than the position are coalesced for this long.
pub const COALESCE: Duration = Duration::from_secs(2);
/// While playing, the position alone is saved at most this often.
pub const POSITION_INTERVAL: Duration = Duration::from_secs(30);

/// `<dir>/playback.json`.
pub fn playback_path(dir: &Path) -> PathBuf {
    dir.join(PLAYBACK_FILE)
}

/// `<dir>/playback.json.tmp`.
pub fn tmp_path(dir: &Path) -> PathBuf {
    dir.join(PLAYBACK_TMP)
}

/// `<dir>/playback.json.bad`.
pub fn bad_path(dir: &Path) -> PathBuf {
    dir.join(PLAYBACK_BAD)
}

/// The file operations persistence needs, so tests can fail any of them.
pub trait Fs {
    fn read(&self, path: &Path) -> io::Result<Vec<u8>>;
    /// Creates `dir` and its parents with mode `0700`.
    fn create_private_dir(&self, dir: &Path) -> io::Result<()>;
    /// Writes a `0600` file (created or truncated) and `fsync`s it.
    fn write_private(&self, path: &Path, bytes: &[u8]) -> io::Result<()>;
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()>;
    fn remove_file(&self, path: &Path) -> io::Result<()>;
}

/// [`Fs`] on `std::fs`.
#[derive(Debug, Clone, Copy, Default)]
pub struct RealFs;

impl Fs for RealFs {
    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        std::fs::read(path)
    }

    fn create_private_dir(&self, dir: &Path) -> io::Result<()> {
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
    }

    fn write_private(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        file.write_all(bytes)?;
        file.sync_all()
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        std::fs::rename(from, to)
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        std::fs::remove_file(path)
    }
}

/// What loading found: the state to restore, and the player's message.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Loaded {
    pub saved: Option<SavedPlayback>,
    pub message: Option<String>,
}

/// Reads `<dir>/playback.json` (spec 0009 AC6). Never fails: a missing
/// file is an empty start; an unreadable one says so; a corrupt one is
/// renamed to `playback.json.bad` and says why.
pub fn load(fs: &dyn Fs, dir: &Path) -> Loaded {
    let path = playback_path(dir);
    let bytes = match fs.read(&path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Loaded::default(),
        Err(e) => {
            tracing::warn!("cannot read {}: {e}", path.display());
            return Loaded {
                saved: None,
                message: Some(format!(
                    "Could not restore the playback state: {}: {e}",
                    path.display()
                )),
            };
        }
    };
    match parse(&bytes) {
        Ok(saved) => Loaded {
            saved: Some(saved),
            message: None,
        },
        Err(reason) => {
            tracing::warn!("corrupt {}: {reason}", path.display());
            if let Err(e) = fs.rename(&path, &bad_path(dir)) {
                tracing::warn!("cannot keep {} as {PLAYBACK_BAD}: {e}", path.display());
            }
            Loaded {
                saved: None,
                message: Some(format!(
                    "Could not restore the playback state (kept as {PLAYBACK_BAD}): {reason}"
                )),
            }
        }
    }
}

/// The file's contents, or why they cannot be used: not JSON, a
/// `version` other than [`SAVED_PLAYBACK_VERSION`], or a missing or
/// wrong field.
fn parse(bytes: &[u8]) -> Result<SavedPlayback, String> {
    let value: serde_json::Value = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    if let Some(version) = value.get("version").and_then(serde_json::Value::as_u64)
        && version != u64::from(SAVED_PLAYBACK_VERSION)
    {
        return Err(format!("unsupported version {version}"));
    }
    serde_json::from_slice(bytes).map_err(|e| e.to_string())
}

/// Writes `saved` to `<dir>/playback.json` through `playback.json.tmp`
/// and a rename, creating the directory `0700` (spec 0009 AC5).
pub fn save(fs: &dyn Fs, dir: &Path, saved: &SavedPlayback) -> io::Result<()> {
    let bytes = serde_json::to_vec(saved).map_err(io::Error::other)?;
    fs.create_private_dir(dir)?;
    let tmp = tmp_path(dir);
    fs.write_private(&tmp, &bytes)?;
    fs.rename(&tmp, &playback_path(dir))
}

/// Deletes `playback.json` and `playback.json.bad` (`logout`, spec 0009
/// AC13); a missing file is fine.
pub fn forget(fs: &dyn Fs, dir: &Path) -> io::Result<()> {
    for path in [playback_path(dir), bad_path(dir)] {
        match fs.remove_file(&path) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
            _ => {}
        }
    }
    Ok(())
}

/// What a [`Persister::save`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveResult {
    /// `remember_playback` is off: nothing was written.
    Disabled,
    Written,
    /// The write failed; `message` is the player's message, given only on
    /// the first failure since the last success.
    Failed {
        message: Option<String>,
    },
}

/// The state directory's playback file, honouring `remember_playback`, and
/// whether the last write failed (spec 0009 AC7, AC11).
#[derive(Debug)]
pub struct Persister<F: Fs = RealFs> {
    fs: F,
    dir: PathBuf,
    enabled: bool,
    failing: bool,
}

impl Persister<RealFs> {
    /// The real file in `state_dir`; `enabled` is `remember_playback`.
    pub fn new(state_dir: PathBuf, enabled: bool) -> Self {
        Self::with_fs(RealFs, state_dir, enabled)
    }
}

impl<F: Fs> Persister<F> {
    pub fn with_fs(fs: F, dir: PathBuf, enabled: bool) -> Self {
        Self {
            fs,
            dir,
            enabled,
            failing: false,
        }
    }

    pub fn fs(&self) -> &F {
        &self.fs
    }

    /// Whether the last write failed.
    pub fn is_failing(&self) -> bool {
        self.failing
    }

    /// [`load`], or nothing (no read) when off.
    pub fn load(&self) -> Loaded {
        if !self.enabled {
            return Loaded::default();
        }
        load(&self.fs, &self.dir)
    }

    /// [`save`] (nothing when off), then [`Self::on_write_result`].
    pub fn save(&mut self, saved: &SavedPlayback) -> SaveResult {
        if !self.enabled {
            return SaveResult::Disabled;
        }
        let result = save(&self.fs, &self.dir, saved);
        let ok = result.is_ok();
        let message = self.on_write_result(result);
        if ok {
            SaveResult::Written
        } else {
            SaveResult::Failed { message }
        }
    }

    /// Records a write's result: the player's message on the first failure
    /// since the last success, `None` otherwise.
    pub fn on_write_result(&mut self, result: io::Result<()>) -> Option<String> {
        match result {
            Ok(()) => {
                self.failing = false;
                None
            }
            Err(e) => {
                tracing::warn!(
                    "cannot save the playback state in {}: {e}",
                    self.dir.display()
                );
                let first = !std::mem::replace(&mut self.failing, true);
                first.then(|| format!("Could not save the playback state: {e}"))
            }
        }
    }
}

/// When to save (spec 0009 "Saving", AC4): a pure schedule over a
/// monotonic clock the caller passes in. A change other than the position
/// is written [`COALESCE`] after the first unsaved change; the position
/// alone, while playing, at most every [`POSITION_INTERVAL`] since the last
/// write; an urgent change at once; nothing when nothing differs from the
/// last write.
#[derive(Debug, Clone)]
pub struct SaveSchedule {
    /// What the file holds; `None` after a failed write (unknown).
    last: Option<SavedPlayback>,
    /// When the file was last written (or the schedule made).
    last_at: Instant,
    /// The newest state not yet written.
    pending: Option<SavedPlayback>,
    deadline: Option<Instant>,
    closed: bool,
}

impl SaveSchedule {
    /// `baseline` is what the file already holds (the restored state, or
    /// the empty state); nothing is written until something differs.
    pub fn new(baseline: SavedPlayback, now: Instant) -> Self {
        Self {
            last: Some(baseline),
            last_at: now,
            pending: None,
            deadline: None,
            closed: false,
        }
    }

    /// After an input: the state to write now, if any. `urgent`: a pause,
    /// stop, seek or track change.
    pub fn on_change(
        &mut self,
        now: Instant,
        saved: SavedPlayback,
        playing: bool,
        urgent: bool,
    ) -> Option<SavedPlayback> {
        if self.closed {
            return None;
        }
        if self.last.as_ref() == Some(&saved) {
            self.pending = None;
            self.deadline = None;
            return None;
        }
        if urgent {
            return Some(self.written(now, saved));
        }
        let position_only = self.last.as_ref().is_some_and(|last| {
            SavedPlayback {
                position_ms: saved.position_ms,
                ..last.clone()
            } == saved
        });
        let due = if position_only && playing {
            self.last_at + POSITION_INTERVAL
        } else {
            now + COALESCE
        };
        self.deadline = Some(self.deadline.map_or(due, |d| d.min(due)));
        self.pending = Some(saved);
        self.on_tick(now)
    }

    /// At (or after) [`Self::next_deadline`]: the state to write, if due.
    pub fn on_tick(&mut self, now: Instant) -> Option<SavedPlayback> {
        if self.closed || self.deadline.is_none_or(|d| d > now) {
            return None;
        }
        let saved = self.pending.take()?;
        Some(self.written(now, saved))
    }

    /// When [`Self::on_tick`] should next be called.
    pub fn next_deadline(&self) -> Option<Instant> {
        self.deadline.filter(|_| !self.closed)
    }

    /// At exit: the state to write now (when it differs from the last
    /// write); nothing is written after this.
    pub fn on_exit(&mut self, now: Instant, saved: SavedPlayback) -> Option<SavedPlayback> {
        if self.closed {
            return None;
        }
        let write = (self.last.as_ref() != Some(&saved)).then(|| self.written(now, saved));
        self.closed = true;
        self.pending = None;
        self.deadline = None;
        write
    }

    /// The last write failed: the next change writes again.
    pub fn write_failed(&mut self) {
        self.last = None;
    }

    fn written(&mut self, now: Instant, saved: SavedPlayback) -> SavedPlayback {
        self.last = Some(saved.clone());
        self.last_at = now;
        self.pending = None;
        self.deadline = None;
        saved
    }
}

#[cfg(test)]
mod tests;
