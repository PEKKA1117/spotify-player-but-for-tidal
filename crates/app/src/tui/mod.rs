//! The terminal client: [`input`] decodes key presses into the UI model's
//! actions, [`render`] draws its state, and [`cover`] loads the album cover
//! the playback window shows. The model itself is `tidal_player_core::ui`.

pub mod cover;
pub mod input;
pub mod render;
