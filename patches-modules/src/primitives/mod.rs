//! Primitives module group (ADR 0076).
//!
//! Thin module wrappers over reusable DSP building blocks.

pub mod comb;
pub mod dc_blocker;

pub use comb::{Comb, CombMode};
pub use dc_blocker::DcBlocker;
