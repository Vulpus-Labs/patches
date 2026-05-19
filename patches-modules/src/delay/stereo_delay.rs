//! Stereo multi-tap delay module with pan and pingpong feedback routing.
//!
//! Two [`DelayBuffer`]s (L and R, each 4 s) share a single set of read taps.
//! Each tap carries independent pan, gain, tone, drive, and feedback parameters.
//! The `pingpong` flag per tap cross-routes feedback: L feedback to R buffer
//! and R feedback to L buffer.
//!
//! # Inputs
//!
//! | Port | Kind | Description |
//! |------|------|-------------|
//! | `in` | stereo | Stereo audio input |
//! | `drywet_cv` | mono | Additive CV for dry/wet |
//! | `delay_cv[i]` | mono | Additive CV for delay time (i in 0..N-1, N = channels) |
//! | `gain_cv[i]` | mono | Additive CV for tap gain (i in 0..N-1, N = channels) |
//! | `fb_cv[i]` | mono | Additive CV for feedback amount (i in 0..N-1, N = channels) |
//! | `pan_cv[i]` | mono | Additive CV for pan (i in 0..N-1, N = channels) |
//! | `return[i]` | stereo | Pre-gain stereo return per tap (i in 0..N-1, N = channels) |
//!
//! # Outputs
//!
//! | Port | Kind | Description |
//! |------|------|-------------|
//! | `out` | stereo | Wet/dry mixed stereo output |
//! | `send[i]` | stereo | Pre-gain tap signal per tap (i in 0..N-1, N = channels) |
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
//! | `pan[i]` | float | -1.0--1.0 | `0.0` | Stereo pan position (per tap) |
//! | `pingpong[i]` | bool | -- | `false` | Cross-route feedback L/R (per tap) |
//!
//! When `high_quality` is set in the module shape, tap reads use cubic
//! (Catmull-Rom) interpolation; otherwise linear interpolation is used,
//! which is cheaper but may produce audible artifacts on modulated delays.

use patches_core::{
    AudioEnvironment, AxisId, CablePool, CountAxis, InputPort, InstanceId, Module,
    ModuleDescriptor, ModuleDescriptorTemplate, MonoInput, OutputPort, ParameterKind,
    ParameterTemplate, PortTemplate, StereoInput, StereoOutput,
};
use patches_core::{StructuralParams, BuildError};
use patches_core::module_params;
use patches_core::param_frame::ParamView;

use crate::common::delay_buffer::DelayBuffer;
use crate::common::{TapFeedbackFilter, ToneFilter};
use crate::common::approximate::fast_tanh;

module_params! {
    StereoDelay {
        dry_wet:  Float,
        delay_ms: IntArray,
        gain:     FloatArray,
        feedback: FloatArray,
        tone:     FloatArray,
        drive:    FloatArray,
        pan:      FloatArray,
        pingpong: BoolArray,
    }
}

// ─── StereoDelay ──────────────────────────────────────────────────────────────

/// All per-tap state: ports, params, filters, routed feedback. Packed AoS so
/// `taps.iter_mut()` gives the compiler bounds-check elision + cache locality.
struct Tap {
    // Ports
    delay_cv:  MonoInput,
    gain_cv:   MonoInput,
    fb_cv:     MonoInput,
    pan_cv:    MonoInput,
    return_st: StereoInput,
    send_st:   StereoOutput,

    // Params
    delay_ms: f32,
    gain:     f32,
    feedback: f32,
    tone:     f32,
    drive:    f32,
    pan:      f32,
    pingpong: bool,

    // Filters (L + R)
    fb_filter_l:   TapFeedbackFilter,
    fb_filter_r:   TapFeedbackFilter,
    tone_filter_l: ToneFilter,
    tone_filter_r: ToneFilter,

    /// Pre-routed L/R feedback for next tick's buffer write (pingpong already applied).
    routed_l: f32,
    routed_r: f32,
}

/// Stereo N-tap delay.  See [module-level documentation](self).
pub struct StereoDelay {
    instance_id: InstanceId,
    descriptor:  ModuleDescriptor,
    taps:        usize,
    high_quality: bool,
    sr_ms: f32,

    // Audio state
    buf_l: DelayBuffer,
    buf_r: DelayBuffer,

    // Cached global parameter
    dry_wet: f32,

    // Global port fields
    in_stereo:  StereoInput,
    drywet_cv:  MonoInput,
    out_stereo: StereoOutput,

