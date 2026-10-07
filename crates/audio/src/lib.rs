//! Audio engine for tidal-player: decode and output. See
//! docs/specs/0001-architecture.md and docs/specs/0003-playback-engine.md.
//!
//! The API is synchronous and the crate does not depend on tokio.

pub mod clock;
pub mod decode;
pub mod engine;
pub mod sink;
pub mod source;
pub mod testing;
mod worker;

pub use engine::{Command, Engine, EngineConfig, EngineError, EngineGone, Event};
pub use sink::{
    Codec, MemoryDevices, MemorySink, MemorySinkHandle, OutputInfo, OutputKind, SampleFormat, Sink,
    SinkCall, SinkError, SinkFactory, SinkScript, SourceFormat, WriteOutcome,
};
pub use source::{ReadOutcome, SegmentSpan, SourceError, SourceLayout, TrackSource};
