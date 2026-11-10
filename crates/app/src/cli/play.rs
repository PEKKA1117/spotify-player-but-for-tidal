//! `tidal-player play` and `tidal-player devices` (spec 0003 "Commands",
//! "Settings", "Edge cases & errors"; AC25, AC26): settings, the status
//! lines and the user-facing error messages, kept pure so they are tested
//! without a terminal, a session or a sound card.

use std::collections::HashMap;
use std::io::{self, Write};
use std::path::Path;
use std::sync::mpsc::Receiver;
use std::time::Duration;

use tidal_player_api::auth::AuthError;
use tidal_player_api::stream::StreamError;
use tidal_player_audio::devices::{PlaybackDevice, parse_devices};
use tidal_player_audio::{
    Codec, EngineError, Event, OutputInfo, OutputKind, SampleFormat, SinkError, SourceError,
    SourceFormat,
};
use tidal_player_core::player::{Failure, FailureKind};
use tidal_player_core::protocol::{
    Command, Event as PlayerEvent, PlaybackState, PlayerSnapshot, RepeatMode,
};
use tidal_player_core::{AudioQuality, Item, Track};

use crate::logging::events::{self, Shutdown};
use crate::player::runtime::{
    EngineControl, ExpandError, Handled, Jobs, Metadata, PlayerRuntime, RuntimeInput, expand_items,
};

/// Highest quality to ask for (spec 0003 "Settings").
/// The playback devices under `dir` (`cards` and `pcm`), read now. A
/// missing file means no card (no ALSA, or a container): `default` only.
/// Any other read error is the `Err` (spec 0014 AC7).
pub fn read_devices(dir: &Path) -> Result<Vec<PlaybackDevice>, String> {
    let read = |name: &str| {
        let path = dir.join(name);
        match std::fs::read_to_string(&path) {
            Ok(text) => Ok(text),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(String::new()),
            Err(e) => Err(format!("cannot read {}: {e}", path.display())),
        }
    };
    Ok(parse_devices(&read("cards")?, &read("pcm")?))
}

/// `Track 77640617: HI_RES_LOSSLESS, FLAC 24-bit 96 kHz stereo`.
pub fn track_line(track_id: u64, granted: AudioQuality, source: &SourceFormat) -> String {
    format!(
        "Track {track_id}: {granted}, {}",
        source_description(source)
    )
}

/// `FLAC 24-bit 96 kHz stereo`: the "Track" line after the quality.
pub fn source_description(source: &SourceFormat) -> String {
    let codec = match source.codec {
        Codec::Flac => "FLAC",
        Codec::AacLc => "AAC",
    };
    let bits = source
        .bits_per_sample
        .map(|b| format!(" {b}-bit"))
        .unwrap_or_default();
    let channels = match source.channels {
        1 => "mono".to_owned(),
        2 => "stereo".to_owned(),
        n => format!("{n} ch"),
    };
    format!("{codec}{bits} {} {channels}", khz(source.sample_rate))
}

/// `Output: hw:1,0 (exclusive) S32_LE 96 kHz 2 ch, bit-perfect`.
pub fn output_line(output: &OutputInfo) -> String {
    let quality = match (&output.not_bit_perfect_reason, output.bit_perfect) {
        (_, true) => "bit-perfect",
        (Some(reason), false) => reason.as_str(),
        (None, false) => "not bit-perfect",
    };
    format!("Output: {}, {quality}", output_description(output))
}

/// `hw:1,0 (exclusive) S32_LE 96 kHz 2 ch`: the "Output" line without the
/// verdict.
pub fn output_description(output: &OutputInfo) -> String {
    let kind = match output.kind {
        OutputKind::Exclusive => "exclusive",
        OutputKind::Fallback => "fallback",
        OutputKind::Shared => "shared",
    };
    let format = match output.sample_format {
        SampleFormat::S16Le => "S16_LE",
        SampleFormat::S24Le => "S24_LE",
        SampleFormat::S24_3Le => "S24_3LE",
        SampleFormat::S32Le => "S32_LE",
    };
    format!(
        "{} ({kind}) {format} {} {} ch",
        output.device,
        khz(output.sample_rate),
        output.channels
    )
}

/// `96 kHz`, `44.1 kHz`, `22.05 kHz`.
fn khz(rate: u32) -> String {
    let whole = rate / 1000;
    let fraction = format!("{:03}", rate % 1000);
    let fraction = fraction.trim_end_matches('0');
    if fraction.is_empty() {
        format!("{whole} kHz")
    } else {
        format!("{whole}.{fraction} kHz")
    }
}

