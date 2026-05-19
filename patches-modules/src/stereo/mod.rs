//! Stereo utility module group (ADR 0076).
//!
//! Image-control utilities: pan, balance, width, mid/side encode-decode,
//! and a sum-mono-bass crossover. Existing `StereoSplitter` /
//! `StereoJoiner` / `StereoSum` will migrate into this group under the
//! source-tree reorganisation ticket 0922.

pub mod balance;
pub mod common;
pub mod mid_side;
pub mod mono_bass;
pub mod pan;
pub mod stereo_width;

pub use balance::Balance;
pub use mid_side::MidSide;
pub use mono_bass::MonoBass;
pub use pan::Pan;
pub use stereo_width::StereoWidth;