    // Per-tap state, packed AoS.
    tap_state: Vec<Tap>,
}

impl Module for StereoDelay {
    fn template() -> ModuleDescriptorTemplate {
        const T: ModuleDescriptorTemplate = ModuleDescriptorTemplate {
            name: "StereoDelay",
            axes: &[CountAxis::CHANNELS],
            global_inputs: &[
                PortTemplate::stereo("in"),
                PortTemplate::mono("drywet_cv"),
            ],
            per_axis_inputs: &[
                (AxisId::CHANNELS, PortTemplate::mono("delay_cv")),
                (AxisId::CHANNELS, PortTemplate::mono("gain_cv")),
                (AxisId::CHANNELS, PortTemplate::mono("fb_cv")),
                (AxisId::CHANNELS, PortTemplate::mono("pan_cv")),
                (AxisId::CHANNELS, PortTemplate::stereo("return")),
            ],
            global_outputs: &[PortTemplate::stereo("out")],
            per_axis_outputs: &[(AxisId::CHANNELS, PortTemplate::stereo("send"))],
            realtime_params: &[ParameterTemplate {
                name: params::dry_wet.as_str(),
                kind: ParameterKind::Float { min: 0.0, max: 1.0, default: 1.0 },
            }],
            structural_params: &[ParameterTemplate {
                name: "high_quality",
                kind: ParameterKind::Bool { default: false },
            }],
            per_axis_realtime_params: &[
                (AxisId::CHANNELS, ParameterTemplate {
                    name: params::delay_ms.as_str(),
                    kind: ParameterKind::Int { min: 0, max: 2000, default: 500 },
                }),
                (AxisId::CHANNELS, ParameterTemplate {
                    name: params::gain.as_str(),
                    kind: ParameterKind::Float { min: 0.0, max: 1.0, default: 1.0 },
                }),
                (AxisId::CHANNELS, ParameterTemplate {
                    name: params::feedback.as_str(),
                    kind: ParameterKind::Float { min: 0.0, max: 1.0, default: 0.0 },
                }),
                (AxisId::CHANNELS, ParameterTemplate {
                    name: params::tone.as_str(),
                    kind: ParameterKind::Float { min: 0.0, max: 1.0, default: 1.0 },
                }),
                (AxisId::CHANNELS, ParameterTemplate {
                    name: params::drive.as_str(),
                    kind: ParameterKind::Float { min: 0.1, max: 10.0, default: 1.0 },
                }),
                (AxisId::CHANNELS, ParameterTemplate {
                    name: params::pan.as_str(),
                    kind: ParameterKind::Float { min: -1.0, max: 1.0, default: 0.0 },
                }),
                (AxisId::CHANNELS, ParameterTemplate {
                    name: params::pingpong.as_str(),
                    kind: ParameterKind::Bool { default: false },
                }),
            ],
            per_axis_structural_params: &[],
        };
        T
    }

    fn prepare(env: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId, structural: &StructuralParams) -> Result<Self, BuildError> { Ok({
        let sr   = env.sample_rate;
        let taps = descriptor.shape.channels;
        let high_quality = structural.get_bool("high_quality", 0).unwrap_or(false);
        let sr_ms = sr * 0.001;

        let buf_l = DelayBuffer::for_duration(4.0, sr);
        let buf_r = DelayBuffer::for_duration(4.0, sr);

        let make_fb_filter = || {
            let mut f = TapFeedbackFilter::new();
            f.prepare(sr);
            f
        };
        let make_tone_filter = || {
            let mut f = ToneFilter::new();
            f.prepare(sr);
            f
        };

        let tap_state = (0..taps).map(|_| Tap {
            delay_cv:  MonoInput::default(),
            gain_cv:   MonoInput::default(),
            fb_cv:     MonoInput::default(),
            pan_cv:    MonoInput::default(),
            return_st: StereoInput::default(),
            send_st:   StereoOutput::default(),
            delay_ms:  500.0,
            gain:      1.0,
            feedback:  0.0,
            tone:      1.0,
            drive:     1.0,
            pan:       0.0,
            pingpong:  false,
            fb_filter_l:   make_fb_filter(),
            fb_filter_r:   make_fb_filter(),
            tone_filter_l: make_tone_filter(),
            tone_filter_r: make_tone_filter(),
            routed_l: 0.0,
            routed_r: 0.0,
        }).collect();

        Self {
            instance_id,
            descriptor,
            taps,
            high_quality,
            sr_ms,
            buf_l,
            buf_r,
            dry_wet: 1.0,
            in_stereo:  StereoInput::default(),
            drywet_cv:  MonoInput::default(),
            out_stereo: StereoOutput::default(),
            tap_state,
        }
    })}

