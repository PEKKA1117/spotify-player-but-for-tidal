//! Audio engine for tidal-player: decode and output. See
//! docs/specs/0001-architecture.md and docs/specs/0003-playback-engine.md.
//!
//! The API is synchronous and the crate does not depend on tokio.

pub mod output;
pub mod sink;

#[cfg(feature = "alsa")]
pub use output::{AlsaBackend, alsa_sink_factory};
pub use output::{
    Clock, FallbackMemo, NoReserver, PcmBackend, PcmError, PcmSink, PcmSinkFactory, ReleaseReply,
    Reserver, SystemClock,
};
pub use sink::{
    Codec, MemorySink, OutputInfo, OutputKind, SampleFormat, Sink, SinkError, SinkFactory,
    SourceFormat, WriteOutcome,
};