/// `  1:23 / 4:56`; the duration is `?:??` when the stream does not say.
pub fn progress_line(position: Duration, duration: Option<Duration>) -> String {
    let duration = duration.map_or_else(|| "?:??".to_owned(), clock_time);
    format!("  {} / {duration}", clock_time(position))
}

/// `m:ss`, or `h:mm:ss` from an hour on (whole seconds, rounded down).
fn clock_time(t: Duration) -> String {
    let s = t.as_secs();
    let (h, m, s) = (s / 3600, s / 60 % 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// The one stderr line for a resolution failure (spec 0003 "Edge cases &
/// errors"); `country` is the session's.
pub fn stream_error_message(track_id: u64, country: &str, error: &StreamError) -> String {
    match error {
        StreamError::PreviewOnly => {
            format!("Track {track_id} is only available as a preview for this account")
        }
        StreamError::NotAvailable => format!("Track {track_id} is not available in {country}"),
        StreamError::NotFound => {
            format!("Track {track_id} was not found, or cannot be streamed in {country}")
        }
        StreamError::Unsupported(what) => format!("Track {track_id} is not playable: {what}"),
        other => capitalise(&other.to_string()),
    }
}

/// The one stderr line for a failed track.
pub fn engine_error_message(track_id: u64, error: &EngineError) -> String {
    match error {
        EngineError::Output(SinkError::Busy { device, holder }) => {
            let holder = holder
                .as_deref()
                .map(|h| format!(" (used by {h})"))
                .unwrap_or_default();
            format!("Output {device} is busy{holder}: close it, or use --device default")
        }
        EngineError::Output(SinkError::NotFound(device)) => {
            format!("No such output device {device}: see \"tidal-player devices\"")
        }
        EngineError::Output(SinkError::Lost(device)) => format!("Output {device} was lost"),
        EngineError::Output(other) => format!("Output error: {other}"),
        EngineError::Source(SourceError::Network(_)) => {
            format!("Network error while streaming track {track_id}")
        }
        EngineError::Source(other) => format!("Error while streaming track {track_id}: {other}"),
        EngineError::Decode(what) => format!("Track {track_id} could not be decoded: {what}"),
        EngineError::Unsupported(what) => format!("Track {track_id} is not playable: {what}"),
    }
}

fn capitalise(message: &str) -> String {
    let mut chars = message.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

/// How a played track ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Played to its end (exit 0).
    Ended,
    /// Failed, with the stderr line (exit 1).
    Failed(String),
}

/// Turns engine events into `play`'s output: the two status lines at
/// `Started`, then the progress line, redrawn in place, only on a terminal.
#[derive(Debug)]
pub struct Reporter {
    track_id: u64,
    granted: AudioQuality,
    duration: Option<Duration>,
    tty: bool,
    progress_shown: bool,
}

impl Reporter {
    pub fn new(
        track_id: u64,
        granted: AudioQuality,
        duration: Option<Duration>,
        tty: bool,
    ) -> Self {
        Self {
            track_id,
            granted,
            duration,
            tty,
            progress_shown: false,
        }
    }

    /// Prints what `event` shows; `Some` once the track is over.
    pub fn on_event(&mut self, event: &Event, out: &mut dyn Write) -> io::Result<Option<Outcome>> {
        match event {
            Event::Started { source, output, .. } | Event::Transitioned { source, output, .. } => {
                self.finish(out)?;
                writeln!(out, "{}", track_line(self.track_id, self.granted, source))?;
                writeln!(out, "{}", output_line(output))?;
                self.progress(Duration::ZERO, out)?;
            }
            Event::Position(position) => self.progress(*position, out)?,
            Event::TrackEnded { .. } => return Ok(Some(Outcome::Ended)),
            Event::Error { error, .. } => {
                return Ok(Some(Outcome::Failed(engine_error_message(
                    self.track_id,
                    error,
                ))));
            }
            _ => {}
        }
        out.flush()?;
        Ok(None)
    }

    /// Redraws the progress line, on a terminal only.
    fn progress(&mut self, position: Duration, out: &mut dyn Write) -> io::Result<()> {
        if self.tty {
            write!(out, "\r\x1b[K{}", progress_line(position, self.duration))?;
            self.progress_shown = true;
        }
        Ok(())
    }

    /// Ends the progress line, if one was drawn.
    pub fn finish(&mut self, out: &mut dyn Write) -> io::Result<()> {
        if std::mem::take(&mut self.progress_shown) {
            writeln!(out)?;
        }
        out.flush()
    }
}

// --- spec 0004: failures ----------------------------------------------------------

/// Classifies a resolution failure (spec 0004 "Failures") with 0003's
/// message. Anything not listed as track-only stops (never skips).
pub fn stream_failure(track_id: u64, country: &str, error: &StreamError) -> Failure {
    let kind = match error {
        StreamError::NotFound
        | StreamError::NotAvailable
        | StreamError::PreviewOnly
        | StreamError::Unsupported(_) => FailureKind::TrackOnly,
        StreamError::Auth(AuthError::LoginRequired | AuthError::Unauthorized) => {
            FailureKind::Session
        }
        StreamError::Server(_) | StreamError::Malformed(_) | StreamError::Auth(_) => {
            FailureKind::Transient
        }
    };
    Failure {
        kind,
        message: stream_error_message(track_id, country, error),
    }
}

/// Classifies an engine (or source opening) failure with 0003's message.
pub fn engine_failure(track_id: u64, error: &EngineError) -> Failure {
    let kind = match error {
        EngineError::Decode(_) | EngineError::Unsupported(_) => FailureKind::TrackOnly,
        EngineError::Output(_) => FailureKind::Output,
        EngineError::Source(_) => FailureKind::Transient,
    };
    Failure {
        kind,
        message: engine_error_message(track_id, error),
    }
}

// --- spec 0004: `play` with a queue -----------------------------------------------------

/// `play`'s new flags, applied to the player before the queue loads.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlayOptions {
    pub shuffle: bool,
    pub repeat: RepeatMode,
    /// `--autoplay` (the environment is in the player's config).
    pub autoplay: bool,
    /// `--start`, for the first track.
    pub start_at: Duration,
}

impl PlayOptions {
    /// Whether a flag 0003's `play` did not have is given.
    pub fn has_new_flag(&self) -> bool {
        self.shuffle || self.repeat != RepeatMode::Off || self.autoplay
    }
}

/// The queue `play` loads: the items expanded in order. One track ID with
/// no new flag and no hook is queued as 0003 played it, without a metadata
/// request (its errors are 0003's, from the stream resolution); a hook needs
/// the track's title (spec 0025).
pub async fn play_tracks(
    meta: &dyn Metadata,
    items: &[Item],
    options: &PlayOptions,
    hooked: bool,
) -> Result<Vec<Track>, ExpandError> {
    if let [Item::Track(id)] = items
        && !options.has_new_flag()
        && !hooked
    {
        return Ok(vec![Track {
            id: *id,
            title: String::new(),
            version: None,
            artists: Vec::new(),
            album: None,
            duration: None,
            streamable: true,
        }]);
    }
    expand_items(meta, items).await
}

/// Where `play` writes.
pub struct PlayOutput<'a> {
    pub out: &'a mut dyn Write,
    pub err: &'a mut dyn Write,
    /// Whether `out` is a terminal (the progress line is drawn only there).
    pub tty: bool,
}

