//! One player per user (spec 0005 "Transport"): an exclusive lock on
//! `player.lock` in the runtime directory, held for the player's whole
//! life, with its pid written into the file.

use std::fs::{File, OpenOptions, TryLockError};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::paths::LOCK_NAME;

/// The lock, held until dropped (or the process ends, however it ends).
#[derive(Debug)]
pub struct PlayerLock {
    _file: File,
    path: PathBuf,
}

/// Why the lock was not taken.
#[derive(Debug, thiserror::Error)]
pub enum LockError {
    /// Another process holds it: "a player is running" (exit 3).
    #[error("Another player is running{}", pid_suffix(*.pid))]
    Held { pid: Option<u32> },
    #[error("Cannot lock {}: {source}", .path.display())]
    Io { path: PathBuf, source: io::Error },
}

fn pid_suffix(pid: Option<u32>) -> String {
    pid.map(|pid| format!(" (pid {pid})")).unwrap_or_default()
}

/// Whether a player holds the lock in a directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Probe {
    /// No player is running.
    Free,
    /// A player is running (with its pid, when it was written yet).
    Held { pid: Option<u32> },
}

impl PlayerLock {
    /// Takes the lock in `dir` and writes this process's pid into it.
    pub fn acquire(dir: &Path) -> Result<Self, LockError> {
        let path = dir.join(LOCK_NAME);
        let io_error = |source| LockError::Io {
            path: path.clone(),
            source,
        };
        let mut file = open(&path).map_err(io_error)?;
        match file.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => {
                return Err(LockError::Held {
                    pid: read_pid(&mut file),
                });
            }
            Err(TryLockError::Error(e)) => return Err(io_error(e)),
        }
        file.set_len(0)
            .and_then(|()| file.seek(SeekFrom::Start(0)))
            .and_then(|_| writeln!(file, "{}", std::process::id()))
            .and_then(|()| file.flush())
            .map_err(io_error)?;
        Ok(Self { _file: file, path })
    }

    /// The lock file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The runtime directory it is in.
    pub fn dir(&self) -> &Path {
        self.path.parent().unwrap_or(Path::new("/"))
    }
}

/// Whether a player is running in `dir`, without staying in the way: a
/// free lock is released at once.
pub fn probe(dir: &Path) -> io::Result<Probe> {
    let path = dir.join(LOCK_NAME);
    let mut file = match OpenOptions::new().read(true).open(&path) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Probe::Free),
        Err(e) => return Err(e),
    };
    match file.try_lock_shared() {
        Ok(()) => Ok(Probe::Free),
        Err(TryLockError::WouldBlock) => Ok(Probe::Held {
            pid: read_pid(&mut file),
        }),
        Err(TryLockError::Error(e)) => Err(e),
    }
}

fn open(path: &Path) -> io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    // Not truncated: the holder's pid stays readable until it is replaced.
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(path)
}

/// The pid in a held lock file. The holder writes it right after taking
/// the lock, so an empty file is read again for a moment.
fn read_pid(file: &mut File) -> Option<u32> {
    for _ in 0..20 {
        let mut text = String::new();
        if file.seek(SeekFrom::Start(0)).is_ok()
            && file.read_to_string(&mut text).is_ok()
            && let Ok(pid) = text.trim().parse()
        {
            return Some(pid);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    None
}
