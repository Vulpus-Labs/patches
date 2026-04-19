//! Vintage BBD reverb module — 4-line FDN built on bucket-brigade delays.
//!
//! Eight [`crate::bbd::Bbd`] 1024-stage lines with mutually-coprime
//! delays are cross-mixed by an 8×8 Hadamard matrix and fed back with a
//! decay coefficient. Eight lines (rather than four) gives enough modal
//! density to avoid audible beating between a sparse set of resonances.
//!
//! No compander. The NE570 pair's round-trip gain is only unity at
//! `ref_level`; at other levels it is `(ref/level)^0.25`, which is
//! benign on a single-pass BBD delay but destabilises an FDN feedback
//! loop (quiet tail → loop gain > 1 → runaway → saturate → compressor
//! drags it silent → cycle). The BBDs' own anti-imaging filters and
//! bucket saturation carry the vintage voice without it.
//!
//! Character: the BBD anti-imaging/reconstruction filters provide the
//! dark HF damping real reverbs need, and the compander's program-
//! dependent hiss fills the tail with gentle analog grit. There is no
//! dedicated damping filter or early-reflection stage — the BBD colour
//! is the reverb's voice.
//!
//! Not a Schroeder reverb and not a faithful model of any specific
//! hardware unit (BBD reverbs were rare); more a plausible vintage
//! plate/room built from the same parts a 1980s pedal-builder had.
//!
//! # Inputs
//!
//! | Port | Kind | Description |
//! |------|------|-------------|
//! | `in` | mono | Audio input |
//! | `drywet_cv` | mono | Additive CV for dry/wet |
//! | `size_cv` | mono | Additive CV for size |
//! | `decay_cv` | mono | Additive CV for decay |
//!
//! # Outputs
//!
//! | Port | Kind | Description |
//! |------|------|-------------|
//! | `out_left` | mono | Left wet/dry output |
//! | `out_right` | mono | Right wet/dry output |
//!
//! # Parameters
//!
//! | Name | Type | Range | Default | Description |
//! |------|------|-------|---------|-------------|
//! | `dry_wet` | float | 0.0--1.0 | `0.3` | Dry/wet mix |
//! | `size` | float | 0.0--1.0 | `0.5` | Room size (scales all four delays) |
//! | `decay` | float | 0.0--0.95 | `0.7` | FDN feedback coefficient |

use patches_core::parameter_map::{ParameterMap, ParameterValue};
use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, Module, ModuleDescriptor, ModuleShape,
    MonoInput, MonoOutput, OutputPort,
};
use patches_dsp::approximate::fast_tanh;

use crate::bbd::{Bbd, BbdDevice};
use crate::compander::{CompanderParams, Compressor, Expander};

const DECAY_MAX: f32 = 0.95;
const N: usize = 8;

/// Mutually-coprime base delays in milliseconds. Scaled by `size` into
/// the 1024-stage BBD's honest range (≲ 85 ms).
const BASE_DELAYS_MS: [f32; N] = [
    19.3, 23.1, 29.7, 31.3, 37.9, 41.7, 47.3, 53.9,
];
/// Size parameter maps linearly from `SIZE_MIN_SCALE` to `SIZE_MAX_SCALE`.
const SIZE_MIN_SCALE: f32 = 0.35;
const SIZE_MAX_SCALE: f32 = 1.45;

/// 8×8 normalised Hadamard, built as `[[H4, H4], [H4, -H4]] / sqrt(2)`.
/// Rows orthonormal with overall factor `1/sqrt(8)`.
#[inline(always)]
fn hadamard8(v: [f32; N]) -> [f32; N] {
    // 4×4 Hadamard sub-block (un-normalised).
    #[inline(always)]
    fn h4(a: f32, b: f32, c: f32, d: f32) -> [f32; 4] {
        [a + b + c + d, a - b + c - d, a + b - c - d, a - b - c + d]
    }
    let t = h4(v[0], v[1], v[2], v[3]);
    let u = h4(v[4], v[5], v[6], v[7]);
    let s = 1.0 / (8.0_f32).sqrt();
    [
        s * (t[0] + u[0]),
        s * (t[1] + u[1]),
        s * (t[2] + u[2]),
        s * (t[3] + u[3]),
        s * (t[0] - u[0]),
        s * (t[1] - u[1]),
        s * (t[2] - u[2]),
        s * (t[3] - u[3]),
    ]
}

