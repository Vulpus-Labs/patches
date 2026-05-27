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
pub use biquad::{BiquadN, MonoBiquad, PolyBiquad};

pub mod svf;
pub use svf::{SvfCoeffs, SvfKernel, SvfState, PolySvfKernel, svf_f, q_to_damp, stability_clamp};

pub mod ladder;
pub use ladder::{LadderCoeffs, LadderKernel, LadderVariant, PolyLadderKernel};

pub mod ota_ladder;
pub use ota_ladder::{OtaLadderCoeffs, OtaLadderKernel, OtaPoles, PolyOtaLadderKernel};

pub mod oscillator;
pub use oscillator::{MonoPhaseAccumulator, PolyPhaseAccumulator, polyblep};

pub mod adsr;
pub use adsr::{AdsrCore, AdsrShape, AdsrStage};

pub mod multistage_env;
pub use multistage_env::{EnvCore, EnvPhase, Stage, MAX_STAGES};

pub mod noise;
pub use noise::{xorshift64, PinkFilter, BrownFilter};

pub mod fft;
pub use fft::RealPackedFft;

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

/// Enable hardware flush-to-zero / denormals-as-zero on the calling thread.
///
/// Subnormal floats trigger microcode fallback paths that cost 10–100× a
/// normal op. In an audio graph this manifests as CPU rising during silence
/// (reverb tails, idle voices with open envelopes). Setting FTZ/DAZ at the
/// top of the audio callback eliminates the cliff at the cost of one
/// register write per buffer.
///
/// Per-thread, per-callback. Some hosts reset MXCSR between callbacks, so
/// call this on every entry, not once at startup. No-op on architectures
/// without a denormal-flushing mode.
///
/// Not IEEE-strict: subnormals (~< 1.18e-38 f32) become zero. Inaudible
/// (~-700 dBFS) but breaks bit-exactness with hardware that doesn't have
/// FTZ enabled. Audit determinism tests before enabling globally.
#[inline]
pub fn enable_flush_to_zero() {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    {
        #[cfg(target_arch = "x86_64")]
        use core::arch::x86_64::{_mm_getcsr, _mm_setcsr};
        #[cfg(target_arch = "x86")]
        use core::arch::x86::{_mm_getcsr, _mm_setcsr};
        // FTZ = bit 15 (0x8000), DAZ = bit 6 (0x0040).
        unsafe { _mm_setcsr(_mm_getcsr() | 0x8040) };
    }
    #[cfg(target_arch = "aarch64")]
    {
        // FPCR.FZ = bit 24. Single bit covers both input and output flushing.
        unsafe {
            let mut fpcr: u64;
            core::arch::asm!("mrs {}, fpcr", out(reg) fpcr, options(nomem, nostack));
            fpcr |= 1u64 << 24;
            core::arch::asm!("msr fpcr, {}", in(reg) fpcr, options(nomem, nostack));
        }
    }
    // Other arches: no portable denormal-flush bit; leave defaults.
}

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
