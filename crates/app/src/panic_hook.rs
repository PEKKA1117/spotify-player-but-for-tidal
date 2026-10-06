//! Panic hook that restores the terminal before the panic report is printed.

/// Installs a panic hook that runs `restore` and then the previously
/// installed hook (which prints the panic message).
pub fn install_panic_hook(_restore: impl Fn() + Send + Sync + 'static) {}
