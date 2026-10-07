//! `tidal-player play` and `tidal-player devices` (spec 0003 "Commands",
//! "Settings", "Edge cases & errors"; AC25, AC26): settings, the status
//! lines and the user-facing error messages, kept pure so they are tested
//! without a terminal, a session or a sound card.

use std::io::{self, Write};
use std::time::Duration;

use tidal_player_api::stream::StreamError;
use tidal_player_audio::{
    Codec, EngineError, Event, OutputInfo, OutputKind, SampleFormat, SinkError, SourceError,
    SourceFormat,
};
use tidal_player_core::{AudioQuality, ParseQualityError};

/// Highest quality to ask for (spec 0003 "Settings").
pub const QUALITY_VAR: &str = "TIDAL_PLAYER_QUALITY";
/// Output device.
pub const DEVICE_VAR: &str = "TIDAL_PLAYER_DEVICE";
/// Where `devices` reads `cards` and `pcm` from instead of `/proc/asound`.
pub const ASOUND_DIR_VAR: &str = "TIDAL_PLAYER_ASOUND_DIR";
/// The default output device (decision 2: shared, never takes the card).
pub const DEFAULT_DEVICE: &str = "default";
/// The default highest quality.
pub const DEFAULT_QUALITY: AudioQuality = AudioQuality::HiResLossless;

/// What `play` uses: flag, then environment, then default.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub quality: AudioQuality,
    pub device: String,
}

/// A setting with a value `play` cannot use (exit 2).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid {setting}: {message}")]
pub struct SettingsError {
    /// `--quality` or the variable's name.
    pub setting: String,
    pub message: String,
}

/// Resolves the settings; `env` reads an environment variable (an empty
/// value counts as unset).
pub fn resolve_settings(
    quality_flag: Option<&str>,
    device_flag: Option<&str>,
    env: impl Fn(&str) -> Option<String>,
) -> Result<Settings, SettingsError> {
    let quality = match quality_flag {
        Some(flag) => parse_quality(flag, "--quality")?,
        None => match non_empty(env(QUALITY_VAR)) {
            Some(value) => parse_quality(&value, QUALITY_VAR)?,
            None => DEFAULT_QUALITY,
        },
    };
    let device = match device_flag {
        Some(flag) => flag.to_owned(),
        None => configured_device(env),
    };
    Ok(Settings { quality, device })
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|v| !v.is_empty())
}

fn parse_quality(value: &str, setting: &str) -> Result<AudioQuality, SettingsError> {
    value.parse().map_err(|e: ParseQualityError| SettingsError {
        setting: setting.to_owned(),
        message: e.to_string(),
    })
}

/// The device `play` would use without `--device`, for the `*` of
/// `devices`.
pub fn configured_device(env: impl Fn(&str) -> Option<String>) -> String {
    non_empty(env(DEVICE_VAR)).unwrap_or_else(|| DEFAULT_DEVICE.into())
}

/// `Track 77640617: HI_RES_LOSSLESS, FLAC 24-bit 96 kHz stereo`.
pub fn track_line(track_id: u64, granted: AudioQuality, source: &SourceFormat) -> String {
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
    format!(
        "Track {track_id}: {granted}, {codec}{bits} {} {channels}",
        khz(source.sample_rate)
    )
}

