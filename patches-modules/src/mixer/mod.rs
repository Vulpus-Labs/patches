//! Mixer modules: [`Mixer`], [`StereoMixer`], [`PolyMixer`], [`StereoPolyMixer`].
//!
//! All four share the same channel-count-driven shape (`ModuleShape::channels`)
//! and mute/solo semantics: if any channel is soloed, only soloed channels that
//! are not muted contribute to the output. Mute wins over solo.
//!
//! Pan law (stereo variants): linear equal-gain.
//! `left_gain  = (1 - pan) * 0.5`
//! `right_gain = (1 + pan) * 0.5`
//! At centre (pan = 0) both gains are 0.5 (-6 dBFS per side).
//!
//! See each struct's documentation for port and parameter tables.

mod mono;
mod stereo;
mod poly;
mod stereo_poly;

pub use mono::Mixer;
pub use stereo::StereoMixer;
pub use poly::PolyMixer;
pub use stereo_poly::StereoPolyMixer;

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests;
