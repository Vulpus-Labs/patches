//! Vintage BBD multi-tap delay module (CE-2 / Small-Clone territory).
//!
//! N independent BBD delay lines built on [`crate::bbd::Bbd`] with the
//! 1024-stage preset, bracketed per-tap by an NE570-style compander
//! ([`crate::compander`]): pre-BBD compressor → BBD → post-BBD
//! expander. Each tap has its own delay, gain, and self-feedback.
//! Character lives in the BBD core (charge-transfer HF rolloff, gentle
//! bucket saturation) and the compander (program-dependent hiss);
//! there is deliberately no tone or drive control.
//!
//! Tap layout mirrors [`Delay`] in `patches-modules`: one audio input,
//! N taps, one summed wet output mixed with the dry signal. Unlike
//! `Delay`, there are no `send`/`return` ports — the BBD's colour is
//! the point, so the signal flow is sealed.
//!
//! # Inputs
//!
//! | Port | Kind | Description |
//! |------|------|-------------|
//! | `in` | mono | Audio input |
//! | `drywet_cv` | mono | Additive CV for dry/wet |
//! | `delay_cv[i]` | mono | Multiplicative CV for delay time (i in 0..N-1, N = channels) |
//! | `gain_cv[i]` | mono | Additive CV for tap gain (i in 0..N-1, N = channels) |
//! | `fb_cv[i]` | mono | Additive CV for feedback (i in 0..N-1, N = channels) |
//!
//! # Outputs
//!
//! | Port | Kind | Description |
//! |------|------|-------------|
//! | `out` | mono | Wet/dry mixed output |
//!
//! # Parameters
//!
//! | Name | Type | Range | Default | Description |
//! |------|------|-------|---------|-------------|
//! | `dry_wet` | float | 0.0--1.0 | `0.5` | Dry/wet mix (global) |
//! | `delay_ms[i]` | float | 1.0--340.0 | `40.0` | Delay time in ms (per tap) |
//! | `gain[i]` | float | 0.0--1.0 | `1.0` | Tap gain (per tap) |
//! | `feedback[i]` | float | 0.0--0.95 | `0.0` | Self-feedback per tap |

use patches_core::module_params;
use patches_core::param_frame::ParamView;
use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, Module, ModuleDescriptor, ModuleShape,
    MonoInput, MonoOutput, OutputPort,
};
use patches_dsp::approximate::fast_tanh;
use std::f32::consts::TAU;

use crate::bbd::{Bbd, BbdDevice};

use crate::compander::{CompanderParams, Compressor, Expander};

// Feedback-path loop filter: DC block + LP damping to stop narrow-band
// self-oscillation ("ticking") at the comb's resonant peak.
const FB_HP_HZ: f32 = 5.0;
const FB_LP_HZ: f32 = 2_000.0;

module_params! {
    VBbd {
        dry_wet:  Float,
        jitter:   Float,
        delay_ms: FloatArray,
        gain:     FloatArray,
        feedback: FloatArray,
    }
}

/// Honest ceiling for a 4096-stage BBD: ~340 ms at ~6 kHz clock, past
/// which image-folding becomes audible. The module uses
/// [`BbdDevice::BBD_4096`] for its taps, putting vintage analog-delay
/// territory in range.
const DELAY_MS_MAX: f32 = 340.0;
const DELAY_MS_MIN: f32 = 1.0;
const FEEDBACK_MAX: f32 = 0.95;

/// Per-tap DSP state.
struct Tap {
    bbd: Bbd,
    comp: Compressor,
    exp: Expander,
    /// Feedback value carried from the previous tick.
    fb_state: f32,
    // One-pole HP (DC block) state for the feedback path.
    fb_hp_x_prev: f32,
    fb_hp_y_prev: f32,
    fb_hp_r: f32,
    // One-pole LP state for the feedback path.
    fb_lp_y_prev: f32,
    fb_lp_alpha: f32,
}

impl Tap {
    fn new(sr: f32, smoothing_interval: u32) -> Self {
        Self {
            bbd: Bbd::new_with_smoothing_interval(&BbdDevice::BBD_4096, sr, smoothing_interval),
            comp: Compressor::new(CompanderParams::NE570_DEFAULT, sr),
            exp: Expander::new(CompanderParams::NE570_DEFAULT, sr),
            fb_state: 0.0,
            fb_hp_x_prev: 0.0,
            fb_hp_y_prev: 0.0,
            fb_hp_r: 1.0 - TAU * FB_HP_HZ / sr,
            fb_lp_y_prev: 0.0,
            fb_lp_alpha: 1.0 - (-TAU * FB_LP_HZ / sr).exp(),
        }
    }

    #[inline]
    fn filter_feedback(&mut self, x: f32) -> f32 {
        let hp = x - self.fb_hp_x_prev + self.fb_hp_r * self.fb_hp_y_prev;
        self.fb_hp_x_prev = x;
        self.fb_hp_y_prev = hp;
        let lp = self.fb_lp_y_prev + self.fb_lp_alpha * (hp - self.fb_lp_y_prev);
        self.fb_lp_y_prev = lp;
        lp
    }
}

