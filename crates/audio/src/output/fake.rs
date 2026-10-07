//! Test fakes for the output seams: a [`PcmBackend`], a [`Reserver`] and a
//! [`Clock`] that all record into one shared call log, so tests can check
//! the order of reservation, open, negotiation and release.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use crate::output::clock::Clock;
use crate::output::format::bytes_per_sample;
use crate::output::open::{HwConfig, PcmBackend, PcmError};
use crate::output::reserve::{ReleaseReply, Reserver};
use crate::sink::SampleFormat;

/// One call into a fake.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Call {
    // Reserver
    RequestRelease {
        card: u32,
        timeout: Duration,
    },
    Claim(u32),
    Release(u32),
    // Clock
    Sleep(Duration),
    // PcmBackend
    CardIndex(String),
    Open(String),
    OpenFailed(String),
    Query {
        rate: u32,
        channels: u16,
        resample: bool,
    },
    Configure(HwConfig),
    Write {
        frames: usize,
    },
    Pause(bool),
    Drop,
    Prepare,
    Drain,
    Recover(PcmError),
    Close,
}

/// The shared call log.
#[derive(Debug, Clone, Default)]
pub struct Log(Arc<Mutex<Vec<Call>>>);

impl Log {
    pub fn push(&self, call: Call) {
        self.0.lock().unwrap().push(call);
    }

    pub fn calls(&self) -> Vec<Call> {
        self.0.lock().unwrap().clone()
    }

    pub fn clear(&self) {
        self.0.lock().unwrap().clear();
    }
}

/// A clock whose `sleep` advances time instantly.
#[derive(Debug)]
pub struct FakeClock {
    log: Log,
    now: Mutex<Duration>,
}

impl FakeClock {
    pub fn new(log: &Log) -> Self {
        Self {
            log: log.clone(),
            now: Mutex::new(Duration::ZERO),
        }
    }
}

impl Clock for FakeClock {
    fn now(&self) -> Duration {
        *self.now.lock().unwrap()
    }

    fn sleep(&self, duration: Duration) {
        self.log.push(Call::Sleep(duration));
        *self.now.lock().unwrap() += duration;
    }
}

/// A bus whose owner always answers the same.
#[derive(Debug)]
pub struct FakeReserver {
    log: Log,
    reply: ReleaseReply,
    claim: Result<(), Option<String>>,
}

impl FakeReserver {
    pub fn new(log: &Log, reply: ReleaseReply, claim: Result<(), Option<String>>) -> Self {
        Self {
            log: log.clone(),
            reply,
            claim,
        }
    }
}

impl Reserver for FakeReserver {
    fn request_release(&mut self, card: u32, timeout: Duration) -> ReleaseReply {
        self.log.push(Call::RequestRelease { card, timeout });
        self.reply.clone()
    }

    fn claim(&mut self, card: u32) -> Result<(), Option<String>> {
        self.log.push(Call::Claim(card));
        self.claim.clone()
    }

    fn release(&mut self, card: u32) {
        self.log.push(Call::Release(card));
    }
}

/// What a fake device accepts. An empty `rates` accepts any rate (a plug
/// layer); an empty `formats` accepts none.
#[derive(Debug, Clone)]
pub struct FakeDevice {
    formats: Vec<SampleFormat>,
    rates: Vec<u32>,
}

impl FakeDevice {
    pub fn new(formats: &[SampleFormat], rates: &[u32]) -> Self {
        Self {
            formats: formats.to_vec(),
            rates: rates.to_vec(),
        }
    }

    /// A plug layer: every format at every rate.
    pub fn plug() -> Self {
        use SampleFormat::*;
        Self::new(&[S16Le, S24Le, S24_3Le, S32Le], &[])
    }

    fn rate_ok(&self, rate: u32) -> bool {
        self.rates.is_empty() || self.rates.contains(&rate)
    }
}

/// The ALSA PCM states the fake models (0003 AC29): `pause` is only
/// accepted where ALSA accepts it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Pcm {
    #[default]
    Setup,
    Prepared,
    Running,
    Paused,
    Xrun,
}

/// What ALSA answers a `pause` in the wrong state with (`EBADFD`, 77).
pub const BAD_STATE: &str = "EBADFD: snd_pcm_pause in the wrong PCM state";

#[derive(Debug, Default)]
struct State {
    pcm: Pcm,
    cards: HashMap<String, u32>,
    devices: HashMap<String, FakeDevice>,
    open_results: VecDeque<Result<(), PcmError>>,
    open_default: Option<PcmError>,
    write_results: VecDeque<Result<usize, PcmError>>,
    open: Option<String>,
    config: Option<HwConfig>,
    written: Vec<u8>,
    delay: i64,
}

/// A scriptable [`PcmBackend`]. Clones share state.
#[derive(Debug, Clone)]
pub struct FakeBackend {
    log: Log,
    state: Arc<Mutex<State>>,
}

impl FakeBackend {
    /// A backend with no devices: every open is `NotFound`.
    pub fn new(log: &Log) -> Self {
        Self {
            log: log.clone(),
            state: Arc::default(),
        }
    }

