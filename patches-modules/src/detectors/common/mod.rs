//! Shared DSP kernels for the audio-to-control detector group (ADR 0076).

pub mod edge;
pub mod gate_schmitt;

pub use edge::{EdgeDetector, EdgeDirection};
pub use gate_schmitt::GateSchmitt;
