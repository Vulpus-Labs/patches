//! Dynamics module group (ADR 0076).
//!
//! Shared kernels live in [`common`]; module wrappers live in sibling
//! files (`compressor`, `limiter`, `transient_shaper`, …).

pub mod common;
pub mod compressor;
pub mod gate;
pub mod limiter;
pub mod stereo_compressor;
pub mod stereo_gate;
pub mod stereo_limiter;
pub mod transient_shaper;

pub use compressor::{CompDetectMode, Compressor};
pub use gate::Gate;
pub use limiter::Limiter;
pub use stereo_compressor::StereoCompressor;
pub use stereo_gate::StereoGate;
pub use stereo_limiter::StereoLimiter;
pub use transient_shaper::TransientShaper;
