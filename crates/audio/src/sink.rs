//! The audio output seam (spec 0003): the engine writes decoded PCM into a
//! [`Sink`]. The ALSA output and the in-memory [`MemorySink`] used by tests
//! implement it.
//!
//! Samples are interleaved `i32`, left-justified: a 16-bit sample `s` is
//! `s << 16` (spec 0001 AC7, spec 0003 "Decode").

use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use crate::clock::{FakeClock, frames_to_duration};

/// Codec of the decoded source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Codec {
    /// FLAC, raw or in (fragmented) MP4. Lossless.
    Flac,
    /// AAC-LC in MP4. Lossy.
    AacLc,
}

/// What the decoder found in the stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceFormat {
    pub codec: Codec,
    /// Frames per second.
    pub sample_rate: u32,
    /// Channels in the source (the engine always outputs stereo, AC9).
    pub channels: u16,
    /// Significant bits per sample; `None` for lossy sources.
    pub bits_per_sample: Option<u16>,
}

impl SourceFormat {
    /// Whether the source is lossless (a precondition for `bit_perfect`).
    pub fn is_lossless(&self) -> bool {
        self.bits_per_sample.is_some()
    }
}

/// PCM sample formats the output may negotiate (spec 0003 "Output (ALSA)").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SampleFormat {
    /// 16-bit little-endian.
    S16Le,
    /// 24 bits in the low 3 bytes of a 4-byte little-endian word, sign-extended.
    S24Le,
    /// 24-bit, 3 bytes little-endian.
    S24_3Le,
    /// 32-bit little-endian.
    S32Le,
}

/// How the output reaches the device (spec 0003 "Output kinds").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputKind {
    /// `hw:` opened directly, card reserved.
    Exclusive,
    /// `plughw:`, given by the user or after a refused negotiation.
    Fallback,
    /// `default` or any other PCM name: through the system mixer.
    Shared,
}

/// What [`Sink::open`] actually opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputInfo {
    /// The device name the sink was created for.
    pub requested: String,
    /// The device actually opened (differs on the `plughw:` fallback).
    pub device: String,
    pub kind: OutputKind,
    pub sample_format: SampleFormat,
    pub sample_rate: u32,
    pub channels: u16,
    /// Whether samples reach the device untouched (spec 0003 "Output kinds").
    pub bit_perfect: bool,
    /// Why `bit_perfect` is false, for the user (`None` when it is true).
    pub not_bit_perfect_reason: Option<String>,
}

/// Errors reported by a [`Sink`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SinkError {
    /// Called before a successful `open`.
    #[error("sink used before it was opened")]
    NotOpen,
    /// The device is in use (`EBUSY` past the retry budget, or the
    /// reservation was refused or timed out). Never downgraded.
    #[error("output {device} is busy{}", holder.as_deref().map(|h| format!(" (used by {h})")).unwrap_or_default())]
    Busy {
        device: String,
        holder: Option<String>,
    },
    /// No such device.
    #[error("no such output device {0}")]
    NotFound(String),
    /// The device went away while open (`ENODEV`, `EIO`, …).
    #[error("output {0} was lost")]
    Lost(String),
    /// Any other backend failure.
    #[error("audio backend error: {0}")]
    Backend(String),
}

/// Result of one [`Sink::write`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WriteOutcome {
    /// Frames consumed from the start of the buffer (may be fewer than given).
    pub frames: usize,
    /// The device underran (or was suspended) and has been recovered.
    /// Frames not consumed must be written again (spec 0003 AC14).
    pub underrun: bool,
}

