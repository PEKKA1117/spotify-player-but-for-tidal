//! Audio engine for tidal-player: decode and output. See
//! docs/specs/0001-architecture.md and docs/specs/0003-playback-engine.md.
//!
//! The API is synchronous and the crate does not depend on tokio.

pub mod sink;

pub use sink::{
    Codec, MemorySink, OutputInfo, OutputKind, SampleFormat, Sink, SinkError, SinkFactory,
    SourceFormat, WriteOutcome,
};
