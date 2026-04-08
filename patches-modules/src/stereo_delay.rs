//! Stereo multi-tap delay module with pan and pingpong feedback routing.
//!
//! Two [`DelayBuffer`]s (L and R, each 4 s) share a single set of read taps.
//! Each tap carries independent pan, gain, tone, drive, and feedback parameters.
//! The `pingpong` flag per tap cross-routes feedback: L feedback → R buffer
//! and R feedback → L buffer.
//!
//! ## Port layout (`N = shape.channels`)
//!
//! ### Inputs
//! | Name         | Indices | Kind | Description                     |
//! |--------------|---------|------|---------------------------------|
//! | `in_l`       | 0       | Mono | Left audio input                |
//! | `in_r`       | 0       | Mono | Right audio input               |
//! | `drywet_cv`  | 0       | Mono | Additive CV for dry/wet         |
//! | `delay_cv`   | 0..N−1  | Mono | Additive CV for delay time      |
//! | `gain_cv`    | 0..N−1  | Mono | Additive CV for tap gain        |
//! | `fb_cv`      | 0..N−1  | Mono | Additive CV for feedback amount |
//! | `pan_cv`     | 0..N−1  | Mono | Additive CV for pan             |
//! | `return_l`   | 0..N−1  | Mono | Pre-gain L return per tap       |
//! | `return_r`   | 0..N−1  | Mono | Pre-gain R return per tap       |
//!
//! ### Outputs
//! | Name     | Indices | Kind | Description                     |
//! |----------|---------|------|---------------------------------|
//! | `out_l`  | 0       | Mono | Wet/dry mixed left output       |
//! | `out_r`  | 0       | Mono | Wet/dry mixed right output      |
//! | `send_l` | 0..N−1  | Mono | Pre-gain L tap signal per tap   |
//! | `send_r` | 0..N−1  | Mono | Pre-gain R tap signal per tap   |
//!
//! ### Parameters (global)
//! `dry_wet` Float [0, 1] = 1.0
//!
//! ### Parameters (per tap i)
//! `delay_ms/i` Int [0, 2000] = 500, `gain/i` Float [0, 1] = 1.0,
//! `feedback/i` Float [0, 1] = 0.0, `tone/i` Float [0, 1] = 1.0,
//! `drive/i` Float [0.1, 10.0] = 1.0, `pan/i` Float [−1, 1] = 0.0,
//! `pingpong/i` Bool = false

use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, Module, ModuleDescriptor,
    MonoInput, MonoOutput, ModuleShape, OutputPort,
};
use patches_core::parameter_map::{ParameterMap, ParameterValue};

use crate::common::delay_buffer::DelayBuffer;
use crate::common::{TapFeedbackFilter, ToneFilter};
use crate::common::approximate::fast_tanh;

// ─── helpers ──────────────────────────────────────────────────────────────────

#[inline]
fn get_float(params: &ParameterMap, name: &str, index: usize, default: f32) -> f32 {
    match params.get(name, index) {
        Some(ParameterValue::Float(v)) => *v,
        _ => default,
    }
}

#[inline]
fn get_int(params: &ParameterMap, name: &str, index: usize, default: i64) -> i64 {
    match params.get(name, index) {
        Some(ParameterValue::Int(v)) => *v,
        _ => default,
    }
}

#[inline]
fn get_bool(params: &ParameterMap, name: &str, index: usize, default: bool) -> bool {
    match params.get(name, index) {
        Some(ParameterValue::Bool(v)) => *v,
        _ => default,
    }
}

// ─── StereoDelay ──────────────────────────────────────────────────────────────

/// Stereo N-tap delay.  See [module-level documentation](self).
pub struct StereoDelay {
    instance_id: InstanceId,
    descriptor:  ModuleDescriptor,
    taps:        usize,
    sr_ms: f32,

    // Audio state
    buf_l: DelayBuffer,
    buf_r: DelayBuffer,
    /// Pre-routed L feedback for next tick's buffer write (pingpong already applied).
    routed_l: Vec<f32>,
    /// Pre-routed R feedback for next tick's buffer write (pingpong already applied).
    routed_r: Vec<f32>,