/// A destination for decoded audio. Owned and driven by the engine thread.
pub trait Sink: Send {
    /// Open the device for `source` (always output as stereo) and negotiate
    /// the format. Re-opening an open sink closes it first.
    fn open(&mut self, source: &SourceFormat) -> Result<OutputInfo, SinkError>;
    /// Write interleaved frames (`samples.len()` is a multiple of the output
    /// channel count). May consume fewer frames than given; never blocks
    /// longer than about one device period.
    fn write(&mut self, samples: &[i32]) -> Result<WriteOutcome, SinkError>;
    /// Frames written but not yet heard (the device's delay).
    fn delay_frames(&self) -> Result<u64, SinkError>;
    /// Stop (`true`) or restart (`false`) output, keeping buffered frames.
    fn set_paused(&mut self, paused: bool) -> Result<(), SinkError>;
    /// Drop buffered frames without playing them (used by seek).
    fn discard(&mut self) -> Result<(), SinkError>;
    /// Block until every buffered frame has been played.
    fn drain(&mut self) -> Result<(), SinkError>;
    /// Close the device and release any reservation. Idempotent.
    fn close(&mut self);
    /// As [`Sink::close`], calling `between` once the PCM is closed and
    /// before the reservation is released (spec 0005: `RequestRelease` is
    /// answered between the two). Calls `between` even when nothing is open.
    fn close_with(&mut self, between: &mut dyn FnMut()) {
        self.close();
        between();
    }
}

/// Creates a [`Sink`] for a device name (`SetDevice`, spec 0003 AC21).
pub trait SinkFactory: Send {
    fn create(&mut self, device: &str) -> Box<dyn Sink>;
}

/// One call a [`MemorySink`] saw, in the log shared by every sink of a
/// [`MemoryDevices`] factory (spec 0003 AC15, AC17, AC21).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SinkCall {
    Open {
        device: String,
        source: SourceFormat,
    },
    /// A write that consumed `frames` frames (writes consuming none are not
    /// logged).
    Write {
        device: String,
        frames: usize,
    },
    Pause {
        device: String,
        paused: bool,
    },
    Discard {
        device: String,
    },
    Drain {
        device: String,
    },
    Close {
        device: String,
    },
    /// The card was reserved, before its `Open` (scripted with
    /// [`SinkScript::reserved`]).
    Reserve {
        device: String,
    },
    /// The reservation was released, after its `Close`.
    Release {
        device: String,
    },
}

/// Scripted behaviour of a [`MemorySink`] (the engine tests' fake device).
#[derive(Debug, Clone, Default)]
pub struct SinkScript {
    /// The device's delay: frames written but not yet heard, capped by the
    /// frames buffered since the last open, discard or drain.
    pub delay_frames: u64,
    /// The n-th `write` call (1-based) consumes only half its frames and
    /// reports an underrun (AC14).
    pub underrun_on_write: Option<usize>,
    /// The n-th `write` call (1-based) fails with [`SinkError::Lost`] (AC14).
    pub lost_on_write: Option<usize>,
    /// Once this many frames were written, the device is "full": writes
    /// wait briefly and consume nothing until [`MemorySinkHandle::release`].
    pub hold_after_frames: Option<u64>,
    /// Advanced by the duration of every frame written, as if the device
    /// played in real time (AC20).
    pub clock: Option<FakeClock>,
    /// The device is reserved while open, like an exclusive output: `Open`
    /// is preceded by `Reserve`, `Close` followed by `Release` (spec 0005).
    pub reserved: bool,
    /// `(n, error)`: the n-th `open` of this device (1-based, counted
    /// across every sink of a [`MemoryDevices`] factory created for it)
    /// fails with `error`, leaving nothing open and nothing reserved (spec
    /// 0005 AC21, 0014 AC5).
    pub open_errors: Vec<(usize, SinkError)>,
}

/// How long a held write waits for a release before consuming nothing,
/// like a real device blocking on a full buffer.
const HOLD_WAIT: Duration = Duration::from_millis(5);

#[derive(Debug, Default)]
struct Shared {
    calls: Vec<SinkCall>,
    samples: Vec<(String, i32)>,
    /// `samples` minus the frames a discard or close dropped unheard (the
    /// device's delay at that moment).
    heard: Vec<i32>,
    open: usize,
    max_open: usize,
    released: bool,
    /// The device of every `open` call, failed or not, in order.
    open_attempts: Vec<String>,
}

type SharedState = Arc<(Mutex<Shared>, Condvar)>;

