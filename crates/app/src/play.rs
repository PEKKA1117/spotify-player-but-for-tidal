//! `tidal-player play` and `tidal-player devices` (spec 0003 "Commands",
//! "Settings", "Edge cases & errors"; AC25, AC26): settings, the status
//! lines and the user-facing error messages, kept pure so they are tested
//! without a terminal, a session or a sound card.

use std::io::{self, Write};
use std::time::Duration;

use tidal_player_api::stream::StreamError;
use tidal_player_audio::{EngineError, Event, OutputInfo, SourceFormat};
use tidal_player_core::AudioQuality;

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
    let _ = (quality_flag, device_flag, env);
    Ok(Settings {
        quality: DEFAULT_QUALITY,
        device: DEFAULT_DEVICE.into(),
    })
}

/// The device `play` would use, for the `*` of `devices`.
pub fn configured_device(env: impl Fn(&str) -> Option<String>) -> String {
    let _ = env;
    DEFAULT_DEVICE.into()
}

/// `Track 77640617: HI_RES_LOSSLESS, FLAC 24-bit 96 kHz stereo`.
pub fn track_line(track_id: u64, granted: AudioQuality, source: &SourceFormat) -> String {
    let _ = (track_id, granted, source);
    String::new()
}

/// `Output: hw:1,0 (exclusive) S32_LE 96 kHz 2 ch, bit-perfect`.
pub fn output_line(output: &OutputInfo) -> String {
    let _ = output;
    String::new()
}

/// `  1:23 / 4:56`; the duration is `?:??` when the stream does not say.
pub fn progress_line(position: Duration, duration: Option<Duration>) -> String {
    let _ = (position, duration);
    String::new()
}

/// The one stderr line for a resolution failure.
pub fn stream_error_message(track_id: u64, country: &str, error: &StreamError) -> String {
    let _ = (track_id, country, error);
    String::new()
}

/// The one stderr line for a failed track.
pub fn engine_error_message(track_id: u64, error: &EngineError) -> String {
    let _ = (track_id, error);
    String::new()
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
        let _ = (
            event,
            out,
            self.track_id,
            self.granted,
            self.duration,
            self.tty,
        );
        Ok(None)
    }

    /// Ends the progress line, if one was drawn.
    pub fn finish(&mut self, out: &mut dyn Write) -> io::Result<()> {
        let _ = (out, self.progress_shown);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tidal_player_api::auth::AuthError;
    use tidal_player_api::stream::Unsupported;
    use tidal_player_audio::{Codec, OutputKind, SampleFormat, SinkError, SourceError};

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
            Event::TrackEnded,
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
        let (out, outcome) = report(false, &[started(), Event::Error(lost)]);
        assert_eq!(out, lines);
        assert_eq!(
            outcome,
            Some(Outcome::Failed("Output hw:1,0 was lost".into()))
        );
    }
}
