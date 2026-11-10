//! `tidal-player playback <command>` (spec 0005 "One-shot commands"): one
//! request to the running player, its answer printed, then exit. The
//! parsing and the `status` lines are pure; [`run`] finds the player.

use std::io::Write;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use tidal_player_core::protocol::{
    ClientMessage, Command, InsertAt, PlaybackState, PlayerSnapshot, RepeatMode, ServerMessage,
};

use tidal_player_audio::devices::{PlaybackDevice, format_devices};
use tidal_player_core::protocol::DeviceEntry;
use tidal_player_core::ui::{DeviceList, device_rows};

use crate::client::{Link, find, locate};
use crate::ipc::client::Connection;
use crate::ipc::codec::encode;
use crate::player::runtime::{EMPTY_DEVICE_NAME, parse_items};
use crate::tui::render::clock;

/// How long a one-shot command waits for the player's answer.
pub const REPLY_TIMEOUT: Duration = Duration::from_secs(5);

/// What a one-shot command says when the player is silent.
pub const NO_ANSWER: &str = "The player did not answer";

/// The argument of `shuffle`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Switch {
    On,
    Off,
}

/// The argument of `repeat`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum RepeatArg {
    Off,
    Queue,
    Track,
}

/// The `playback` subcommands.
#[derive(Debug, Clone, PartialEq, Eq, clap::Subcommand)]
pub enum PlaybackCommand {
    /// Play or pause.
    PlayPause,
    /// Start or resume playing.
    Play,
    /// Pause.
    Pause,
    /// Stop, keeping the current entry at 0:00.
    Stop,
    /// Skip to the next entry.
    Next,
    /// Back to the start, or to the previous entry.
    Previous,
    /// Seek: S seconds into the track, or +S / -S from here.
    Seek {
        #[arg(value_name = "S", allow_hyphen_values = true)]
        position: String,
    },
    /// Set the volume to N % (0-100), or change it by +N / -N.
    Volume {
        #[arg(value_name = "N", allow_hyphen_values = true)]
        level: String,
    },
    /// Mute or unmute.
    Mute,
    /// Shuffle: on or off; without an argument, toggle.
    Shuffle {
        #[arg(value_enum, value_name = "on|off")]
        state: Option<Switch>,
    },
    /// Repeat: off, queue, track; without an argument, cycle.
    Repeat {
        #[arg(value_enum, value_name = "off|queue|track")]
        mode: Option<RepeatArg>,
    },
    /// Autoplay on or off.
    Autoplay,
    /// Replace the queue with these items and play the first.
    Load {
        #[arg(value_name = "ITEM", required = true)]
        items: Vec<String>,
    },
    /// Add items at the end of the queue (or after the current entry).
    Add {
        /// Right after the current entry.
        #[arg(long)]
        next: bool,
        #[arg(value_name = "ITEM", required = true)]
        items: Vec<String>,
    },
    /// The running player's output devices (`*`: the one it uses), or
    /// switch it to NAME (an ALSA PCM name, as "tidal-player devices" lists).
    Device {
        #[arg(value_name = "NAME")]
        name: Option<String>,
    },
    /// Print what is playing.
    Status {
        /// The player's state as one JSON line.
        #[arg(long)]
        json: bool,
    },
}

/// What one command sends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plan {
    Request(Command),
    /// `Subscribe`, print the `Welcome`, leave.
    Status {
        json: bool,
    },
    /// `Devices` and `Subscribe`, print the list with `*` on the player's
    /// device, leave.
    Devices,
}

/// A bad argument or item (exit 2, nothing sent).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UsageError {
    #[error("Invalid seek {0:?}: expected seconds, +S or -S")]
    Seek(String),
    #[error("Invalid volume {0:?}: expected 0 to 100, +N or -N")]
    Volume(String),
    #[error("{0}")]
    Item(String),
    #[error("{EMPTY_DEVICE_NAME}")]
    EmptyDevice,
}

