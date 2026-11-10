use std::cell::RefCell;
use std::collections::VecDeque;
use std::os::unix::fs::PermissionsExt;

use tidal_player_core::player::{PlayerEffect, PlayerInput, update};
use tidal_player_core::protocol::{Command, InsertAt, QueueEntry, RepeatMode};
use tidal_player_core::{AlbumRef, ArtistRef, EntryId, PlayerConfig, PlayerState, Track, TrackId};

use super::*;

fn track(id: u64) -> Track {
    Track {
        id: TrackId(id),
        title: format!("Track {id}"),
        version: None,
        artists: vec![ArtistRef {
            id: 1,
            name: "Artist".into(),
        }],
        album: Some(AlbumRef {
            id: 1,
            title: "Album".into(),
            cover: Some("2e4a5d2d-9a0d-4c3a-a0ba-42b0bd16a6ec".into()),
        }),
        duration: Some(Duration::from_secs(296)),
        streamable: true,
    }
}

fn sample() -> SavedPlayback {
    SavedPlayback {
        version: SAVED_PLAYBACK_VERSION,
        entries: (1..=3)
            .map(|n| QueueEntry {
                id: EntryId(n),
                track: track(n * 10),
                suggested: n == 3,
            })
            .collect(),
        play_order: vec![EntryId(2), EntryId(1), EntryId(3)],
        current: Some(EntryId(2)),
        position_ms: 83_000,
        shuffle: true,
        repeat: RepeatMode::Queue,
        autoplay: false,
        volume: 80,
        muted: false,
    }
}

fn mode(path: &Path) -> u32 {
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

/// Which operation a [`FakeFs`] fails, and with what.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Read,
    CreateDir,
    Write,
    Rename,
    Remove,
}

/// `std::fs` underneath, every call recorded, and queued failures per
/// operation.
#[derive(Default)]
struct FakeFs {
    calls: RefCell<Vec<String>>,
    failures: RefCell<VecDeque<(Op, io::Error)>>,
}

impl FakeFs {
    fn failing(failures: Vec<(Op, io::Error)>) -> Self {
        Self {
            calls: RefCell::default(),
            failures: RefCell::new(failures.into()),
        }
    }

    fn fail(&self, op: Op, call: String) -> io::Result<()> {
        self.calls.borrow_mut().push(call);
        let mut failures = self.failures.borrow_mut();
        if failures.front().is_some_and(|(o, _)| *o == op) {
            return Err(failures.pop_front().unwrap().1);
        }
        Ok(())
    }

    fn calls(&self) -> Vec<String> {
        self.calls.borrow().clone()
    }
}

fn name(path: &Path) -> String {
    path.file_name().unwrap().to_string_lossy().into_owned()
}

impl Fs for FakeFs {
    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        self.fail(Op::Read, format!("read {}", name(path)))?;
        RealFs.read(path)
    }
    fn create_private_dir(&self, dir: &Path) -> io::Result<()> {
        self.fail(Op::CreateDir, format!("mkdir {}", name(dir)))?;
        RealFs.create_private_dir(dir)
    }
    fn write_private(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        self.fail(Op::Write, format!("write {}", name(path)))?;
        RealFs.write_private(path, bytes)
    }
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        self.fail(Op::Rename, format!("rename {} {}", name(from), name(to)))?;
        RealFs.rename(from, to)
    }
    fn remove_file(&self, path: &Path) -> io::Result<()> {
        self.fail(Op::Remove, format!("remove {}", name(path)))?;
        RealFs.remove_file(path)
    }
}

fn enospc() -> io::Error {
    io::Error::from_raw_os_error(28)
}

// --- AC4: the save schedule ----------------------------------------------

/// One input to the schedule at a time (ms after the start).
#[derive(Clone, Copy)]
enum Step {
    /// A changed state: what changes, playing, urgent.
    Change(fn(&mut SavedPlayback), bool, bool),
    Exit(fn(&mut SavedPlayback)),
}

