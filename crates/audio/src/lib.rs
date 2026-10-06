//! Audio output engine for tidal-player. See docs/specs/0001-architecture.md.
//!
//! The API is synchronous and the crate does not depend on tokio.

#[cfg(feature = "alsa")]
pub mod alsa_sink;
pub mod sink;

pub use sink::{AudioFormat, MemorySink, Sink, SinkError};
