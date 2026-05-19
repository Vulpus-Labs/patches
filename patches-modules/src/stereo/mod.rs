//! Stereo utility module group (ADR 0076).
//!
//! Image-control utilities: pan, balance, width, mid/side encode-decode,
//! sum-mono-bass crossover, plus the `StereoSplitter` / `StereoJoiner`
//! channel-pair adapters and `StereoSum` mixer.

pub mod balance;
pub mod common;
pub mod mid_side;
pub mod mono_bass;
pub mod pan;
pub mod stereo_split;
pub mod stereo_sum;
pub mod stereo_width;

pub use balance::Balance;
pub use mid_side::MidSide;
pub use mono_bass::MonoBass;
pub use pan::Pan;
pub use stereo_split::{StereoJoiner, StereoSplitter};
pub use stereo_sum::StereoSum;
pub use stereo_width::StereoWidth;