/// Plays `tracks` as one queue through `runtime` and returns the exit code
/// (spec 0004 "Commands"): `0` when the queue ran out and at least one track
/// played to its end, `1` when it stopped on a failure or nothing played,
/// `130` once `interrupted` says so (the engine is stopped first).
pub fn play_queue<E: EngineControl, J: Jobs>(
    runtime: &mut PlayerRuntime<E, J>,
    tracks: Vec<Track>,
    options: &PlayOptions,
    inputs: &Receiver<RuntimeInput>,
    poll: Duration,
    interrupted: &mut dyn FnMut() -> bool,
    output: &mut PlayOutput<'_>,
) -> u8 {
    let mut reporter = QueueReporter::new(output.tty);
    let mut commands = Vec::new();
    if options.shuffle {
        commands.push(Command::ToggleShuffle);
    }
    let mut repeat = RepeatMode::Off;
    while repeat != options.repeat {
        commands.push(Command::CycleRepeat);
        repeat = repeat.cycled();
    }
    commands.push(Command::LoadQueue { tracks, start: 0 });
    if !options.start_at.is_zero() {
        commands.push(Command::SeekTo(options.start_at));
    }
    for command in commands {
        let handled = runtime.handle(RuntimeInput::Command(command));
        reporter.report(&handled, output);
    }
    loop {
        // The player stops for good only once nothing can restart it.
        let snapshot = runtime.snapshot();
        if snapshot.state == PlaybackState::Stopped && !runtime.suggestions_pending() {
            reporter.finish(output);
            events::shutting_down(Shutdown::Quit);
            runtime.hook_shutdown();
            return reporter.exit_code(&snapshot);
        }
        let input = loop {
            if interrupted() {
                events::shutting_down(Shutdown::Signal);
                runtime.handle(RuntimeInput::Command(Command::Shutdown));
                reporter.finish(output);
                return 130;
            }
            if let Some(input) = runtime.next_input(inputs, poll) {
                break input;
            }
        };
        let handled = runtime.handle(input);
        reporter.report(&handled, output);
    }
}

