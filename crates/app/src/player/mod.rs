//! The player side (spec 0001 "Application pattern"): [`runtime`] runs the
//! pure player and executes its effects; the other modules are what those
//! effects reach. Only a process that plays (standalone, daemon, `play`)
//! uses them.

pub mod cover_cache;
pub mod hook;
pub mod http_source;
pub mod mpris;
pub mod persist;
pub mod playback;
pub mod reserve;
pub mod runtime;