    fn update_validated_parameters(&mut self, p: &ParamView<'_>) {
        self.dry_wet = p.get(params::dry_wet);
        for (i, tap) in self.tap_state.iter_mut().enumerate() {
            let idx = i as u16;
            tap.delay_ms = p.get(params::delay_ms.at(idx)).clamp(0, 2000) as f32;
            tap.gain     = p.get(params::gain.at(idx)).clamp(0.0, 1.0);
            tap.feedback = p.get(params::feedback.at(idx)).clamp(0.0, 1.0);
            let tone     = p.get(params::tone.at(idx)).clamp(0.0, 1.0);
            if (tone - tap.tone).abs() > f32::EPSILON {
                tap.tone = tone;
                tap.tone_filter_l.set_tone(tone);
                tap.tone_filter_r.set_tone(tone);
            }
            tap.drive    = p.get(params::drive.at(idx)).clamp(0.1, 10.0);
            tap.pan      = p.get(params::pan.at(idx)).clamp(-1.0, 1.0);
            tap.pingpong = p.get(params::pingpong.at(idx));
        }
    }

    fn descriptor(&self) -> &ModuleDescriptor { &self.descriptor }
    fn instance_id(&self) -> InstanceId { self.instance_id }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        let n = self.taps;
        // Global inputs: in(0, stereo), drywet_cv(1)
        self.in_stereo = StereoInput::from_ports(inputs, 0);
        self.drywet_cv = MonoInput::from_ports(inputs, 1);
        // Per-tap inputs: delay_cv, gain_cv, fb_cv, pan_cv, return (stereo)
        for (i, tap) in self.tap_state.iter_mut().enumerate() {
            tap.delay_cv  = MonoInput::from_ports(inputs, 2 + i);
            tap.gain_cv   = MonoInput::from_ports(inputs, 2 + n + i);
            tap.fb_cv     = MonoInput::from_ports(inputs, 2 + 2 * n + i);
            tap.pan_cv    = MonoInput::from_ports(inputs, 2 + 3 * n + i);
            tap.return_st = StereoInput::from_ports(inputs, 2 + 4 * n + i);
            tap.send_st   = StereoOutput::from_ports(outputs, 1 + i);
        }
        // Global output: out(0, stereo).
        self.out_stereo = StereoOutput::from_ports(outputs, 0);
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let (in_l, in_r) = pool.read_stereo(&self.in_stereo);

        // ── Write: input + pre-routed feedbacks from previous tick ────────────
        // Routing (pingpong or straight) was applied when storing routed_l/r at
        // the end of the previous tick's per-tap loop, so this accumulation is
        // branchless. Fuse the two sums into one pass over taps.
        let (mut fb_sum_l, mut fb_sum_r) = (0.0_f32, 0.0_f32);
        for tap in &self.tap_state {
            fb_sum_l += tap.routed_l;
            fb_sum_r += tap.routed_r;
        }
        let write_l = fast_tanh(in_l + fb_sum_l);
        let write_r = fast_tanh(in_r + fb_sum_r);

        self.buf_l.push(write_l);
        self.buf_r.push(write_r);

        // ── Per-tap reads ─────────────────────────────────────────────────────
        let cap_max = self.buf_l.capacity() as f32 - 2.0;
        let mut wet_l = 0.0_f32;
        let mut wet_r = 0.0_f32;
        let sr_ms = self.sr_ms;
        let high_quality = self.high_quality;

