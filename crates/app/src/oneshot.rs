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

use crate::client::{Link, find};
use crate::ipc::codec::encode;
use crate::player_runtime::{EMPTY_DEVICE_NAME, parse_items};
use crate::ui::clock;

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
mod tests {
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::io;
    use std::rc::Rc;

    use clap::Parser;

    use super::*;
    use crate::ipc::client::RecvError;
    use tidal_player_core::protocol::{NowPlaying, QueueEntry};
    use tidal_player_core::{AlbumRef, ArtistRef, AudioQuality, EntryId, Item, Track, TrackId};

    #[derive(Debug, Parser)]
    struct Cli {
        #[command(subcommand)]
        command: PlaybackCommand,
    }

    fn parse(args: &[&str]) -> Result<Plan, String> {
        let cli = Cli::try_parse_from(std::iter::once("playback").chain(args.iter().copied()))
            .map_err(|e| e.kind().to_string())?;
        plan(&cli.command).map_err(|e| e.to_string())
    }

    fn request(command: Command) -> Result<Plan, String> {
        Ok(Plan::Request(command))
    }

    /// AC15: every row of the "One-shot commands" table maps to its
    /// message; bad numbers and items are refused (exit 2, nothing sent).
    #[test]
    fn ac15_parse() {
        const ALBUM: &str = "https://tidal.com/browse/album/10";
        let rows: Vec<(&[&str], Result<Plan, String>)> = vec![
            (&["play-pause"], request(Command::TogglePause)),
            (&["play"], request(Command::Play)),
            (&["pause"], request(Command::Pause)),
            (&["stop"], request(Command::Stop)),
            (&["shuffle", "on"], request(Command::SetShuffle(true))),
            (&["shuffle", "off"], request(Command::SetShuffle(false))),
            (
                &["repeat", "off"],
                request(Command::SetRepeat(RepeatMode::Off)),
            ),
            (
                &["repeat", "queue"],
                request(Command::SetRepeat(RepeatMode::Queue)),
            ),
            (
                &["repeat", "track"],
                request(Command::SetRepeat(RepeatMode::Track)),
            ),
            (&["next"], request(Command::Next)),
            (&["previous"], request(Command::Previous)),
            (
                &["seek", "90"],
                request(Command::SeekTo(Duration::from_secs(90))),
            ),
            (
                &["seek", "1.5"],
                request(Command::SeekTo(Duration::from_millis(1500))),
            ),
            (&["seek", "+5"], request(Command::SeekBy(5000))),
            (&["seek", "-2.5"], request(Command::SeekBy(-2500))),
            (&["volume", "80"], request(Command::SetVolume(80))),
            (&["volume", "0"], request(Command::SetVolume(0))),
            (&["volume", "100"], request(Command::SetVolume(100))),
            (&["volume", "+5"], request(Command::ChangeVolume(5))),
            (&["volume", "-100"], request(Command::ChangeVolume(-100))),
            (&["mute"], request(Command::ToggleMute)),
            (&["shuffle"], request(Command::ToggleShuffle)),
            (&["repeat"], request(Command::CycleRepeat)),
            (&["autoplay"], request(Command::ToggleAutoplay)),
            (
                &["load", ALBUM, "3"],
                request(Command::Open {
                    items: vec![Item::Album(10), Item::Track(TrackId(3))],
                    at: None,
                }),
            ),
            (
                &["add", "3"],
                request(Command::Open {
                    items: vec![Item::Track(TrackId(3))],
                    at: Some(InsertAt::End),
                }),
            ),
            (
                &["add", "--next", ALBUM],
                request(Command::Open {
                    items: vec![Item::Album(10)],
                    at: Some(InsertAt::Next),
                }),
            ),
            (&["status"], Ok(Plan::Status { json: false })),
            (&["status", "--json"], Ok(Plan::Status { json: true })),
            // Refused.
            (
                &["shuffle", "maybe"],
                Err("one of the values isn't valid for an argument".into()),
            ),
            (
                &["repeat", "all"],
                Err("one of the values isn't valid for an argument".into()),
            ),
            (
                &["volume", "101"],
                Err("Invalid volume \"101\": expected 0 to 100, +N or -N".into()),
            ),
            (
                &["volume", "+101"],
                Err("Invalid volume \"+101\": expected 0 to 100, +N or -N".into()),
            ),
            (
                &["volume", "loud"],
                Err("Invalid volume \"loud\": expected 0 to 100, +N or -N".into()),
            ),
            (
                &["seek", "x"],
                Err("Invalid seek \"x\": expected seconds, +S or -S".into()),
            ),
            (
                &["seek", "-"],
                Err("Invalid seek \"-\": expected seconds, +S or -S".into()),
            ),
            (
                &["seek", "inf"],
                Err("Invalid seek \"inf\": expected seconds, +S or -S".into()),
            ),
            (
                &["load", "https://tidal.com/browse/artist/1"],
                Err(
                    "Not a Tidal track, album or playlist: https://tidal.com/browse/artist/1"
                        .into(),
                ),
            ),
        ];
        for (args, want) in rows {
            assert_eq!(parse(args), want, "{args:?}");
        }
        // clap refuses these itself (exit 2): no item, an unknown command.
        for args in [&["load"][..], &["add", "--next"], &["nope"], &["seek"]] {
            assert!(parse(args).is_err(), "{args:?} accepted");
        }
    }

