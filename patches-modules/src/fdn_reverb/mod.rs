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
mod params;

use crate::common::approximate::fast_sine;
use crate::common::delay_buffer::{DelayBuffer, ThiranInterp};
use patches_dsp::MonoBiquad;
use crate::common::phase_accumulator::MonoPhaseAccumulator;

use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, Module, ModuleDescriptor,
    MonoInput, MonoOutput, ModuleShape, OutputPort, PeriodicUpdate,
};
use patches_core::parameter_map::{ParameterMap, ParameterValue};

use line::{absorption_coeffs, BASE_MS};
use matrix::{hadamard8, INV_SQRT8, LINES, OUT_L, OUT_R};
use params::{
    char_index, derive_params, ScaledCharacter, CHARS, MAX_LINE_SECS, MAX_PRE_DELAY_SECS,
};

/// Stereo FDN reverb with 8 delay lines, Hadamard mixing, per-line high-shelf
/// absorption, and Thiran all-pass interpolation for LFO-modulated reads.
///
/// See [module-level documentation](self).
pub struct FdnReverb {
    instance_id:  InstanceId,
    descriptor:   ModuleDescriptor,
    // Ports
    in_l:             MonoInput,
    in_r:             MonoInput,
    in_size_cv:       MonoInput,
    in_brightness_cv: MonoInput,
    in_pre_delay_cv:  MonoInput,
    in_mix_cv:        MonoInput,
    out_l: MonoOutput,
    out_r: MonoOutput,
    // Parameters
    size_param:      f32,
    bright_param:    f32,
    pre_delay_param: f32,
    mix_param:       f32,
    character:       usize,
    // Audio state
    sample_rate:   f32,
    sr_recip:      f32,
    interval_recip: f32,
    // Delay infrastructure
    delays:     [DelayBuffer; LINES],
    thiran:     [ThiranInterp; LINES],
    absorption: [MonoBiquad;  LINES],
    // LFO phase accumulators (unit-range [0,1), increment cached on character change)
    lfo_phases: [MonoPhaseAccumulator; LINES],
    // Pre-delay (always two buffers; see prepare notes)
    pre_l: DelayBuffer,
    pre_r: DelayBuffer,
    // T-0185: SR-scaled character values, rebuilt on character change
    sc: ScaledCharacter,
    // T-0180: skip recompute_absorption when CV unconnected and params unchanged
    absorption_dirty: bool,
    // T-0179: cached derived scale to avoid per-sample derive_params (powf)
    cached_scale:    f32,
    last_eff_size:   f32,
    last_eff_bright: f32,
    last_character:  usize,
    // Connectivity flags (derived from set_connectivity and set_ports)
    stereo_in:  bool,
    stereo_out: bool,
}

