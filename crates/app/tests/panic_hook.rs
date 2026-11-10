//! AC10: the terminal is restored before the panic message is reported.
//! Own test binary because panic hooks are process-global.

use std::panic::{catch_unwind, set_hook};
use std::sync::{Arc, Mutex};

use tidal_player::logging::{self, Mode};
use tidal_player::panic_hook::install_panic_hook;
use tidal_player_core::log::LogSetting;

/// The hooks are process-global, so the tests of this binary take turns.
static HOOKS: Mutex<()> = Mutex::new(());

#[test]
fn ac10_restore_runs_before_report() {
    let _turn = HOOKS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let log = Arc::new(Mutex::new(Vec::<&'static str>::new()));

    let previous = Arc::clone(&log);
    set_hook(Box::new(move |_| previous.lock().unwrap().push("report")));

    let restore = Arc::clone(&log);
    install_panic_hook(move || restore.lock().unwrap().push("restore"));

    let _ = catch_unwind(|| panic!("boom"));

    // Copy out first: a failing assert panics into the hook above, which locks
    // the log again, so the guard must not be alive during the assert.
    let seen = log.lock().unwrap().clone();
    assert_eq!(seen, ["restore", "report"]);
}

/// 0027 AC18: with a log installed, the hook writes the panic's line to the
/// file and the buffer before `restore` and the previous hook run, in that
/// order; 0001 AC10's order (restore, then report) still holds.
#[test]
fn ac18_panic_is_logged_first() {
    let _turn = HOOKS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = tempfile::tempdir().unwrap();
    let handle = logging::install(Mode::Standalone, LogSetting::Info, dir.path()).unwrap();
    let file = dir.path().join(logging::LOG_FILE);

    let seen = Arc::new(Mutex::new(Vec::<String>::new()));
    // What the log holds at this moment, for the step that asks.
    let snapshot = {
        let handle = handle.clone();
        move |step: &str| {
            let text = std::fs::read_to_string(&file).unwrap_or_default();
            let in_file = text.contains("panic location=") && text.contains("message=boom");
            let in_buffer = handle
                .batch(None)
                .lines
                .iter()
                .any(|l| l.text.contains("message=boom"));
            format!("{step} file={in_file} buffer={in_buffer}")
        }
    };

    let report = Arc::clone(&seen);
    let at_report = snapshot.clone();
    std::panic::set_hook(Box::new(move |_| {
        report.lock().unwrap().push(at_report("report"));
    }));
    let restore = Arc::clone(&seen);
    install_panic_hook(move || restore.lock().unwrap().push(snapshot("restore")));

    let _ = std::panic::catch_unwind(|| panic!("boom"));

    let seen = seen.lock().unwrap().clone();
    assert_eq!(
        seen,
        [
            "restore file=true buffer=true",
            "report file=true buffer=true"
        ]
    );
    let lines = handle.batch(None).lines;
    let line = lines.last().expect("the panic line");
    assert_eq!(line.level, tidal_player_core::log::LogLevel::Error);
    assert!(
        line.text.starts_with("panic location=") && line.text.contains("panic_hook.rs:"),
        "{}",
        line.text
    );
}