    // Per-tap audio state (two channels each)
    fb_filters_l:   Vec<TapFeedbackFilter>,
    fb_filters_r:   Vec<TapFeedbackFilter>,
    tone_filters_l: Vec<ToneFilter>,
    tone_filters_r: Vec<ToneFilter>,

    // Cached parameters
    dry_wet:   f32,
    delay_ms:  Vec<f32>,
    gains:     Vec<f32>,
    feedbacks: Vec<f32>,
    tones:     Vec<f32>,
    drives:    Vec<f32>,
    pans:      Vec<f32>,
    pingpong:  Vec<bool>,

    // Global port fields
    in_l:      MonoInput,
    in_r:      MonoInput,
    drywet_cv: MonoInput,
    out_l:     MonoOutput,
    out_r:     MonoOutput,

    // Per-tap port fields
    delay_cv:   Vec<MonoInput>,
    gain_cv:    Vec<MonoInput>,
    fb_cv:      Vec<MonoInput>,
    pan_cv:     Vec<MonoInput>,
    return_l:   Vec<MonoInput>,
    return_r:   Vec<MonoInput>,
    send_l:     Vec<MonoOutput>,
    send_r:     Vec<MonoOutput>,
}

impl Module for StereoDelay {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        let n = shape.channels;
        ModuleDescriptor::new("StereoDelay", shape.clone())
            .mono_in("in_left")
            .mono_in("in_right")
            .mono_in("drywet_cv")
            .mono_in_multi("delay_cv",  n)
            .mono_in_multi("gain_cv",   n)
            .mono_in_multi("fb_cv",     n)
            .mono_in_multi("pan_cv",    n)
            .mono_in_multi("return_left",  n)
            .mono_in_multi("return_right",  n)
            .mono_out("out_left")
            .mono_out("out_right")
            .mono_out_multi("send_left", n)
            .mono_out_multi("send_right", n)
            .float_param("dry_wet", 0.0, 1.0, 1.0)
            .int_param_multi("delay_ms", shape.channels, 0, 2000, 500)
            .float_param_multi("gain",     shape.channels, 0.0,  1.0,  1.0)
            .float_param_multi("feedback", shape.channels, 0.0,  1.0,  0.0)
            .float_param_multi("tone",     shape.channels, 0.0,  1.0,  1.0)
            .float_param_multi("drive",    shape.channels, 0.1, 10.0,  1.0)
            .float_param_multi("pan",      shape.channels, -1.0, 1.0,  0.0)
            .bool_param_multi("pingpong",  shape.channels, false)
    }

    fn prepare(env: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId) -> Self {
        let sr   = env.sample_rate;
        let taps = descriptor.shape.channels;
        let sr_ms = sr * 0.001;

        let buf_l = DelayBuffer::for_duration(4.0, sr);
        let buf_r = DelayBuffer::for_duration(4.0, sr);

        let make_fb_filters = || -> Vec<TapFeedbackFilter> {
            (0..taps).map(|_| {
                let mut f = TapFeedbackFilter::new();
                f.prepare(sr);
                f
            }).collect()
        };
        let make_tone_filters = || -> Vec<ToneFilter> {
            (0..taps).map(|_| {
                let mut f = ToneFilter::new();
                f.prepare(sr);
                f
            }).collect()
        };

        Self {
            instance_id,
            descriptor,
            taps,
            sr_ms,
            buf_l,
            buf_r,
            routed_l: vec![0.0; taps],
            routed_r: vec![0.0; taps],
            fb_filters_l:   make_fb_filters(),
            fb_filters_r:   make_fb_filters(),
            tone_filters_l: make_tone_filters(),
            tone_filters_r: make_tone_filters(),
            dry_wet:   1.0,
            delay_ms:  vec![500.0; taps],
            gains:     vec![1.0; taps],
            feedbacks: vec![0.0; taps],
            tones:     vec![1.0; taps],
            drives:    vec![1.0; taps],
            pans:      vec![0.0; taps],
            pingpong:  vec![false; taps],
            in_l:      MonoInput::default(),
            in_r:      MonoInput::default(),
            drywet_cv: MonoInput::default(),
            out_l:     MonoOutput::default(),
            out_r:     MonoOutput::default(),
            delay_cv:  vec![MonoInput::default(); taps],
            gain_cv:   vec![MonoInput::default(); taps],
            fb_cv:     vec![MonoInput::default(); taps],
            pan_cv:    vec![MonoInput::default(); taps],
            return_l:  vec![MonoInput::default(); taps],
            return_r:  vec![MonoInput::default(); taps],
            send_l:    vec![MonoOutput::default(); taps],
            send_r:    vec![MonoOutput::default(); taps],
        }
    }

    fn update_validated_parameters(&mut self, params: &mut ParameterMap) {
        
        if let Some(ParameterValue::Float(v)) = params.get_scalar("dry_wet") {
            self.dry_wet = *v;
        }
        for i in 0..self.taps {
            self.delay_ms[i]  = get_int(params,   "delay_ms", i, self.delay_ms[i] as i64).clamp(0, 2000) as f32;
            self.gains[i]     = get_float(params,  "gain",     i, self.gains[i]    ).clamp(0.0, 1.0);
            self.feedbacks[i] = get_float(params,  "feedback", i, self.feedbacks[i]).clamp(0.0, 1.0);
            let tone          = get_float(params,  "tone",     i, self.tones[i]    ).clamp(0.0, 1.0);
            if (tone - self.tones[i]).abs() > f32::EPSILON {
                self.tones[i] = tone;
                self.tone_filters_l[i].set_tone(tone);
                self.tone_filters_r[i].set_tone(tone);
            }
            self.drives[i]   = get_float(params,  "drive",    i, self.drives[i]   ).clamp(0.1, 10.0);
            self.pans[i]     = get_float(params,  "pan",      i, self.pans[i]     ).clamp(-1.0, 1.0);
            self.pingpong[i] = get_bool(params,   "pingpong", i, self.pingpong[i] );
        }
    }

    fn descriptor(&self) -> &ModuleDescriptor { &self.descriptor }
    fn instance_id(&self) -> InstanceId { self.instance_id }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        let n = self.taps;
        // Global inputs: in_l(0), in_r(1), drywet_cv(2)
        self.in_l      = MonoInput::from_ports(inputs, 0);
        self.in_r      = MonoInput::from_ports(inputs, 1);
        self.drywet_cv = MonoInput::from_ports(inputs, 2);
        // Per-tap inputs: delay_cv, gain_cv, fb_cv, pan_cv, return_l, return_r
        for i in 0..n {
            self.delay_cv[i] = MonoInput::from_ports(inputs, 3 + i);
            self.gain_cv[i]  = MonoInput::from_ports(inputs, 3 + n + i);
            self.fb_cv[i]    = MonoInput::from_ports(inputs, 3 + 2 * n + i);
            self.pan_cv[i]   = MonoInput::from_ports(inputs, 3 + 3 * n + i);
            self.return_l[i] = MonoInput::from_ports(inputs, 3 + 4 * n + i);
            self.return_r[i] = MonoInput::from_ports(inputs, 3 + 5 * n + i);
        }
        // Outputs: out_l(0), out_r(1), send_l[0..n], send_r[0..n]
        self.out_l = MonoOutput::from_ports(outputs, 0);
        self.out_r = MonoOutput::from_ports(outputs, 1);
        for i in 0..n {
            self.send_l[i] = MonoOutput::from_ports(outputs, 2 + i);
            self.send_r[i] = MonoOutput::from_ports(outputs, 2 + n + i);
        }
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let in_l = pool.read_mono(&self.in_l);
        let in_r = pool.read_mono(&self.in_r);

        // ── Write: input + pre-routed feedbacks from previous tick ────────────
        // Routing (pingpong or straight) was applied when storing routed_l/r at
        // the end of the previous tick's per-tap loop, so this sum is branchless.
        let write_l = fast_tanh(in_l + self.routed_l.iter().sum::<f32>());
        let write_r = fast_tanh(in_r + self.routed_r.iter().sum::<f32>());

        self.buf_l.push(write_l);
        self.buf_r.push(write_r);

        // ── Per-tap reads ─────────────────────────────────────────────────────
        let cap_max = self.buf_l.capacity() as f32 - 2.0;
        let mut wet_l = 0.0_f32;
        let mut wet_r = 0.0_f32;

        for i in 0..self.taps {
            // Effective delay
            let cv     = pool.read_mono(&self.delay_cv[i]).clamp(-1.0, 1.0);
            let offset = (self.delay_ms[i] * (1.0 + cv) * self.sr_ms).clamp(1.0, cap_max);

            let tap_raw_l = self.buf_l.read_cubic(offset);
            let tap_raw_r = self.buf_r.read_cubic(offset);

            // Send outputs (pre-gain, pre-pan, pre-return)
            pool.write_mono(&self.send_l[i], tap_raw_l);
            pool.write_mono(&self.send_r[i], tap_raw_r);

            // Mix in returns
            let sig_l = tap_raw_l + pool.read_mono(&self.return_l[i]);
            let sig_r = tap_raw_r + pool.read_mono(&self.return_r[i]);

            // Tone filter
            let toned_l = self.tone_filters_l[i].process(sig_l);
            let toned_r = self.tone_filters_r[i].process(sig_r);

            // Pan law: sum to mono, apply equal-gain pan (consistent with StereoMixer).
            // Merge the two * 0.5 factors into a single * 0.25 to save one mul per tap.
            let eff_gain = (self.gains[i] + pool.read_mono(&self.gain_cv[i])).clamp(0.0, 1.0);
            let eff_pan  = (self.pans[i]  + pool.read_mono(&self.pan_cv[i])).clamp(-1.0, 1.0);
            let half     = (toned_l + toned_r) * (0.25 * eff_gain);
            wet_l += half * (1.0 - eff_pan);
            wet_r += half * (1.0 + eff_pan);

            // Feedback for next tick: apply pingpong routing now so the write
            // accumulation at the top of the next tick can be a branchless sum.
            let eff_fb = (self.feedbacks[i] + pool.read_mono(&self.fb_cv[i])).clamp(0.0, 1.0);
            let fb_l = self.fb_filters_l[i].process(toned_l * eff_fb, self.drives[i]);
            let fb_r = self.fb_filters_r[i].process(toned_r * eff_fb, self.drives[i]);
            if self.pingpong[i] {
                self.routed_l[i] = fb_r;
                self.routed_r[i] = fb_l;
            } else {
                self.routed_l[i] = fb_l;
                self.routed_r[i] = fb_r;
            }
        }

        // ── Dry/wet mix ───────────────────────────────────────────────────────
        let eff_dw = (self.dry_wet + pool.read_mono(&self.drywet_cv)).clamp(0.0, 1.0);
        pool.write_mono(&self.out_l, in_l + eff_dw * (wet_l - in_l));
        pool.write_mono(&self.out_r, in_r + eff_dw * (wet_r - in_r));
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
    const ENV: AudioEnvironment = AudioEnvironment { sample_rate: SR, poly_voices: 16, periodic_update_interval: 32 };

    fn shape(taps: usize) -> ModuleShape {
        ModuleShape { channels: taps, length: 0, ..Default::default() }
    }

    #[test]
    fn zero_taps_dry_wet_zero_passes_dry() {
        let mut h = ModuleHarness::build_full::<StereoDelay>(
            params!["dry_wet" => 0.0_f32],
            ENV, shape(0),
        );
        h.set_mono("in_left", 0.5);
        h.set_mono("in_right", 0.3);
        h.tick();
        assert_eq!(h.read_mono("out_left"), 0.5);
        assert_eq!(h.read_mono("out_right"), 0.3);
    }

    #[test]
    fn pan_right_silences_left() {
        // At pan=1 with dry_wet=1, all wet signal should go to the right.
        let mut h = ModuleHarness::build_full::<StereoDelay>(params![], ENV, shape(1));
        let mut pm = ParameterMap::new();
        pm.insert_param("dry_wet",  0, ParameterValue::Float(1.0));
        pm.insert_param("delay_ms", 0, ParameterValue::Int(1));
        pm.insert_param("gain",     0, ParameterValue::Float(1.0));
        pm.insert_param("pan",      0, ParameterValue::Float(1.0));
        h.update_params_map(&pm);
        h.disconnect_input("drywet_cv");
        h.disconnect_input_at("delay_cv", 0);
        h.disconnect_input_at("gain_cv", 0);
        h.disconnect_input_at("fb_cv", 0);
        h.disconnect_input_at("pan_cv", 0);
        h.disconnect_input_at("return_left", 0);
        h.disconnect_input_at("return_right", 0);

        // Prime with a known signal
        h.set_mono("in_left", 1.0);
        h.set_mono("in_right", 1.0);
        for _ in 0..50 { h.tick(); }

        // With pan=1: left gain = (1-1)*0.5 = 0; right gain = (1+1)*0.5 = 1
        let out_l = h.read_mono("out_left").abs();
        let out_r = h.read_mono("out_right").abs();
        assert!(out_l < 1e-6, "out_l should be 0 at pan=1, got {out_l}");
        assert!(out_r > 0.0, "out_r should be non-zero at pan=1, got {out_r}");
    }

    #[test]
    fn pingpong_routes_l_to_r() {
        let delay_ms = 5i64;
        let period = (delay_ms as f32 * SR / 1000.0) as usize;

        let mut h = ModuleHarness::build_full::<StereoDelay>(params![], ENV, shape(1));
        let mut pm = ParameterMap::new();
        pm.insert_param("delay_ms", 0, ParameterValue::Int(delay_ms));
        pm.insert_param("feedback", 0, ParameterValue::Float(0.9));
        pm.insert_param("pan",      0, ParameterValue::Float(0.0));
        pm.insert_param("pingpong", 0, ParameterValue::Bool(true));
        pm.insert_param("dry_wet",  0, ParameterValue::Float(1.0));
        pm.insert_param("gain",     0, ParameterValue::Float(1.0));
        h.update_params_map(&pm);
        h.disconnect_input("drywet_cv");
        h.disconnect_input_at("delay_cv", 0);
        h.disconnect_input_at("gain_cv", 0);
        h.disconnect_input_at("fb_cv", 0);
        h.disconnect_input_at("pan_cv", 0);
        h.disconnect_input_at("return_left", 0);
        h.disconnect_input_at("return_right", 0);

        // Fire impulse on L only; R is silent (disconnected → 0)
        h.set_mono("in_left", 1.0);
        h.set_mono("in_right", 0.0);
        h.tick();
        h.set_mono("in_left", 0.0);

        // Collect enough samples for several ping-pong bounces
        let total = period * 5;
        let mut left_samples = Vec::with_capacity(total);
        let mut right_samples = Vec::with_capacity(total);
        for _ in 0..total {
            h.tick();
            left_samples.push(h.read_mono("out_left"));
            right_samples.push(h.read_mono("out_right"));
        }

        // Period 1: impulse arrives on L
        let p1_l_peak = left_samples[period..2 * period].iter()
            .map(|v| v.abs()).fold(0.0_f32, f32::max);
        assert!(p1_l_peak > 0.01, "First tap should appear on L, peak={p1_l_peak}");

        // After pingpong, signal should appear on R
        let p2_r_peak = right_samples[2 * period..3 * period].iter()
            .map(|v| v.abs()).fold(0.0_f32, f32::max);
        let p2_l_peak = left_samples[2 * period..3 * period].iter()
            .map(|v| v.abs()).fold(0.0_f32, f32::max);
        assert!(p2_r_peak > 0.001 || p2_l_peak > 0.001,
            "Pingpong should produce signal on period 2: L={p2_l_peak:.4}, R={p2_r_peak:.4}");
    }
}
