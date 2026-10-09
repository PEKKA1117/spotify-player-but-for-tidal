//! MPRIS2 (spec 0010): the player published on the session bus as
//! `org.mpris.MediaPlayer2.tidal_player`, for desktop widgets, media keys
//! and `playerctl`. [`model`] is the pure view, [`covers`] the cover cache.

pub mod covers;
pub mod model;
