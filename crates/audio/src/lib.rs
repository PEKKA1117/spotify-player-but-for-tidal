//! Audio engine for tidal-player: decode and output. See
//! docs/specs/0001-architecture.md and docs/specs/0003-playback-engine.md.
//!
//! The API is synchronous and the crate does not depend on tokio.

pub mod clock;
pub mod decode;
pub mod devices;
pub mod engine;
pub mod output;
pub mod sink;
pub mod source;
pub mod testing;
mod worker;

pub use engine::{
    ANSWER_WITHIN, Command, DEFAULT_RELEASE_PAUSED, Engine, EngineConfig, EngineError, EngineGone,
    Event, ReleaseRequests,
};
#[cfg(feature = "alsa")]
pub use output::{AlsaBackend, alsa_sink_factory};
pub use output::{
    Clock, FallbackMemo, NoReserver, PcmBackend, PcmError, PcmSink, PcmSinkFactory, ReleaseReply,
    Reserver, SystemClock,
};
pub use sink::{
    Codec, MemoryDevices, MemorySink, MemorySinkHandle, OutputInfo, OutputKind, SampleFormat, Sink,
    SinkCall, SinkError, SinkFactory, SinkScript, SourceFormat, WriteOutcome,
};
pub use source::{ReadOutcome, SegmentSpan, SourceError, SourceLayout, TrackSource};
