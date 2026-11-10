//! Session storage backends: keyring, age-encrypted file, and the fallback
//! chain over both. See docs/specs/0002-auth.md ("Storage", AC10, AC11).

use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use age::secrecy::SecretString;
use serde::Serialize;
use tidal_player_api::auth::{Session, SessionStore, StoreError};
use zeroize::Zeroizing;

use crate::session::passphrase::{Passphrase, PassphraseError, PassphraseSource, Purpose};

/// Version written into, and required from, stored sessions.
const FORMAT_VERSION: u32 = 1;

#[derive(Serialize)]
struct Envelope<'a> {
    version: u32,
    #[serde(flatten)]
    session: &'a Session,
}

/// The one encoding of a session, shared by every backend:
/// `{"version":1, ...session fields}`.
pub fn encode_session(session: &Session) -> Result<Zeroizing<Vec<u8>>, StoreError> {
    serde_json::to_vec(&Envelope {
        version: FORMAT_VERSION,
        session,
    })
    .map(Zeroizing::new)
    // The serde error text never carries values for serialisation, but keep it out anyway.
    .map_err(|_| StoreError::Other("cannot encode the session".into()))
}

/// Inverse of [`encode_session`]. Unparseable data and unknown versions are
/// "not logged in" (`None`) with a logged warning, never an error. The
/// parse error text is not logged: serde may quote parts of the input.
pub fn decode_session(bytes: &[u8]) -> Option<Session> {
    let value: serde_json::Value = match serde_json::from_slice(bytes) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(kind = ?e.classify(), "stored session is not valid JSON, ignoring it");
            return None;
        }
    };
    let version = value.get("version").and_then(serde_json::Value::as_u64);
    if version != Some(u64::from(FORMAT_VERSION)) {
        tracing::warn!(
            ?version,
            "stored session has an unknown version, ignoring it"
        );
        return None;
    }
    match serde_json::from_value::<Session>(value) {
        Ok(session) => Some(session),
        Err(e) => {
            tracing::warn!(kind = ?e.classify(), "stored session has unexpected fields, ignoring it");
            None
        }
    }
}

/// Session in an age file encrypted with a passphrase (scrypt recipient).
pub struct EncryptedFileStore {
    path: PathBuf,
    passphrase: Arc<dyn PassphraseSource>,
    work_factor: Option<u8>,
}

impl EncryptedFileStore {
    /// `path` is the session file; its directory is created (`0700`) on save.
    /// `passphrase` is only consulted when the file has to be read or written.
    pub fn new(path: PathBuf, passphrase: Arc<dyn PassphraseSource>) -> Self {
        Self {
            path,
            passphrase,
            work_factor: None,
        }
    }

    /// Low scrypt cost, so tests do not spend a second per round trip.
    #[cfg(test)]
    fn with_work_factor(mut self, log_n: u8) -> Self {
        self.work_factor = Some(log_n);
        self
    }

    fn encrypt(&self, plaintext: &[u8], passphrase: &Passphrase) -> Result<Vec<u8>, StoreError> {
        let mut recipient = age::scrypt::Recipient::new(SecretString::from(passphrase.as_str()));
        if let Some(log_n) = self.work_factor {
            recipient.set_work_factor(log_n);
        }
        age::encrypt(&recipient, plaintext)
            .map_err(|_| StoreError::Other("cannot encrypt the session".into()))
    }

    /// `Ok(None)`: the file decrypted by nobody's fault but is not a session
    /// file we understand (foreign age file).
    fn decrypt(
        bytes: &[u8],
        passphrase: &Passphrase,
    ) -> Result<Option<Zeroizing<Vec<u8>>>, StoreError> {
        let identity = age::scrypt::Identity::new(SecretString::from(passphrase.as_str()));
        match age::decrypt(&identity, bytes) {
            Ok(plain) => Ok(Some(Zeroizing::new(plain))),
            Err(
                age::DecryptError::DecryptionFailed
                | age::DecryptError::KeyDecryptionFailed
                | age::DecryptError::NoMatchingKeys,
            ) => Err(StoreError::WrongPassphrase),
            Err(age::DecryptError::InvalidHeader | age::DecryptError::UnknownFormat) => {
                tracing::warn!("session file is not an age passphrase file, ignoring it");
                Ok(None)
            }
            Err(e) => Err(StoreError::Other(format!(
                "cannot decrypt the session file: {e}"
            ))),
        }
    }

    fn get_passphrase(&self, purpose: Purpose<'_>) -> Result<Passphrase, StoreError> {
        self.passphrase.passphrase(purpose).map_err(|e| match e {
            PassphraseError::WrongPassphrase => StoreError::WrongPassphrase,
            other @ PassphraseError::Unavailable => StoreError::Unavailable(other.to_string()),
            other => StoreError::Other(other.to_string()),
        })
    }

    fn read_file(&self) -> Result<Option<Vec<u8>>, StoreError> {
        match std::fs::read(&self.path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(io_error("cannot read the session file", &e)),
        }
    }
}

fn io_error(what: &str, e: &std::io::Error) -> StoreError {
    StoreError::Other(format!("{what}: {e}"))
}

