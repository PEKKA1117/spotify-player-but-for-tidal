//! Where the socket and the lock live (spec 0005 "Transport"): the runtime
//! directory, chosen from the environment, created private and refused
//! when it is not.

use std::io;
use std::path::{Path, PathBuf};

/// Overrides the runtime directory (tests, unusual setups).
pub const RUNTIME_DIR_VAR: &str = "TIDAL_PLAYER_RUNTIME_DIR";

/// The socket's file name in the runtime directory.
pub const SOCKET_NAME: &str = "player.sock";

/// The lock's file name in the runtime directory.
pub const LOCK_NAME: &str = "player.lock";

/// The runtime directory: `$TIDAL_PLAYER_RUNTIME_DIR` if set and not empty,
/// else `$XDG_RUNTIME_DIR/tidal-player`, else `/tmp/tidal-player-<uid>`.
pub fn runtime_dir(env: impl Fn(&str) -> Option<String>, uid: u32) -> PathBuf {
    let set = |key: &str| env(key).filter(|v| !v.is_empty()).map(PathBuf::from);
    set(RUNTIME_DIR_VAR)
        .or_else(|| set("XDG_RUNTIME_DIR").map(|dir| dir.join("tidal-player")))
        .unwrap_or_else(|| PathBuf::from(format!("/tmp/tidal-player-{uid}")))
}

/// What [`check_private`] looks at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirMeta {
    pub is_dir: bool,
    /// The owner.
    pub uid: u32,
    /// The permission bits (`st_mode & 0o7777`).
    pub mode: u32,
}

impl DirMeta {
    /// The metadata of `path` itself (a symlink is not followed).
    pub fn of(path: &Path) -> io::Result<Self> {
        use std::os::unix::fs::MetadataExt;
        let meta = std::fs::symlink_metadata(path)?;
        Ok(Self {
            is_dir: meta.is_dir(),
            uid: meta.uid(),
            mode: meta.mode() & 0o7777,
        })
    }
}

/// `Ok` when a directory with `meta` is private to `uid`, else the reason.
pub fn check_private(meta: DirMeta, uid: u32) -> Result<(), String> {
    if !meta.is_dir {
        Err("not a directory".to_owned())
    } else if meta.uid != uid {
        Err(format!("owned by uid {}", meta.uid))
    } else if meta.mode & 0o077 != 0 {
        Err(format!(
            "mode {:04o}, group or others have access",
            meta.mode & 0o777
        ))
    } else {
        Ok(())
    }
}

/// Why the runtime directory cannot be used (exit 1).
#[derive(Debug, thiserror::Error)]
pub enum RuntimeDirError {
    #[error("Runtime directory {} is not private: {reason}", .path.display())]
    NotPrivate { path: PathBuf, reason: String },
    #[error("Cannot create the runtime directory {}: {source}", .path.display())]
    Io { path: PathBuf, source: io::Error },
}

/// Creates `path` with mode `0700` (its parents as needed), or checks the
/// existing one is a directory owned by `uid` that only `uid` can use.
/// Nothing is ever chmod'ed.
pub fn prepare_runtime_dir(path: &Path, uid: u32) -> Result<(), RuntimeDirError> {
    use std::os::unix::fs::DirBuilderExt;
    let io_error = |source| RuntimeDirError::Io {
        path: path.to_owned(),
        source,
    };
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(io_error)?;
    }
    match std::fs::DirBuilder::new().mode(0o700).create(path) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(io_error(e)),
    }
    let meta = DirMeta::of(path).map_err(io_error)?;
    check_private(meta, uid).map_err(|reason| RuntimeDirError::NotPrivate {
        path: path.to_owned(),
        reason,
    })
}

