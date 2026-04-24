mod halfband;
pub use halfband::HalfbandFir;
pub use halfband::{DEFAULT_TAPS, DEFAULT_CENTRE};

mod interpolator;
pub use interpolator::HalfbandInterpolator;

mod delay_buffer;
pub use delay_buffer::{DelayBuffer, ThiranInterp, PolyDelayBuffer, PolyThiranInterp};

mod peak_window;
pub use peak_window::{PeakWindow, DEFAULT_PEAK_WINDOW_LEN};

mod tone_filter;
pub use tone_filter::ToneFilter;

mod tap_feedback_filter;
pub use tap_feedback_filter::TapFeedbackFilter;

pub mod approximate;
pub use approximate::{fast_tanh, lookup_sine, fast_sine, fast_exp2};

pub mod wavetable;
pub use wavetable::{SineTable, SINE_TABLE};

pub mod biquad;
pub use biquad::{MonoBiquad, PolyBiquad};

pub mod svf;
pub use svf::{SvfCoeffs, SvfKernel, SvfState, PolySvfKernel, svf_f, q_to_damp, stability_clamp};

pub mod ladder;
pub use ladder::{LadderCoeffs, LadderKernel, LadderVariant, PolyLadderKernel};

pub mod ota_ladder;
pub use ota_ladder::{OtaLadderCoeffs, OtaLadderKernel, OtaPoles, PolyOtaLadderKernel};

pub mod oscillator;
pub use oscillator::{MonoPhaseAccumulator, PolyPhaseAccumulator, polyblep, sync_blep_residual};

pub mod adsr;
pub use adsr::{AdsrCore, AdsrShape, AdsrStage};

pub mod noise;
pub use noise::{xorshift64, PinkFilter, BrownFilter};

pub mod fft;
pub use fft::RealPackedFft;

mod window_buffer;
pub use window_buffer::WindowBuffer;

pub mod slot_deck;

pub mod spectral_pitch_shift;
pub use spectral_pitch_shift::SpectralPitchShifter;

pub mod partitioned_convolution;
pub use partitioned_convolution::{PartitionedConvolver, IrPartitions, NonUniformConvolver};

pub mod sinc_resample;
pub use sinc_resample::resample;

mod atomic_f32;
pub use atomic_f32::AtomicF32;

mod bitcrusher;
pub use bitcrusher::BitcrusherKernel;

mod dc_blocker;
pub use dc_blocker::DcBlocker;

mod limiter_core;
pub use limiter_core::LimiterCore;

mod envelope_follower;
pub use envelope_follower::EnvelopeFollower;

pub mod coef_ramp;
pub use coef_ramp::{CoefRamp, CoefTargets, PolyCoefRamp, PolyCoefTargets};

pub mod time_utils;
pub use time_utils::{ms_to_samples, compute_time_coeff};

pub mod drum;
pub use drum::{DecayEnvelope, PitchSweep, MetallicTone, BurstGenerator, saturate};

/// Flush subnormal floats to zero.
///
/// Audio filters with a feedback path can settle into subnormal values after
/// long stretches of silence; on x86 these trigger microcode traps that cost
/// tens of cycles per operation. Flushing to zero avoids the stall with no
/// audible effect.
#[inline]
pub fn flush_denormal(x: f32) -> f32 {
    if !x.is_normal() && x != 0.0 {
        0.0
    } else {
        x
    }
}

#[cfg(test)]
mod test_support;