fn lock(state: &SharedState) -> MutexGuard<'_, Shared> {
    state.0.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A [`Sink`] that records everything written to it, for tests. It grows
/// into the engine tests' fake device with a [`SinkScript`]; a
/// [`MemorySinkHandle`] inspects it after it was handed to the engine.
#[derive(Debug)]
pub struct MemorySink {
    device: String,
    script: SinkScript,
    shared: SharedState,
    source: Option<SourceFormat>,
    written: u64,
    buffered: u64,
    write_calls: usize,
}

impl Default for MemorySink {
    fn default() -> Self {
        Self::new("memory", SinkScript::default())
    }
}

impl MemorySink {
    /// A sink for `device` with its own log.
    pub fn new(device: &str, script: SinkScript) -> Self {
        Self::with_shared(device, script, SharedState::default())
    }

    fn with_shared(device: &str, script: SinkScript, shared: SharedState) -> Self {
        Self {
            device: device.to_owned(),
            script,
            shared,
            source: None,
            written: 0,
            buffered: 0,
            write_calls: 0,
        }
    }

    /// Forget the frames still in the device: they were never heard.
    fn drop_unheard(&mut self, shared: &mut Shared) {
        let unheard = self.script.delay_frames.min(self.buffered) as usize * 2;
        let keep = shared.heard.len().saturating_sub(unheard);
        shared.heard.truncate(keep);
        self.buffered = 0;
    }

    /// A handle on this sink's log and samples.
    pub fn handle(&self) -> MemorySinkHandle {
        MemorySinkHandle {
            shared: self.shared.clone(),
        }
    }

    /// The source format passed to `open`, if open.
    pub fn source(&self) -> Option<SourceFormat> {
        self.source
    }

    /// All samples written so far, in order.
    pub fn samples(&self) -> Vec<i32> {
        self.handle().samples()
    }
}

impl Sink for MemorySink {
    fn open(&mut self, source: &SourceFormat) -> Result<OutputInfo, SinkError> {
        self.close();
        let open_calls = {
            let mut shared = lock(&self.shared);
            shared.open_attempts.push(self.device.clone());
            shared
                .open_attempts
                .iter()
                .filter(|d| **d == self.device)
                .count()
        };
        if let Some((_, error)) = self
            .script
            .open_errors
            .iter()
            .find(|(n, _)| *n == open_calls)
        {
            return Err(error.clone());
        }
        self.source = Some(*source);
        self.buffered = 0;
        {
            let mut shared = lock(&self.shared);
            if self.script.reserved {
                shared.calls.push(SinkCall::Reserve {
                    device: self.device.clone(),
                });
            }
            shared.calls.push(SinkCall::Open {
                device: self.device.clone(),
                source: *source,
            });
            shared.open += 1;
            shared.max_open = shared.max_open.max(shared.open);
        }
        Ok(OutputInfo {
            requested: self.device.clone(),
            device: self.device.clone(),
            kind: OutputKind::Exclusive,
            sample_format: SampleFormat::S32Le,
            sample_rate: source.sample_rate,
            channels: 2,
            bit_perfect: source.is_lossless(),
            not_bit_perfect_reason: (!source.is_lossless()).then(|| "lossy source".into()),
        })
    }