/// `Output: hw:1,0 (exclusive) S32_LE 96 kHz 2 ch, bit-perfect`.
pub fn output_line(output: &OutputInfo) -> String {
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
    let quality = match (&output.not_bit_perfect_reason, output.bit_perfect) {
        (_, true) => "bit-perfect",
        (Some(reason), false) => reason.as_str(),
        (None, false) => "not bit-perfect",
    };
    format!(
        "Output: {} ({kind}) {format} {} {} ch, {quality}",
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

#[cfg(test)]
mod tests {
    use super::*;
    use tidal_player_api::auth::AuthError;
    use tidal_player_api::stream::Unsupported;

    #[test]
    fn ac26_settings_precedence() {
        type Row = (
            &'static str,
            Option<&'static str>,
            Option<&'static str>,
            &'static [(&'static str, &'static str)],
            Result<(AudioQuality, &'static str), &'static str>,
        );
        let rows: &[Row] = &[
            (
                "defaults",
                None,
                None,
                &[],
                Ok((AudioQuality::HiResLossless, "default")),
            ),
            (
                "environment",
                None,
                None,
                &[(QUALITY_VAR, "high"), (DEVICE_VAR, "hw:1,0")],
                Ok((AudioQuality::High, "hw:1,0")),
            ),
            (
                "flags beat environment",
                Some("lossless"),
                Some("hw:2,0"),
                &[(QUALITY_VAR, "high"), (DEVICE_VAR, "hw:1,0")],
                Ok((AudioQuality::Lossless, "hw:2,0")),
            ),
            (
                "empty environment counts as unset",
                None,
                None,
                &[(QUALITY_VAR, ""), (DEVICE_VAR, "")],
                Ok((AudioQuality::HiResLossless, "default")),
            ),
            (
                "flag beats a bad environment value",
                Some("hi-res"),
                None,
                &[(QUALITY_VAR, "ultra")],
                Ok((AudioQuality::HiResLossless, "default")),
            ),
            ("unknown flag", Some("ultra"), None, &[], Err("--quality")),
            ("low flag", Some("low"), None, &[], Err("--quality")),
            (
                "unknown environment",
                None,
                None,
                &[(QUALITY_VAR, "ultra")],
                Err(QUALITY_VAR),
            ),
            (
                "low environment",
                None,
                None,
                &[(QUALITY_VAR, "low")],
                Err(QUALITY_VAR),
            ),
        ];
        for (name, quality, device, env, want) in rows {
            let lookup = |key: &str| {
                env.iter()
                    .find(|(k, _)| *k == key)
                    .map(|(_, v)| (*v).to_string())
            };
            let got = resolve_settings(*quality, *device, lookup);
            match want {
                Ok((q, d)) => assert_eq!(
                    got,
                    Ok(Settings {
                        quality: *q,
                        device: (*d).into()
                    }),
                    "{name}"
                ),
                Err(setting) => {
                    let err = got.expect_err(name);
                    assert_eq!(err.setting, *setting, "{name}");
                    if quality == &Some("low") || env.iter().any(|(_, v)| *v == "low") {
                        assert!(err.to_string().contains("HE-AAC"), "{name}: {err}");
                    }
                }
            }
        }
        assert_eq!(configured_device(|_| None), "default");
        assert_eq!(
            configured_device(|k| (k == DEVICE_VAR).then(|| "hw:1,0".into())),
            "hw:1,0"
        );
    }

    const FLAC_24_96: SourceFormat = SourceFormat {
        codec: Codec::Flac,
        sample_rate: 96_000,
        channels: 2,
        bits_per_sample: Some(24),
    };
    const FLAC_16_44: SourceFormat = SourceFormat {
        codec: Codec::Flac,
        sample_rate: 44_100,
        channels: 2,
        bits_per_sample: Some(16),
    };
    const FLAC_MONO: SourceFormat = SourceFormat {
        codec: Codec::Flac,
        sample_rate: 48_000,
        channels: 1,
        bits_per_sample: Some(16),
    };
    const AAC: SourceFormat = SourceFormat {
        codec: Codec::AacLc,
        sample_rate: 44_100,
        channels: 2,
        bits_per_sample: None,
    };

    fn output(
        device: &str,
        kind: OutputKind,
        format: SampleFormat,
        rate: u32,
        reason: Option<&str>,
    ) -> OutputInfo {
        OutputInfo {
            requested: device.replace("plughw", "hw"),
            device: device.into(),
            kind,
            sample_format: format,
            sample_rate: rate,
            channels: 2,
            bit_perfect: reason.is_none(),
            not_bit_perfect_reason: reason.map(Into::into),
        }
    }

    #[test]
    fn ac26_status_lines() {
        let tracks = [
            (
                AudioQuality::HiResLossless,
                FLAC_24_96,
                "Track 77640617: HI_RES_LOSSLESS, FLAC 24-bit 96 kHz stereo",
            ),
            (
                AudioQuality::Lossless,
                FLAC_16_44,
                "Track 77640617: LOSSLESS, FLAC 16-bit 44.1 kHz stereo",
            ),
            (
                AudioQuality::Lossless,
                FLAC_MONO,
                "Track 77640617: LOSSLESS, FLAC 16-bit 48 kHz mono",
            ),
            // Asked for hi-res, granted less: the line shows the grant.
            (
                AudioQuality::High,
                AAC,
                "Track 77640617: HIGH, AAC 44.1 kHz stereo",
            ),
        ];
        for (granted, source, want) in tracks {
            assert_eq!(track_line(77_640_617, granted, &source), want);
        }

        let outputs = [
            (
                output(
                    "hw:1,0",
                    OutputKind::Exclusive,
                    SampleFormat::S32Le,
                    96_000,
                    None,
                ),
                "Output: hw:1,0 (exclusive) S32_LE 96 kHz 2 ch, bit-perfect",
            ),
            (
                output(
                    "hw:1,0",
                    OutputKind::Exclusive,
                    SampleFormat::S24_3Le,
                    44_100,
                    None,
                ),
                "Output: hw:1,0 (exclusive) S24_3LE 44.1 kHz 2 ch, bit-perfect",
            ),
            (
                output(
                    "plughw:1,0",
                    OutputKind::Fallback,
                    SampleFormat::S24Le,
                    96_000,
                    Some(
                        "resampled (plughw fallback: device refused S24_3LE/S24_LE/S32_LE at 96 kHz)",
                    ),
                ),
                "Output: plughw:1,0 (fallback) S24_LE 96 kHz 2 ch, resampled (plughw fallback: \
                 device refused S24_3LE/S24_LE/S32_LE at 96 kHz)",
            ),
            (
                output(
                    "default",
                    OutputKind::Shared,
                    SampleFormat::S16Le,
                    48_000,
                    Some("shared (system mixer)"),
                ),
                "Output: default (shared) S16_LE 48 kHz 2 ch, shared (system mixer)",
            ),
            (
                output(
                    "hw:1,0",
                    OutputKind::Exclusive,
                    SampleFormat::S32Le,
                    44_100,
                    Some("lossy source"),
                ),
                "Output: hw:1,0 (exclusive) S32_LE 44.1 kHz 2 ch, lossy source",
            ),
        ];
        for (info, want) in outputs {
            assert_eq!(output_line(&info), want);
        }

        let progress = [
            (
                Duration::from_secs(83),
                Some(Duration::from_secs(296)),
                "  1:23 / 4:56",
            ),
            (
                Duration::ZERO,
                Some(Duration::from_millis(291_008)),
                "  0:00 / 4:51",
            ),
            (Duration::from_millis(5_999), None, "  0:05 / ?:??"),
            (
                Duration::from_secs(3_725),
                Some(Duration::from_secs(4_000)),
                "  1:02:05 / 1:06:40",
            ),
        ];
        for (position, duration, want) in progress {
            assert_eq!(progress_line(position, duration), want);
        }
    }

    #[test]
    fn ac26_error_messages() {
        let stream = [
            (
                StreamError::PreviewOnly,
                "Track 123 is only available as a preview for this account",
            ),
            (
                StreamError::NotAvailable,
                "Track 123 is not available in NO",
            ),
            (
                StreamError::NotFound,
                "Track 123 was not found, or cannot be streamed in NO",
            ),
            (
                StreamError::Unsupported(Unsupported::Codec("mp4a.40.5".into())),
                "Track 123 is not playable: codec mp4a.40.5 is not supported",
            ),
            (
                StreamError::Auth(AuthError::LoginRequired),
                "Session expired: run \"tidal-player login\"",
            ),
            (StreamError::Server(500), "Tidal server error (HTTP 500)"),
        ];
        for (error, want) in stream {
            assert_eq!(stream_error_message(123, "NO", &error), want, "{error:?}");
        }

        let engine = [
            (
                EngineError::Output(SinkError::Busy {
                    device: "hw:1,0".into(),
                    holder: Some("PipeWire".into()),
                }),
                "Output hw:1,0 is busy (used by PipeWire): close it, or use --device default",
            ),
            (
                EngineError::Output(SinkError::Busy {
                    device: "hw:1,0".into(),
                    holder: None,
                }),
                "Output hw:1,0 is busy: close it, or use --device default",
            ),
            (
                EngineError::Output(SinkError::NotFound("hw:5,0".into())),
                "No such output device hw:5,0: see \"tidal-player devices\"",
            ),
            (
                EngineError::Output(SinkError::Lost("hw:1,0".into())),
                "Output hw:1,0 was lost",
            ),
            (
                EngineError::Source(SourceError::Network("timed out".into())),
                "Network error while streaming track 123",
            ),
            (
                EngineError::Unsupported("HE-AAC".into()),
                "Track 123 is not playable: HE-AAC",
            ),
        ];
        for (error, want) in engine {
            assert_eq!(engine_error_message(123, &error), want, "{error:?}");
        }
    }

    fn started() -> Event {
        Event::Started {
            tag: 0,
            source: FLAC_24_96,
            output: output(
                "hw:1,0",
                OutputKind::Exclusive,
                SampleFormat::S32Le,
                96_000,
                None,
            ),
        }
    }

    fn report(tty: bool, events: &[Event]) -> (String, Option<Outcome>) {
        let mut reporter = Reporter::new(
            77_640_617,
            AudioQuality::HiResLossless,
            Some(Duration::from_secs(296)),
            tty,
        );
        let mut out = Vec::new();
        let mut outcome = None;
        for event in events {
            if let Some(o) = reporter.on_event(event, &mut out).unwrap() {
                outcome = Some(o);
                break;
            }
        }
        reporter.finish(&mut out).unwrap();
        (String::from_utf8(out).unwrap(), outcome)
    }

    #[test]
    fn ac26_progress_only_on_terminal() {
        let events = [
            started(),
            Event::Position(Duration::from_secs(1)),
            Event::Position(Duration::from_secs(83)),
            Event::TrackEnded { tag: 0 },
        ];
        let lines = "Track 77640617: HI_RES_LOSSLESS, FLAC 24-bit 96 kHz stereo\n\
                     Output: hw:1,0 (exclusive) S32_LE 96 kHz 2 ch, bit-perfect\n";

        let (out, outcome) = report(false, &events);
        assert_eq!(out, lines);
        assert_eq!(outcome, Some(Outcome::Ended));

        let (out, outcome) = report(true, &events);
        assert_eq!(
            out,
            format!("{lines}\r\x1b[K  0:00 / 4:56\r\x1b[K  0:01 / 4:56\r\x1b[K  1:23 / 4:56\n")
        );
        assert_eq!(outcome, Some(Outcome::Ended));

        let lost = EngineError::Output(SinkError::Lost("hw:1,0".into()));
        let (out, outcome) = report(
            false,
            &[
                started(),
                Event::Error {
                    tag: 0,
                    error: lost,
                },
            ],
        );
        assert_eq!(out, lines);
        assert_eq!(
            outcome,
            Some(Outcome::Failed("Output hw:1,0 was lost".into()))
        );
    }
}
