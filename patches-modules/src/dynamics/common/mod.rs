//! Shared DSP kernels for the dynamics module group (ADR 0076).

pub mod comp_detector;
pub mod gate_detector;

pub use comp_detector::{CompDetector, DetectMode};
pub use gate_detector::GateDetector;
