//! See docs/specs/0001-architecture.md.

pub mod item;
pub mod player;
pub mod protocol;
pub mod quality;
pub mod track;
pub mod ui;

pub use item::{Item, ItemError};
pub use player::{PlayerConfig, PlayerEffect, PlayerInput, PlayerState};
pub use quality::{AudioQuality, ParseQualityError};
pub use track::{EntryId, Track, TrackId};