    fn track(id: u64, title: &str, artists: &[&str], album: &str, secs: Option<u64>) -> Track {
        Track {
            id: TrackId(id),
            title: title.into(),
            version: None,
            artists: artists
                .iter()
                .map(|a| ArtistRef {
                    id: 1,
                    name: (*a).into(),
                })
                .collect(),
            album: Some(AlbumRef {
                id: 1,
                title: album.into(),
                cover: None,
            }),
            duration: secs.map(Duration::from_secs),
            streamable: true,
        }
    }

    fn queue(n: u64) -> Vec<QueueEntry> {
        (1..=n)
            .map(|i| QueueEntry {
                id: EntryId(i),
                track: track(i, &format!("Track {i}"), &["Artist"], "Album", Some(200)),
                suggested: false,
            })
            .collect()
    }

    fn playing() -> PlayerSnapshot {
        let mut queue = queue(12);
        queue[1].track = track(
            2,
            "Hell Above",
            &["Pierce The Veil"],
            "Collide With The Sky",
            Some(212),
        );
        PlayerSnapshot {
            queue,
            current: Some(EntryId(2)),
            state: PlaybackState::Playing,
            position: Duration::from_secs(83),
            shuffle: true,
            repeat: RepeatMode::Queue,
            autoplay: false,
            volume: 80,
            muted: false,
            now_playing: Some(NowPlaying {
                quality: AudioQuality::Lossless,
                source: "FLAC 16-bit 44.1 kHz stereo".into(),
                output: "hw:1,0 S32_LE 44.1 kHz".into(),
                bit_perfect: false,
                bit_perfect_reason: Some("volume below 100%".into()),
                released: false,
            }),
            message: None,
            device: "default".into(),
        }
    }

    /// AC15: `status` prints the three lines (state symbol, indicators,
    /// queue position), `Nothing playing` alone without a current entry,
    /// and a fourth line for a message or an expired session.
    #[test]
    fn ac15_status_lines() {
        let paused_released = {
            let mut s = playing();
            s.state = PlaybackState::Paused;
            s.shuffle = false;
            s.repeat = RepeatMode::Off;
            s.autoplay = true;
            s.muted = true;
            if let Some(np) = s.now_playing.as_mut() {
                np.released = true;
            }
            s
        };
        let failed = {
            let mut s = paused_released.clone();
            s.message = Some("Output hw:1,0 is busy (used by firefox)".into());
            s
        };
        let empty = PlayerSnapshot {
            queue: Vec::new(),
            current: None,
            state: PlaybackState::Stopped,
            position: Duration::ZERO,
            now_playing: None,
            message: None,
            ..playing()
        };
        let mut unknown = playing();
        unknown.queue[1].track.duration = None;
        let rows: Vec<(&str, PlayerSnapshot, bool, &str)> = vec![
            (
                "playing",
                playing(),
                false,
                "▶ Hell Above · Pierce The Veil · Collide With The Sky\n\
                 1:23 / 3:32 · shuffle · repeat: queue · 80% · default\n\
                 Queue: 2 of 12\n",
            ),
            (
                "paused, released",
                paused_released,
                false,
                "⏸ Hell Above · Pierce The Veil · Collide With The Sky\n\
                 1:23 / 3:32 · autoplay · muted · default · device released\n\
                 Queue: 2 of 12\n",
            ),
            (
                "a message",
                failed,
                false,
                "⏸ Hell Above · Pierce The Veil · Collide With The Sky\n\
                 1:23 / 3:32 · autoplay · muted · default · device released\n\
                 Queue: 2 of 12\n\
                 Output hw:1,0 is busy (used by firefox)\n",
            ),
            (
                "unknown duration",
                unknown,
                false,
                "▶ Hell Above · Pierce The Veil · Collide With The Sky\n\
                 1:23 / ?:?? · shuffle · repeat: queue · 80% · default\n\
                 Queue: 2 of 12\n",
            ),
            ("nothing playing", empty.clone(), false, "Nothing playing\n"),
            (
                "session expired",
                empty,
                true,
                "Nothing playing\nSession expired: run \"tidal-player login\"\n",
            ),
            (
                "session expired while playing",
                playing(),
                true,
                "▶ Hell Above · Pierce The Veil · Collide With The Sky\n\
                 1:23 / 3:32 · shuffle · repeat: queue · 80% · default\n\
                 Queue: 2 of 12\n\
                 Session expired: run \"tidal-player login\"\n",
            ),
        ];
        for (name, snapshot, login_required, want) in rows {
            assert_eq!(status_lines(&snapshot, login_required), want, "{name}");
        }
    }

