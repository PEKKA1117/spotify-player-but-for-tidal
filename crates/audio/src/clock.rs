//! Time as the engine sees it (spec 0003 AC20). The engine reads a [`Clock`]
//! only to pace `Position` events; tests inject a [`FakeClock`] that the fake
//! device advances as it "plays" frames, so no test sleeps.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// A monotonic clock.
pub trait Clock: Send + Sync {
    /// Time elapsed since an arbitrary, fixed origin.
    fn now(&self) -> Duration;
}

/// The real clock ([`Instant`]-based).
#[derive(Debug, Clone, Copy)]
pub struct SystemClock {
    origin: Instant,
}

impl SystemClock {
    pub fn new() -> Self {
        Self {
            origin: Instant::now(),
        }
    }
}

impl Default for SystemClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for SystemClock {
    fn now(&self) -> Duration {
        self.origin.elapsed()
    }
}

/// A clock that only moves when told to. Clones share the same time.
#[derive(Debug, Clone, Default)]
pub struct FakeClock {
    nanos: Arc<AtomicU64>,
}

impl FakeClock {
    pub fn new() -> Self {
        Self::default()
    }

    /// Move the clock forward.
    pub fn advance(&self, by: Duration) {
        let by = u64::try_from(by.as_nanos()).unwrap_or(u64::MAX);
        self.nanos.fetch_add(by, Ordering::SeqCst);
    }
}

impl Clock for FakeClock {
    fn now(&self) -> Duration {
        Duration::from_nanos(self.nanos.load(Ordering::SeqCst))
    }
}

/// `round(t × rate)`: the frame a position falls on (spec 0003 AC19).
pub fn duration_to_frames(t: Duration, rate: u32) -> u64 {
    let frames = (t.as_nanos() * u128::from(rate) + 500_000_000) / 1_000_000_000;
    u64::try_from(frames).unwrap_or(u64::MAX)
}

/// The time at which frame `frames` starts (truncated to the nanosecond).
pub fn frames_to_duration(frames: u64, rate: u32) -> Duration {
    if rate == 0 {
        return Duration::ZERO;
    }
    let nanos = u128::from(frames) * 1_000_000_000 / u128::from(rate);
    Duration::from_nanos(u64::try_from(nanos).unwrap_or(u64::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_clock_moves_only_when_advanced() {
        let clock = FakeClock::new();
        let other = clock.clone();
        assert_eq!(clock.now(), Duration::ZERO);
        other.advance(Duration::from_millis(250));
        assert_eq!(clock.now(), Duration::from_millis(250));
    }

    #[test]
    fn frame_conversions() {
        let cases = [
            (Duration::ZERO, 44_100, 0),
            (Duration::from_millis(100), 44_100, 4_410),
            (Duration::from_micros(10), 44_100, 0), // 0.441 rounds down
            (Duration::from_micros(12), 44_100, 1), // 0.529 rounds up
            (Duration::from_millis(250), 96_000, 24_000),
        ];
        for (t, rate, frames) in cases {
            assert_eq!(duration_to_frames(t, rate), frames, "{t:?} at {rate}");
        }
        assert_eq!(
            frames_to_duration(4_410, 44_100),
            Duration::from_millis(100)
        );
        assert_eq!(frames_to_duration(1, 0), Duration::ZERO);
    }
}