    fn write(&mut self, samples: &[i32]) -> Result<WriteOutcome, SinkError> {
        let Some(source) = self.source else {
            return Err(SinkError::NotOpen);
        };
        self.write_calls += 1;
        if self.script.lost_on_write == Some(self.write_calls) {
            return Err(SinkError::Lost(self.device.clone()));
        }
        let mut frames = samples.len() / 2;
        let mut shared = lock(&self.shared);
        if let Some(limit) = self.script.hold_after_frames
            && !shared.released
        {
            if self.written >= limit {
                shared = self
                    .shared
                    .1
                    .wait_timeout(shared, HOLD_WAIT)
                    .unwrap_or_else(PoisonError::into_inner)
                    .0;
                if !shared.released {
                    return Ok(WriteOutcome::default());
                }
            } else {
                frames = frames.min((limit - self.written) as usize);
            }
        }
        let underrun = self.script.underrun_on_write == Some(self.write_calls);
        if underrun {
            frames /= 2;
        }
        if frames > 0 {
            shared.calls.push(SinkCall::Write {
                device: self.device.clone(),
                frames,
            });
            shared.samples.extend(
                samples[..frames * 2]
                    .iter()
                    .map(|&s| (self.device.clone(), s)),
            );
            shared.heard.extend_from_slice(&samples[..frames * 2]);
        }
        drop(shared);
        self.written += frames as u64;
        self.buffered += frames as u64;
        if let Some(clock) = &self.script.clock {
            clock.advance(frames_to_duration(frames as u64, source.sample_rate));
        }
        Ok(WriteOutcome { frames, underrun })
    }

    fn delay_frames(&self) -> Result<u64, SinkError> {
        Ok(self.script.delay_frames.min(self.buffered))
    }

    fn set_paused(&mut self, paused: bool) -> Result<(), SinkError> {
        lock(&self.shared).calls.push(SinkCall::Pause {
            device: self.device.clone(),
            paused,
        });
        Ok(())
    }

    fn discard(&mut self) -> Result<(), SinkError> {
        let shared = self.shared.clone();
        let mut shared = lock(&shared);
        self.drop_unheard(&mut shared);
        shared.calls.push(SinkCall::Discard {
            device: self.device.clone(),
        });
        Ok(())
    }

    fn drain(&mut self) -> Result<(), SinkError> {
        self.buffered = 0;
        lock(&self.shared).calls.push(SinkCall::Drain {
            device: self.device.clone(),
        });
        Ok(())
    }

    fn close(&mut self) {
        self.close_with(&mut || {});
    }

    fn close_with(&mut self, between: &mut dyn FnMut()) {
        if self.source.take().is_none() {
            between();
            return;
        }
        let shared = self.shared.clone();
        {
            let mut shared = lock(&shared);
            self.drop_unheard(&mut shared);
            shared.calls.push(SinkCall::Close {
                device: self.device.clone(),
            });
            shared.open -= 1;
        }
        between();
        if self.script.reserved {
            lock(&shared).calls.push(SinkCall::Release {
                device: self.device.clone(),
            });
        }
    }
}

/// Inspects the [`MemorySink`]s it was taken from, from any thread.
#[derive(Debug, Clone)]
pub struct MemorySinkHandle {
    shared: SharedState,
}

impl MemorySinkHandle {
    /// Every call logged so far, in order.
    pub fn calls(&self) -> Vec<SinkCall> {
        lock(&self.shared).calls.clone()
    }

    /// Every sample written so far, to any device, in order.
    pub fn samples(&self) -> Vec<i32> {
        lock(&self.shared).samples.iter().map(|(_, s)| *s).collect()
    }

    /// The samples written to `device`, in order.
    pub fn samples_of(&self, device: &str) -> Vec<i32> {
        lock(&self.shared)
            .samples
            .iter()
            .filter(|(d, _)| d == device)
            .map(|(_, s)| *s)
            .collect()
    }

    /// What the listener heard: every sample written, to any device, in
    /// order, minus the frames a discard or close dropped from the device
    /// before they were played (its delay at that moment).
    pub fn heard(&self) -> Vec<i32> {
        lock(&self.shared).heard.clone()
    }

    /// The device of every `open` call so far, failed or not, in order
    /// (a failed open logs no [`SinkCall`]).
    pub fn open_attempts(&self) -> Vec<String> {
        lock(&self.shared).open_attempts.clone()
    }

    /// The number of frames written so far, to any device.
    pub fn frames_written(&self) -> usize {
        lock(&self.shared).samples.len() / 2
    }

    /// Devices open right now.
    pub fn open_now(&self) -> usize {
        lock(&self.shared).open
    }

    /// The most devices that were ever open at the same time.
    pub fn max_open(&self) -> usize {
        lock(&self.shared).max_open
    }

