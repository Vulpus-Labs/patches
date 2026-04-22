//! Shared DSP core for `VDco` and `VPolyDco`.
//!
//! One phase accumulator per voice drives saw, variable-width pulse, and a
//! phase-locked ÷2 sub square. A white-noise source and an internal mixer are
//! folded into the same module so the output is a single pre-mixed signal.
//!
//! PolyBLEP corrections are applied at each waveshape's own discontinuities;
//! the pulse comparator reads the raw phase (never the BLEP-corrected saw), so
//! duty cycle is bit-accurate.

use patches_dsp::{fast_exp2, polyblep, xorshift64};

/// Reference pitch: C0 ≈ 16.35 Hz. Duplicated from `patches-modules` to keep
/// `patches-vintage` free of the modules-crate dependency.
pub const C0_FREQ: f32 = 16.351_598;

/// Effective PWM range. Full travel (0 or 1) produces a DC-only pulse; the
/// clamp keeps the output audible at the extremes.
const PWM_MIN: f32 = 0.02;
const PWM_MAX: f32 = 0.98;

/// Internal scale on the noise source (≈ 0.5 full-scale at `noise_level = 1`).
const NOISE_SCALE: f32 = 0.5;

/// Mixer settings (biased, not equal — worst-case sum ≈ 3.5× a single source).
#[derive(Clone, Copy)]
pub struct VDcoMix {
    pub saw_on: bool,
    pub pulse_on: bool,
    pub sub_level: f32,
    pub noise_level: f32,
}

impl VDcoMix {
    pub const DEFAULT: Self = Self {
        saw_on: true,
        pulse_on: false,
        sub_level: 0.0,
        noise_level: 0.0,
    };
}

/// Per-voice mutable state: phase, increment, sub flip-flop, noise PRNG.
pub struct VDcoVoice {
    pub phase: f32,
    pub phase_increment: f32,
    pub sub_flipflop: bool,
    pub prng_state: u64,
}

impl VDcoVoice {
    pub fn new(seed: u64) -> Self {
        Self {
            phase: 0.0,
            phase_increment: 0.0,
            sub_flipflop: false,
            // xorshift64 requires non-zero state.
            prng_state: seed.wrapping_add(1),
        }
    }
}

/// Phase increment per sample for `voct` (V/oct relative to C0).
///
/// Clamped below 1.0 so `advance` can wrap with a single conditional subtract.
#[inline]
pub fn voct_to_increment(voct: f32, sample_rate: f32) -> f32 {
    let freq = C0_FREQ * fast_exp2(voct);
    (freq / sample_rate).min(0.999_999)
}

/// Advance the phase by `phase_increment`, wrapping to `[0, 1)`. Toggles the
/// sub flip-flop on each wrap so the ÷2 square stays phase-locked.
#[inline]
pub fn advance(voice: &mut VDcoVoice) {
    let next = voice.phase + voice.phase_increment;
    if next >= 1.0 {
        voice.phase = next - 1.0;
        voice.sub_flipflop = !voice.sub_flipflop;
    } else {
        voice.phase = next;
    }
}

/// Render one mixed sample for `voice` using the current phase, then advance.
///
/// `pwm` is the raw PWM CV (clamped internally). The saw, pulse, and sub all
/// derive from the same raw `phase`, each with its own polyBLEP correction.
#[inline]
pub fn render_and_advance(voice: &mut VDcoVoice, pwm: f32, mix: &VDcoMix) -> f32 {
    let phase = voice.phase;
    let dt = voice.phase_increment;

    let mut y = 0.0_f32;

    if mix.saw_on {
        y += (2.0 * phase - 1.0) - polyblep(phase, dt);
    }

    if mix.pulse_on {
        let pwm_c = pwm.clamp(PWM_MIN, PWM_MAX);
        let raw = if phase < pwm_c { 1.0 } else { -1.0 };
        let blep = polyblep(phase, dt) - polyblep((phase - pwm_c).rem_euclid(1.0), dt);
        y += raw + blep;
    }

    if mix.sub_level > 0.0 {
        // Sub: half-rate square phase-locked to main via the flip-flop.
        let sub_phase = phase * 0.5 + if voice.sub_flipflop { 0.5 } else { 0.0 };
        let sub_dt = dt * 0.5;
        let raw = if sub_phase < 0.5 { 1.0 } else { -1.0 };
        let blep =
            polyblep(sub_phase, sub_dt) - polyblep((sub_phase - 0.5).rem_euclid(1.0), sub_dt);
        y += mix.sub_level * (raw + blep);
    }

    if mix.noise_level > 0.0 {
        let w = xorshift64(&mut voice.prng_state);
        y += mix.noise_level * NOISE_SCALE * w;
    }

    advance(voice);
    y
}
