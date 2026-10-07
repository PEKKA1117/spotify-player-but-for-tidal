//! Opening an output: the [`PcmBackend`] seam over the ALSA calls, and the
//! decisions taken over it (spec 0003 "Output kinds", "Output (ALSA)"; AC12).

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::output::clock::Clock;
use crate::output::format::{choose_format, format_bits, format_name, preference};
use crate::output::reserve::{Reserver, reserve};
use crate::sink::{OutputInfo, OutputKind, SampleFormat, SinkError, SourceFormat};

/// Frames per device period.
pub const PERIOD_FRAMES: u32 = 1024;
/// Periods in the buffer for `hw:` and `plughw:`.
pub const EXCLUSIVE_PERIODS: u32 = 4;
/// Periods in the buffer through the system mixer.
pub const SHARED_PERIODS: u32 = 8;
/// The engine always outputs stereo.
pub const OUTPUT_CHANNELS: u16 = 2;
/// Wait between two opens of a device that answered `EBUSY`.
pub const BUSY_RETRY_INTERVAL: Duration = Duration::from_millis(100);
/// Give up on `EBUSY` once this much time has passed since the first open.
pub const BUSY_RETRY_BUDGET: Duration = Duration::from_millis(800);

/// Errors from a [`PcmBackend`], by what the decision logic does with them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PcmError {
    /// `EBUSY`: another application has the device open.
    Busy,
    /// `ENOENT`/`ENODEV` on open: no such device.
    NotFound,
    /// The device refused the hardware parameters (`EINVAL`).
    Refused(String),
    /// `EPIPE`: underrun. Recoverable.
    Underrun,
    /// `ESTRPIPE`: suspended. Recoverable.
    Suspended,
    /// `ENODEV`/`EIO` while open: the device went away.
    Lost,
    /// Anything else.
    Other(String),
}

/// Hardware parameters to set on an open device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HwConfig {
    pub format: SampleFormat,
    /// Exact rate (never "nearest").
    pub rate: u32,
    pub channels: u16,
    /// Whether ALSA may resample (`snd_pcm_hw_params_set_rate_resample`).
    pub resample: bool,
    pub period_frames: u32,
    pub periods: u32,
}

/// The thin seam over one ALSA PCM handle. At most one device is open at a
/// time; [`PcmBackend::open`] is only called when none is.
pub trait PcmBackend: Send {
    /// The index of the card named `card` (an ALSA card id such as `DAC`),
    /// for the reservation. Numeric cards are not asked.
    fn card_index(&mut self, card: &str) -> Option<u32>;
    /// Open the PCM `name` for blocking playback.
    fn open(&mut self, name: &str) -> Result<(), PcmError>;
    /// The sample formats the open device accepts with interleaved access,
    /// exactly `rate` and `channels`, and resampling on or off. Empty when
    /// the rate or channel count is refused. Sets nothing.
    fn accepted_formats(
        &mut self,
        rate: u32,
        channels: u16,
        resample: bool,
    ) -> Result<Vec<SampleFormat>, PcmError>;
    /// Install `config` (interleaved access) and prepare the device.
    fn configure(&mut self, config: &HwConfig) -> Result<(), PcmError>;
    /// Write whole frames packed in the configured format; returns the frames
    /// written. Blocks until there is room.
    fn write(&mut self, bytes: &[u8]) -> Result<usize, PcmError>;
    /// `snd_pcm_delay`: frames written but not yet played.
    fn delay(&self) -> Result<i64, PcmError>;
    /// Stop or restart playback, keeping the buffered frames.
    fn pause(&mut self, paused: bool) -> Result<(), PcmError>;
    /// `snd_pcm_drop`: discard the buffered frames.
    fn drop_frames(&mut self) -> Result<(), PcmError>;
    /// `snd_pcm_prepare`: make the device ready for writes again.
    fn prepare(&mut self) -> Result<(), PcmError>;
    /// `snd_pcm_drain`: play the buffered frames, then stop.
    fn drain(&mut self) -> Result<(), PcmError>;
    /// `snd_pcm_recover` after [`PcmError::Underrun`] or [`PcmError::Suspended`].
    fn recover(&mut self, error: &PcmError) -> Result<(), PcmError>;
    /// Close the device, if open. Idempotent.
    fn close(&mut self);
}

/// Devices whose exclusive negotiation was refused, for the rest of the
/// process: later opens go straight to `plughw:`. Clones share the set.
#[derive(Debug, Clone, Default)]
pub struct FallbackMemo(Arc<Mutex<HashSet<String>>>);

