//! Per-sample processor loop and parameter machinery for [`super::FdnReverb`].

use patches_core::parameter_map::{ParameterMap, ParameterValue};
use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, Module, ModuleDescriptor, ModuleShape,
    MonoInput, MonoOutput, OutputPort, PeriodicUpdate,
};
use patches_dsp::MonoBiquad;

use crate::common::approximate::fast_sine;
use crate::common::delay_buffer::{DelayBuffer, ThiranInterp};
use crate::common::phase_accumulator::MonoPhaseAccumulator;

use super::line::{absorption_coeffs, BASE_MS};
use super::matrix::{hadamard8, INV_SQRT8, LINES, OUT_L, OUT_R};
use super::params::{
    derive_params, Character, ScaledCharacter, CHARS, MAX_LINE_SECS, MAX_PRE_DELAY_SECS,
};
use super::FdnReverb;

impl FdnReverb {
    /// Recompute absorption coefficients for all 8 lines and ramp to new targets.
    /// Used on the CV-connected path so coefficient changes interpolate smoothly.
    pub(super) fn recompute_absorption(&mut self, size: f32, bright: f32) {
        let (scale, rt60_lf, rt60_hf, crossover) =
            derive_params(size, bright, self.character);
        for (i, &base_ms) in BASE_MS.iter().enumerate() {
            let (b0, b1, b2, a1, a2) =
                absorption_coeffs(base_ms, scale, rt60_lf, rt60_hf, crossover, self.sample_rate, self.sr_recip);
            self.absorption[i].begin_ramp(b0, b1, b2, a1, a2, self.interval_recip);
        }
    }

    /// Apply absorption coefficients immediately (no interpolation, zeroes deltas).
    /// Used on the static path (no CV) to avoid drift from non-zero per-sample deltas.
    pub(super) fn apply_static_absorption(&mut self, size: f32, bright: f32) {
        let (scale, rt60_lf, rt60_hf, crossover) =
            derive_params(size, bright, self.character);
        for (i, &base_ms) in BASE_MS.iter().enumerate() {
            let (b0, b1, b2, a1, a2) =
                absorption_coeffs(base_ms, scale, rt60_lf, rt60_hf, crossover, self.sample_rate, self.sr_recip);
            self.absorption[i].set_static(b0, b1, b2, a1, a2);
        }
    }
}

impl Module for FdnReverb {
    fn describe(_shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("FdnReverb", ModuleShape { channels: 0, length: 0, ..Default::default() })
            .mono_in("in_left")
            .mono_in("in_right")
            .mono_in("size_cv")
            .mono_in("brightness_cv")
            .mono_in("pre_delay_cv")
            .mono_in("mix_cv")
            .mono_out("out_left")
            .mono_out("out_right")
            .float_param("size",       0.0, 1.0, 0.5)
            .float_param("brightness", 0.0, 1.0, 0.5)
            .float_param("pre_delay",  0.0, 1.0, 0.0)
            .float_param("mix",        0.0, 1.0, 1.0)
            .enum_param("character", Character::VARIANTS, "hall")
    }

