//! Mono multi-tap delay module.
//!
//! One shared [`DelayBuffer`] (4 s capacity) with N independent read taps.
//! Each tap is placed by a millisecond parameter modulatable by a CV input.
//! All tap feedbacks sum back into the buffer write before the next sample.
//!
//! # Inputs
//!
//! | Port | Kind | Description |
//! |------|------|-------------|
//! | `in` | mono | Audio input |
//! | `drywet_cv` | mono | Additive CV for dry/wet |
//! | `sync_ms[i]` | mono | When connected, overrides delay time for tap i in ms (i in 0..N-1, N = channels) |
//! | `delay_cv[i]` | mono | Additive CV for delay time (i in 0..N-1, N = channels) |
//! | `gain_cv[i]` | mono | Additive CV for tap gain (i in 0..N-1, N = channels) |
//! | `fb_cv[i]` | mono | Additive CV for feedback (i in 0..N-1, N = channels) |
//! | `return[i]` | mono | Pre-gain return signal per tap (i in 0..N-1, N = channels) |
//!
//! # Outputs
//!
//! | Port | Kind | Description |
//! |------|------|-------------|
//! | `out` | mono | Wet/dry mixed output |
//! | `send[i]` | mono | Pre-gain tap signal per tap (i in 0..N-1, N = channels) |
//!
//! # Parameters
//!
//! | Name | Type | Range | Default | Description |
//! |------|------|-------|---------|-------------|
//! | `dry_wet` | float | 0.0--1.0 | `1.0` | Dry/wet mix (global) |
//! | `delay_ms[i]` | int | 0--2000 | `500` | Delay time in ms (per tap) |
//! | `gain[i]` | float | 0.0--1.0 | `1.0` | Tap gain (per tap) |
//! | `feedback[i]` | float | 0.0--1.0 | `0.0` | Feedback amount (per tap) |
//! | `tone[i]` | float | 0.0--1.0 | `1.0` | Tone filter (per tap) |
//! | `drive[i]` | float | 0.1--10.0 | `1.0` | Feedback saturation drive (per tap) |
//!
//! When `high_quality` is set in the module shape, tap reads use cubic
//! (Catmull-Rom) interpolation; otherwise linear interpolation is used,
//! which is cheaper but may produce audible artifacts on modulated delays.

use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, Module, ModuleDescriptor,
    MonoInput, MonoOutput, ModuleShape, OutputPort,
};
use patches_core::module_params;
use patches_core::param_frame::ParamView;

use crate::common::delay_buffer::DelayBuffer;
use crate::common::{TapFeedbackFilter, ToneFilter};
use crate::common::approximate::fast_tanh;

module_params! {
    Delay {
        dry_wet:  Float,
        delay_ms: IntArray,
        gain:     FloatArray,
        feedback: FloatArray,
        tone:     FloatArray,
        drive:    FloatArray,
    }
}

// ─── Delay ────────────────────────────────────────────────────────────────────

/// Mono N-tap delay.  See [module-level documentation](self).
pub struct Delay {
    instance_id: InstanceId,
    descriptor:  ModuleDescriptor,
    taps:        usize,
    high_quality: bool,
    sr_ms: f32,

    // Audio state
    buffer:   DelayBuffer,
    /// Feedback values carried from the previous tick (pre-allocated, no alloc on audio thread).
    feedback: Vec<f32>,

    // Per-tap audio state
    fb_filters:   Vec<TapFeedbackFilter>,
    tone_filters: Vec<ToneFilter>,

    // Cached parameters
    dry_wet:    f32,
    delay_ms:   Vec<f32>,   // stored as f32 for cheap ms→samples conversion
    gains:      Vec<f32>,
    feedbacks:  Vec<f32>,
    tones:      Vec<f32>,
    drives:     Vec<f32>,

    // Port fields (global)
    in_port:    MonoInput,
    drywet_cv:  MonoInput,
    out_port:   MonoOutput,
    // Port fields (per tap)
    sync_ms:    Vec<MonoInput>,
    delay_cv:   Vec<MonoInput>,
    gain_cv:    Vec<MonoInput>,
    fb_cv:      Vec<MonoInput>,
    return_in:  Vec<MonoInput>,
    send_out:   Vec<MonoOutput>,
}

