//! Stereo N-channel mixer.
//!
//! Mute/solo semantics: if any channel is soloed, only soloed channels that
//! are not muted contribute to the output. Mute wins over solo.
//!
//! Pan law: linear equal-gain.
//! `left_gain  = (1 - pan) * 0.5`
//! `right_gain = (1 + pan) * 0.5`
//! At centre (pan = 0) both gains are 0.5 (-6 dBFS per side).
//!
//! See [`StereoMixer`] for port and parameter tables.

mod stereo;

pub use stereo::StereoMixer;

#[cfg(test)]
mod tests;
