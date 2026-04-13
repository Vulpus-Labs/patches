//! Zero-cost accessor structs for structured poly frames (ADR 0033).
//!
//! Poly cables carry `[f32; 16]` — sixteen lanes of untyped floating-point
//! data. These accessor structs give lanes named, typed access points for
//! specific structured frame formats.

mod midi_frame;
mod transport_frame;

pub use midi_frame::MidiFrame;
pub use transport_frame::TransportFrame;