/// Turns what the runtime did into `play`'s output: the "Track" and
/// "Output" lines at each track start, the progress line on a terminal,
/// each new message on stderr; and decides the exit code.
#[derive(Debug)]
struct QueueReporter {
    tty: bool,
    progress_shown: bool,
    duration: Option<Duration>,
    message: Option<String>,
    kinds: HashMap<String, FailureKind>,
    played_to_end: bool,
}

impl QueueReporter {
    fn new(tty: bool) -> Self {
        Self {
            tty,
            progress_shown: false,
            duration: None,
            message: None,
            kinds: HashMap::new(),
            played_to_end: false,
        }
    }

    /// Writes what `handled` shows; write errors (a closed pipe) are
    /// ignored, playback goes on.
    fn report(&mut self, handled: &Handled, output: &mut PlayOutput<'_>) {
        let _ = self.try_report(handled, output);
    }

    fn try_report(&mut self, handled: &Handled, output: &mut PlayOutput<'_>) -> io::Result<()> {
        for failure in &handled.failures {
            self.kinds.insert(failure.message.clone(), failure.kind);
        }
        self.played_to_end |= handled.ended;
        // A failure the player acted on (it answered with a snapshot) is
        // shown even when the player replaced its message in the same step
        // (with the run-limit summary).
        let mut shown = None;
        if !handled.events.is_empty()
            && let Some(failure) = handled.failures.last()
        {
            self.end_progress(output.out)?;
            writeln!(output.err, "{}", failure.message)?;
            shown = Some(failure.message.as_str());
        }
        if let Some(start) = &handled.started {
            self.end_progress(output.out)?;
            writeln!(
                output.out,
                "{}",
                track_line(start.track.0, start.quality, &start.source)
            )?;
            writeln!(output.out, "{}", output_line(&start.output))?;
            self.duration = start.duration;
            self.progress(Duration::ZERO, output.out)?;
        }
        for event in &handled.events {
            match event {
                PlayerEvent::Position { position, .. } => self.progress(*position, output.out)?,
                PlayerEvent::Player(snapshot) if snapshot.message != self.message => {
                    self.message.clone_from(&snapshot.message);
                    let Some(message) = &snapshot.message else {
                        continue;
                    };
                    // The failure shown just now is not printed twice.
                    if Some(message.as_str()) != shown {
                        self.end_progress(output.out)?;
                        writeln!(output.err, "{message}")?;
                    }
                }
                _ => {}
            }
        }
        output.out.flush()
    }

    fn progress(&mut self, position: Duration, out: &mut dyn Write) -> io::Result<()> {
        if self.tty {
            write!(out, "\r\x1b[K{}", progress_line(position, self.duration))?;
            self.progress_shown = true;
        }
        Ok(())
    }

    fn end_progress(&mut self, out: &mut dyn Write) -> io::Result<()> {
        if std::mem::take(&mut self.progress_shown) {
            writeln!(out)?;
        }
        Ok(())
    }

    fn finish(&mut self, output: &mut PlayOutput<'_>) {
        let _ = self.end_progress(output.out);
        let _ = output.out.flush();
    }

    /// The exit code once the player stopped for good.
    fn exit_code(&self, snapshot: &PlayerSnapshot) -> u8 {
        let kind = snapshot
            .message
            .as_ref()
            .and_then(|m| self.kinds.get(m).copied());
        if kind.is_some_and(|k| k != FailureKind::TrackOnly) {
            return 1;
        }
        let last = snapshot.queue.last().map(|e| e.id);
        let ran_out = snapshot.repeat == RepeatMode::Off
            && snapshot.current.is_some()
            && snapshot.current == last;
        if ran_out && self.played_to_end { 0 } else { 1 }
    }
}

#[cfg(test)]
mod tests;
