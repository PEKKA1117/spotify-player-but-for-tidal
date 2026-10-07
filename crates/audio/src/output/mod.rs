//! ALSA output (spec 0003 "Output kinds", "Output (ALSA)"): format choice and
//! packing, the open/fallback/reservation decisions over the [`PcmBackend`]
//! seam, and the [`PcmSink`] that implements [`crate::Sink`] on top of them.
//! The real backend is `alsa::AlsaBackend` (feature `alsa`).

#[cfg(feature = "alsa")]
pub mod alsa;
pub mod clock;
#[cfg(test)]
mod fake;
pub mod format;
pub mod open;
pub mod pcm_sink;
pub mod reserve;

#[cfg(feature = "alsa")]
pub use alsa::{AlsaBackend, alsa_sink_factory};
pub use clock::{Clock, SystemClock};
pub use format::{choose_format, pack, pack_into};
pub use open::{FallbackMemo, HwConfig, Opened, PcmBackend, PcmError, open_output};
pub use pcm_sink::{PcmSink, PcmSinkFactory};
pub use reserve::{NoReserver, ReleaseReply, Reserver};