    fn prepare(env: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId) -> Self {
        let sr       = env.sample_rate;
        let sr_recip = sr.recip();
        let char_idx = 3; // "hall" default
        // Initial absorption at default parameters (size=0.5, brightness=0.5, hall).
        let (scale0, rt60_lf0, rt60_hf0, cross0) = derive_params(0.5, 0.5, char_idx);
        let absorption = std::array::from_fn(|i| {
            let (b0, b1, b2, a1, a2) =
                absorption_coeffs(BASE_MS[i], scale0, rt60_lf0, rt60_hf0, cross0, sr, sr_recip);
            MonoBiquad::new(b0, b1, b2, a1, a2)
        });

        // All delay lines are sized for the worst-case archetype (cathedral) so
        // the character parameter can be changed without reallocating.
        let delays  = std::array::from_fn(|_| DelayBuffer::for_duration(MAX_LINE_SECS, sr));
        let thiran  = std::array::from_fn(|_| ThiranInterp::new());

        // Both pre-delay buffers are always allocated to avoid audio-thread
        // allocation when transitioning to stereo-in mode (memory cost: one extra
        // ~80 ms buffer).
        let pre_l = DelayBuffer::for_duration(MAX_PRE_DELAY_SECS, sr);
        let pre_r = DelayBuffer::for_duration(MAX_PRE_DELAY_SECS, sr);

        // Initial LFO phases staggered evenly across the unit range; increment
        // cached here so process() never recomputes it.
        let lfo_inc = CHARS[char_idx].lfo_rate_hz / sr;
        let lfo_phases = std::array::from_fn(|i| {
            let mut acc = MonoPhaseAccumulator::new();
            acc.phase = i as f32 / LINES as f32;
            acc.phase_increment = lfo_inc;
            acc
        });

        Self {
            instance_id,
            descriptor,
            in_l:             MonoInput::default(),
            in_r:             MonoInput::default(),
            in_size_cv:       MonoInput::default(),
            in_brightness_cv: MonoInput::default(),
            in_pre_delay_cv:  MonoInput::default(),
            in_mix_cv:        MonoInput::default(),
            out_l:            MonoOutput::default(),
            out_r:            MonoOutput::default(),
            size_param:       0.5,
            bright_param:     0.5,
            pre_delay_param:  0.0,
            mix_param:        1.0,
            character:        char_idx,
            sample_rate:      sr,
            sr_recip,
            interval_recip:   1.0 / env.periodic_update_interval as f32,
            delays,
            thiran,
            absorption,
            lfo_phases,
            pre_l,
            pre_r,
            sc:               ScaledCharacter::new(char_idx, sr),
            absorption_dirty: false,
            cached_scale:    scale0,
            last_eff_size:   0.5,
            last_eff_bright: 0.5,
            last_character:  char_idx,
            stereo_in:  false,
            stereo_out: false,
        }
    }

    fn update_validated_parameters(&mut self, params: &ParameterMap) {
        if let Some(ParameterValue::Float(v)) = params.get_scalar("size") {
            if self.size_param != *v {
                self.size_param = *v;
                self.absorption_dirty = true;
            }
        }
        if let Some(ParameterValue::Float(v)) = params.get_scalar("brightness") {
            if self.bright_param != *v {
                self.bright_param = *v;
                self.absorption_dirty = true;
            }
        }
        if let Some(ParameterValue::Float(v)) = params.get_scalar("pre_delay") {
            self.pre_delay_param = *v;
        }
        if let Some(ParameterValue::Float(v)) = params.get_scalar("mix") {
            self.mix_param = *v;
        }
        if let Some(&ParameterValue::Enum(v)) = params.get_scalar("character") {
            let new_char = Character::try_from(v).unwrap_or(Character::Hall) as usize;
            if self.character != new_char {
                self.character = new_char;
                self.sc = ScaledCharacter::new(new_char, self.sample_rate);
                self.absorption_dirty = true;
                let new_inc = CHARS[new_char].lfo_rate_hz / self.sample_rate;
                for acc in &mut self.lfo_phases {
                    acc.phase_increment = new_inc;
                }
            }
        }
    }

    fn descriptor(&self) -> &ModuleDescriptor { &self.descriptor }
    fn instance_id(&self) -> InstanceId { self.instance_id }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        self.in_l             = inputs[0].expect_mono();
        self.in_r             = inputs[1].expect_mono();
        self.in_size_cv       = inputs[2].expect_mono();
        self.in_brightness_cv = inputs[3].expect_mono();
        self.in_pre_delay_cv  = inputs[4].expect_mono();
        self.in_mix_cv        = inputs[5].expect_mono();
        self.out_l = outputs[0].expect_mono();
        self.out_r = outputs[1].expect_mono();
        // Derive connectivity flags from port state; set_connectivity may override.
        self.stereo_in  = self.in_r.is_connected();
        self.stereo_out = self.out_r.is_connected();
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        // ── CV reads ──────────────────────────────────────────────────────────
        let size_cv   = if self.in_size_cv.is_connected()
            { pool.read_mono(&self.in_size_cv) }   else { 0.0 };
        let bright_cv = if self.in_brightness_cv.is_connected()
            { pool.read_mono(&self.in_brightness_cv) } else { 0.0 };
        let pre_delay_cv = if self.in_pre_delay_cv.is_connected()
            { pool.read_mono(&self.in_pre_delay_cv) } else { 0.0 };
        let mix_cv = if self.in_mix_cv.is_connected()
            { pool.read_mono(&self.in_mix_cv) } else { 0.0 };
        let eff_size      = (self.size_param      + size_cv).clamp(0.0, 1.0);
        let eff_bright    = (self.bright_param    + bright_cv).clamp(0.0, 1.0);
        let eff_pre_delay = (self.pre_delay_param + pre_delay_cv).clamp(0.0, 1.0);
        let eff_mix       = (self.mix_param       + mix_cv).clamp(0.0, 1.0);