impl SessionStore for EncryptedFileStore {
    fn load(&self) -> Result<Option<Session>, StoreError> {
        let Some(bytes) = self.read_file()? else {
            return Ok(None);
        };
        let verify =
            |p: &Passphrase| !matches!(Self::decrypt(&bytes, p), Err(StoreError::WrongPassphrase));
        let passphrase = self.get_passphrase(Purpose::Unlock(&verify))?;
        Ok(Self::decrypt(&bytes, &passphrase)?.and_then(|plain| decode_session(&plain)))
    }

    fn save(&self, session: &Session) -> Result<(), StoreError> {
        let existing = self.read_file()?;
        let passphrase = match &existing {
            Some(bytes) => {
                // Opening an existing file with the wrong passphrase must not
                // let us overwrite it with a session nobody can read.
                let verify = |p: &Passphrase| {
                    !matches!(Self::decrypt(bytes, p), Err(StoreError::WrongPassphrase))
                };
                self.get_passphrase(Purpose::Unlock(&verify))?
            }
            None => self.get_passphrase(Purpose::Create)?,
        };
        let ciphertext = self.encrypt(&encode_session(session)?, &passphrase)?;

        let dir = self
            .path
            .parent()
            .ok_or_else(|| StoreError::Other("session file path has no directory".into()))?;
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
            .map_err(|e| io_error("cannot create the session directory", &e))?;

        let tmp = tmp_path(&self.path);
        write_private(&tmp, &ciphertext)
            .and_then(|()| std::fs::rename(&tmp, &self.path))
            .map_err(|e| {
                let _ = std::fs::remove_file(&tmp);
                io_error("cannot write the session file", &e)
            })
    }

    fn delete(&self) -> Result<(), StoreError> {
        match std::fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(io_error("cannot delete the session file", &e)),
        }
    }
}

fn tmp_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".tmp");
    path.with_file_name(name)
}

/// Writes `bytes` to a new `0600` file and syncs it.
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

/// Session in the Secret Service, via `keyring` (service `tidal-player`,
/// account `session`). Not unit-tested: it sits behind [`SessionStore`] and
/// is checked by hand at acceptance (spec 0002, test plan).
#[derive(Debug, Default)]
pub struct KeyringStore;

impl KeyringStore {
    const SERVICE: &'static str = "tidal-player";
    const ACCOUNT: &'static str = "session";

    pub fn new() -> Self {
        Self
    }

    fn entry() -> Result<keyring::Entry, StoreError> {
        if let Err(e) = keyring::Entry::store_status() {
            return Err(map_keyring_error(e));
        }
        keyring::Entry::new(Self::SERVICE, Self::ACCOUNT).map_err(|e| map_keyring_error(&e))
    }
}

/// No Secret Service, no provider, locked and not unlockable: unavailable,
/// so the fallback store takes over. Anything else is a plain error.
fn map_keyring_error(e: &keyring::Error) -> StoreError {
    use keyring::Error as E;
    match e {
        E::NoDefaultStore | E::NoStorageAccess(_) | E::PlatformFailure(_) => {
            StoreError::Unavailable(format!("keyring: {e}"))
        }
        // Never include `BadDataFormat` & co. payloads: they hold secret bytes.
        E::NoEntry => StoreError::Other("keyring: no entry".into()),
        _ => StoreError::Other("keyring: unexpected error".into()),
    }
}

impl SessionStore for KeyringStore {
    fn load(&self) -> Result<Option<Session>, StoreError> {
        match Self::entry()?.get_secret() {
            Ok(bytes) => Ok(decode_session(&Zeroizing::new(bytes))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(map_keyring_error(&e)),
        }
    }

    fn save(&self, session: &Session) -> Result<(), StoreError> {
        Self::entry()?
            .set_secret(&encode_session(session)?)
            .map_err(|e| map_keyring_error(&e))
    }

    fn delete(&self) -> Result<(), StoreError> {
        match Self::entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(map_keyring_error(&e)),
        }
    }
}

/// Primary store first (keyring), secondary (encrypted file) when the primary
/// is unavailable; reads look at both.
pub struct FallbackStore {
    primary: Box<dyn SessionStore>,
    secondary: Box<dyn SessionStore>,
}

impl FallbackStore {
    pub fn new(primary: Box<dyn SessionStore>, secondary: Box<dyn SessionStore>) -> Self {
        Self { primary, secondary }
    }
}

impl SessionStore for FallbackStore {
    fn load(&self) -> Result<Option<Session>, StoreError> {
        match self.primary.load() {
            Ok(Some(session)) => Ok(Some(session)),
            Ok(None) | Err(StoreError::Unavailable(_)) => self.secondary.load(),
            Err(e) => Err(e),
        }
    }

    fn save(&self, session: &Session) -> Result<(), StoreError> {
        match self.primary.save(session) {
            Err(StoreError::Unavailable(reason)) => {
                tracing::info!(%reason, "primary session store unavailable, using the fallback");
                self.secondary.save(session)
            }
            other => other,
        }
    }

    fn delete(&self) -> Result<(), StoreError> {
        let primary = match self.primary.delete() {
            Err(StoreError::Unavailable(_)) => Ok(()),
            other => other,
        };
        let secondary = self.secondary.delete();
        primary.and(secondary)
    }
}

#[cfg(test)]
mod tests;