/// The message `command` sends.
pub fn plan(command: &PlaybackCommand) -> Result<Plan, UsageError> {
    let command = match command {
        PlaybackCommand::PlayPause => Command::TogglePause,
        PlaybackCommand::Next => Command::Next,
        PlaybackCommand::Previous => Command::Previous,
        PlaybackCommand::Seek { position } => seek(position)?,
        PlaybackCommand::Volume { level } => volume(level)?,
        PlaybackCommand::Mute => Command::ToggleMute,
        PlaybackCommand::Play => Command::Play,
        PlaybackCommand::Pause => Command::Pause,
        PlaybackCommand::Stop => Command::Stop,
        PlaybackCommand::Shuffle { state: None } => Command::ToggleShuffle,
        PlaybackCommand::Shuffle { state: Some(state) } => {
            Command::SetShuffle(*state == Switch::On)
        }
        PlaybackCommand::Repeat { mode: None } => Command::CycleRepeat,
        PlaybackCommand::Repeat { mode: Some(mode) } => Command::SetRepeat(match mode {
            RepeatArg::Off => RepeatMode::Off,
            RepeatArg::Queue => RepeatMode::Queue,
            RepeatArg::Track => RepeatMode::Track,
        }),
        PlaybackCommand::Autoplay => Command::ToggleAutoplay,
        PlaybackCommand::Load { items } => Command::Open {
            items: items_of(items)?,
            at: None,
        },
        PlaybackCommand::Add { next, items } => Command::Open {
            items: items_of(items)?,
            at: Some(if *next { InsertAt::Next } else { InsertAt::End }),
        },
        PlaybackCommand::Status { json } => return Ok(Plan::Status { json: *json }),
        PlaybackCommand::Device { name: None } => return Ok(Plan::Devices),
        PlaybackCommand::Device { name: Some(name) } if name.is_empty() => {
            return Err(UsageError::EmptyDevice);
        }
        PlaybackCommand::Device { name: Some(name) } => Command::SetDevice(name.clone()),
    };
    Ok(Plan::Request(command))
}

fn items_of(args: &[String]) -> Result<Vec<tidal_player_core::Item>, UsageError> {
    parse_items(args).map_err(|e| UsageError::Item(e.to_string()))
}

/// `S` → `SeekTo`, `+S`/`-S` → `SeekBy`, in seconds (fractions allowed).
fn seek(text: &str) -> Result<Command, UsageError> {
    let bad = || UsageError::Seek(text.to_owned());
    let (sign, number) = split_sign(text);
    let seconds: f64 = number.parse().map_err(|_| bad())?;
    // Finite, not negative, and small enough for milliseconds in an i64.
    if !seconds.is_finite() || !(0.0..=1e12).contains(&seconds) {
        return Err(bad());
    }
    let millis = (seconds * 1000.0).round() as i64;
    Ok(match sign {
        None => Command::SeekTo(Duration::from_millis(millis as u64)),
        Some(true) => Command::SeekBy(millis),
        Some(false) => Command::SeekBy(-millis),
    })
}

/// `N` → `SetVolume`, `+N`/`-N` → `ChangeVolume`; `N` 0–100.
fn volume(text: &str) -> Result<Command, UsageError> {
    let bad = || UsageError::Volume(text.to_owned());
    let (sign, number) = split_sign(text);
    if !number.bytes().all(|b| b.is_ascii_digit()) {
        return Err(bad());
    }
    let n: u8 = number.parse().map_err(|_| bad())?;
    if n > 100 {
        return Err(bad());
    }
    let change = i8::try_from(n).map_err(|_| bad())?;
    Ok(match sign {
        None => Command::SetVolume(n),
        Some(true) => Command::ChangeVolume(change),
        Some(false) => Command::ChangeVolume(-change),
    })
}

/// A leading `+` (`Some(true)`) or `-` (`Some(false)`), and the rest.
fn split_sign(text: &str) -> (Option<bool>, &str) {
    if let Some(rest) = text.strip_prefix('+') {
        (Some(true), rest)
    } else if let Some(rest) = text.strip_prefix('-') {
        (Some(false), rest)
    } else {
        (None, text)
    }
}

