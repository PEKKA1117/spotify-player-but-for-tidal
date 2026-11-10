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
