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
pub use approximate::{fast_tanh, lookup_sine, lookup_sine_q32, fast_sine, fast_exp2};

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
pub use oscillator::{
    MonoPhaseAccumulator, PolyPhaseAccumulator, MonoPhaseAccumulatorQ32, PolyPhaseAccumulatorQ32,
    polyblep,
};

pub mod hypersaw;
pub use hypersaw::{HyperSawCore, N_COPIES, N_PAIRS, N_VOICES};

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

/// Enables hardware flush-to-zero / denormals-as-zero for the lifetime of the
/// guard, restoring the previous mode on drop.
///
/// Subnormal floats trigger microcode fallback paths that cost 10–100× a
/// normal op. In an audio graph this manifests as CPU rising during silence
/// (reverb tails, idle voices with open envelopes). Setting FTZ/DAZ at the
/// top of the audio callback eliminates the cliff at the cost of one
/// register write per buffer.
///
/// Construct once at the top of a block handler and bind to a named local so
/// it lives for the whole block — never per sample. `Drop` writes the saved
/// mode back, so FTZ is a scoped property of "running the DSP graph for this
/// block" on whatever thread constructs the guard. Because ADR 0051 mandates
/// `panic = "unwind"`, the restore also runs when a module panics mid-block,
/// rather than leaking the flipped mode to the host's thread.
///
/// Per-thread, per-callback. Some hosts reset MXCSR between callbacks, so
/// construct on every entry, not once at startup. No-op (still constructs and
/// drops cleanly) on architectures without a denormal-flushing mode.
///
/// Not IEEE-strict: subnormals (~< 1.18e-38 f32) become zero. Inaudible
/// (~-700 dBFS) but breaks bit-exactness with hardware that doesn't have
/// FTZ enabled. Audit determinism tests before enabling globally.
pub struct FtzGuard {
    /// Saved mode to restore on drop: MXCSR on x86; low bits of FPCR on
    /// aarch64; unused on arches without a denormal-flushing mode.
    #[cfg_attr(
        not(any(target_arch = "x86", target_arch = "x86_64", target_arch = "aarch64")),
        allow(dead_code)
    )]
    saved: u32,
}

impl FtzGuard {
    #[inline]
    pub fn enable() -> Self {
        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        {
            #[cfg(target_arch = "x86_64")]
            use core::arch::x86_64::{_mm_getcsr, _mm_setcsr};
            #[cfg(target_arch = "x86")]
            use core::arch::x86::{_mm_getcsr, _mm_setcsr};
            // FTZ = bit 15 (0x8000), DAZ = bit 6 (0x0040).
            let saved = unsafe { _mm_getcsr() };
            unsafe { _mm_setcsr(saved | 0x8040) };
            Self { saved }
        }
        #[cfg(target_arch = "aarch64")]
        {
            // FPCR.FZ = bit 24. Single bit covers both input and output
            // flushing; there is no separate DAZ on aarch64. The architectural
            // FPCR fits in 32 bits (upper bits RES0), so a u32 round-trips it.
            let saved: u64;
            unsafe {
                core::arch::asm!("mrs {}, fpcr", out(reg) saved, options(nomem, nostack));
                core::arch::asm!(
                    "msr fpcr, {}",
                    in(reg) saved | (1u64 << 24),
                    options(nomem, nostack)
                );
            }
            Self { saved: saved as u32 }
        }
        #[cfg(not(any(target_arch = "x86", target_arch = "x86_64", target_arch = "aarch64")))]
        {
            // No portable denormal-flush bit; leave defaults.
            Self { saved: 0 }
        }
    }
}

impl Drop for FtzGuard {
    #[inline]
    fn drop(&mut self) {
        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        {
            #[cfg(target_arch = "x86_64")]
            use core::arch::x86_64::_mm_setcsr;
            #[cfg(target_arch = "x86")]
            use core::arch::x86::_mm_setcsr;
            unsafe { _mm_setcsr(self.saved) };
        }
        #[cfg(target_arch = "aarch64")]
        {
            unsafe {
                core::arch::asm!(
                    "msr fpcr, {}",
                    in(reg) self.saved as u64,
                    options(nomem, nostack)
                );
            }
        }
    }
}

#[cfg(test)]
#[cfg(any(target_arch = "x86", target_arch = "x86_64", target_arch = "aarch64"))]
mod ftz_guard_tests {
    use super::FtzGuard;

    fn read_mode() -> u32 {
        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        {
            #[cfg(target_arch = "x86_64")]
            use core::arch::x86_64::_mm_getcsr;
            #[cfg(target_arch = "x86")]
            use core::arch::x86::_mm_getcsr;
            unsafe { _mm_getcsr() }
        }
        #[cfg(target_arch = "aarch64")]
        {
            let fpcr: u64;
            unsafe {
                core::arch::asm!("mrs {}, fpcr", out(reg) fpcr, options(nomem, nostack));
            }
            fpcr as u32
        }
    }

    fn ftz_set(mode: u32) -> bool {
        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        {
            mode & 0x8040 == 0x8040
        }
        #[cfg(target_arch = "aarch64")]
        {
            mode & (1 << 24) != 0
        }
    }

    #[test]
    fn enable_sets_ftz_and_drop_restores() {
        let before = read_mode();
        {
            let _guard = FtzGuard::enable();
            assert!(ftz_set(read_mode()), "FTZ not set while guard held");
        }
        assert_eq!(read_mode(), before, "mode not restored on drop");
    }

    #[test]
    fn drop_restores_on_panic_unwind() {
        let before = read_mode();
        let result = std::panic::catch_unwind(|| {
            let _guard = FtzGuard::enable();
            assert!(ftz_set(read_mode()), "FTZ not set while guard held");
            panic!("unwind through the guard");
        });
        assert!(result.is_err(), "closure should have panicked");
        assert_eq!(read_mode(), before, "mode not restored on unwind");
    }
}

#[cfg(test)]
mod test_support;
