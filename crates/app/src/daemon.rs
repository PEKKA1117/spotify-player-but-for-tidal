//! `tidal-player daemon` (spec 0005 "The daemon"): what the headless
//! player adds to a player: telling systemd it is ready, `SIGTERM`/`SIGINT`
//! as a `Shutdown`, `daemon stop` and the systemd user unit.

use std::io;
use std::os::unix::net::{SocketAddr, UnixDatagram};
use std::path::Path;
use std::process::ExitCode;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use tidal_player_core::protocol::{ClientMessage, Command, ServerMessage};

use crate::client::{Link, find};
use crate::ipc::lock::{self, Probe};
use crate::oneshot::NO_ANSWER;
use crate::player_runtime::RuntimeInput;

/// The variable systemd sets for `Type=notify` services.
pub const NOTIFY_SOCKET_VAR: &str = "NOTIFY_SOCKET";

/// How long `daemon stop` waits for the reply, then for the lock.
pub const STOP_TIMEOUT: Duration = Duration::from_secs(5);

/// The systemd user unit, starting `exe daemon` (spec 0005 "systemd user
/// service"). Exit 1, 2 and 3 are not restarted: a missing login, bad
/// settings or another player are not fixed by trying again (tidalt #8).
pub fn unit(exe: &Path) -> String {
    let exe = exe.display().to_string();
    // systemd splits `ExecStart` on whitespace unless quoted.
    let exe = if exe.contains(char::is_whitespace) {
        format!("\"{exe}\"")
    } else {
        exe
    };
    format!(
        "[Unit]
Description=tidal-player daemon
Documentation=https://github.com/PEKKA1117/spotify-player-but-for-tidal/blob/main/docs/daemon.md
StartLimitIntervalSec=300
StartLimitBurst=5

[Service]
Type=notify
ExecStart={exe} daemon
Restart=on-failure
RestartSec=5
# 1: not logged in or the session store is unusable; 2: bad settings;
# 3: another player is running. Restarting fixes none of them.
RestartPreventExitStatus=1 2 3
# Settings (docs/playback.md#settings), e.g.:
#Environment=TIDAL_PLAYER_DEVICE=hw:1,0
# Passphrase for an encrypted session file (docs/login.md):
#LoadCredential=tidal-player-passphrase:%h/.config/tidal-player/passphrase

[Install]
WantedBy=default.target
"
    )
}

/// Sends `READY=1` to `$NOTIFY_SOCKET` (a path, or `@name` for an abstract
/// socket), when it is set; `Ok(false)` when it is not.
pub fn notify_ready(env: impl Fn(&str) -> Option<String>) -> io::Result<bool> {
    let Some(target) = env(NOTIFY_SOCKET_VAR).filter(|t| !t.is_empty()) else {
        return Ok(false);
    };
    let socket = UnixDatagram::unbound()?;
    match target.strip_prefix('@') {
        Some(name) => {
            use std::os::linux::net::SocketAddrExt;
            let address = SocketAddr::from_abstract_name(name.as_bytes())?;
            socket.send_to_addr(b"READY=1", &address)?;
        }
        None => {
            socket.send_to(b"READY=1", &target)?;
        }
    }
    Ok(true)
}

/// Turns `SIGTERM` and `SIGINT` into a `Shutdown` for the player. The
/// handlers are in place when this returns.
pub fn forward_signals(
    runtime: &tokio::runtime::Handle,
    inputs: Sender<RuntimeInput>,
) -> io::Result<()> {
    use tokio::signal::unix::{SignalKind, signal};
    let _entered = runtime.enter();
    let mut term = signal(SignalKind::terminate())?;
    let mut int = signal(SignalKind::interrupt())?;
    runtime.spawn(async move {
        tokio::select! {
            _ = term.recv() => {}
            _ = int.recv() => {}
        }
        let _ = inputs.send(RuntimeInput::Command(Command::Shutdown));
    });
    Ok(())
}

/// `tidal-player daemon stop`: asks the player to shut down and waits
/// until its lock is free (at most [`STOP_TIMEOUT`] for each).
pub fn stop() -> ExitCode {
    let (mut link, dir) = match find(|key| std::env::var(key).ok()) {
        Ok(found) => found,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(1);
        }
    };
    let request = ClientMessage::Request {
        id: 0,
        command: Command::Shutdown,
    };
    if let Err(e) = Link::send(&mut link, &request) {
        eprintln!("{e}");
        return ExitCode::from(1);
    }
    // The reply, or the player closing the connection as it goes.
    let deadline = Instant::now() + STOP_TIMEOUT;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match Link::recv(&mut link, Some(left)) {
            Ok(Some(ServerMessage::Reply { id: 0, .. })) | Err(_) => break,
            Ok(Some(_)) => {}
            Ok(None) => {
                eprintln!("{NO_ANSWER}");
                return ExitCode::from(1);
            }
        }
    }
    Link::close(&mut link);
    let deadline = Instant::now() + STOP_TIMEOUT;
    while Instant::now() < deadline {
        match lock::probe(&dir) {
            Ok(Probe::Free) => return ExitCode::SUCCESS,
            Ok(Probe::Held { .. }) => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => {
                eprintln!("{e}");
                return ExitCode::from(1);
            }
        }
    }
    eprintln!("The player did not shut down within {STOP_TIMEOUT:?}");
    ExitCode::from(1)
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
