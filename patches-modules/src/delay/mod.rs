//! Delay module group (ADR 0076).

#[allow(clippy::module_inception)]
pub mod delay;
pub mod stereo_delay;

pub use delay::Delay;
pub use stereo_delay::StereoDelay;