        // ── Scale cache (T-0179) ──────────────────────────────────────────────
        // derive_params contains a powf; skip when inputs haven't changed.
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

        // ── Pre-delay ─────────────────────────────────────────────────────────
        let in_l    = if self.in_l.is_connected() { pool.read_mono(&self.in_l) } else { 0.0 };
        let in_r    = if self.stereo_in           { pool.read_mono(&self.in_r) } else { in_l };
        let pre_cap = self.pre_l.capacity() - 1;
        let pre_s   = (((eff_size + eff_pre_delay) * self.sc.max_pre_delay_samp) as usize)
                      .clamp(1, pre_cap);

        self.pre_l.push(in_l);
        let x_l = self.pre_l.read_nearest(pre_s);
        let x_r = if self.stereo_in {
            self.pre_r.push(in_r);
            self.pre_r.read_nearest(pre_s)
        } else {
            x_l
        };

        // ── LFO-modulated delay reads via Thiran ──────────────────────────────
        let scale   = self.cached_scale;
        let cap_max = self.delays[0].capacity() as f32 - 2.0;

        let mut raw = [0.0_f32; LINES];
        for (i, raw_i) in raw.iter_mut().enumerate() {
            let lfo_val = fast_sine(self.lfo_phases[i].phase);
            self.lfo_phases[i].advance();
            let base_samp = self.sc.base_samps[i] * scale;
            let offset    = (base_samp + self.sc.lfo_depth_samp * lfo_val).clamp(1.0, cap_max);
            *raw_i = self.thiran[i].read(&self.delays[i], offset);
        }

        // ── Per-line absorption ───────────────────────────────────────────────
        let mut damp = [0.0_f32; LINES];
        for i in 0..LINES {
            damp[i] = self.absorption[i].tick(raw[i], false);
        }

        // ── Hadamard feedback mixing ──────────────────────────────────────────
        let f = hadamard8(damp);

        // ── Write into delay lines ────────────────────────────────────────────
        // Lines 0,2,4,6 → x_l; lines 1,3,5,7 → x_r (interleaved injection).
        for (i, (&fi, delay)) in f.iter().zip(self.delays.iter_mut()).enumerate() {
            let inj = if i % 2 == 0 { x_l } else { x_r };
            delay.push(INV_SQRT8 * inj + fi);
        }

        // ── Dry/wet mix and outputs ──────────────────────────────────────────
        let dry = 1.0 - eff_mix;
        let wet = eff_mix;
        if self.stereo_out {
            let mut wet_l = 0.0_f32;
            let mut wet_r = 0.0_f32;
            for i in 0..LINES {
                wet_l += OUT_L[i] * damp[i];
                wet_r += OUT_R[i] * damp[i];
            }
            if self.out_l.is_connected() { pool.write_mono(&self.out_l, dry * in_l + wet * wet_l); }
            if self.out_r.is_connected() { pool.write_mono(&self.out_r, dry * in_r + wet * wet_r); }
        } else {
            let wet_mono: f32 = damp.iter().sum::<f32>() * INV_SQRT8;
            if self.out_l.is_connected() { pool.write_mono(&self.out_l, dry * in_l + wet * wet_mono); }
        }
    }

    fn as_any(&self) -> &dyn std::any::Any { self }

    fn as_periodic(&mut self) -> Option<&mut dyn PeriodicUpdate> {
        Some(self)
    }
}

impl PeriodicUpdate for FdnReverb {
    fn periodic_update(&mut self, pool: &CablePool<'_>) {
        let size_cv   = if self.in_size_cv.is_connected()       { pool.read_mono(&self.in_size_cv) }       else { 0.0 };
        let bright_cv = if self.in_brightness_cv.is_connected() { pool.read_mono(&self.in_brightness_cv) } else { 0.0 };
        let eff_size   = (self.size_param  + size_cv).clamp(0.0, 1.0);
        let eff_bright = (self.bright_param + bright_cv).clamp(0.0, 1.0);

        let cv_connected =
            self.in_size_cv.is_connected() || self.in_brightness_cv.is_connected();

        // On the dirty+static path, use set_static (no interpolation) to avoid
        // non-zero per-sample deltas that would drift the filter coefficients.
        // On the CV path or dirty+CV path, use begin_ramp for smoothness.
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
}
