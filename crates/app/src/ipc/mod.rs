//! The player's socket (spec 0005 "Transport" and "Messages"): where it
//! lives ([`paths`]), the one-player lock ([`lock`]), the framing
//! ([`codec`]), the player's side ([`server`]) and a client's
//! ([`client`]).
//!
//! A player: [`claim`], then [`bind`], then [`server::serve`]. A client:
//! [`paths::runtime_dir`], [`lock::probe`], then
//! [`client::Connection::connect`].

pub mod client;
pub mod codec;
pub mod lock;
pub mod paths;
pub mod server;

use std::io;
use std::os::unix::net::UnixListener;
use std::path::PathBuf;

use lock::{LockError, PlayerLock};
use paths::{RuntimeDirError, SOCKET_NAME, prepare_runtime_dir, runtime_dir};

/// Why a process cannot become the player.
#[derive(Debug, thiserror::Error)]
pub enum ClaimError {
    #[error(transparent)]
    Dir(#[from] RuntimeDirError),
    #[error(transparent)]
    Lock(#[from] LockError),
    #[error("Cannot listen on {}: {source}", .path.display())]
    Bind { path: PathBuf, source: io::Error },
}

impl ClaimError {
    /// `3` when another player is running, else `1`.
    pub fn exit_code(&self) -> u8 {
        match self {
            Self::Lock(LockError::Held { .. }) => 3,
            _ => 1,
        }
    }
}

/// Takes the one-player lock in the runtime directory chosen from `env`,
/// which is created (or checked) first.
pub fn claim(env: impl Fn(&str) -> Option<String>, uid: u32) -> Result<PlayerLock, ClaimError> {
    let dir = runtime_dir(env, uid);
    prepare_runtime_dir(&dir, uid)?;
    Ok(PlayerLock::acquire(&dir)?)
}

/// Binds the socket next to `lock`, removing a crashed player's leftover
/// first (holding the lock, any socket there is one). Returns the
/// listener and the socket's path, for [`server::serve`].
pub fn bind(lock: &PlayerLock) -> Result<(UnixListener, PathBuf), ClaimError> {
    let path = lock.dir().join(SOCKET_NAME);
    let bind_error = |source| ClaimError::Bind {
        path: path.clone(),
        source,
    };
    match std::fs::remove_file(&path) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(bind_error(e)),
    }
    let listener = UnixListener::bind(&path).map_err(bind_error)?;
    Ok((listener, path))
}