impl FallbackMemo {
    pub fn contains(&self, device: &str) -> bool {
        self.0.lock().is_ok_and(|set| set.contains(device))
    }

    pub fn insert(&self, device: &str) {
        if let Ok(mut set) = self.0.lock() {
            set.insert(device.to_owned());
        }
    }
}

/// The output kind a device name gets (spec 0003 "Output kinds").
pub fn output_kind(device: &str) -> OutputKind {
    if device.starts_with("hw:") {
        OutputKind::Exclusive
    } else if device.starts_with("plughw:") {
        OutputKind::Fallback
    } else {
        OutputKind::Shared
    }
}

/// Why the output is not bit-perfect, or `None` when it is (spec 0003
/// "Output kinds"): exclusive, lossless source, exact rate, stereo, and a
/// format that holds at least the source's bits.
pub fn not_bit_perfect_reason(
    kind: OutputKind,
    source: &SourceFormat,
    format: SampleFormat,
    rate: u32,
    channels: u16,
) -> Option<String> {
    let _ = (kind, source, format, rate, channels);
    None
}

/// A successfully opened output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Opened {
    pub info: OutputInfo,
    pub config: HwConfig,
    /// The card whose ReserveDevice1 name is held; release it on close.
    pub reserved: Option<u32>,
}