impl Module for Delay {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        let n = shape.channels;
        ModuleDescriptor::new("Delay", shape.clone())
            .mono_in("in")
            .mono_in("drywet_cv")
            .mono_in_multi("sync_ms",  n)
            .mono_in_multi("delay_cv", n)
            .mono_in_multi("gain_cv",  n)
            .mono_in_multi("fb_cv",    n)
            .mono_in_multi("return",   n)
            .mono_out("out")
            .mono_out_multi("send", n)
            .float_param(params::dry_wet, 0.0, 1.0, 1.0)
            .int_param_multi(params::delay_ms, n, 0, 2000, 500)
            .float_param_multi(params::gain,     n, 0.0, 1.0, 1.0)
            .float_param_multi(params::feedback, n, 0.0, 1.0, 0.0)
            .float_param_multi(params::tone,     n, 0.0, 1.0, 1.0)
            .float_param_multi(params::drive,    n, 0.1, 10.0, 1.0)
    }

    fn prepare(env: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId) -> Self {
        let sr   = env.sample_rate;
        let taps = descriptor.shape.channels;
        let high_quality = descriptor.shape.high_quality;
        let sr_ms = sr * 0.001;

        let buffer = DelayBuffer::for_duration(4.0, sr);

        let fb_filters: Vec<TapFeedbackFilter> = (0..taps)
            .map(|_| {
                let mut f = TapFeedbackFilter::new();
                f.prepare(sr);
                f
            })
            .collect();

        let tone_filters: Vec<ToneFilter> = (0..taps)
            .map(|_| {
                let mut f = ToneFilter::new();
                f.prepare(sr);
                f
            })
            .collect();

        Self {
            instance_id,
            descriptor,
            taps,
            high_quality,
            sr_ms,
            buffer,
            feedback:     vec![0.0; taps],
            fb_filters,
            tone_filters,
            dry_wet:   1.0,
            delay_ms:  vec![500.0; taps],
            gains:     vec![1.0; taps],
            feedbacks: vec![0.0; taps],
            tones:     vec![1.0; taps],
            drives:    vec![1.0; taps],
            in_port:   MonoInput::default(),
            drywet_cv: MonoInput::default(),
            out_port:  MonoOutput::default(),
            sync_ms:   vec![MonoInput::default(); taps],
            delay_cv:  vec![MonoInput::default(); taps],
            gain_cv:   vec![MonoInput::default(); taps],
            fb_cv:     vec![MonoInput::default(); taps],
            return_in: vec![MonoInput::default(); taps],
            send_out:  vec![MonoOutput::default(); taps],
        }
    }

    fn update_validated_parameters(&mut self, p: &ParamView<'_>) {
        self.dry_wet = p.get(params::dry_wet);
        for i in 0..self.taps {
            let idx = i as u16;
            self.delay_ms[i]  = (p.get(params::delay_ms.at(idx))).clamp(0, 2000) as f32;
            self.gains[i]     = p.get(params::gain.at(idx)).clamp(0.0, 1.0);
            self.feedbacks[i] = p.get(params::feedback.at(idx)).clamp(0.0, 1.0);
            let tone          = p.get(params::tone.at(idx)).clamp(0.0, 1.0);
            if (tone - self.tones[i]).abs() > f32::EPSILON {
                self.tones[i] = tone;
                self.tone_filters[i].set_tone(tone);
            }
            self.drives[i]    = p.get(params::drive.at(idx)).clamp(0.1, 10.0);
        }
    }

    fn descriptor(&self) -> &ModuleDescriptor { &self.descriptor }
    fn instance_id(&self) -> InstanceId { self.instance_id }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        let n = self.taps;
        // Global inputs: in(0), drywet_cv(1)
        self.in_port   = MonoInput::from_ports(inputs, 0);
        self.drywet_cv = MonoInput::from_ports(inputs, 1);
        // Per-tap inputs: sync_ms[0..n], delay_cv[0..n], gain_cv[0..n], fb_cv[0..n], return[0..n]
        for i in 0..n {
            self.sync_ms[i]   = MonoInput::from_ports(inputs, 2 + i);
            self.delay_cv[i]  = MonoInput::from_ports(inputs, 2 + n + i);
            self.gain_cv[i]   = MonoInput::from_ports(inputs, 2 + 2 * n + i);
            self.fb_cv[i]     = MonoInput::from_ports(inputs, 2 + 3 * n + i);
            self.return_in[i] = MonoInput::from_ports(inputs, 2 + 4 * n + i);
        }
        // Outputs: out(0), send[0..n]
        self.out_port = MonoOutput::from_ports(outputs, 0);
        for i in 0..n {
            self.send_out[i] = MonoOutput::from_ports(outputs, 1 + i);
        }
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let in_val = pool.read_mono(&self.in_port);

        // ── Write: input + all tap feedbacks from previous tick ───────────────
        let mut write_val = in_val;
        for &fb in &self.feedback {
            write_val += fb;
        }
        self.buffer.push(fast_tanh(write_val));

        // ── Per-tap reads ─────────────────────────────────────────────────────
        let cap_max = self.buffer.capacity() as f32 - 2.0;
        let mut wet_sum = 0.0_f32;

        for i in 0..self.taps {
            // sync_ms overrides the tap's delay_ms when connected
            let base_ms = if self.sync_ms[i].is_connected() {
                pool.read_mono(&self.sync_ms[i]).clamp(0.0, 4000.0)
            } else {
                self.delay_ms[i]
            };
            let cv     = pool.read_mono(&self.delay_cv[i]).clamp(-1.0, 1.0);
            let offset = (base_ms * (1.0 + cv) * self.sr_ms).clamp(1.0, cap_max);

            let tap_raw = if self.high_quality {
                self.buffer.read_cubic(offset)
            } else {
                self.buffer.read_linear(offset)
            };

            // Send output (pre-gain, pre-return)
            pool.write_mono(&self.send_out[i], tap_raw);

            // Mix in return
            let tap_sig = tap_raw + pool.read_mono(&self.return_in[i]);

            // Tone filter (no allocation, coefficient pre-computed)
            let tap_toned = self.tone_filters[i].process(tap_sig);

            // Gain
            let eff_gain = (self.gains[i] + pool.read_mono(&self.gain_cv[i])).clamp(0.0, 1.0);
            wet_sum += tap_toned * eff_gain;

            // Feedback for next tick
            let eff_fb  = (self.feedbacks[i] + pool.read_mono(&self.fb_cv[i])).clamp(0.0, 1.0);
            self.feedback[i] = self.fb_filters[i].process(tap_toned * eff_fb, self.drives[i]);
        }

        // ── Dry/wet mix ───────────────────────────────────────────────────────
        let eff_dw  = (self.dry_wet + pool.read_mono(&self.drywet_cv)).clamp(0.0, 1.0);
        let out_val = in_val + eff_dw * (wet_sum - in_val);
        pool.write_mono(&self.out_port, out_val);
    }

    fn as_any(&self) -> &dyn std::any::Any { self }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::{AudioEnvironment, ModuleShape};
    use patches_core::parameter_map::{ParameterMap, ParameterValue};
    use patches_core::test_support::{ModuleHarness, params};

    const SR: f32 = 44_100.0;
    const ENV: AudioEnvironment = AudioEnvironment { sample_rate: SR, poly_voices: 16, periodic_update_interval: 32, hosted: false };

    fn shape(taps: usize) -> ModuleShape {
        ModuleShape { channels: taps, length: 0, ..Default::default() }
    }

    #[test]
    fn zero_taps_passes_dry_signal() {
        let mut h = ModuleHarness::build_full::<Delay>(
            params!["dry_wet" => 0.0_f32],
            ENV, shape(0),
        );
        h.set_mono("in", 0.5);
        h.tick();
        assert_eq!(h.read_mono("out"), 0.5);
    }

    #[test]
    fn dry_wet_zero_passes_only_dry() {
        let mut h = ModuleHarness::build_full::<Delay>(
            params!["dry_wet" => 0.0_f32],
            ENV, shape(1),
        );
        h.set_mono("in", 0.7);
        h.tick();
        assert_eq!(h.read_mono("out"), 0.7);
    }

    #[test]
    fn impulse_appears_at_correct_offset() {
        let delay_ms = 10i64;
        let expected_sample = (delay_ms as f32 * SR / 1000.0) as usize;

        let mut h = ModuleHarness::build_full::<Delay>(params![], ENV, shape(1));
        let mut pm = ParameterMap::new();
        pm.insert_param("delay_ms", 0, ParameterValue::Int(delay_ms));
        pm.insert_param("dry_wet",  0, ParameterValue::Float(1.0));
        h.update_params_map(&pm);

        h.disconnect_input("drywet_cv");
        h.disconnect_input_at("sync_ms", 0);
        h.disconnect_input_at("delay_cv", 0);
        h.disconnect_input_at("gain_cv", 0);
        h.disconnect_input_at("fb_cv", 0);
        h.disconnect_input_at("return", 0);

        // Fire impulse at sample 0
        h.set_mono("in", 1.0);
        h.tick();
        h.set_mono("in", 0.0);

        // Run until past expected_sample + 1 to allow for rounding
        let out: Vec<f32> = (1..=expected_sample + 2)
            .map(|_| { h.tick(); h.read_mono("out") })
            .collect();

        // The peak must appear at the expected offset (±1 sample)
        let peak_idx = out.iter().enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .map(|(i, _)| i)
            .unwrap_or(0);

        let deviation = (peak_idx as isize - (expected_sample - 1) as isize).abs();
        assert!(deviation <= 1,
            "Peak at sample {} but expected ~{} (±1)", peak_idx + 1, expected_sample);
    }

    #[test]
    fn send_is_pre_return() {
        // send should carry tap_raw only; return is added after.
        let mut h = ModuleHarness::build_full::<Delay>(params![], ENV, shape(1));
        let mut pm = ParameterMap::new();
        pm.insert_param("dry_wet",  0, ParameterValue::Float(1.0));
        pm.insert_param("delay_ms", 0, ParameterValue::Int(1));
        pm.insert_param("gain",     0, ParameterValue::Float(1.0));
        h.update_params_map(&pm);
        h.disconnect_input("drywet_cv");
        h.disconnect_input_at("delay_cv", 0);
        h.disconnect_input_at("gain_cv", 0);
        h.disconnect_input_at("fb_cv", 0);

        // Prime the buffer with a known value
        h.set_mono("in", 1.0);
        h.tick();
        h.set_mono("in", 0.0);
        // Inject a large return signal
        h.set_mono_at("return", 0, 99.0);
        h.tick();

        // send should be the raw tap (small), not tap + return
        let send = h.read_mono_at("send", 0);
        assert!(send.abs() < 2.0, "send should be pre-return tap, got {send}");
    }

    #[test]
    fn feedback_decay() {
        let delay_ms = 5i64;
        let period = (delay_ms as f32 * SR / 1000.0) as usize;

        let mut h = ModuleHarness::build_full::<Delay>(params![], ENV, shape(1));
        let mut pm = ParameterMap::new();
        pm.insert_param("delay_ms", 0, ParameterValue::Int(delay_ms));
        pm.insert_param("feedback", 0, ParameterValue::Float(0.5));
        pm.insert_param("dry_wet",  0, ParameterValue::Float(1.0));
        pm.insert_param("gain",     0, ParameterValue::Float(1.0));
        h.update_params_map(&pm);
        h.disconnect_input("drywet_cv");
        h.disconnect_input_at("sync_ms", 0);
        h.disconnect_input_at("delay_cv", 0);
        h.disconnect_input_at("gain_cv", 0);
        h.disconnect_input_at("fb_cv", 0);
        h.disconnect_input_at("return", 0);

        // Fire impulse at sample 0
        h.set_mono("in", 1.0);
        h.tick();
        h.set_mono("in", 0.0);

        let total = period * 8;
        let samples: Vec<f32> = (1..total).map(|_| { h.tick(); h.read_mono("out") }).collect();

        // Find peak in each period window (skip first period = direct tap)
        let period_peaks: Vec<f32> = (1..7).map(|rep| {
            let start = rep * period;
            let end = (start + period).min(samples.len());
            samples[start..end].iter().map(|v| v.abs()).fold(0.0_f32, f32::max)
        }).collect();

        for i in 1..period_peaks.len() {
            assert!(period_peaks[i] < period_peaks[i - 1],
                "Repeat {} ({:.4}) not less than repeat {} ({:.4})",
                i, period_peaks[i], i - 1, period_peaks[i - 1]);
        }
    }

    #[test]
    fn feedback_saturation_bound() {
        let mut h = ModuleHarness::build_full::<Delay>(params![], ENV, shape(1));
        let mut pm = ParameterMap::new();
        pm.insert_param("delay_ms", 0, ParameterValue::Int(5));
        pm.insert_param("feedback", 0, ParameterValue::Float(1.0));
        pm.insert_param("drive",    0, ParameterValue::Float(10.0));
        pm.insert_param("dry_wet",  0, ParameterValue::Float(1.0));
        pm.insert_param("gain",     0, ParameterValue::Float(1.0));
        h.update_params_map(&pm);
        h.disconnect_input("drywet_cv");
        h.disconnect_input_at("sync_ms", 0);
        h.disconnect_input_at("delay_cv", 0);
        h.disconnect_input_at("gain_cv", 0);
        h.disconnect_input_at("fb_cv", 0);
        h.disconnect_input_at("return", 0);

        for i in 0..5_000 {
            let t = i as f32 / SR;
            h.set_mono("in", (std::f32::consts::TAU * 440.0 * t).sin());
            h.tick();
            let out = h.read_mono("out");
            assert!(out.abs() < 2.5,
                "Output exceeded ±2.5 with saturating feedback at tick {i}: {out}");
        }
    }

    #[test]
    fn dry_wet_one_first_sample_cold() {
        let mut h = ModuleHarness::build_full::<Delay>(params![], ENV, shape(1));
        let mut pm = ParameterMap::new();
        pm.insert_param("delay_ms", 0, ParameterValue::Int(100));
        pm.insert_param("dry_wet",  0, ParameterValue::Float(1.0));
        h.update_params_map(&pm);
        h.disconnect_input("drywet_cv");
        h.disconnect_input_at("sync_ms", 0);
        h.disconnect_input_at("delay_cv", 0);
        h.disconnect_input_at("gain_cv", 0);
        h.disconnect_input_at("fb_cv", 0);
        h.disconnect_input_at("return", 0);

        h.set_mono("in", 1.0);
        h.tick();
        // Buffer was cold → wet=0; dry excluded at dw=1 → output 0.0
        assert_eq!(h.read_mono("out"), 0.0, "dry_wet=1 first tick should be silent");
    }

    #[test]
    fn sync_ms_overrides_delay_time() {
        let sync_ms = 10.0_f32;
        let expected_sample = (sync_ms * SR / 1000.0) as usize;

        let mut h = ModuleHarness::build_full::<Delay>(params![], ENV, shape(1));
        let mut pm = ParameterMap::new();
        // Set a long delay_ms that would normally be used
        pm.insert_param("delay_ms", 0, ParameterValue::Int(500));
        pm.insert_param("dry_wet",  0, ParameterValue::Float(1.0));
        h.update_params_map(&pm);

        h.disconnect_input("drywet_cv");
        h.disconnect_input_at("delay_cv", 0);
        h.disconnect_input_at("gain_cv", 0);
        h.disconnect_input_at("fb_cv", 0);
        h.disconnect_input_at("return", 0);
        // Connect sync_ms[0] and set it to 10 ms — should override delay_ms
        h.set_mono_at("sync_ms", 0, sync_ms);

        h.set_mono("in", 1.0);
        h.tick();
        h.set_mono("in", 0.0);

        let out: Vec<f32> = (1..=expected_sample + 2)
            .map(|_| { h.tick(); h.read_mono("out") })
            .collect();

        let peak_idx = out.iter().enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .map(|(i, _)| i)
            .unwrap_or(0);

        let deviation = (peak_idx as isize - (expected_sample - 1) as isize).abs();
        assert!(deviation <= 1,
            "sync_ms=10: peak at sample {} but expected ~{} (±1)", peak_idx + 1, expected_sample);
    }
}