/// `status`: three lines (`Nothing playing` alone without a current
/// entry), then the message or the expired session, if any.
pub fn status_lines(snapshot: &PlayerSnapshot, login_required: bool) -> String {
    let mut lines = Vec::new();
    let current = snapshot
        .current
        .and_then(|id| snapshot.queue.iter().position(|e| e.id == id));
    match current {
        None => lines.push("Nothing playing".to_owned()),
        Some(index) => {
            let track = &snapshot.queue[index].track;
            let symbol = match snapshot.state {
                PlaybackState::Playing => "▶",
                PlaybackState::Paused => "⏸",
                PlaybackState::Loading | PlaybackState::Buffering => "…",
                PlaybackState::Stopped => "■",
            };
            let mut first = vec![format!("{symbol} {}", track.title)];
            if !track.artists.is_empty() {
                first.push(track.artist_names());
            }
            if let Some(album) = track.album_title().filter(|a| !a.is_empty()) {
                first.push(album.to_owned());
            }
            lines.push(first.join(" · "));

            let duration = track.duration.map_or_else(|| "?:??".to_owned(), clock);
            let mut second = vec![format!("{} / {duration}", clock(snapshot.position))];
            if snapshot.shuffle {
                second.push("shuffle".to_owned());
            }
            match snapshot.repeat {
                RepeatMode::Off => {}
                RepeatMode::Queue => second.push("repeat: queue".to_owned()),
                RepeatMode::Track => second.push("repeat: track".to_owned()),
            }
            if snapshot.autoplay {
                second.push("autoplay".to_owned());
            }
            second.push(if snapshot.muted {
                "muted".to_owned()
            } else {
                format!("{}%", snapshot.volume)
            });
            if !snapshot.device.is_empty() {
                second.push(snapshot.device.clone());
            }
            if snapshot.now_playing.as_ref().is_some_and(|np| np.released) {
                second.push("device released".to_owned());
            }
            lines.push(second.join(" · "));
            lines.push(format!("Queue: {} of {}", index + 1, snapshot.queue.len()));
        }
    }
    if login_required {
        lines.push("Session expired: run \"tidal-player login\"".to_owned());
    } else if let Some(message) = &snapshot.message {
        lines.push(message.clone());
    }
    lines.iter().map(|line| format!("{line}\n")).collect()
}

/// Sends `plan` over `link` and prints the answer: the exit code.
pub fn execute<L: Link>(
    link: &mut L,
    plan: &Plan,
    timeout: Duration,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8 {
    let messages = match plan {
        Plan::Request(command) => vec![ClientMessage::Request {
            id: 0,
            command: command.clone(),
        }],
        Plan::Status { .. } => vec![ClientMessage::Subscribe],
        // The list, and the snapshot for the player's device.
        Plan::Devices => vec![ClientMessage::Devices { id: 0 }, ClientMessage::Subscribe],
    };
    for message in &messages {
        if let Err(e) = link.send(message) {
            let _ = writeln!(err, "{e}");
            return 1;
        }
    }
    let deadline = Instant::now() + timeout;
    let mut devices: Option<Vec<DeviceEntry>> = None;
    let mut selected: Option<String> = None;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        let answer = match link.recv(Some(left)) {
            Ok(Some(answer)) => answer,
            Ok(None) => {
                let _ = writeln!(err, "{NO_ANSWER}");
                return 1;
            }
            Err(e) => {
                let _ = writeln!(err, "{e}");
                return 1;
            }
        };
        match (plan, answer) {
            (Plan::Request(_), ServerMessage::Reply { id: 0, result }) => {
                return match result {
                    Ok(()) => 0,
                    Err(message) => {
                        let _ = writeln!(err, "{message}");
                        1
                    }
                };
            }
            (Plan::Status { json }, welcome @ ServerMessage::Welcome { .. }) => {
                let written = if *json {
                    out.write_all(&encode(&welcome))
                } else if let ServerMessage::Welcome {
                    snapshot,
                    login_required,
                } = &welcome
                {
                    out.write_all(status_lines(snapshot, *login_required).as_bytes())
                } else {
                    Ok(())
                };
                return u8::from(written.and_then(|()| out.flush()).is_err());
            }
            (Plan::Devices, ServerMessage::DevicesReply { id: 0, result }) => match result {
                Ok(list) => devices = Some(list),
                Err(message) => {
                    let _ = writeln!(err, "Cannot list devices: {message}");
                    return 1;
                }
            },
            (Plan::Devices, ServerMessage::Welcome { snapshot, .. }) => {
                selected = Some(snapshot.device);
            }
            // Anything else (an event before the `Welcome`) is not the answer.
            _ => {}
        }
        if let (Some(list), Some(selected)) = (&devices, &selected) {
            let written = out.write_all(device_lines(list, selected).as_bytes());
            return u8::from(written.and_then(|()| out.flush()).is_err());
        }
    }
}

