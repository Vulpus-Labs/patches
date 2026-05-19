//! Oscillator / noise module group (ADR 0076).

pub mod noise;
pub mod oscillator;
pub mod poly_osc;

pub use noise::{Noise, PolyNoise};
pub use oscillator::{OscFmType, Oscillator};
pub use poly_osc::PolyOsc;