/// This process's user ID: the owner of `/proc/self` (std has no
/// `getuid`, and the workspace denies `unsafe`).
pub fn current_uid() -> io::Result<u32> {
    use std::os::unix::fs::MetadataExt;
    Ok(std::fs::metadata("/proc/self")?.uid())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::os::unix::fs::PermissionsExt;

    /// AC9: the directory per "Transport", for each variable set, empty or
    /// unset.
    #[test]
    fn ac9_runtime_dir() {
        let rows: [(&[(&str, &str)], &str); 7] = [
            (&[], "/tmp/tidal-player-1000"),
            (
                &[("XDG_RUNTIME_DIR", "/run/user/1000")],
                "/run/user/1000/tidal-player",
            ),
            (&[("XDG_RUNTIME_DIR", "")], "/tmp/tidal-player-1000"),
            (&[(RUNTIME_DIR_VAR, "/srv/tp")], "/srv/tp"),
            (&[(RUNTIME_DIR_VAR, "")], "/tmp/tidal-player-1000"),
            (
                &[
                    (RUNTIME_DIR_VAR, "/srv/tp"),
                    ("XDG_RUNTIME_DIR", "/run/user/1000"),
                ],
                "/srv/tp",
            ),
            (
                &[(RUNTIME_DIR_VAR, ""), ("XDG_RUNTIME_DIR", "/run/user/1000")],
                "/run/user/1000/tidal-player",
            ),
        ];
        for (vars, want) in rows {
            let env: HashMap<&str, &str> = vars.iter().copied().collect();
            let got = runtime_dir(|k| env.get(k).map(|v| (*v).to_owned()), 1000);
            assert_eq!(got, PathBuf::from(want), "{vars:?}");
        }
        assert_eq!(
            runtime_dir(|_| None, 0),
            PathBuf::from("/tmp/tidal-player-0")
        );
    }

    /// AC9: created `0700`; an existing private one is accepted; a file,
    /// another owner or group/world access is refused, never chmod'ed.
    #[test]
    fn ac9_private_dir() {
        let uid = current_uid().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let mode = |p: &Path| DirMeta::of(p).unwrap().mode;

        // Created, parents included, with 0700.
        let dir = tmp.path().join("a/b/tidal-player");
        prepare_runtime_dir(&dir, uid).unwrap();
        assert!(dir.is_dir());
        assert_eq!(mode(&dir), 0o700);
        // Again: accepted as it is.
        prepare_runtime_dir(&dir, uid).unwrap();

        // Group- or world-accessible: refused, left as it is.
        for bits in [0o750, 0o705, 0o777, 0o710] {
            let dir = tmp.path().join(format!("open-{bits:o}"));
            std::fs::create_dir(&dir).unwrap();
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(bits)).unwrap();
            let err = prepare_runtime_dir(&dir, uid).unwrap_err();
            assert!(
                err.to_string().starts_with(&format!(
                    "Runtime directory {} is not private: ",
                    dir.display()
                )),
                "{err}"
            );
            assert_eq!(mode(&dir), bits);
        }

        // A file.
        let file = tmp.path().join("file");
        std::fs::write(&file, "").unwrap();
        let err = prepare_runtime_dir(&file, uid).unwrap_err();
        assert_eq!(
            err.to_string(),
            format!(
                "Runtime directory {} is not private: not a directory",
                file.display()
            )
        );

        // Owned by someone else: as another uid sees our directory.
        let err = prepare_runtime_dir(&dir, uid + 1).unwrap_err();
        assert_eq!(
            err.to_string(),
            format!(
                "Runtime directory {} is not private: owned by uid {uid}",
                dir.display()
            )
        );

        // The pure check, on fake metadata.
        let meta = |is_dir, uid, mode| DirMeta { is_dir, uid, mode };
        let rows = [
            (meta(true, 1000, 0o700), Ok(())),
            (meta(true, 1000, 0o1700), Ok(())),
            (meta(true, 0, 0o700), Err("owned by uid 0".to_owned())),
            (meta(true, 1001, 0o700), Err("owned by uid 1001".to_owned())),
            (
                meta(true, 1000, 0o755),
                Err("mode 0755, group or others have access".to_owned()),
            ),
            (
                meta(true, 1000, 0o701),
                Err("mode 0701, group or others have access".to_owned()),
            ),
            (meta(false, 1000, 0o600), Err("not a directory".to_owned())),
        ];
        for (meta, want) in rows {
            assert_eq!(check_private(meta, 1000), want, "{meta:?}");
        }
    }
}
