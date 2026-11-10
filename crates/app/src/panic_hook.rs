//! Panic hook that logs the panic, restores the terminal and then prints the
//! panic report.

use std::panic;

/// Installs a panic hook that writes the panic's line to the log (spec 0027
/// AC18; nothing without a log), runs `restore` and then the previously
/// installed hook (which prints the panic message), so the message lands on
/// the normal screen instead of being lost in raw mode / the alternate screen.
pub fn install_panic_hook(restore: impl Fn() + Send + Sync + 'static) {
    let previous = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        // First: the report may take the process down.
        crate::logging::log_panic(info);
        restore();
        previous(info);
    }));
}
