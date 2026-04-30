//! Pure DSP kernel for the FDN reverb.
//!
//! Holds all per-sample state (delay lines, Thiran interps, absorption biquads,
//! LFO accumulators, pre-delay buffers) and exposes a per-sample entry point
//! that consumes already-effective parameter values. The `Module` wrapper in
//! `processor.rs` is responsible for cable I/O, parameter resolution, and CV
//! summation; everything DSP lives here.

use patches_core::AudioEnvironment;
use patches_dsp::MonoBiquad;

use crate::common::approximate::fast_sine;
use crate::common::delay_buffer::{DelayBuffer, ThiranInterp};
use crate::common::phase_accumulator::MonoPhaseAccumulator;

use super::line::{absorption_coeffs, BASE_MS};
use super::matrix::{hadamard8, INV_SQRT8, LINES, OUT_L, OUT_R};
use super::params::{derive_params, ScaledCharacter, CHARS, MAX_LINE_SECS, MAX_PRE_DELAY_SECS};

pub(super) struct FdnReverbKernel {
    pub(super) sample_rate: f32,
    pub(super) sr_recip: f32,
    pub(super) interval_recip: f32,

    pub(super) character: usize,

    pub(super) delays: [DelayBuffer; LINES],
    pub(super) thiran: [ThiranInterp; LINES],
    pub(super) absorption: [MonoBiquad; LINES],
    pub(super) lfo_phases: [MonoPhaseAccumulator; LINES],
    pub(super) pre_l: DelayBuffer,
    pub(super) pre_r: DelayBuffer,

    pub(super) sc: ScaledCharacter,

    pub(super) absorption_dirty: bool,
    pub(super) cached_scale: f32,
    pub(super) last_eff_size: f32,
    pub(super) last_eff_bright: f32,
    pub(super) last_character: usize,
}

impl FdnReverbKernel {
    pub(super) fn new(env: &AudioEnvironment, character: usize) -> Self {
        let sr = env.sample_rate;
        let sr_recip = sr.recip();
        let (scale0, rt60_lf0, rt60_hf0, cross0) = derive_params(0.5, 0.5, character);
        let absorption = std::array::from_fn(|i| {
            let (b0, b1, b2, a1, a2) =
                absorption_coeffs(BASE_MS[i], scale0, rt60_lf0, rt60_hf0, cross0, sr, sr_recip);
            MonoBiquad::new(b0, b1, b2, a1, a2)
        });

        let delays = std::array::from_fn(|_| DelayBuffer::for_duration(MAX_LINE_SECS, sr));
        let thiran = std::array::from_fn(|_| ThiranInterp::new());

        let pre_l = DelayBuffer::for_duration(MAX_PRE_DELAY_SECS, sr);
        let pre_r = DelayBuffer::for_duration(MAX_PRE_DELAY_SECS, sr);

        let lfo_inc = CHARS[character].lfo_rate_hz / sr;
        let lfo_phases = std::array::from_fn(|i| {
            let mut acc = MonoPhaseAccumulator::new();
            acc.phase = i as f32 / LINES as f32;
            acc.phase_increment = lfo_inc;
            acc
        });

        Self {
            sample_rate: sr,
            sr_recip,
            interval_recip: 1.0 / env.periodic_update_interval as f32,
            character,
            delays,
            thiran,
            absorption,
            lfo_phases,
            pre_l,
            pre_r,
            sc: ScaledCharacter::new(character, sr),
            absorption_dirty: false,
            cached_scale: scale0,
            last_eff_size: 0.5,
            last_eff_bright: 0.5,
            last_character: character,
        }
    }

    pub(super) fn mark_absorption_dirty(&mut self) {
        self.absorption_dirty = true;
    }

    pub(super) fn set_character(&mut self, new_char: usize) {
        if self.character == new_char {
            return;
        }
        self.character = new_char;
        self.sc = ScaledCharacter::new(new_char, self.sample_rate);
        self.absorption_dirty = true;
        let new_inc = CHARS[new_char].lfo_rate_hz / self.sample_rate;
        for acc in &mut self.lfo_phases {
            acc.phase_increment = new_inc;
        }
    }

