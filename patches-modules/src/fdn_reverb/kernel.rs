//! Pure DSP kernel for the FDN reverb.
//!
//! Holds all per-sample state (delay lines, Thiran interps, absorption biquads,
//! LFO accumulators, pre-delay buffers) and exposes a per-sample entry point
//! that consumes already-effective parameter values. The `Module` wrapper in
//! `processor.rs` is responsible for cable I/O, parameter resolution, and CV
//! summation; everything DSP lives here.

use patches_core::AudioEnvironment;
use patches_dsp::BiquadN;

use crate::common::delay_buffer::DelayBuffer;

use super::line::{absorption_coeffs, BASE_MS};
use super::matrix::{hadamard8, INV_SQRT8, LINES};
use super::params::{derive_params, ScaledCharacter, CHARS, MAX_LINE_SECS, MAX_PRE_DELAY_SECS};

pub(super) struct FdnReverbKernel {
    pub(super) sample_rate: f32,
    pub(super) sr_recip: f32,
    pub(super) interval_recip: f32,

    pub(super) character: usize,

    pub(super) delays: [DelayBuffer; LINES],
    /// 8-voice SoA biquad covering all line absorption filters. Each
    /// per-sample tick runs the TDFII recurrence as four loops over N=8,
    /// which LLVM auto-vectorises (NEON 4-lane × 2 passes, AVX2 8-lane × 1).
    pub(super) absorption: BiquadN<LINES>,
    /// SoA LFO phase state — `[f32; 8]` so the per-sample sine, advance, and
    /// offset-compute loops auto-vectorise alongside the biquad stage.
    pub(super) lfo_phase: [f32; LINES],
    pub(super) lfo_inc: f32,
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
        let mut absorption = BiquadN::<LINES>::new_static(0.0, 0.0, 0.0, 0.0, 0.0);
        for (i, &base_ms) in BASE_MS.iter().enumerate() {
            let (b0, b1, b2, a1, a2) =
                absorption_coeffs(base_ms, scale0, rt60_lf0, rt60_hf0, cross0, sr, sr_recip);
            absorption.set_static_voice(i, b0, b1, b2, a1, a2);
        }

        let delays = std::array::from_fn(|_| DelayBuffer::for_duration(MAX_LINE_SECS, sr));

        let pre_l = DelayBuffer::for_duration(MAX_PRE_DELAY_SECS, sr);
        let pre_r = DelayBuffer::for_duration(MAX_PRE_DELAY_SECS, sr);

        let lfo_inc = CHARS[character].lfo_rate_hz / sr;
        let lfo_phase = std::array::from_fn(|i| i as f32 / LINES as f32);

        Self {
            sample_rate: sr,
            sr_recip,
            interval_recip: 1.0 / env.periodic_update_interval as f32,
            character,
            delays,
            absorption,
            lfo_phase,
            lfo_inc,
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
        self.lfo_inc = CHARS[new_char].lfo_rate_hz / self.sample_rate;
    }

    fn recompute_absorption(&mut self, size: f32, bright: f32) {
        let (scale, rt60_lf, rt60_hf, crossover) = derive_params(size, bright, self.character);
        for (i, &base_ms) in BASE_MS.iter().enumerate() {
            let (b0, b1, b2, a1, a2) = absorption_coeffs(
                base_ms, scale, rt60_lf, rt60_hf, crossover, self.sample_rate, self.sr_recip,
            );
            self.absorption
                .begin_ramp_voice(i, b0, b1, b2, a1, a2, self.interval_recip);
        }
    }

    fn apply_static_absorption(&mut self, size: f32, bright: f32) {
        let (scale, rt60_lf, rt60_hf, crossover) = derive_params(size, bright, self.character);
        for (i, &base_ms) in BASE_MS.iter().enumerate() {
            let (b0, b1, b2, a1, a2) = absorption_coeffs(
                base_ms, scale, rt60_lf, rt60_hf, crossover, self.sample_rate, self.sr_recip,
            );
            self.absorption.set_static_voice(i, b0, b1, b2, a1, a2);
        }
        self.absorption.clear_has_cv();
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

        // SoA LFO sine: Bhaskara-with-Moser, expressed as independent per-lane
        // ops so LLVM autovectorises (NEON 4-lane × 2 passes for N=8).
        // The plain `for i in 0..N` indexing matches the pattern used by
        // BiquadN::tick_all and produces the cleanest auto-vectorised code.
        #[allow(clippy::needless_range_loop)]
        let lfo_val: [f32; LINES] = {
            let mut out = [0.0_f32; LINES];
            for i in 0..LINES {
                let x1 = self.lfo_phase[i] - 0.5;
                let x2 = x1 * 16.0 * (x1.abs() - 0.5);
                out[i] = x2 + 0.225 * x2 * (x2.abs() - 1.0);
            }
            out
        };
        // SoA phase advance with branchless wrap (single conditional subtract).
        // Increment is shared across lines and < 1.0, so one wrap suffices.
        let inc = self.lfo_inc;
        #[allow(clippy::needless_range_loop)]
        for i in 0..LINES {
            let next = self.lfo_phase[i] + inc;
            let wrap = if next >= 1.0 { 1.0 } else { 0.0 };
            self.lfo_phase[i] = next - wrap;
        }
        // SoA offset compute.
        #[allow(clippy::needless_range_loop)]
        let offset: [f32; LINES] = {
            let mut out = [0.0_f32; LINES];
            for i in 0..LINES {
                let base_samp = self.sc.base_samps[i] * scale;
                out[i] = (base_samp + self.sc.lfo_depth_samp * lfo_val[i])
                    .clamp(1.0, cap_max);
            }
            out
        };
        // Per-line delay reads remain serial — each line owns its own buffer
        // with an offset that's already computed.
        let mut raw = [0.0_f32; LINES];
        for (i, raw_i) in raw.iter_mut().enumerate() {
            *raw_i = self.delays[i].read_linear(offset[i]);
        }
        let damp = self.absorption.tick_all(&raw, false, self.absorption.has_cv);

        let f = hadamard8(damp);

        for (i, (&fi, delay)) in f.iter().zip(self.delays.iter_mut()).enumerate() {
            let inj = if i % 2 == 0 { x_l } else { x_r };
            delay.push(INV_SQRT8 * inj + fi);
        }

        // OUT_L pattern: + - + - + - + -  (× INV_SQRT8)
        // OUT_R pattern: + + - - + + - -  (× INV_SQRT8)
        // Fold INV_SQRT8 into the wet gain; the sums themselves are sign-only.
        let wet_l_raw = (damp[0] - damp[1]) + (damp[2] - damp[3])
                      + (damp[4] - damp[5]) + (damp[6] - damp[7]);
        let wet_r_raw = (damp[0] + damp[1]) - (damp[2] + damp[3])
                      + (damp[4] + damp[5]) - (damp[6] + damp[7]);
        let dry = 1.0 - eff_mix;
        let wet_g = eff_mix * INV_SQRT8;
        (dry * in_l + wet_g * wet_l_raw, dry * in_r + wet_g * wet_r_raw)
    }
}
