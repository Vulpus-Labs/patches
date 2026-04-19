//! FDN (Feedback Delay Network) reverb module.
//!
//! An 8-line FDN with Hadamard mixing matrix, per-line high-shelf absorption
//! (MonoBiquad), Thiran all-pass interpolation for LFO-modulated delay reads,
//! and stereo output via orthogonal output gain vectors.
//!
//! Defines [`FdnReverb`] (stereo in/out).
//!
//! # Inputs
//!
//! | Port | Kind | Description |
//! |------|------|-------------|
//! | `in_left` | mono | Left audio input |
//! | `in_right` | mono | Right audio input |
//! | `size_cv` | mono | Additive CV for size |
//! | `brightness_cv` | mono | Additive CV for brightness |
//! | `pre_delay_cv` | mono | Additive CV for pre-delay |
//! | `mix_cv` | mono | Additive CV for dry/wet mix |
//!
//! # Outputs
//!
//! | Port | Kind | Description |
//! |------|------|-------------|
//! | `out_left` | mono | Left reverb output |
//! | `out_right` | mono | Right reverb output |
//!
//! # Parameters
//!
//! | Name | Type | Range | Default | Description |
//! |------|------|-------|---------|-------------|
//! | `size` | float | 0.0--1.0 | `0.5` | Room size |
//! | `brightness` | float | 0.0--1.0 | `0.5` | High-frequency damping |
//! | `pre_delay` | float | 0.0--1.0 | `0.0` | Pre-delay amount |
//! | `mix` | float | 0.0--1.0 | `1.0` | Dry/wet mix |
//! | `character` | enum | plate/room/chamber/hall/cathedral | `hall` | Reverb archetype |

mod line;
mod matrix;
pub mod params;
mod processor;

use crate::common::delay_buffer::{DelayBuffer, ThiranInterp};
use crate::common::phase_accumulator::MonoPhaseAccumulator;
use patches_core::{InstanceId, ModuleDescriptor, MonoInput, MonoOutput};
use patches_dsp::MonoBiquad;

use matrix::LINES;
use params::ScaledCharacter;

/// Stereo FDN reverb with 8 delay lines, Hadamard mixing, per-line high-shelf
/// absorption, and Thiran all-pass interpolation for LFO-modulated reads.
///
/// See [module-level documentation](self).
pub struct FdnReverb {
    pub(in crate::fdn_reverb) instance_id:  InstanceId,
    pub(in crate::fdn_reverb) descriptor:   ModuleDescriptor,
    // Ports
    pub(in crate::fdn_reverb) in_l:             MonoInput,
    pub(in crate::fdn_reverb) in_r:             MonoInput,
    pub(in crate::fdn_reverb) in_size_cv:       MonoInput,
    pub(in crate::fdn_reverb) in_brightness_cv: MonoInput,
    pub(in crate::fdn_reverb) in_pre_delay_cv:  MonoInput,
    pub(in crate::fdn_reverb) in_mix_cv:        MonoInput,
    pub(in crate::fdn_reverb) out_l: MonoOutput,
    pub(in crate::fdn_reverb) out_r: MonoOutput,
    // Parameters
    pub(in crate::fdn_reverb) size_param:      f32,
    pub(in crate::fdn_reverb) bright_param:    f32,
    pub(in crate::fdn_reverb) pre_delay_param: f32,
    pub(in crate::fdn_reverb) mix_param:       f32,
    pub(in crate::fdn_reverb) character:       usize,
    // Audio state
    pub(in crate::fdn_reverb) sample_rate:   f32,
    pub(in crate::fdn_reverb) sr_recip:      f32,
    pub(in crate::fdn_reverb) interval_recip: f32,
    // Delay infrastructure
    pub(in crate::fdn_reverb) delays:     [DelayBuffer; LINES],
    pub(in crate::fdn_reverb) thiran:     [ThiranInterp; LINES],
    pub(in crate::fdn_reverb) absorption: [MonoBiquad;  LINES],
    // LFO phase accumulators (unit-range [0,1), increment cached on character change)
    pub(in crate::fdn_reverb) lfo_phases: [MonoPhaseAccumulator; LINES],
    // Pre-delay (always two buffers; see prepare notes)
    pub(in crate::fdn_reverb) pre_l: DelayBuffer,
    pub(in crate::fdn_reverb) pre_r: DelayBuffer,
    // T-0185: SR-scaled character values, rebuilt on character change
    pub(in crate::fdn_reverb) sc: ScaledCharacter,
    // T-0180: skip recompute_absorption when CV unconnected and params unchanged
    pub(in crate::fdn_reverb) absorption_dirty: bool,
    // T-0179: cached derived scale to avoid per-sample derive_params (powf)
    pub(in crate::fdn_reverb) cached_scale:    f32,
    pub(in crate::fdn_reverb) last_eff_size:   f32,
    pub(in crate::fdn_reverb) last_eff_bright: f32,
    pub(in crate::fdn_reverb) last_character:  usize,
    // Connectivity flags (derived from set_connectivity and set_ports)
    pub(in crate::fdn_reverb) stereo_in:  bool,
    pub(in crate::fdn_reverb) stereo_out: bool,
}

#[cfg(test)]
mod tests;
