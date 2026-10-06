//! Panic hook that restores the terminal before the panic report is printed.

use std::panic;

/// Installs a panic hook that runs `restore` and then the previously
/// installed hook (which prints the panic message), so the message lands on
/// the normal screen instead of being lost in raw mode / the alternate screen.
pub fn install_panic_hook(restore: impl Fn() + Send + Sync + 'static) {
    let previous = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        restore();
        previous(info);
    }));
}