    /// A link that records what it was sent and answers from a script
    /// (then: nothing in time).
    struct Scripted {
        sent: Rc<RefCell<Vec<ClientMessage>>>,
        answers: VecDeque<Result<ServerMessage, RecvError>>,
    }

    impl Link for Scripted {
        fn send(&mut self, message: &ClientMessage) -> io::Result<()> {
            self.sent.borrow_mut().push(message.clone());
            Ok(())
        }

        fn recv(&mut self, _: Option<Duration>) -> Result<Option<ServerMessage>, RecvError> {
            self.answers.pop_front().transpose()
        }

        fn close(&mut self) {}
    }

    fn exec(
        plan: &Plan,
        answers: Vec<Result<ServerMessage, RecvError>>,
    ) -> (u8, String, String, Vec<ClientMessage>) {
        let sent = Rc::default();
        let mut link = Scripted {
            sent: Rc::clone(&sent),
            answers: answers.into(),
        };
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = execute(
            &mut link,
            plan,
            Duration::from_millis(50),
            &mut out,
            &mut err,
        );
        let sent = sent.borrow().clone();
        (
            code,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
            sent,
        )
    }

    /// AC15 (with a fake link; the binary's exit codes are in
    /// `tests/daemon.rs`): `Ok` → exit 0 and nothing printed; `Err` → its
    /// message on stderr, exit 1; no reply in time → exit 1; `status`
    /// subscribes and prints the `Welcome`, as lines or as one JSON line.
    #[test]
    fn ac15_execute() {
        let next = Plan::Request(Command::Next);
        let reply = |result| Ok(ServerMessage::Reply { id: 0, result });
        let asked = vec![ClientMessage::Request {
            id: 0,
            command: Command::Next,
        }];
        assert_eq!(
            exec(&next, vec![reply(Ok(()))]),
            (0, String::new(), String::new(), asked.clone())
        );
        assert_eq!(
            exec(&next, vec![reply(Err("Album 404 was not found".into()))]),
            (
                1,
                String::new(),
                "Album 404 was not found\n".into(),
                asked.clone()
            )
        );
        assert_eq!(
            exec(&next, vec![]),
            (1, String::new(), format!("{NO_ANSWER}\n"), asked.clone())
        );
        let (code, _, err, _) = exec(&next, vec![Err(RecvError::Closed)]);
        assert_eq!((code, err.is_empty()), (1, false));

        let welcome = ServerMessage::Welcome {
            snapshot: playing(),
            login_required: false,
        };
        let (code, out, err, sent) = exec(&Plan::Status { json: false }, vec![Ok(welcome.clone())]);
        assert_eq!((code, err.as_str()), (0, ""));
        assert_eq!(out, status_lines(&playing(), false));
        assert_eq!(sent, vec![ClientMessage::Subscribe]);
        let (code, out, _, _) = exec(&Plan::Status { json: true }, vec![Ok(welcome.clone())]);
        assert_eq!(code, 0);
        assert_eq!(out.lines().count(), 1, "{out}");
        assert_eq!(
            serde_json::from_str::<ServerMessage>(out.trim_end()).ok(),
            Some(welcome)
        );
        assert_eq!(
            exec(&Plan::Status { json: false }, vec![]),
            (
                1,
                String::new(),
                format!("{NO_ANSWER}\n"),
                vec![ClientMessage::Subscribe]
            )
        );
    }