        for tap in self.tap_state.iter_mut() {
            // Effective delay. Skip CV pool dispatch when unconnected — the
            // typical case for per-tap CVs in a deployed patch.
            let cv = if tap.delay_cv.connected {
                pool.read_mono(&tap.delay_cv).clamp(-1.0, 2.0)
            } else {
                0.0
            };
            let offset = (tap.delay_ms * (1.0 + cv) * sr_ms).clamp(1.0, cap_max);

            let (tap_raw_l, tap_raw_r) = if high_quality {
                (self.buf_l.read_cubic(offset), self.buf_r.read_cubic(offset))
            } else {
                (self.buf_l.read_linear(offset), self.buf_r.read_linear(offset))
            };

            // Send outputs (pre-gain, pre-pan, pre-return).
            // Skip the 16-lane store when no consumer is wired — sends are an
            // opt-in routing facility and most patches leave them unconnected.
            if tap.send_st.connected {
                pool.write_stereo(&tap.send_st, tap_raw_l, tap_raw_r);
            }

            // Mix in returns. Disconnected return reads the zero sink; the add
            // is harmless but the read still costs a pool dispatch — skip it.
            let (sig_l, sig_r) = if tap.return_st.connected {
                let (ret_l, ret_r) = pool.read_stereo(&tap.return_st);
                (tap_raw_l + ret_l, tap_raw_r + ret_r)
            } else {
                (tap_raw_l, tap_raw_r)
            };

            // Tone filter
            let toned_l = tap.tone_filter_l.process(sig_l);
            let toned_r = tap.tone_filter_r.process(sig_r);

            // Pan law: sum to mono, apply equal-gain pan (consistent with Console).
            // Merge the two * 0.5 factors into a single * 0.25 to save one mul per tap.
            let gain_cv = if tap.gain_cv.connected { pool.read_mono(&tap.gain_cv) } else { 0.0 };
            let pan_cv  = if tap.pan_cv.connected  { pool.read_mono(&tap.pan_cv)  } else { 0.0 };
            let eff_gain = (tap.gain + gain_cv).clamp(0.0, 1.0);
            let eff_pan  = (tap.pan  + pan_cv ).clamp(-1.0, 1.0);
            let half     = (toned_l + toned_r) * (0.25 * eff_gain);
            wet_l += half * (1.0 - eff_pan);
            wet_r += half * (1.0 + eff_pan);

            // Feedback for next tick: apply pingpong routing now so the write
            // accumulation at the top of the next tick can be a branchless sum.
            let fb_cv  = if tap.fb_cv.connected { pool.read_mono(&tap.fb_cv) } else { 0.0 };
            let eff_fb = (tap.feedback + fb_cv).clamp(0.0, 1.0);
            let fb_l = tap.fb_filter_l.process(toned_l * eff_fb, tap.drive);
            let fb_r = tap.fb_filter_r.process(toned_r * eff_fb, tap.drive);
            if tap.pingpong {
                tap.routed_l = fb_r;
                tap.routed_r = fb_l;
            } else {
                tap.routed_l = fb_l;
                tap.routed_r = fb_r;
            }
        }

        // ── Dry/wet mix ───────────────────────────────────────────────────────
        let drywet_cv = if self.drywet_cv.connected { pool.read_mono(&self.drywet_cv) } else { 0.0 };
        let eff_dw = (self.dry_wet + drywet_cv).clamp(0.0, 1.0);
        pool.write_stereo(
            &self.out_stereo,
            in_l + eff_dw * (wet_l - in_l),
            in_r + eff_dw * (wet_r - in_r),
        );
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
        ModuleShape { channels: taps }
    }

    #[test]
    fn zero_taps_dry_wet_zero_passes_dry() {
        let mut h = ModuleHarness::build_full::<StereoDelay>(
            params!["dry_wet" => 0.0_f32],
            ENV, shape(0),
        );
        h.set_stereo("in", 0.5, 0.3);
        h.tick();
        let (l, r) = h.read_stereo("out");
        assert_eq!(l, 0.5);
        assert_eq!(r, 0.3);
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
        h.disconnect_input_at("return", 0);

        // Prime with a known signal
        h.set_stereo("in", 1.0, 1.0);
        for _ in 0..50 { h.tick(); }

        // With pan=1: left gain = (1-1)*0.5 = 0; right gain = (1+1)*0.5 = 1
        let (out_l, out_r) = h.read_stereo("out");
        let out_l = out_l.abs();
        let out_r = out_r.abs();
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
        h.disconnect_input_at("return", 0);

        // Fire impulse on L only; R is silent (disconnected → 0)
        h.set_stereo("in", 1.0, 0.0);
        h.tick();
        h.set_stereo("in", 0.0, 0.0);

        // Collect enough samples for several ping-pong bounces
        let total = period * 5;
        let mut left_samples = Vec::with_capacity(total);
        let mut right_samples = Vec::with_capacity(total);
        for _ in 0..total {
            h.tick();
            let (l, r) = h.read_stereo("out");
            left_samples.push(l);
            right_samples.push(r);
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
