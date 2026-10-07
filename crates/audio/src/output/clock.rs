//! Injected time for the output's retry and reservation budgets (spec 0003
//! "Timing budgets"), so tests never sleep for real.

use std::time::{Duration, Instant};

/// A monotonic clock that can also sleep.
pub trait Clock: Send + Sync {
    /// Time elapsed since an arbitrary, fixed origin.
    fn now(&self) -> Duration;
    /// Block the calling thread for `duration`.
    fn sleep(&self, duration: Duration);
}

/// The real clock.
#[derive(Debug)]
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

    fn sleep(&self, duration: Duration) {
        std::thread::sleep(duration);
    }
}