    /// 0014 AC10: `device` lists, `device NAME` sends `SetDevice(NAME)`,
    /// an empty name is refused before anything is sent (exit 2); the
    /// list is printed like `tidal-player devices` with `*` on the
    /// player's device, a player's error is exit 1.
    #[test]
    fn ac10_parse_device() {
        use tidal_player_core::protocol::DeviceEntry;

        assert_eq!(parse(&["device"]), Ok(Plan::Devices));
        assert_eq!(
            parse(&["device", "hw:1,0"]),
            request(Command::SetDevice("hw:1,0".into()))
        );
        assert_eq!(
            parse(&["device", "plughw:1,0"]),
            request(Command::SetDevice("plughw:1,0".into()))
        );
        assert_eq!(parse(&["device", ""]), Err("Device name is empty".into()));
        assert!(parse(&["device", "a", "b"]).is_err(), "two names accepted");

        // Switching: the player's answer is the exit code.
        let set = Plan::Request(Command::SetDevice("hw:9,0".into()));
        let reply = |result| Ok(ServerMessage::Reply { id: 0, result });
        let asked = vec![ClientMessage::Request {
            id: 0,
            command: Command::SetDevice("hw:9,0".into()),
        }];
        assert_eq!(
            exec(&set, vec![reply(Ok(()))]),
            (0, String::new(), String::new(), asked.clone())
        );
        assert_eq!(
            exec(&set, vec![reply(Err("Device name is empty".into()))]),
            (1, String::new(), "Device name is empty\n".into(), asked)
        );

        // Listing: `*` on the player's device (from its snapshot), the
        // answer in either order, events before them skipped.
        let entry = |name: &str, description: &str| DeviceEntry {
            name: name.into(),
            description: description.into(),
        };
        let list = vec![
            entry("default", "shared, through the system mixer"),
            entry("hw:0,0", "HDA Intel PCH: ALC892 Analog"),
            entry("hw:1,0", "E30 II: USB Audio"),
        ];
        let mut snapshot = playing();
        snapshot.device = "hw:1,0".into();
        let welcome = ServerMessage::Welcome {
            snapshot: snapshot.clone(),
            login_required: false,
        };
        let devices = ServerMessage::DevicesReply {
            id: 0,
            result: Ok(list.clone()),
        };
        let want = "  default  shared, through the system mixer\n  \
                    hw:0,0   HDA Intel PCH: ALC892 Analog\n\
                    * hw:1,0   E30 II: USB Audio\n";
        for answers in [
            vec![Ok(welcome.clone()), Ok(devices.clone())],
            vec![
                Ok(devices.clone()),
                Ok(ServerMessage::Event(
                    tidal_player_core::protocol::Event::Player(playing()),
                )),
                Ok(welcome.clone()),
            ],
        ] {
            let (code, out, err, sent) = exec(&Plan::Devices, answers);
            assert_eq!((code, out.as_str(), err.as_str()), (0, want, ""));
            assert_eq!(
                sent,
                vec![ClientMessage::Devices { id: 0 }, ClientMessage::Subscribe]
            );
        }
        // A selected device the list does not have: printed first, marked,
        // `not found`.
        let mut missing = snapshot;
        missing.device = "plughw:1,0".into();
        let (code, out, _, _) = exec(
            &Plan::Devices,
            vec![
                Ok(ServerMessage::Welcome {
                    snapshot: missing,
                    login_required: false,
                }),
                Ok(devices),
            ],
        );
        assert_eq!(code, 0);
        assert_eq!(out.lines().next(), Some("* plughw:1,0  not found"), "{out}");
        // The player cannot list: its message, exit 1.
        let (code, out, err, _) = exec(
            &Plan::Devices,
            vec![
                Ok(welcome),
                Ok(ServerMessage::DevicesReply {
                    id: 0,
                    result: Err("cannot read /proc/asound/cards: denied".into()),
                }),
            ],
        );
        assert_eq!(
            (code, out.as_str(), err.as_str()),
            (
                1,
                "",
                "Cannot list devices: cannot read /proc/asound/cards: denied\n"
            )
        );
        // No answer in time.
        let (code, _, err, _) = exec(&Plan::Devices, vec![]);
        assert_eq!((code, err), (1, format!("{NO_ANSWER}\n")));
    }

    /// 0014 AC10: `status`'s second line has the selected device after the
    /// volume (before `device released`).
    #[test]
    fn ac10_status_device_line() {
        let second = |s: &PlayerSnapshot| status_lines(s, false).lines().nth(1).map(str::to_owned);
        let mut dac = playing();
        dac.device = "hw:1,0".into();
        assert_eq!(
            second(&dac).as_deref(),
            Some("1:23 / 3:32 · shuffle · repeat: queue · 80% · hw:1,0")
        );
        dac.muted = true;
        if let Some(np) = dac.now_playing.as_mut() {
            np.released = true;
        }
        assert_eq!(
            second(&dac).as_deref(),
            Some("1:23 / 3:32 · shuffle · repeat: queue · muted · hw:1,0 · device released")
        );
        // Nothing playing: one line, as before.
        let idle = PlayerSnapshot {
            current: None,
            ..playing()
        };
        assert_eq!(status_lines(&idle, false), "Nothing playing\n");
    }
}
