//! Motor de audio do Sound Effects Stream Deck.
//!
//! Separado do nucleo para manter `soundbar-core` testavel sem hardware.
//! [`mixer`] e a logica pura de mixagem; [`output`] cuida do dispositivo real.

pub mod mixer;

#[cfg(target_os = "linux")]
pub mod pulse;

pub use mixer::{Mixer, Voice};