    pub fn with_card(self, card: &str, index: u32) -> Self {
        self.state().cards.insert(card.into(), index);
        self
    }

    pub fn with_device(self, name: &str, device: FakeDevice) -> Self {
        self.state().devices.insert(name.into(), device);
        self
    }

    /// Results for the next opens, in order, before the devices decide.
    pub fn with_open_results(self, results: Vec<Result<(), PcmError>>) -> Self {
        self.state().open_results = results.into();
        self
    }

    /// Every open fails with this error (unless an open result is queued).
    pub fn with_open_default(self, result: Result<(), PcmError>) -> Self {
        self.state().open_default = result.err();
        self
    }

    /// Results for the next writes; afterwards every write takes everything.
    pub fn with_write_results(self, results: Vec<Result<usize, PcmError>>) -> Self {
        self.state().write_results = results.into();
        self
    }

    pub fn set_delay(&self, delay: i64) {
        self.state().delay = delay;
    }

    /// Every byte written so far.
    pub fn written(&self) -> Vec<u8> {
        self.state().written.clone()
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap()
    }
}

impl PcmBackend for FakeBackend {
    fn card_index(&mut self, card: &str) -> Option<u32> {
        self.log.push(Call::CardIndex(card.into()));
        self.state().cards.get(card).copied()
    }

    fn open(&mut self, name: &str) -> Result<(), PcmError> {
        let mut state = self.state();
        let result = match state.open_results.pop_front() {
            Some(result) => result,
            None => match &state.open_default {
                Some(err) => Err(err.clone()),
                None if state.devices.contains_key(name) => Ok(()),
                None => Err(PcmError::NotFound),
            },
        };
        match &result {
            Ok(()) => {
                self.log.push(Call::Open(name.into()));
                state.open = Some(name.into());
                state.config = None;
            }
            Err(_) => self.log.push(Call::OpenFailed(name.into())),
        }
        result
    }

    fn accepted_formats(
        &mut self,
        rate: u32,
        channels: u16,
        resample: bool,
    ) -> Result<Vec<SampleFormat>, PcmError> {
        self.log.push(Call::Query {
            rate,
            channels,
            resample,
        });
        let state = self.state();
        let name = state.open.as_ref().expect("query before open");
        let device = &state.devices[name];
        if channels != 2 || !device.rate_ok(rate) {
            return Ok(Vec::new());
        }
        Ok(device.formats.clone())
    }

    fn configure(&mut self, config: &HwConfig) -> Result<(), PcmError> {
        self.log.push(Call::Configure(config.clone()));
        let mut state = self.state();
        let name = state.open.clone().expect("configure before open");
        let device = &state.devices[&name];
        if !device.rate_ok(config.rate) || !device.formats.contains(&config.format) {
            return Err(PcmError::Refused("fake refuses".into()));
        }
        state.config = Some(config.clone());
        state.pcm = Pcm::Prepared;
        Ok(())
    }

    fn write(&mut self, bytes: &[u8]) -> Result<usize, PcmError> {
        let mut state = self.state();
        let config = state.config.clone().expect("write before configure");
        let frame = bytes_per_sample(config.format) * usize::from(config.channels);
        assert_eq!(bytes.len() % frame, 0, "partial frame written");
        let frames = bytes.len() / frame;
        let result = state.write_results.pop_front().unwrap_or(Ok(frames));
        match result {
            Ok(n) => {
                assert!(n <= frames);
                state.written.extend_from_slice(&bytes[..n * frame]);
                self.log.push(Call::Write { frames: n });
                if n > 0 && state.pcm == Pcm::Prepared {
                    state.pcm = Pcm::Running;
                }
            }
            Err(PcmError::Underrun) => state.pcm = Pcm::Xrun,
            Err(_) => {}
        }
        result
    }

    fn delay(&self) -> Result<i64, PcmError> {
        Ok(self.state().delay)
    }

    fn pause(&mut self, paused: bool) -> Result<(), PcmError> {
        self.log.push(Call::Pause(paused));
        let mut state = self.state();
        state.pcm = match (paused, state.pcm) {
            (true, Pcm::Running) => Pcm::Paused,
            (false, Pcm::Paused) => Pcm::Running,
            _ => return Err(PcmError::Other(BAD_STATE.into())),
        };
        Ok(())
    }

    fn drop_frames(&mut self) -> Result<(), PcmError> {
        self.log.push(Call::Drop);
        self.state().pcm = Pcm::Setup;
        Ok(())
    }

    fn prepare(&mut self) -> Result<(), PcmError> {
        self.log.push(Call::Prepare);
        self.state().pcm = Pcm::Prepared;
        Ok(())
    }

    fn drain(&mut self) -> Result<(), PcmError> {
        self.log.push(Call::Drain);
        self.state().pcm = Pcm::Setup;
        Ok(())
    }

    fn recover(&mut self, error: &PcmError) -> Result<(), PcmError> {
        self.log.push(Call::Recover(error.clone()));
        self.state().pcm = Pcm::Prepared;
        Ok(())
    }

    fn close(&mut self) {
        let mut state = self.state();
        if state.open.take().is_some() {
            state.config = None;
            self.log.push(Call::Close);
        }
    }
}