fn volume_down(s: &mut SavedPlayback) {
    s.volume -= 1;
}
fn queue_edit(s: &mut SavedPlayback) {
    s.entries.pop();
    s.play_order.retain(|id| *id != EntryId(3));
}
fn toggle_shuffle(s: &mut SavedPlayback) {
    s.shuffle = !s.shuffle;
}
fn tick_second(s: &mut SavedPlayback) {
    s.position_ms += 1000;
}
fn seek(s: &mut SavedPlayback) {
    s.position_ms = 20_000;
}
fn next_track(s: &mut SavedPlayback) {
    s.current = Some(EntryId(1));
    s.position_ms = 0;
}
fn nothing(_: &mut SavedPlayback) {}
fn half_second(s: &mut SavedPlayback) {
    s.position_ms += 500;
}

/// Runs `steps` over a fake clock, calling `on_tick` at every deadline
/// up to 120 s; the writes as (ms, position).
fn run(steps: &[(u64, Step)]) -> Vec<(u64, u64)> {
    let t0 = Instant::now();
    let ms = |t: Instant| u64::try_from((t - t0).as_millis()).unwrap();
    let mut state = sample();
    let mut schedule = SaveSchedule::new(state.clone(), t0);
    let mut writes = Vec::new();
    let tick_until = |schedule: &mut SaveSchedule, until: Instant, writes: &mut Vec<_>| {
        while let Some(deadline) = schedule.next_deadline().filter(|d| *d <= until) {
            if let Some(saved) = schedule.on_tick(deadline) {
                writes.push((ms(deadline), saved.position_ms));
            } else {
                break;
            }
        }
    };
    for (at, step) in steps {
        let now = t0 + Duration::from_millis(*at);
        tick_until(&mut schedule, now, &mut writes);
        let written = match *step {
            Step::Change(change, playing, urgent) => {
                change(&mut state);
                schedule.on_change(now, state.clone(), playing, urgent)
            }
            Step::Exit(change) => {
                change(&mut state);
                schedule.on_exit(now, state.clone())
            }
        };
        if let Some(saved) = written {
            assert_eq!(saved, state, "writes the current state");
            writes.push((*at, saved.position_ms));
        }
    }
    tick_until(&mut schedule, t0 + Duration::from_secs(120), &mut writes);
    writes
}

