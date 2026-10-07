//! `tidal-player daemon` (spec 0005 "The daemon"): what the headless
//! player adds to a player: telling systemd it is ready, `SIGTERM`/`SIGINT`
//! as a `Shutdown`, `daemon stop` and the systemd user unit.

use std::io;
use std::path::Path;
use std::process::ExitCode;
use std::sync::mpsc::Sender;

use crate::player_runtime::RuntimeInput;

/// The variable systemd sets for `Type=notify` services.
pub const NOTIFY_SOCKET_VAR: &str = "NOTIFY_SOCKET";

/// The systemd user unit, starting `exe daemon` (spec 0005 "systemd user
/// service").
pub fn unit(exe: &Path) -> String {
    let _ = exe;
    String::new()
}

/// Sends `READY=1` to `$NOTIFY_SOCKET` (a path, or `@name` for an abstract
/// socket), when it is set; `Ok(false)` when it is not.
pub fn notify_ready(env: impl Fn(&str) -> Option<String>) -> io::Result<bool> {
    let _ = env(NOTIFY_SOCKET_VAR);
    Ok(false)
}

/// Turns `SIGTERM` and `SIGINT` into a `Shutdown` for the player.
pub fn forward_signals(
    runtime: &tokio::runtime::Handle,
    inputs: Sender<RuntimeInput>,
) -> io::Result<()> {
    let _ = (runtime, inputs);
    Ok(())
}

/// `tidal-player daemon stop`: asks the player to shut down and waits
/// until its lock is free.
pub fn stop() -> ExitCode {
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    /// AC18: the unit under "systemd user service", with `ExecStart` the
    /// given executable.
    #[test]
    fn ac18_unit() {
        let text = unit(Path::new("/home/me/.cargo/bin/tidal-player"));
        for part in [
            "Type=notify",
            "RestartPreventExitStatus=1 2 3",
            "StartLimitBurst=5",
            "ExecStart=/home/me/.cargo/bin/tidal-player daemon\n",
            "Restart=on-failure",
            "WantedBy=default.target",
        ] {
            assert!(text.contains(part), "{part:?} missing:\n{text}");
        }
        insta::assert_snapshot!(text);
    }
}