/// `playback device`: the player's list as `tidal-player devices` prints
/// it, `*` on the player's `selected` device, which comes first as `not
/// found` when the list does not have it (spec 0014 "Listing devices").
pub fn device_lines(list: &[DeviceEntry], selected: &str) -> String {
    let rows: Vec<PlaybackDevice> = device_rows(&DeviceList::Loaded(list.to_vec()), Some(selected))
        .into_iter()
        .map(|row| PlaybackDevice {
            name: row.name,
            description: row.description,
        })
        .collect();
    format_devices(&rows, selected)
}

/// How long `tidal-player devices` waits for a running player's
/// `Welcome` before treating it as absent (spec 0014 AC13).
pub const PLAYER_DEVICE_TIMEOUT: Duration = Duration::from_secs(1);

/// `tidal-player devices`: the local `list`, `*` on the running `player`'s
/// selected device and `  (configured)` after the `configured` device's row
/// when it differs; with no player, `*` on the configured device as before
/// (spec 0014 AC13).
pub fn local_device_lines(
    list: &[PlaybackDevice],
    player: Option<&str>,
    configured: &str,
) -> String {
    let Some(player) = player else {
        return format_devices(list, configured);
    };
    let player = normalise(player);
    let configured = normalise(configured);
    let entries: Vec<DeviceEntry> = list
        .iter()
        .map(|d| DeviceEntry {
            name: d.name.clone(),
            description: d.description.clone(),
        })
        .collect();
    let rows: Vec<PlaybackDevice> = device_rows(&DeviceList::Loaded(entries), Some(&player))
        .into_iter()
        .map(|row| PlaybackDevice {
            name: row.name,
            description: row.description,
        })
        .collect();
    let text = format_devices(&rows, &player);
    rows.iter()
        .zip(text.lines())
        .map(|(row, line)| {
            if row.name == configured && configured != player {
                format!("{line}  (configured)\n")
            } else {
                format!("{line}\n")
            }
        })
        .collect()
}

/// `hw:C` names the card's first device, `hw:C,0`, as `format_devices`
/// compares names.
fn normalise(device: &str) -> String {
    match device.strip_prefix("hw:") {
        Some(card) if !card.contains(',') => format!("hw:{card},0"),
        _ => device.to_owned(),
    }
}

/// The running player's selected device, when one answers with its
/// `Welcome` within `timeout`; `None` otherwise (spec 0014 AC13).
/// The whole exchange runs on its own thread so a socket that accepts but
/// never greets cannot hold `devices` past `timeout`.
pub fn player_device(timeout: Duration) -> Option<String> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(ask_player_device(timeout));
    });
    rx.recv_timeout(timeout).ok().flatten()
}

/// One connection attempt (no lock probe, no retry): `Subscribe`, then the
/// `Welcome`'s device.
fn ask_player_device(timeout: Duration) -> Option<String> {
    let deadline = Instant::now() + timeout;
    let (_, socket) = locate(|key| std::env::var(key).ok()).ok()?;
    let mut link = Connection::connect(&socket).ok()?;
    let device = (|| {
        Link::send(&mut link, &ClientMessage::Subscribe).ok()?;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if let ServerMessage::Welcome { snapshot, .. } =
                Link::recv(&mut link, Some(left)).ok()??
            {
                return Some(snapshot.device);
            }
        }
    })();
    Link::close(&mut link);
    device
}

/// `tidal-player playback <command>`.
pub fn run(command: &PlaybackCommand) -> ExitCode {
    let plan = match plan(command) {
        Ok(plan) => plan,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    let mut link = match find(|key| std::env::var(key).ok()) {
        Ok((link, _)) => link,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(1);
        }
    };
    let code = execute(
        &mut link,
        &plan,
        REPLY_TIMEOUT,
        &mut std::io::stdout(),
        &mut std::io::stderr(),
    );
    Link::close(&mut link);
    ExitCode::from(code)
}

#[cfg(test)]
mod tests;