/// Vintage BBD reverb. See the module-level documentation.
pub struct VReverb {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,

    bbds: [Bbd; N],
    comps: [Compressor; N],
    exps: [Expander; N],
    /// Previous-sample BBD outputs carried through the 1-sample cable
    /// delay that makes the FDN causal.
    y_prev: [f32; N],

    dry_wet: f32,
    size: f32,
    decay: f32,

    in_port: MonoInput,
    drywet_cv: MonoInput,
    size_cv: MonoInput,
    decay_cv: MonoInput,
    out_l: MonoOutput,
    out_r: MonoOutput,
}

impl Module for VReverb {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("VReverb", shape.clone())
            .mono_in("in")
            .mono_in("drywet_cv")
            .mono_in("size_cv")
            .mono_in("decay_cv")
            .mono_out("out_left")
            .mono_out("out_right")
            .float_param("dry_wet", 0.0, 1.0, 0.3)
            .float_param("size", 0.0, 1.0, 0.5)
            .float_param("decay", 0.0, DECAY_MAX, 0.7)
    }

    fn prepare(
        env: &AudioEnvironment,
        descriptor: ModuleDescriptor,
        instance_id: InstanceId,
    ) -> Self {
        let sr = env.sample_rate;
        Self {
            instance_id,
            descriptor,
            bbds: std::array::from_fn(|_| Bbd::new(&BbdDevice::BBD_1024, sr)),
            comps: std::array::from_fn(|_| {
                Compressor::new(CompanderParams::NE570_DEFAULT, sr)
            }),
            exps: std::array::from_fn(|_| {
                Expander::new(CompanderParams::NE570_DEFAULT, sr)
            }),
            y_prev: [0.0; N],
            dry_wet: 0.3,
            size: 0.5,
            decay: 0.7,
            in_port: MonoInput::default(),
            drywet_cv: MonoInput::default(),
            size_cv: MonoInput::default(),
            decay_cv: MonoInput::default(),
            out_l: MonoOutput::default(),
            out_r: MonoOutput::default(),
        }
    }

    fn update_validated_parameters(&mut self, params: &ParameterMap) {
        if let Some(ParameterValue::Float(v)) = params.get_scalar("dry_wet") {
            self.dry_wet = (*v).clamp(0.0, 1.0);
        }
        if let Some(ParameterValue::Float(v)) = params.get_scalar("size") {
            self.size = (*v).clamp(0.0, 1.0);
        }
        if let Some(ParameterValue::Float(v)) = params.get_scalar("decay") {
            self.decay = (*v).clamp(0.0, DECAY_MAX);
        }
    }

    fn descriptor(&self) -> &ModuleDescriptor {
        &self.descriptor
    }

    fn instance_id(&self) -> InstanceId {
        self.instance_id
    }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        self.in_port = MonoInput::from_ports(inputs, 0);
        self.drywet_cv = MonoInput::from_ports(inputs, 1);
        self.size_cv = MonoInput::from_ports(inputs, 2);
        self.decay_cv = MonoInput::from_ports(inputs, 3);
        self.out_l = MonoOutput::from_ports(outputs, 0);
        self.out_r = MonoOutput::from_ports(outputs, 1);
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let in_val = pool.read_mono(&self.in_port);

        let size = (self.size + pool.read_mono(&self.size_cv)).clamp(0.0, 1.0);
        let decay = (self.decay + pool.read_mono(&self.decay_cv)).clamp(0.0, DECAY_MAX);
        let scale = SIZE_MIN_SCALE + (SIZE_MAX_SCALE - SIZE_MIN_SCALE) * size;
        for (k, base) in BASE_DELAYS_MS.iter().enumerate() {
            self.bbds[k].set_delay_seconds(base * scale * 0.001);
        }

        let x = fast_tanh(in_val);
        let mixed = hadamard8(self.y_prev);

        let mut y = [0.0_f32; N];
        for k in 0..N {
            // Soft-clip the recirculating path: Hadamard + tanh is
            // strictly passive, so this bounds the loop at `decay < 1`.
            let drive = x + fast_tanh(decay * mixed[k]);
            let compressed = self.comps[k].process(drive);
            let bbd_out = self.bbds[k].process(compressed);
            y[k] = self.exps[k].process(bbd_out);
        }
        self.y_prev = y;

        // Decorrelated stereo pickoff: alternating signs across the
        // eight taps so L and R draw on orthogonal state combinations.
        let norm = 1.0 / (N as f32).sqrt();
        let wet_l = norm * (y[0] - y[1] + y[2] - y[3] + y[4] - y[5] + y[6] - y[7]);
        let wet_r = norm * (y[0] + y[1] - y[2] - y[3] + y[4] + y[5] - y[6] - y[7]);

        let eff_dw = (self.dry_wet + pool.read_mono(&self.drywet_cv)).clamp(0.0, 1.0);
        pool.write_mono(&self.out_l, in_val + eff_dw * (wet_l - in_val));
        pool.write_mono(&self.out_r, in_val + eff_dw * (wet_r - in_val));
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
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

    fn shape() -> ModuleShape {
        ModuleShape { channels: 0, length: 0, ..Default::default() }
    }

    fn disconnect_cvs(h: &mut ModuleHarness) {
        h.disconnect_input("drywet_cv");
        h.disconnect_input("size_cv");
        h.disconnect_input("decay_cv");
    }

    #[test]
    fn dry_wet_zero_passes_only_dry() {
        let mut h =
            ModuleHarness::build_full::<VReverb>(params!["dry_wet" => 0.0_f32], ENV, shape());
        disconnect_cvs(&mut h);
        h.set_mono("in", 0.7);
        h.tick();
        assert_eq!(h.read_mono("out_left"), 0.7);
        assert_eq!(h.read_mono("out_right"), 0.7);
    }

    #[test]
    fn output_is_bounded_under_sustained_input() {
        let mut h = ModuleHarness::build_full::<VReverb>(params![], ENV, shape());
        let mut pm = ParameterMap::new();
        pm.insert_param("dry_wet", 0, ParameterValue::Float(1.0));
        pm.insert_param("size", 0, ParameterValue::Float(0.7));
        pm.insert_param("decay", 0, ParameterValue::Float(0.9));
        h.update_params_map(&pm);
        disconnect_cvs(&mut h);

        for i in 0..40_000 {
            let t = i as f32 / SR;
            h.set_mono("in", 0.5 * (std::f32::consts::TAU * 440.0 * t).sin());
            h.tick();
            let l = h.read_mono("out_left");
            let r = h.read_mono("out_right");
            assert!(
                l.is_finite() && r.is_finite() && l.abs() < 5.0 && r.abs() < 5.0,
                "diverged at i={i}: l={l} r={r}"
            );
        }
    }

    #[test]
    fn impulse_tail_decays() {
        let mut h = ModuleHarness::build_full::<VReverb>(params![], ENV, shape());
        let mut pm = ParameterMap::new();
        pm.insert_param("dry_wet", 0, ParameterValue::Float(1.0));
        pm.insert_param("size", 0, ParameterValue::Float(0.5));
        pm.insert_param("decay", 0, ParameterValue::Float(0.6));
        h.update_params_map(&pm);
        disconnect_cvs(&mut h);

        h.set_mono("in", 1.0);
        h.tick();
        h.set_mono("in", 0.0);

        let mut early_peak = 0.0_f32;
        for _ in 0..((0.2 * SR) as usize) {
            h.tick();
            let m = h.read_mono("out_left").abs().max(h.read_mono("out_right").abs());
            if m > early_peak {
                early_peak = m;
            }
        }
        let mut late_peak = 0.0_f32;
        for _ in 0..((0.5 * SR) as usize) {
            h.tick();
            let m = h.read_mono("out_left").abs().max(h.read_mono("out_right").abs());
            if m > late_peak {
                late_peak = m;
            }
        }
        assert!(
            late_peak < early_peak,
            "tail should decay: early={early_peak} late={late_peak}"
        );
    }
}
