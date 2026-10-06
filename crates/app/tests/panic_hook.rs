//! AC10: the terminal is restored before the panic message is reported.
//! Own test binary because panic hooks are process-global.

use std::panic::{catch_unwind, set_hook};
use std::sync::{Arc, Mutex};

use tidal_player::panic_hook::install_panic_hook;

#[test]
fn ac10_restore_runs_before_report() {
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
