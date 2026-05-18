//! Dynamics module group (ADR 0076).
//!
//! Shared kernels live in [`common`]; module wrappers live in sibling
//! files (`compressor`, `stereo_compressor`, …). Existing dynamics
//! modules (`Limiter`, `StereoLimiter`, `TransientShaper`) will migrate
//! into this group under the source-tree reorganisation tickets.

pub mod common;
pub mod compressor;
pub mod gate;
pub mod stereo_compressor;
pub mod stereo_gate;

pub use compressor::{CompDetectMode, Compressor};
pub use gate::Gate;
pub use stereo_compressor::StereoCompressor;
pub use stereo_gate::StereoGate;
