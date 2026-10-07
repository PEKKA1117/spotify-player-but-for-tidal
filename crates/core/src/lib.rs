//! See docs/specs/0001-architecture.md.

pub mod protocol;
pub mod quality;
pub mod ui;

pub use quality::{AudioQuality, ParseQualityError};