/// AC4: write times per input sequence.
#[test]
fn ac4_save_schedule() {
    use Step::{Change, Exit};
    let ten_volume: Vec<(u64, Step)> = (0..10)
        .map(|i| (i * 100, Change(volume_down, false, false)))
        .collect();
    let playing_65s: Vec<(u64, Step)> = (1..=65)
        .map(|s| (s * 1000, Change(tick_second, true, false)))
        .collect();
    let mut playing_then_pause: Vec<(u64, Step)> = (1..=10)
        .map(|s| (s * 1000, Change(tick_second, true, false)))
        .collect();
    playing_then_pause.push((10_500, Change(half_second, false, true)));
    let mut exit_after_changes = vec![
        (0, Change(volume_down, false, false)),
        (300, Change(tick_second, true, false)),
        (500, Exit(half_second)),
        (1000, Change(volume_down, false, false)),
        (40_000, Change(toggle_shuffle, false, true)),
    ];
    exit_after_changes.extend((2..=40).map(|s| (s * 1000, Change(tick_second, true, false))));
    exit_after_changes.sort_by_key(|(at, _)| *at);

    let base = sample().position_ms;
    type Row = (&'static str, Vec<(u64, Step)>, Vec<(u64, u64)>);
    let rows: Vec<Row> = vec![
        (
            "a volume change: one write at +2 s",
            vec![(0, Change(volume_down, false, false))],
            vec![(2000, base)],
        ),
        (
            "ten volume changes within 1 s: one write",
            ten_volume,
            vec![(2000, base)],
        ),
        (
            "a queue edit: one write within 2 s",
            vec![(500, Change(queue_edit, false, false))],
            vec![(2500, base)],
        ),
        (
            "a mode toggle: one write within 2 s",
            vec![(700, Change(toggle_shuffle, true, false))],
            vec![(2700, base)],
        ),
        (
            "position only, playing: every 30 s and not between",
            playing_65s,
            vec![
                (30_000, base + 29_000),
                (60_000, base + 59_000),
                (90_000, base + 65_000),
            ],
        ),
        (
            "pause: at once",
            playing_then_pause,
            vec![(10_500, base + 10_500)],
        ),
        (
            "seek while paused: at once",
            vec![(5000, Change(seek, false, true))],
            vec![(5000, 20_000)],
        ),
        (
            "seek while playing: at once",
            vec![(5000, Change(seek, true, true))],
            vec![(5000, 20_000)],
        ),
        (
            "stop: at once",
            vec![(3000, Change(tick_second, false, true))],
            vec![(3000, base + 1000)],
        ),
        (
            "a track change: at once",
            vec![(4000, Change(next_track, true, true))],
            vec![(4000, 0)],
        ),
        (
            "nothing changed: none",
            vec![
                (0, Change(nothing, false, false)),
                (1000, Change(nothing, true, false)),
                (2000, Change(nothing, false, true)),
                (3000, Exit(nothing)),
            ],
            vec![],
        ),
        (
            "exit: at once with the exact position, then nothing",
            exit_after_changes,
            vec![(500, base + 1500)],
        ),
    ];
    for (name, steps, want) in rows {
        assert_eq!(run(&steps), want, "{name}");
    }
}

// --- AC5: writing --------------------------------------------------------

/// AC5: `playback.json` (`0600`) in a directory created `0700`, through
/// the temp file and a rename; it reads back.
#[test]
fn ac5_write_file() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("state");
    let fs = FakeFs::default();
    save(&fs, &dir, &sample()).unwrap();
    let path = dir.join(PLAYBACK_FILE);
    assert_eq!(mode(&dir), 0o700, "directory mode");
    assert_eq!(mode(&path), 0o600, "file mode");
    assert!(!dir.join(PLAYBACK_TMP).exists(), "temp file renamed away");
    assert_eq!(
        fs.calls(),
        [
            "mkdir state",
            "write playback.json.tmp",
            "rename playback.json.tmp playback.json",
        ]
    );
    let back: SavedPlayback = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(back, sample());
    // A second save replaces the file.
    let mut next = sample();
    next.volume = 10;
    save(&fs, &dir, &next).unwrap();
    assert_eq!(load(&RealFs, &dir).saved, Some(next));
}

/// AC5: a failure between the write and the rename leaves the old file
/// intact.
#[test]
fn ac5_write_is_atomic() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    save(&RealFs, dir, &sample()).unwrap();
    let before = std::fs::read(dir.join(PLAYBACK_FILE)).unwrap();
    let fs = FakeFs::failing(vec![(Op::Rename, io::Error::other("crash"))]);
    let mut next = sample();
    next.entries.clear();
    next.play_order.clear();
    next.current = None;
    assert!(
        save(&fs, dir, &next).is_err(),
        "the failed rename is reported"
    );
    assert!(
        fs.calls().contains(&"write playback.json.tmp".to_owned()),
        "written to the temp file: {:?}",
        fs.calls()
    );
    assert_eq!(std::fs::read(dir.join(PLAYBACK_FILE)).unwrap(), before);
}

// --- AC6: loading --------------------------------------------------------

