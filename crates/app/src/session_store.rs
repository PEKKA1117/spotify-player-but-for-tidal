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

use crate::passphrase::{Passphrase, PassphraseError, PassphraseSource, Purpose};

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
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, SystemTime};
    use tidal_player_api::auth::MemoryStore;

    fn session(tag: &str) -> Session {
        Session {
            access_token: format!("FAKE-ACCESS-{tag}"),
            refresh_token: format!("FAKE-REFRESH-{tag}"),
            expires_at: SystemTime::UNIX_EPOCH + Duration::from_secs(14_400),
            user_id: 7,
            country_code: "FI".into(),
        }
    }

    /// Hands out a fixed passphrase and counts how often it was asked.
    struct Fixed {
        value: &'static str,
        asked: AtomicUsize,
    }

    impl Fixed {
        fn new(value: &'static str) -> Arc<Self> {
            Arc::new(Self {
                value,
                asked: AtomicUsize::new(0),
            })
        }
    }

    impl PassphraseSource for Fixed {
        fn passphrase(&self, _: Purpose<'_>) -> Result<Passphrase, PassphraseError> {
            self.asked.fetch_add(1, Ordering::SeqCst);
            Ok(Passphrase::new(self.value.to_owned()))
        }
    }

    fn file_store(path: &Path, source: &Arc<Fixed>) -> EncryptedFileStore {
        EncryptedFileStore::new(path.to_path_buf(), source.clone()).with_work_factor(2)
    }

    fn mode(path: &Path) -> u32 {
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn ac10_encrypted_file_store() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("state").join("tidal-player");
        let path = dir.join("session.age");
        let right = Fixed::new("right");

        // missing file: None, without asking for a passphrase
        let store = file_store(&path, &right);
        assert_eq!(store.load(), Ok(None));
        assert_eq!(right.asked.load(Ordering::SeqCst), 0, "lazy passphrase");

        // round trip, on-disk shape, permissions
        store.save(&session("1")).unwrap();
        assert_eq!(store.load(), Ok(Some(session("1"))));
        let raw = std::fs::read(&path).unwrap();
        assert!(raw.starts_with(b"age-encryption.org/v1"));
        let text = String::from_utf8_lossy(&raw);
        assert!(!text.contains("FAKE-ACCESS") && !text.contains("FAKE-REFRESH"));
        assert_eq!(mode(&path), 0o600);
        assert_eq!(mode(&dir), 0o700);

        // wrong passphrase: error, never None, and the file is not overwritten
        let wrong = Fixed::new("wrong");
        let bad = file_store(&path, &wrong);
        assert_eq!(bad.load(), Err(StoreError::WrongPassphrase));
        assert_eq!(std::fs::read(&path).unwrap(), raw);

        // a leftover temp file from a crashed write does not disturb the old file
        std::fs::write(tmp_path(&path), b"half a write").unwrap();
        assert_eq!(store.load(), Ok(Some(session("1"))));
        store.save(&session("2")).unwrap();
        assert_eq!(store.load(), Ok(Some(session("2"))));
        assert!(!tmp_path(&path).exists(), "temp file is renamed away");

        // decrypts fine but is not a session: None
        let plain_cases: [(&str, &[u8]); 4] = [
            ("garbage", b"this is not json"),
            ("version 2", br#"{"version":2,"access_token":"x"}"#),
            ("no version", br#"{"access_token":"x"}"#),
            ("wrong fields", br#"{"version":1,"user_id":"seven"}"#),
        ];
        for (name, plaintext) in plain_cases {
            let ciphertext = {
                let mut r = age::scrypt::Recipient::new(SecretString::from("right"));
                r.set_work_factor(2);
                age::encrypt(&r, plaintext).unwrap()
            };
            std::fs::write(&path, ciphertext).unwrap();
            assert_eq!(store.load(), Ok(None), "{name}");
        }
        // not an age file at all
        std::fs::write(&path, b"plain text").unwrap();
        assert_eq!(store.load(), Ok(None), "foreign file");

        // delete never needs the passphrase; double delete is fine
        let silent = Fixed::new("never asked");
        let deleter = file_store(&path, &silent);
        deleter.delete().unwrap();
        deleter.delete().unwrap();
        assert!(!path.exists());
        assert_eq!(silent.asked.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn session_encoding_is_versioned_json() {
        let bytes = encode_session(&session("1")).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["version"], 1);
        assert_eq!(value["user_id"], 7);
        assert_eq!(decode_session(&bytes), Some(session("1")));
    }

    /// A store scripted to be available, unavailable or failing.
    #[derive(Clone, Copy, PartialEq)]
    enum Mode {
        Available,
        Unavailable,
    }

    struct Fake {
        mode: Mode,
        inner: MemoryStore,
        calls: Mutex<Vec<&'static str>>,
    }

    impl Fake {
        fn new(mode: Mode, held: Option<Session>) -> Self {
            Self {
                mode,
                inner: held.map_or_else(MemoryStore::default, MemoryStore::with_session),
                calls: Mutex::new(Vec::new()),
            }
        }

        fn gate(&self, call: &'static str) -> Result<(), StoreError> {
            self.calls.lock().unwrap().push(call);
            match self.mode {
                Mode::Available => Ok(()),
                Mode::Unavailable => Err(StoreError::Unavailable("no secret service".into())),
            }
        }
    }

    impl SessionStore for Fake {
        fn load(&self) -> Result<Option<Session>, StoreError> {
            self.gate("load")?;
            self.inner.load()
        }
        fn save(&self, s: &Session) -> Result<(), StoreError> {
            self.gate("save")?;
            self.inner.save(s)
        }
        fn delete(&self) -> Result<(), StoreError> {
            self.gate("delete")?;
            self.inner.delete()
        }
    }

    /// Shares a `Fake` between the test and the `FallbackStore`.
    struct Shared(Arc<Fake>);

    impl SessionStore for Shared {
        fn load(&self) -> Result<Option<Session>, StoreError> {
            self.0.load()
        }
        fn save(&self, s: &Session) -> Result<(), StoreError> {
            self.0.save(s)
        }
        fn delete(&self) -> Result<(), StoreError> {
            self.0.delete()
        }
    }

    fn chain(primary: &Arc<Fake>, secondary: &Arc<Fake>) -> FallbackStore {
        FallbackStore::new(
            Box::new(Shared(primary.clone())),
            Box::new(Shared(secondary.clone())),
        )
    }

    #[test]
    fn ac11_fallback_store() {
        use Mode::{Available, Unavailable};
        let (a, b) = (session("primary"), session("secondary"));

        // load: (primary mode, primary holds, secondary holds) -> expected
        let loads = [
            (Available, Some(&a), Some(&b), Some(&a)),
            (Available, None, Some(&b), Some(&b)),
            (Available, None, None, None),
            (Unavailable, None, Some(&b), Some(&b)),
            (Unavailable, Some(&a), None, None),
        ];
        for (i, (mode, p, s, expected)) in loads.into_iter().enumerate() {
            let primary = Arc::new(Fake::new(mode, p.cloned()));
            let secondary = Arc::new(Fake::new(Available, s.cloned()));
            let got = chain(&primary, &secondary).load();
            assert_eq!(got, Ok(expected.cloned()), "load case {i}");
        }

        // save: available primary takes it, the secondary is untouched
        let primary = Arc::new(Fake::new(Available, None));
        let secondary = Arc::new(Fake::new(Available, None));
        chain(&primary, &secondary).save(&a).unwrap();
        assert_eq!(primary.inner.load(), Ok(Some(a.clone())));
        assert_eq!(secondary.inner.load(), Ok(None));
        assert!(secondary.calls.lock().unwrap().is_empty());

        // save: unavailable primary routes to the secondary
        let primary = Arc::new(Fake::new(Unavailable, None));
        let secondary = Arc::new(Fake::new(Available, None));
        chain(&primary, &secondary).save(&b).unwrap();
        assert_eq!(secondary.inner.load(), Ok(Some(b.clone())));

        // delete: removes from both
        let primary = Arc::new(Fake::new(Available, Some(a.clone())));
        let secondary = Arc::new(Fake::new(Available, Some(b.clone())));
        chain(&primary, &secondary).delete().unwrap();
        assert_eq!(primary.inner.load(), Ok(None));
        assert_eq!(secondary.inner.load(), Ok(None));

        // delete: unavailable primary is skipped, the secondary is still cleared
        let primary = Arc::new(Fake::new(Unavailable, None));
        let secondary = Arc::new(Fake::new(Available, Some(b)));
        chain(&primary, &secondary).delete().unwrap();
        assert_eq!(secondary.inner.load(), Ok(None));

        // a wrong passphrase on the secondary surfaces (not "not logged in")
        struct Wrong;
        impl SessionStore for Wrong {
            fn load(&self) -> Result<Option<Session>, StoreError> {
                Err(StoreError::WrongPassphrase)
            }
            fn save(&self, _: &Session) -> Result<(), StoreError> {
                Err(StoreError::WrongPassphrase)
            }
            fn delete(&self) -> Result<(), StoreError> {
                Ok(())
            }
        }
        let primary = Arc::new(Fake::new(Unavailable, None));
        let store = FallbackStore::new(Box::new(Shared(primary)), Box::new(Wrong));
        assert_eq!(store.load(), Err(StoreError::WrongPassphrase));
    }
}
