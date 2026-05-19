//! FDN (Feedback Delay Network) reverb module.
//!
//! An 8-line FDN with Hadamard mixing matrix, per-line high-shelf absorption
//! (MonoBiquad), linear interpolation for LFO-modulated delay reads, and
//! stereo output via orthogonal output gain vectors.
//!
//! Defines [`FdnReverb`] (stereo in/out).
//!
//! # Inputs
//!
//! | Port | Kind | Description |
//! |------|------|-------------|
//! | `in` | stereo | Stereo audio input (mono sources auto-broadcast per ADR 0059 §2) |
//! | `size_cv` | mono | Additive CV for size |
//! | `brightness_cv` | mono | Additive CV for brightness |
//! | `pre_delay_cv` | mono | Additive CV for pre-delay |
//! | `mix_cv` | mono | Additive CV for dry/wet mix |
//!
//! # Outputs
//!
//! | Port | Kind | Description |
//! |------|------|-------------|
//! | `out` | stereo | Stereo reverb output |
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

mod kernel;
mod line;
mod matrix;
pub mod params;
mod processor;

use patches_core::{InstanceId, ModuleDescriptor, MonoInput, StereoInput, StereoOutput};

use kernel::FdnReverbKernel;

/// Opaque kernel handle re-exported for microbenchmarks in `patches-profiling`.
/// Not part of the stable public API.
#[doc(hidden)]
pub mod bench_support {
    use patches_core::AudioEnvironment;
    use super::kernel::FdnReverbKernel;

    pub struct KernelHandle(FdnReverbKernel);

    pub fn new_kernel(env: &AudioEnvironment, character: usize) -> KernelHandle {
        KernelHandle(FdnReverbKernel::new(env, character))
    }

    #[inline]
    pub fn process_sample(
        k: &mut KernelHandle,
        in_l: f32, in_r: f32,
        eff_size: f32, eff_bright: f32,
        eff_pre_delay: f32, eff_mix: f32,
    ) -> (f32, f32) {
        k.0.process_sample(in_l, in_r, eff_size, eff_bright, eff_pre_delay, eff_mix)
    }
}

/// Stereo FDN reverb with 8 delay lines, Hadamard mixing, per-line high-shelf
/// absorption, and linear interpolation for LFO-modulated reads.
///
/// See [module-level documentation](self).
pub struct FdnReverb {
    pub(in crate::fdn_reverb) instance_id: InstanceId,
    pub(in crate::fdn_reverb) descriptor:  ModuleDescriptor,
    // Ports
    pub(in crate::fdn_reverb) in_stereo:        StereoInput,
    pub(in crate::fdn_reverb) in_size_cv:       MonoInput,
    pub(in crate::fdn_reverb) in_brightness_cv: MonoInput,
    pub(in crate::fdn_reverb) in_pre_delay_cv:  MonoInput,
    pub(in crate::fdn_reverb) in_mix_cv:        MonoInput,
    pub(in crate::fdn_reverb) out_stereo:       StereoOutput,
    // Parameters (resolved into effective values per-tick before kernel call)
    pub(in crate::fdn_reverb) size_param:      f32,
    pub(in crate::fdn_reverb) bright_param:    f32,
    pub(in crate::fdn_reverb) pre_delay_param: f32,
    pub(in crate::fdn_reverb) mix_param:       f32,
    // Pure DSP state.
    pub(in crate::fdn_reverb) kernel: FdnReverbKernel,
}

#[cfg(test)]
mod tests;