    /// End every `hold_after_frames`: writes are consumed again.
    pub fn release(&self) {
        lock(&self.shared).released = true;
        self.shared.1.notify_all();
    }
}

/// A [`SinkFactory`] of [`MemorySink`]s sharing one log (AC21).
#[derive(Debug, Default)]
pub struct MemoryDevices {
    scripts: Vec<(String, SinkScript)>,
    default_script: SinkScript,
    shared: SharedState,
}

impl MemoryDevices {
    pub fn new() -> Self {
        Self::default()
    }

    /// The script for every device without its own.
    pub fn with_default_script(mut self, script: SinkScript) -> Self {
        self.default_script = script;
        self
    }

    /// The script for `device`.
    pub fn with_script(mut self, device: &str, script: SinkScript) -> Self {
        self.scripts.push((device.to_owned(), script));
        self
    }

    /// A handle on every device this factory creates.
    pub fn handle(&self) -> MemorySinkHandle {
        MemorySinkHandle {
            shared: self.shared.clone(),
        }
    }
}

impl SinkFactory for MemoryDevices {
    fn create(&mut self, device: &str) -> Box<dyn Sink> {
        let script = self
            .scripts
            .iter()
            .find(|(d, _)| d == device)
            .map_or_else(|| self.default_script.clone(), |(_, s)| s.clone());
        Box::new(MemorySink::with_shared(device, script, self.shared.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::Clock;

    const SOURCE: SourceFormat = SourceFormat {
        codec: Codec::Flac,
        sample_rate: 44_100,
        channels: 2,
        bits_per_sample: Some(16),
    };

    #[test]
    fn ac7_memory_sink_records_samples() {
        let mut sink = MemorySink::default();
        sink.open(&SOURCE).unwrap();
        sink.write(&[1, 2, 3, 4]).unwrap();
        sink.write(&[5, 6]).unwrap();
        assert_eq!(sink.source(), Some(SOURCE));
        assert_eq!(sink.samples(), &[1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn ac7_write_before_open_fails() {
        let mut sink = MemorySink::default();
        assert!(matches!(sink.write(&[1, 2]), Err(SinkError::NotOpen)));
        assert!(sink.samples().is_empty());
    }

    #[test]
    fn scripted_underrun_lost_delay_and_clock() {
        let clock = FakeClock::new();
        let script = SinkScript {
            delay_frames: 3,
            underrun_on_write: Some(1),
            lost_on_write: Some(3),
            clock: Some(clock.clone()),
            ..SinkScript::default()
        };
        let mut sink = MemorySink::new("hw:1,0", script);
        let handle = sink.handle();
        sink.open(&SOURCE).unwrap();
        assert_eq!(sink.delay_frames(), Ok(0));
        let first = sink.write(&[1, 1, 2, 2, 3, 3, 4, 4]).unwrap();
        assert_eq!(
            first,
            WriteOutcome {
                frames: 2,
                underrun: true
            }
        );
        assert_eq!(sink.delay_frames(), Ok(2));
        assert!(!sink.write(&[3, 3, 4, 4]).unwrap().underrun);
        assert_eq!(sink.delay_frames(), Ok(3));
        assert_eq!(sink.write(&[5, 5]), Err(SinkError::Lost("hw:1,0".into())));
        assert_eq!(handle.samples(), [1, 1, 2, 2, 3, 3, 4, 4]);
        assert_eq!(clock.now(), frames_to_duration(4, 44_100));
        sink.drain().unwrap();
        assert_eq!(sink.delay_frames(), Ok(0));
        sink.close();
        sink.close();
        assert_eq!(
            handle.calls(),
            [
                SinkCall::Open {
                    device: "hw:1,0".into(),
                    source: SOURCE
                },
                SinkCall::Write {
                    device: "hw:1,0".into(),
                    frames: 2
                },
                SinkCall::Write {
                    device: "hw:1,0".into(),
                    frames: 2
                },
                SinkCall::Drain {
                    device: "hw:1,0".into()
                },
                SinkCall::Close {
                    device: "hw:1,0".into()
                },
            ]
        );
    }

    /// The 0005 fake: reservation log around open and close (with
    /// `close_with`'s step between), scripted open failures, and the
    /// heard samples (a discard or close drops the delay unheard).
    #[test]
    fn reserved_open_errors_and_heard() {
        let script = SinkScript {
            delay_frames: 1,
            reserved: true,
            open_errors: vec![(
                2,
                SinkError::Busy {
                    device: "d".into(),
                    holder: None,
                },
            )],
            ..SinkScript::default()
        };
        let mut sink = MemorySink::new("d", script);
        let handle = sink.handle();
        sink.open(&SOURCE).unwrap();
        sink.write(&[1, 1, 2, 2]).unwrap();
        let log = handle.clone();
        let mut at_between = Vec::new();
        sink.close_with(&mut || at_between = log.calls());
        assert!(matches!(at_between.last(), Some(SinkCall::Close { .. })));
        assert!(matches!(
            handle.calls().last(),
            Some(SinkCall::Release { .. })
        ));
        assert_eq!(handle.heard(), [1, 1], "the last frame was in the device");
        assert!(matches!(sink.open(&SOURCE), Err(SinkError::Busy { .. })));
        assert_eq!(handle.open_now(), 0);
        sink.open(&SOURCE).unwrap();
        sink.write(&[3, 3]).unwrap();
        sink.drain().unwrap();
        sink.close();
        assert_eq!(handle.heard(), [1, 1, 3, 3], "drained frames are heard");
        let reserves = handle
            .calls()
            .iter()
            .filter(|c| matches!(c, SinkCall::Reserve { .. }))
            .count();
        assert_eq!(reserves, 2, "a failed open reserves nothing");
    }

    #[test]
    fn hold_until_release() {
        let script = SinkScript {
            hold_after_frames: Some(1),
            ..SinkScript::default()
        };
        let mut sink = MemorySink::new("d", script);
        sink.open(&SOURCE).unwrap();
        assert_eq!(sink.write(&[1, 1, 2, 2]).unwrap().frames, 1);
        assert_eq!(sink.write(&[2, 2]).unwrap().frames, 0);
        sink.handle().release();
        assert_eq!(sink.write(&[2, 2]).unwrap().frames, 1);
    }

    #[test]
    fn factory_devices_share_a_log_and_count_open_devices() {
        let mut devices = MemoryDevices::new();
        let handle = devices.handle();
        let mut a = devices.create("a");
        let mut b = devices.create("b");
        a.open(&SOURCE).unwrap();
        a.write(&[1, 1]).unwrap();
        a.close();
        b.open(&SOURCE).unwrap();
        b.write(&[2, 2]).unwrap();
        assert_eq!(handle.max_open(), 1);
        b.open(&SOURCE).unwrap(); // reopening closes first
        assert_eq!(handle.max_open(), 1);
        assert_eq!(handle.open_now(), 1);
        a.open(&SOURCE).unwrap();
        assert_eq!(handle.max_open(), 2);
        assert_eq!(handle.samples_of("a"), [1, 1]);
        assert_eq!(handle.samples(), [1, 1, 2, 2]);
    }

    /// 0014 AC5's fake: `open_errors` counts the opens of a device across
    /// every sink created for it, and every attempt is logged.
    #[test]
    fn open_errors_count_per_device() {
        let error = SinkError::NotFound("a".into());
        let mut devices = MemoryDevices::new().with_script(
            "a",
            SinkScript {
                open_errors: vec![(2, error.clone())],
                ..SinkScript::default()
            },
        );
        let handle = devices.handle();
        devices.create("a").open(&SOURCE).unwrap();
        devices.create("b").open(&SOURCE).unwrap();
        assert_eq!(devices.create("a").open(&SOURCE), Err(error));
        devices.create("a").open(&SOURCE).unwrap();
        assert_eq!(handle.open_attempts(), ["a", "b", "a", "a"]);
    }
}