/// Vintage multi-tap BBD delay. See the module-level documentation.
pub struct VBbd {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    taps: usize,

    tap_state: Vec<Tap>,

    dry_wet: f32,
    delay_ms: Vec<f32>,
    gains: Vec<f32>,
    feedbacks: Vec<f32>,

    in_port: MonoInput,
    drywet_cv: MonoInput,
    out_port: MonoOutput,
    delay_cv: Vec<MonoInput>,
    gain_cv: Vec<MonoInput>,
    fb_cv: Vec<MonoInput>,
}

impl Module for VBbd {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        let n = shape.channels;
        ModuleDescriptor::new("VBbd", shape.clone())
            .mono_in("in")
            .mono_in("drywet_cv")
            .mono_in_multi("delay_cv", n)
            .mono_in_multi("gain_cv", n)
            .mono_in_multi("fb_cv", n)
            .mono_out("out")
            .float_param(params::dry_wet, 0.0, 1.0, 0.5)
            .float_param(params::jitter, 0.0, 1.0, 0.0)
            .float_param_multi(params::delay_ms, n, DELAY_MS_MIN, DELAY_MS_MAX, 40.0)
            .float_param_multi(params::gain, n, 0.0, 1.0, 1.0)
            .float_param_multi(params::feedback, n, 0.0, FEEDBACK_MAX, 0.0)
    }

    fn prepare(
        env: &AudioEnvironment,
        descriptor: ModuleDescriptor,
        instance_id: InstanceId,
    ) -> Self {
        let sr = env.sample_rate;
        let interval = env.periodic_update_interval;
        let taps = descriptor.shape.channels;
        let seed_base = (instance_id.as_u64() ^ 0xBBD0_0001) as u32;
        let tap_state: Vec<Tap> = (0..taps)
            .map(|i| {
                let mut t = Tap::new(sr, interval);
                t.bbd.set_jitter_seed(seed_base.wrapping_add(i as u32));
                t
            })
            .collect();

        Self {
            instance_id,
            descriptor,
            taps,
            tap_state,
            dry_wet: 0.5,
            delay_ms: vec![40.0; taps],
            gains: vec![1.0; taps],
            feedbacks: vec![0.0; taps],
            in_port: MonoInput::default(),
            drywet_cv: MonoInput::default(),
            out_port: MonoOutput::default(),
            delay_cv: vec![MonoInput::default(); taps],
            gain_cv: vec![MonoInput::default(); taps],
            fb_cv: vec![MonoInput::default(); taps],
        }
    }

    fn update_validated_parameters(&mut self, p: &ParamView<'_>) {
        self.dry_wet = p.get(params::dry_wet).clamp(0.0, 1.0);
        let jitter = p.get(params::jitter).clamp(0.0, 1.0);
        for i in 0..self.taps {
            let idx = i as u16;
            self.delay_ms[i] = p.get(params::delay_ms.at(idx)).clamp(DELAY_MS_MIN, DELAY_MS_MAX);
            self.gains[i] = p.get(params::gain.at(idx)).clamp(0.0, 1.0);
            self.feedbacks[i] = p.get(params::feedback.at(idx)).clamp(0.0, FEEDBACK_MAX);
            self.tap_state[i].bbd.set_jitter_amount(jitter);
        }
    }

    fn descriptor(&self) -> &ModuleDescriptor {
        &self.descriptor
    }

    fn instance_id(&self) -> InstanceId {
        self.instance_id
    }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        let n = self.taps;
        self.in_port = MonoInput::from_ports(inputs, 0);
        self.drywet_cv = MonoInput::from_ports(inputs, 1);
        for i in 0..n {
            self.delay_cv[i] = MonoInput::from_ports(inputs, 2 + i);
            self.gain_cv[i] = MonoInput::from_ports(inputs, 2 + n + i);
            self.fb_cv[i] = MonoInput::from_ports(inputs, 2 + 2 * n + i);
        }
        self.out_port = MonoOutput::from_ports(outputs, 0);
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let in_val = pool.read_mono(&self.in_port);

        let mut wet_sum = 0.0_f32;
        for i in 0..self.taps {
            // Compander sits on the dry path only; feedback is summed
            // post-compressor and the loop contains just BBD + HP/LP +
            // tanh. Keeping compression out of the loop avoids its
            // low-signal gain (>1) regenerating narrow-band residue.
            let compressed = self.tap_state[i].comp.process(fast_tanh(in_val));
            let with_fb = fast_tanh(compressed + self.tap_state[i].fb_state);
            let bbd_out = self.tap_state[i].bbd.process(with_fb);
            let tap_out = self.tap_state[i].exp.process(bbd_out);

            let eff_gain = (self.gains[i] + pool.read_mono(&self.gain_cv[i])).clamp(0.0, 1.0);
            wet_sum += tap_out * eff_gain;

            let eff_fb =
                (self.feedbacks[i] + pool.read_mono(&self.fb_cv[i])).clamp(0.0, FEEDBACK_MAX);
            let fb_filtered = self.tap_state[i].filter_feedback(bbd_out);
            self.tap_state[i].fb_state = fb_filtered * eff_fb;
        }

        let eff_dw = (self.dry_wet + pool.read_mono(&self.drywet_cv)).clamp(0.0, 1.0);
        let out_val = in_val + eff_dw * (wet_sum - in_val);
        pool.write_mono(&self.out_port, out_val);
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn wants_periodic(&self) -> bool { true }

    fn periodic_update(&mut self, pool: &CablePool<'_>) {
        // Delay is driven by `delay_ms[i]` parameter + `delay_cv[i]`
        // input; both change at Periodic cadence, so one `set_delay`
        // per Periodic tick per tap is enough. The BBD smooths
        // internally across its own (finer) smoothing interval.
        for i in 0..self.taps {
            let cv = pool.read_mono(&self.delay_cv[i]).clamp(-1.0, 1.0);
            let delay_s = (self.delay_ms[i] * (1.0 + cv) * 0.001)
                .clamp(DELAY_MS_MIN * 0.001, DELAY_MS_MAX * 0.001);
            self.tap_state[i].bbd.set_delay_seconds(delay_s);
        }
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::parameter_map::{ParameterMap, ParameterValue};
    use patches_core::test_support::{params, ModuleHarness};
    use patches_core::{AudioEnvironment, ModuleShape};

    const SR: f32 = 48_000.0;
    const ENV: AudioEnvironment = AudioEnvironment {
        sample_rate: SR,
        poly_voices: 16,
        periodic_update_interval: 32,
        hosted: false,
    };

    fn shape(taps: usize) -> ModuleShape {
        ModuleShape { channels: taps, length: 0, ..Default::default() }
    }

    #[test]
    fn zero_taps_dry_wet_zero_passes_dry() {
        let mut h =
            ModuleHarness::build_full::<VBbd>(params!["dry_wet" => 0.0_f32], ENV, shape(0));
        h.set_mono("in", 0.5);
        h.tick();
        assert_eq!(h.read_mono("out"), 0.5);
    }

    #[test]
    fn dry_wet_zero_passes_only_dry() {
        let mut h =
            ModuleHarness::build_full::<VBbd>(params!["dry_wet" => 0.0_f32], ENV, shape(1));
        h.set_mono("in", 0.7);
        h.tick();
        assert_eq!(h.read_mono("out"), 0.7);
    }

    #[test]
    fn output_is_bounded_under_sustained_input() {
        let mut h = ModuleHarness::build_full::<VBbd>(params![], ENV, shape(2));
        let mut pm = ParameterMap::new();
        pm.insert_param("dry_wet", 0, ParameterValue::Float(1.0));
        pm.insert_param("delay_ms", 0, ParameterValue::Float(5.0));
        pm.insert_param("delay_ms", 1, ParameterValue::Float(15.0));
        pm.insert_param("feedback", 0, ParameterValue::Float(0.9));
        pm.insert_param("feedback", 1, ParameterValue::Float(0.9));
        pm.insert_param("gain", 0, ParameterValue::Float(1.0));
        pm.insert_param("gain", 1, ParameterValue::Float(1.0));
        h.update_params_map(&pm);
        h.disconnect_input("drywet_cv");
        for i in 0..2 {
            h.disconnect_input_at("delay_cv", i);
            h.disconnect_input_at("gain_cv", i);
            h.disconnect_input_at("fb_cv", i);
        }

        for i in 0..20_000 {
            let t = i as f32 / SR;
            h.set_mono("in", 0.5 * (std::f32::consts::TAU * 440.0 * t).sin());
            h.tick();
            let out = h.read_mono("out");
            assert!(out.is_finite() && out.abs() < 5.0, "diverged at i={i}: {out}");
        }
    }

    #[test]
    fn longer_delay_tap_appears_later() {
        // Single-tap impulse response peaks around delay_ms.
        fn time_to_peak(delay_ms: f32) -> usize {
            let mut h = ModuleHarness::build_full::<VBbd>(params![], ENV, shape(1));
            let mut pm = ParameterMap::new();
            pm.insert_param("dry_wet", 0, ParameterValue::Float(1.0));
            pm.insert_param("delay_ms", 0, ParameterValue::Float(delay_ms));
            pm.insert_param("gain", 0, ParameterValue::Float(1.0));
            h.update_params_map(&pm);
            h.disconnect_input("drywet_cv");
            h.disconnect_input_at("delay_cv", 0);
            h.disconnect_input_at("gain_cv", 0);
            h.disconnect_input_at("fb_cv", 0);

            h.set_mono("in", 1.0);
            h.tick();
            h.set_mono("in", 0.0);
            let horizon = ((delay_ms * 0.001 + 0.01) * SR) as usize;
            let mut peak_idx = 0;
            let mut peak_abs = 0.0_f32;
            for i in 1..horizon {
                h.tick();
                let y = h.read_mono("out").abs();
                if y > peak_abs {
                    peak_abs = y;
                    peak_idx = i;
                }
            }
            peak_idx
        }
        let short = time_to_peak(3.0);
        let long = time_to_peak(40.0);
        assert!(long > short, "longer delay should peak later: short={short} long={long}");
    }
}