/// Open `device` for `source` (AC12). On error nothing is left open or
/// reserved.
pub fn open_output<B: PcmBackend + ?Sized>(
    backend: &mut B,
    reserver: &mut dyn Reserver,
    clock: &dyn Clock,
    memo: &FallbackMemo,
    device: &str,
    source: &SourceFormat,
) -> Result<Opened, SinkError> {
    let _ = (reserver, clock, memo, reserve, choose_format, preference);
    backend
        .open(device)
        .map_err(|e| SinkError::Backend(format!("{e:?}")))?;
    let config = HwConfig {
        format: SampleFormat::S32Le,
        rate: source.sample_rate,
        channels: OUTPUT_CHANNELS,
        resample: false,
        period_frames: PERIOD_FRAMES,
        periods: EXCLUSIVE_PERIODS,
    };
    backend
        .configure(&config)
        .map_err(|e| SinkError::Backend(format!("{e:?}")))?;
    let _ = (format_bits, format_name);
    Ok(Opened {
        info: OutputInfo {
            requested: device.into(),
            device: device.into(),
            kind: OutputKind::Exclusive,
            sample_format: config.format,
            sample_rate: config.rate,
            channels: config.channels,
            bit_perfect: true,
            not_bit_perfect_reason: None,
        },
        config,
        reserved: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::fake::{Call, FakeBackend, FakeClock, FakeDevice, FakeReserver, Log};
    use crate::output::reserve::ReleaseReply;
    use crate::sink::Codec;
    use SampleFormat::*;

    const ALL: &[SampleFormat] = &[S16Le, S24Le, S24_3Le, S32Le];

    fn flac(bits: u16, rate: u32) -> SourceFormat {
        SourceFormat {
            codec: Codec::Flac,
            sample_rate: rate,
            channels: 2,
            bits_per_sample: Some(bits),
        }
    }

    const AAC: SourceFormat = SourceFormat {
        codec: Codec::AacLc,
        sample_rate: 44_100,
        channels: 2,
        bits_per_sample: None,
    };

    fn rr(card: u32) -> Call {
        Call::RequestRelease {
            card,
            timeout: Duration::from_millis(500),
        }
    }

    fn query(rate: u32, resample: bool) -> Call {
        Call::Query {
            rate,
            channels: 2,
            resample,
        }
    }

    fn configure(format: SampleFormat, rate: u32, resample: bool, periods: u32) -> Call {
        Call::Configure(HwConfig {
            format,
            rate,
            channels: 2,
            resample,
            period_frames: 1024,
            periods,
        })
    }

    fn info(
        requested: &str,
        device: &str,
        kind: OutputKind,
        format: SampleFormat,
        rate: u32,
        reason: Option<&str>,
    ) -> OutputInfo {
        OutputInfo {
            requested: requested.into(),
            device: device.into(),
            kind,
            sample_format: format,
            sample_rate: rate,
            channels: 2,
            bit_perfect: reason.is_none(),
            not_bit_perfect_reason: reason.map(Into::into),
        }
    }

    struct Row {
        name: &'static str,
        device: &'static str,
        backend: fn(&Log) -> FakeBackend,
        source: SourceFormat,
        want: Result<OutputInfo, SinkError>,
        calls: Vec<Call>,
    }

    fn run(row: &Row, memo: &FallbackMemo) -> (Result<Opened, SinkError>, Vec<Call>) {
        let log = Log::default();
        let mut backend = (row.backend)(&log);
        let mut reserver = FakeReserver::new(&log, ReleaseReply::NoOwner, Ok(()));
        let clock = FakeClock::new(&log);
        let got = open_output(
            &mut backend,
            &mut reserver,
            &clock,
            memo,
            row.device,
            &row.source,
        );
        (got, log.calls())
    }

    const REFUSED_24_96: &str =
        "resampled (plughw fallback: device refused S24_3LE/S24_LE/S32_LE at 96 kHz)";

    #[test]
    fn ac12_open_output() {
        let sleep = Call::Sleep(Duration::from_millis(100));
        let busy_hw = Call::OpenFailed("hw:1,0".into());
        let mut busy_forever = vec![rr(1), Call::Claim(1), busy_hw.clone()];
        for _ in 0..8 {
            busy_forever.extend([sleep.clone(), busy_hw.clone()]);
        }
        busy_forever.push(Call::Release(1));

        let rows = vec![
            Row {
                name: "exclusive 16/44.1: S32_LE first, exact rate, no resampling, 4 periods",
                device: "hw:1,0",
                backend: |log| {
                    FakeBackend::new(log)
                        .with_device("hw:1,0", FakeDevice::new(&[S16Le, S32Le], &[44_100]))
                },
                source: flac(16, 44_100),
                want: Ok(info(
                    "hw:1,0",
                    "hw:1,0",
                    OutputKind::Exclusive,
                    S32Le,
                    44_100,
                    None,
                )),
                calls: vec![
                    rr(1),
                    Call::Claim(1),
                    Call::Open("hw:1,0".into()),
                    query(44_100, false),
                    configure(S32Le, 44_100, false, 4),
                ],
            },
            Row {
                name: "exclusive 24/96 on a device without S24_3LE",
                device: "hw:2",
                backend: |log| {
                    FakeBackend::new(log)
                        .with_device("hw:2", FakeDevice::new(&[S24Le, S32Le], &[44_100, 96_000]))
                },
                source: flac(24, 96_000),
                want: Ok(info(
                    "hw:2",
                    "hw:2",
                    OutputKind::Exclusive,
                    S24Le,
                    96_000,
                    None,
                )),
                calls: vec![
                    rr(2),
                    Call::Claim(2),
                    Call::Open("hw:2".into()),
                    query(96_000, false),
                    configure(S24Le, 96_000, false, 4),
                ],
            },
            Row {
                name: "exclusive, lossy source: not bit-perfect",
                device: "hw:1,0",
                backend: |log| {
                    FakeBackend::new(log).with_device("hw:1,0", FakeDevice::new(ALL, &[44_100]))
                },
                source: AAC,
                want: Ok(info(
                    "hw:1,0",
                    "hw:1,0",
                    OutputKind::Exclusive,
                    S32Le,
                    44_100,
                    Some("lossy source"),
                )),
                calls: vec![
                    rr(1),
                    Call::Claim(1),
                    Call::Open("hw:1,0".into()),
                    query(44_100, false),
                    configure(S32Le, 44_100, false, 4),
                ],
            },
            Row {
                name: "rate refused: plughw on the same card",
                device: "hw:1,0",
                backend: |log| {
                    FakeBackend::new(log)
                        .with_device("hw:1,0", FakeDevice::new(ALL, &[44_100, 48_000]))
                        .with_device("plughw:1,0", FakeDevice::plug())
                },
                source: flac(24, 96_000),
                want: Ok(info(
                    "hw:1,0",
                    "plughw:1,0",
                    OutputKind::Fallback,
                    S24_3Le,
                    96_000,
                    Some(REFUSED_24_96),
                )),
                calls: vec![
                    rr(1),
                    Call::Claim(1),
                    Call::Open("hw:1,0".into()),
                    query(96_000, false),
                    Call::Close,
                    Call::Open("plughw:1,0".into()),
                    query(96_000, true),
                    configure(S24_3Le, 96_000, true, 4),
                ],
            },
            Row {
                name: "formats refused (fixed 16-bit interface, 24-bit source): plughw",
                device: "hw:1,0",
                backend: |log| {
                    FakeBackend::new(log)
                        .with_device("hw:1,0", FakeDevice::new(&[S16Le], &[96_000]))
                        .with_device("plughw:1,0", FakeDevice::plug())
                },
                source: flac(24, 96_000),
                want: Ok(info(
                    "hw:1,0",
                    "plughw:1,0",
                    OutputKind::Fallback,
                    S24_3Le,
                    96_000,
                    Some(REFUSED_24_96),
                )),
                calls: vec![
                    rr(1),
                    Call::Claim(1),
                    Call::Open("hw:1,0".into()),
                    query(96_000, false),
                    Call::Close,
                    Call::Open("plughw:1,0".into()),
                    query(96_000, true),
                    configure(S24_3Le, 96_000, true, 4),
                ],
            },
            Row {
                name: "EBUSY three times, then free: retried every 100 ms",
                device: "hw:1,0",
                backend: |log| {
                    FakeBackend::new(log)
                        .with_device("hw:1,0", FakeDevice::new(ALL, &[44_100]))
                        .with_open_results(vec![
                            Err(PcmError::Busy),
                            Err(PcmError::Busy),
                            Err(PcmError::Busy),
                        ])
                },
                source: flac(16, 44_100),
                want: Ok(info(
                    "hw:1,0",
                    "hw:1,0",
                    OutputKind::Exclusive,
                    S32Le,
                    44_100,
                    None,
                )),
                calls: vec![
                    rr(1),
                    Call::Claim(1),
                    busy_hw.clone(),
                    sleep.clone(),
                    busy_hw.clone(),
                    sleep.clone(),
                    busy_hw.clone(),
                    sleep.clone(),
                    Call::Open("hw:1,0".into()),
                    query(44_100, false),
                    configure(S32Le, 44_100, false, 4),
                ],
            },
            Row {
                name: "EBUSY for 800 ms: busy, never plughw, reservation released",
                device: "hw:1,0",
                backend: |log| {
                    FakeBackend::new(log)
                        .with_device("hw:1,0", FakeDevice::new(ALL, &[44_100]))
                        .with_device("plughw:1,0", FakeDevice::plug())
                        .with_open_default(Err(PcmError::Busy))
                },
                source: flac(16, 44_100),
                want: Err(SinkError::Busy {
                    device: "hw:1,0".into(),
                    holder: None,
                }),
                calls: busy_forever,
            },
            Row {
                name: "no such device",
                device: "hw:5,0",
                backend: |log| FakeBackend::new(log),
                source: flac(16, 44_100),
                want: Err(SinkError::NotFound("hw:5,0".into())),
                calls: vec![
                    rr(5),
                    Call::Claim(5),
                    Call::OpenFailed("hw:5,0".into()),
                    Call::Release(5),
                ],
            },
            Row {
                name: "plughw given by the user: fallback, reserved, resampling on, 4 periods",
                device: "plughw:1,0",
                backend: |log| FakeBackend::new(log).with_device("plughw:1,0", FakeDevice::plug()),
                source: flac(16, 44_100),
                want: Ok(info(
                    "plughw:1,0",
                    "plughw:1,0",
                    OutputKind::Fallback,
                    S32Le,
                    44_100,
                    Some("plughw (ALSA may convert the samples)"),
                )),
                calls: vec![
                    rr(1),
                    Call::Claim(1),
                    Call::Open("plughw:1,0".into()),
                    query(44_100, true),
                    configure(S32Le, 44_100, true, 4),
                ],
            },
            Row {
                name: "shared: no reservation, resampling on, 8 periods, S32_LE first",
                device: "default",
                backend: |log| FakeBackend::new(log).with_device("default", FakeDevice::plug()),
                source: flac(24, 96_000),
                want: Ok(info(
                    "default",
                    "default",
                    OutputKind::Shared,
                    S32Le,
                    96_000,
                    Some("shared (system mixer)"),
                )),
                calls: vec![
                    Call::Open("default".into()),
                    query(96_000, true),
                    configure(S32Le, 96_000, true, 8),
                ],
            },
            Row {
                name: "named card: index asked of the backend",
                device: "hw:CARD=DAC,DEV=0",
                backend: |log| {
                    FakeBackend::new(log)
                        .with_card("DAC", 3)
                        .with_device("hw:CARD=DAC,DEV=0", FakeDevice::new(ALL, &[44_100]))
                },
                source: flac(16, 44_100),
                want: Ok(info(
                    "hw:CARD=DAC,DEV=0",
                    "hw:CARD=DAC,DEV=0",
                    OutputKind::Exclusive,
                    S32Le,
                    44_100,
                    None,
                )),
                calls: vec![
                    Call::CardIndex("DAC".into()),
                    rr(3),
                    Call::Claim(3),
                    Call::Open("hw:CARD=DAC,DEV=0".into()),
                    query(44_100, false),
                    configure(S32Le, 44_100, false, 4),
                ],
            },
        ];
        for row in &rows {
            let (got, calls) = run(row, &FallbackMemo::default());
            assert_eq!(got.map(|o| o.info), row.want, "{}", row.name);
            assert_eq!(calls, row.calls, "{}", row.name);
            if row.want.is_err() {
                assert!(
                    !calls.iter().any(|c| matches!(c,
                        Call::Open(n) | Call::OpenFailed(n) if n.starts_with("plughw:"))),
                    "{}: an error path opened plughw",
                    row.name
                );
            }
        }
    }

    #[test]
    fn ac12_fallback_is_memoised_per_device() {
        let row = Row {
            name: "rate refused",
            device: "hw:1,0",
            backend: |log| {
                FakeBackend::new(log)
                    .with_device("hw:1,0", FakeDevice::new(ALL, &[44_100]))
                    .with_device("plughw:1,0", FakeDevice::plug())
                    .with_device("hw:2,0", FakeDevice::new(ALL, &[96_000]))
            },
            source: flac(24, 96_000),
            want: Ok(info(
                "hw:1,0",
                "plughw:1,0",
                OutputKind::Fallback,
                S24_3Le,
                96_000,
                Some(REFUSED_24_96),
            )),
            calls: vec![],
        };
        let memo = FallbackMemo::default();
        let (first, calls) = run(&row, &memo);
        assert_eq!(first.map(|o| o.info), row.want);
        assert!(calls.contains(&Call::Open("hw:1,0".into())));

        // Second open of the same device: straight to plughw.
        let (second, calls) = run(&row, &memo);
        assert_eq!(second.map(|o| o.info), row.want);
        assert_eq!(
            calls,
            vec![
                rr(1),
                Call::Claim(1),
                Call::Open("plughw:1,0".into()),
                query(96_000, true),
                configure(S24_3Le, 96_000, true, 4),
            ]
        );

        // Another device is not affected.
        let other = Row {
            device: "hw:2,0",
            ..row
        };
        let (third, _) = run(&other, &memo);
        assert_eq!(third.unwrap().info.kind, OutputKind::Exclusive);
    }

    #[test]
    fn ac12_bit_perfect() {
        use OutputKind::*;
        // (kind, source bits, negotiated format, device rate, device channels, bit-perfect)
        let rows: &[(OutputKind, Option<u16>, SampleFormat, u32, u16, bool)] = &[
            (Exclusive, Some(16), S32Le, 44_100, 2, true),
            (Exclusive, Some(16), S16Le, 44_100, 2, true),
            (Exclusive, Some(16), S24_3Le, 44_100, 2, true),
            (Exclusive, Some(16), S24Le, 44_100, 2, true),
            (Exclusive, Some(24), S24_3Le, 44_100, 2, true),
            (Exclusive, Some(24), S24Le, 44_100, 2, true),
            (Exclusive, Some(24), S32Le, 44_100, 2, true),
            (Exclusive, Some(24), S16Le, 44_100, 2, false),
            (Exclusive, Some(32), S24Le, 44_100, 2, false),
            (Exclusive, Some(32), S32Le, 44_100, 2, true),
            (Exclusive, Some(16), S32Le, 48_000, 2, false),
            (Exclusive, Some(24), S24_3Le, 48_000, 2, false),
            (Exclusive, Some(16), S32Le, 44_100, 1, false),
            (Exclusive, None, S32Le, 44_100, 2, false),
            (Fallback, Some(16), S32Le, 44_100, 2, false),
            (Fallback, Some(24), S24_3Le, 44_100, 2, false),
            (Shared, Some(16), S32Le, 44_100, 2, false),
            (Shared, Some(24), S32Le, 44_100, 2, false),
        ];
        for &(kind, bits, format, rate, channels, want) in rows {
            let source = SourceFormat {
                codec: if bits.is_some() {
                    Codec::Flac
                } else {
                    Codec::AacLc
                },
                sample_rate: 44_100,
                channels: 2,
                bits_per_sample: bits,
            };
            let reason = not_bit_perfect_reason(kind, &source, format, rate, channels);
            assert_eq!(
                reason.is_none(),
                want,
                "{kind:?} {bits:?} {format:?} {rate} Hz {channels} ch: {reason:?}"
            );
        }
        // A mono source is output as stereo, bit-perfect (spec 0003 "Decode").
        let mono = SourceFormat {
            channels: 1,
            ..flac(16, 44_100)
        };
        assert_eq!(
            not_bit_perfect_reason(Exclusive, &mono, S32Le, 44_100, 2),
            None
        );
    }
}