impl FdnReverb {
    /// Recompute absorption coefficients for all 8 lines and ramp to new targets.
    /// Used on the CV-connected path so coefficient changes interpolate smoothly.
    fn recompute_absorption(&mut self, size: f32, bright: f32) {
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
    fn apply_static_absorption(&mut self, size: f32, bright: f32) {
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
            .enum_param("character", &["plate", "room", "chamber", "hall", "cathedral"], "hall")
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

    fn update_validated_parameters(&mut self, params: &mut ParameterMap) {
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
        if let Some(ParameterValue::Enum(v)) = params.get_scalar("character") {
            let new_char = char_index(v);
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

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::AudioEnvironment;
    use patches_core::test_support::{ModuleHarness, params};

    const SR: f32 = 44_100.0;

    fn make_fdn(char_name: &'static str, size: f32, brightness: f32) -> ModuleHarness {
        ModuleHarness::build_with_env::<FdnReverb>(
            params!["size" => size, "brightness" => brightness, "character" => char_name],
            AudioEnvironment { sample_rate: SR, poly_voices: 16, periodic_update_interval: 32, hosted: false },
        )
    }

    #[test]
    fn descriptor_ports_and_params() {
        let desc = FdnReverb::describe(&ModuleShape { channels: 0, length: 0, ..Default::default() });
        assert_eq!(desc.module_name, "FdnReverb");
        assert_eq!(desc.inputs.len(),  6);
        assert_eq!(desc.outputs.len(), 2);
        assert_eq!(desc.inputs[0].name,  "in_left");
        assert_eq!(desc.inputs[1].name,  "in_right");
        assert_eq!(desc.inputs[2].name,  "size_cv");
        assert_eq!(desc.inputs[3].name,  "brightness_cv");
        assert_eq!(desc.inputs[4].name,  "pre_delay_cv");
        assert_eq!(desc.inputs[5].name,  "mix_cv");
        assert_eq!(desc.outputs[0].name, "out_left");
        assert_eq!(desc.outputs[1].name, "out_right");
        let names: Vec<&str> = desc.parameters.iter().map(|p| p.name).collect();
        assert!(names.contains(&"size"));
        assert!(names.contains(&"brightness"));
        assert!(names.contains(&"pre_delay"));
        assert!(names.contains(&"mix"));
        assert!(names.contains(&"character"));
    }

    /// An impulse through every character: output stays bounded, is non-zero,
    /// and the late tail has lower RMS than the early tail (proper decay,
    /// not divergence or sustain).
    #[test]
    fn impulse_decays_for_all_characters() {
        for char_name in ["plate", "room", "chamber", "hall", "cathedral"] {
            let mut h = make_fdn(char_name, 0.5, 0.5);
            h.disconnect_input("in_right");
            h.disconnect_input("size_cv");
            h.disconnect_input("brightness_cv");
            h.disconnect_input("pre_delay_cv");
            h.disconnect_input("mix_cv");
            h.disconnect_output("out_right");

            h.set_mono("in_left", 1.0);
            h.tick();
            h.set_mono("in_left", 0.0);

            // 32k samples ≈ 0.74 s — enough for cathedral pre-delay plus
            // multiple delay-line passes; long enough to see clear decay.
            let n = 32_768;
            let out: Vec<f32> = (0..n).map(|_| { h.tick(); h.read_mono("out_left") }).collect();

            let peak = out.iter().map(|v| v.abs()).fold(0.0_f32, f32::max);
            assert!(peak.is_finite(), "character={char_name}: non-finite output");
            assert!(peak > 0.0, "character={char_name}: zero output after impulse");
            // Bounded response: with mix=0.5 and a unit impulse, output must
            // not exceed unity by more than a small headroom; runaway feedback
            // would blow past this.
            assert!(
                peak < 2.0,
                "character={char_name}: peak {peak} exceeds bounded-response limit"
            );

            // Decay check: RMS of the last quarter must be measurably smaller
            // than RMS of the first quarter after the pre-delay region.
            let q = n / 4;
            let early: f32 = out[q..2 * q].iter().map(|v| v * v).sum::<f32>() / q as f32;
            let late: f32 = out[3 * q..].iter().map(|v| v * v).sum::<f32>() / q as f32;
            assert!(
                early > 0.0 && late < early * 0.5,
                "character={char_name}: late RMS² ({late:.6e}) must be < 50% of early RMS² ({early:.6e}) — no decay"
            );
        }
    }

    /// A sustained DC input produces finite, non-zero output after settling.
    #[test]
    fn dc_input_produces_finite_output() {
        // Use plate (short delays, fastest settling) at small size.
        let mut h = make_fdn("plate", 0.1, 0.5);
        h.disconnect_input("in_right");
        h.disconnect_input("size_cv");
        h.disconnect_input("brightness_cv");
        h.disconnect_input("pre_delay_cv");
        h.disconnect_input("mix_cv");
        h.disconnect_output("out_right");

        let dc = 0.1_f32;
        h.set_mono("in_left", dc);
        let outputs: Vec<f32> = (0..4096).map(|_| { h.tick(); h.read_mono("out_left") }).collect();
        for (i, &v) in outputs.iter().enumerate() {
            assert!(v.is_finite(), "output[{i}] is not finite: {v}");
        }
        let max_out = outputs.iter().map(|v| v.abs()).fold(0.0_f32, f32::max);
        assert!(max_out > 0.0, "DC input produced no output");
        // A passive reverb driven by DC=0.1 must not amplify beyond a small
        // multiple of the input — guards against unstable feedback gain.
        assert!(
            max_out < dc * 10.0,
            "DC input {dc} produced unbounded output {max_out}"
        );
        // Steady state: last 256 samples should have small variance compared
        // to a comparable input excursion (system has settled, not oscillating
        // wildly).
        let tail = &outputs[outputs.len() - 256..];
        let mean = tail.iter().sum::<f32>() / tail.len() as f32;
        let var = tail.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / tail.len() as f32;
        assert!(
            var < (dc * dc),
            "DC steady-state variance {var:.6} too large vs input² {:.6}",
            dc * dc
        );
    }

    /// In mono mode (out_r disconnected), out_r's pool slot is never written.
    #[test]
    fn mono_mode_out_r_unchanged() {
        let mut h = make_fdn("hall", 0.5, 0.5);
        h.disconnect_input("in_right");
        h.disconnect_input("size_cv");
        h.disconnect_input("brightness_cv");
        h.disconnect_output("out_right");

        // Seed pool with a sentinel value — if out_r is written, it will change.
        h.init_pool(patches_core::CableValue::Mono(99.0));

        h.set_mono("in_left", 1.0);
        for _ in 0..64 {
            h.tick();
        }

        // out_r slot should still hold the sentinel (99.0).
        let out_r_val = h.read_mono("out_right");
        // After disconnect the port is not connected, so reads return the pool sentinel.
        // The precise check: the out_r cable slot must not have been written by the module.
        // Since we seeded with 99.0 and the module should skip out_r in mono mode, it stays 99.0.
        assert!(
            (out_r_val - 99.0).abs() < 1e-5,
            "out_r was written in mono mode: {out_r_val}"
        );
    }

    /// In stereo mode with mono input, out_l and out_r differ (channel decorrelation).
    #[test]
    fn stereo_output_decorrelation() {
        let mut h = make_fdn("hall", 0.5, 0.5);
        h.disconnect_input("in_right");
        h.disconnect_input("size_cv");
        h.disconnect_input("brightness_cv");
        h.disconnect_input("pre_delay_cv");
        h.disconnect_input("mix_cv");
        // Keep out_r connected; set_ports will set stereo_out = true.

        // Run enough samples for the reverb to build up.
        h.set_mono("in_left", 0.5);
        for _ in 0..2048 {
            h.tick();
        }
        let l = h.read_mono("out_left");
        let r = h.read_mono("out_right");

        assert!(l.is_finite() && r.is_finite(), "stereo output contains NaN/inf");
        // L and R should differ due to orthogonal output gain vectors.
        assert!(
            (l - r).abs() > 1e-6,
            "out_l ({l}) and out_r ({r}) are identical — no decorrelation"
        );
    }
}