/// AC6: missing, unreadable, corrupt (with the `.bad` rename) and valid.
#[test]
fn ac6_load() {
    let valid = serde_json::to_string(&sample()).unwrap();
    let mut no_field: serde_json::Value = serde_json::from_str(&valid).unwrap();
    no_field.as_object_mut().unwrap().remove("muted");
    let no_field = no_field.to_string();
    // A file written before 0010: no `cover` on any album (AC19).
    let mut old: serde_json::Value = serde_json::from_str(&valid).unwrap();
    for e in old["entries"].as_array_mut().unwrap() {
        e["track"]["album"].as_object_mut().unwrap().remove("cover");
    }
    let old = old.to_string();
    assert!(!old.contains("cover") && valid.contains("cover"));
    let mut sample_old = sample();
    for e in &mut sample_old.entries {
        e.track.album.as_mut().unwrap().cover = None;
    }
    let v2 = valid.replace("\"version\":1", "\"version\":2");
    assert_ne!(v2, valid);

    enum File {
        Missing,
        Dir,
        /// Mode `000` (tests run as root here, so the error is faked:
        /// `EACCES`, what the mode gives a normal user).
        Mode000,
        Text(String),
    }
    enum Want {
        Empty,
        Unreadable,
        Corrupt(&'static str),
        Valid(Box<SavedPlayback>),
    }
    let rows: Vec<(&str, File, Want)> = vec![
        ("missing", File::Missing, Want::Empty),
        ("a directory", File::Dir, Want::Unreadable),
        ("mode 000", File::Mode000, Want::Unreadable),
        (
            "truncated JSON",
            File::Text(valid[..valid.len() / 2].to_owned()),
            Want::Corrupt("EOF while parsing"),
        ),
        (
            "a missing field",
            File::Text(no_field),
            Want::Corrupt("missing field `muted`"),
        ),
        (
            "version 2",
            File::Text(v2),
            Want::Corrupt("unsupported version 2"),
        ),
        (
            "an old file without covers (AC19)",
            File::Text(old),
            Want::Valid(Box::new(sample_old)),
        ),
        (
            "valid, with covers (AC19)",
            File::Text(valid),
            Want::Valid(Box::new(sample())),
        ),
    ];
    for (name, file, want) in rows {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let path = dir.join(PLAYBACK_FILE);
        let bad = dir.join(PLAYBACK_BAD);
        std::fs::write(&bad, "older evidence").unwrap();
        let mut failures = Vec::new();
        match &file {
            File::Missing => {}
            File::Dir => std::fs::create_dir(&path).unwrap(),
            File::Mode000 => {
                std::fs::write(&path, "{}").unwrap();
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
                failures.push((Op::Read, io::Error::from_raw_os_error(13)));
            }
            File::Text(text) => std::fs::write(&path, text).unwrap(),
        }
        let fs = FakeFs::failing(failures);
        let got = load(&fs, dir);
        let older = || std::fs::read_to_string(&bad).unwrap();
        match want {
            Want::Empty => {
                assert_eq!(got, Loaded::default(), "{name}");
                assert_eq!(older(), "older evidence", "{name}: .bad untouched");
            }
            Want::Unreadable => {
                assert_eq!(got.saved, None, "{name}");
                let message = got.message.unwrap_or_default();
                let prefix = format!("Could not restore the playback state: {}: ", path.display());
                assert!(
                    message.starts_with(&prefix) && message.len() > prefix.len(),
                    "{name}: {message}"
                );
                assert!(path.exists(), "{name}: left where it is");
                assert_eq!(older(), "older evidence", "{name}: .bad untouched");
            }
            Want::Corrupt(reason) => {
                assert_eq!(got.saved, None, "{name}");
                let message = got.message.unwrap_or_default();
                let prefix = "Could not restore the playback state (kept as playback.json.bad): ";
                assert!(
                    message.starts_with(prefix) && message[prefix.len()..].contains(reason),
                    "{name}: {message}"
                );
                assert!(!path.exists(), "{name}: renamed away");
                let File::Text(text) = &file else {
                    unreachable!()
                };
                assert_eq!(&older(), text, "{name}: older .bad replaced");
            }
            Want::Valid(want) => {
                assert_eq!(
                    got,
                    Loaded {
                        saved: Some(*want),
                        message: None
                    },
                    "{name}"
                );
                assert!(path.exists(), "{name}: kept");
                assert!(
                    !bad.exists() || older() == "older evidence",
                    "{name}: no .bad rename"
                );
            }
        }
    }
}

// --- AC7: a failed write -------------------------------------------------

/// AC7: `ENOSPC`, then again, then success: the message once, the
/// failing flag cleared by the success, the player untouched.
#[test]
fn ac7_write_failure() {
    let tmp = tempfile::tempdir().unwrap();
    let fs = FakeFs::failing(vec![(Op::Write, enospc()), (Op::Write, enospc())]);
    let mut persister = Persister::with_fs(fs, tmp.path().to_owned(), true);

    // The player runs the same commands with and without the failing
    // saves in between: the same effects and the same state.
    let commands = [
        Command::AddToQueue {
            tracks: vec![track(1), track(2)],
            at: InsertAt::End,
        },
        Command::ChangeVolume(-10),
        Command::ToggleShuffle,
    ];
    let mut with = PlayerState::new(PlayerConfig::default(), 7);
    let mut without = PlayerState::new(PlayerConfig::default(), 7);
    let mut results = Vec::new();
    for command in &commands {
        let a: Vec<PlayerEffect> = update(&mut with, PlayerInput::Command(command.clone()));
        results.push(persister.save(&with.saved()));
        let b = update(&mut without, PlayerInput::Command(command.clone()));
        assert_eq!(a, b, "{command:?}: same effects");
    }
    assert_eq!(with.snapshot(), without.snapshot(), "same state");

    let message = "Could not save the playback state: No space left on device (os error 28)";
    assert_eq!(
        results,
        [
            SaveResult::Failed {
                message: Some(message.into())
            },
            SaveResult::Failed { message: None },
            SaveResult::Written,
        ]
    );
    assert!(!persister.is_failing(), "the success clears the flag");
    assert_eq!(load(&RealFs, tmp.path()).saved, Some(with.saved()));

    // Failing again after a success says so again.
    let mut again = Persister::with_fs(
        FakeFs::failing(vec![(Op::Write, enospc())]),
        tmp.path().to_owned(),
        true,
    );
    assert_eq!(
        again.on_write_result(Err(enospc())),
        Some(message.into()),
        "on_write_result: first failure"
    );
    assert!(again.is_failing());
    assert_eq!(again.on_write_result(Err(enospc())), None, "not repeated");
    assert_eq!(again.on_write_result(Ok(())), None);
    assert!(!again.is_failing());
    assert_eq!(
        again.on_write_result(Err(enospc())),
        Some(message.into()),
        "a new run of failures"
    );

    // The schedule writes again on the next change after a failure.
    let t0 = Instant::now();
    let mut schedule = SaveSchedule::new(sample(), t0);
    let written = schedule.on_change(t0, sample_with_volume(1), false, true);
    assert!(written.is_some());
    schedule.write_failed();
    assert_eq!(
        schedule.on_exit(t0, sample_with_volume(1)),
        Some(sample_with_volume(1)),
        "retried at exit although unchanged since the failed write"
    );
}

fn sample_with_volume(volume: u8) -> SavedPlayback {
    SavedPlayback { volume, ..sample() }
}

// --- AC11: remember_playback off -----------------------------------------

/// AC11: off → no read, no write, an existing file untouched.
#[test]
fn ac11_remember_off() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join(PLAYBACK_FILE);
    std::fs::write(&path, "not even JSON").unwrap();
    let mut persister = Persister::with_fs(FakeFs::default(), tmp.path().to_owned(), false);
    assert_eq!(persister.load(), Loaded::default());
    assert_eq!(persister.save(&sample()), SaveResult::Disabled);
    assert_eq!(persister.fs().calls(), Vec::<String>::new(), "no I/O");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "not even JSON");
    assert!(!tmp.path().join(PLAYBACK_BAD).exists());

    // On, the same file is read (and found corrupt).
    let on = Persister::with_fs(FakeFs::default(), tmp.path().to_owned(), true);
    assert!(on.load().message.is_some());
    assert!(!on.fs().calls().is_empty());
}

/// AC13's helper: `forget` deletes both files, and a missing one is fine.
#[test]
fn ac13_forget() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    forget(&RealFs, dir).unwrap();
    std::fs::write(dir.join(PLAYBACK_FILE), "{}").unwrap();
    std::fs::write(dir.join(PLAYBACK_BAD), "{}").unwrap();
    std::fs::write(dir.join("session.age"), "x").unwrap();
    forget(&RealFs, dir).unwrap();
    assert!(!dir.join(PLAYBACK_FILE).exists());
    assert!(!dir.join(PLAYBACK_BAD).exists());
    assert!(dir.join("session.age").exists(), "only the playback files");
}