    fn recompute_absorption(&mut self, size: f32, bright: f32) {
        let (scale, rt60_lf, rt60_hf, crossover) = derive_params(size, bright, self.character);
        for (i, &base_ms) in BASE_MS.iter().enumerate() {
            let (b0, b1, b2, a1, a2) = absorption_coeffs(
                base_ms, scale, rt60_lf, rt60_hf, crossover, self.sample_rate, self.sr_recip,
            );
            self.absorption[i].begin_ramp(b0, b1, b2, a1, a2, self.interval_recip);
        }
    }

    fn apply_static_absorption(&mut self, size: f32, bright: f32) {
        let (scale, rt60_lf, rt60_hf, crossover) = derive_params(size, bright, self.character);
        for (i, &base_ms) in BASE_MS.iter().enumerate() {
            let (b0, b1, b2, a1, a2) = absorption_coeffs(
                base_ms, scale, rt60_lf, rt60_hf, crossover, self.sample_rate, self.sr_recip,
            );
            self.absorption[i].set_static(b0, b1, b2, a1, a2);
        }
    }

    /// Periodic absorption recompute. `cv_connected` distinguishes the static
    /// (set immediate) path from the CV-driven (ramped) path.
    pub(super) fn periodic_update(&mut self, eff_size: f32, eff_bright: f32, cv_connected: bool) {
        if self.absorption_dirty {
            if cv_connected {
                self.recompute_absorption(eff_size, eff_bright);
            } else {
                self.apply_static_absorption(eff_size, eff_bright);
            }
            self.absorption_dirty = false;
        } else if cv_connected {
            self.recompute_absorption(eff_size, eff_bright);
        }
    }

    /// One-sample DSP step. All inputs are already clamped to [0, 1] for the
    /// 0..1 parameters; `in_l`/`in_r` are the raw stereo input.
    pub(super) fn process_sample(
        &mut self,
        in_l: f32,
        in_r: f32,
        eff_size: f32,
        eff_bright: f32,
        eff_pre_delay: f32,
        eff_mix: f32,
    ) -> (f32, f32) {
        if eff_size != self.last_eff_size
            || eff_bright != self.last_eff_bright
            || self.character != self.last_character
        {
            let (scale, _, _, _) = derive_params(eff_size, eff_bright, self.character);
            self.cached_scale = scale;
            self.last_eff_size = eff_size;
            self.last_eff_bright = eff_bright;
            self.last_character = self.character;
        }

        let pre_cap = self.pre_l.capacity() - 1;
        let pre_s = (((eff_size + eff_pre_delay) * self.sc.max_pre_delay_samp) as usize)
            .clamp(1, pre_cap);

        self.pre_l.push(in_l);
        self.pre_r.push(in_r);
        let x_l = self.pre_l.read_nearest(pre_s);
        let x_r = self.pre_r.read_nearest(pre_s);

        let scale = self.cached_scale;
        let cap_max = self.delays[0].capacity() as f32 - 2.0;

        let mut raw = [0.0_f32; LINES];
        for (i, raw_i) in raw.iter_mut().enumerate() {
            let lfo_val = fast_sine(self.lfo_phases[i].phase);
            self.lfo_phases[i].advance();
            let base_samp = self.sc.base_samps[i] * scale;
            let offset = (base_samp + self.sc.lfo_depth_samp * lfo_val).clamp(1.0, cap_max);
            *raw_i = self.thiran[i].read(&self.delays[i], offset);
        }

        let mut damp = [0.0_f32; LINES];
        for i in 0..LINES {
            damp[i] = self.absorption[i].tick(raw[i], false);
        }

        let f = hadamard8(damp);

        for (i, (&fi, delay)) in f.iter().zip(self.delays.iter_mut()).enumerate() {
            let inj = if i % 2 == 0 { x_l } else { x_r };
            delay.push(INV_SQRT8 * inj + fi);
        }

        let dry = 1.0 - eff_mix;
        let wet = eff_mix;
        let mut wet_l = 0.0_f32;
        let mut wet_r = 0.0_f32;
        for i in 0..LINES {
            wet_l += OUT_L[i] * damp[i];
            wet_r += OUT_R[i] * damp[i];
        }
        (dry * in_l + wet * wet_l, dry * in_r + wet * wet_r)
    }
}
