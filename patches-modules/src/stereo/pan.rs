//! Equal-power mono-to-stereo pan (ADR 0076).
//!
//! `pan` ∈ `[-1, 1]`; the panning angle θ = (pan + 1) · π/4 places the
//! input on the unit-power circle as `(L, R) = in · (cos θ, sin θ)`. At
//! the centre (`pan = 0`) each channel receives `in / √2` (−3 dB), so
//! total power `L² + R²` is preserved across the sweep. The CV input
//! `pan` is added to the parameter and clamped to `[-1, 1]` before the
//! angle is computed.
//!
//! # Inputs
//!
//! | Port  | Kind | Description                       |
//! |-------|------|-----------------------------------|
//! | `in`  | mono | Source signal                     |
//! | `pan` | mono | Additive CV (offsets `pan` param) |
//!
//! # Outputs
//!
//! | Port  | Kind   | Description           |
//! |-------|--------|-----------------------|
//! | `out` | stereo | Equal-power L/R image |
//!
//! # Parameters
//!
//! | Name  | Type  | Range  | Default | Description                       |
//! |-------|-------|--------|---------|-----------------------------------|
//! | `pan` | float | -1..1  | `0`     | Base position; -1 = L, +1 = R     |

use std::f32::consts::FRAC_PI_4;

use patches_core::modules::{CountAxis, ModuleDescriptorTemplate, ParameterTemplate, PortTemplate};
use patches_core::param_frame::ParamView;
use patches_core::{
    module_params, AudioEnvironment, BuildError, CablePool, InputPort, InstanceId, Module,
    ModuleDescriptor, MonoInput, OutputPort, ParameterKind, StereoOutput, StructuralParams,
};

module_params! {
    Pan {
        pan: Float,
    }
}

pub struct Pan {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    pan_base: f32,
    in_audio: MonoInput,
    in_pan_cv: MonoInput,
    out_stereo: StereoOutput,
}

#[inline]
fn equal_power_gains(pan: f32) -> (f32, f32) {
    let theta = (pan.clamp(-1.0, 1.0) + 1.0) * FRAC_PI_4;
    (theta.cos(), theta.sin())
}

impl Module for Pan {
    fn template() -> ModuleDescriptorTemplate {
        const T: ModuleDescriptorTemplate = ModuleDescriptorTemplate {
            name: "Pan",
            axes: &[CountAxis::CHANNELS],
            global_inputs: &[PortTemplate::mono("in"), PortTemplate::mono("pan")],
            per_axis_inputs: &[],
            global_outputs: &[PortTemplate::stereo("out")],
            per_axis_outputs: &[],
            realtime_params: &[ParameterTemplate {
                name: params::pan.as_str(),
                kind: ParameterKind::Float { min: -1.0, max: 1.0, default: 0.0 },
            }],
            structural_params: &[],
            per_axis_realtime_params: &[],
            per_axis_structural_params: &[],
        };
        T
    }

    fn prepare(
        _env: &AudioEnvironment,
        descriptor: ModuleDescriptor,
        instance_id: InstanceId,
        _structural: &StructuralParams,
    ) -> Result<Self, BuildError> {
        Ok(Self {
            instance_id,
            descriptor,
            pan_base: 0.0,
            in_audio: MonoInput::default(),
            in_pan_cv: MonoInput::default(),
            out_stereo: StereoOutput::default(),
        })
    }

    fn update_validated_parameters(&mut self, p: &ParamView<'_>) {
        self.pan_base = p.get(params::pan);
    }

    fn descriptor(&self) -> &ModuleDescriptor {
        &self.descriptor
    }
    fn instance_id(&self) -> InstanceId {
        self.instance_id
    }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        self.in_audio = MonoInput::from_ports(inputs, 0);
        self.in_pan_cv = MonoInput::from_ports(inputs, 1);
        self.out_stereo = StereoOutput::from_ports(outputs, 0);
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let x = pool.read_mono(&self.in_audio);
        let cv = pool.read_mono(&self.in_pan_cv);
        let (gl, gr) = equal_power_gains(self.pan_base + cv);
        pool.write_stereo(&self.out_stereo, x * gl, x * gr);
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::parameter_map::ParameterValue;
    use patches_core::test_support::ModuleHarness;

    const ENV: AudioEnvironment = AudioEnvironment {
        sample_rate: 48_000.0,
        poly_voices: 1,
        periodic_update_interval: 32,
        hosted: false,
    };

    fn build(params: &[(&'static str, ParameterValue)]) -> ModuleHarness {
        ModuleHarness::build_full::<Pan>(params, ENV, patches_core::ModuleShape { channels: 0 })
    }

    #[test]
    fn descriptor_shape() {
        let h = build(&[]);
        let desc = h.descriptor();
        assert_eq!(desc.inputs.len(), 2);
        assert_eq!(desc.outputs.len(), 1);
        assert_eq!(desc.inputs[0].name, "in");
        assert_eq!(desc.inputs[1].name, "pan");
        assert_eq!(desc.outputs[0].name, "out");
    }

    #[test]
    fn centre_outputs_equal_power_minus_3db() {
        let mut h = build(&[("pan", ParameterValue::Float(0.0))]);
        h.disconnect_input("pan");
        h.set_mono("in", 1.0);
        h.tick();
        let (l, r) = h.read_stereo("out");
        let expected = std::f32::consts::FRAC_1_SQRT_2; // 1/√2
        assert!((l - expected).abs() < 1e-6, "L={l}, expected {expected}");
        assert!((r - expected).abs() < 1e-6, "R={r}, expected {expected}");
        // Total power preserved.
        let power = l * l + r * r;
        assert!((power - 1.0).abs() < 1e-6, "power={power}");
    }

    #[test]
    fn hard_left_routes_only_to_left() {
        let mut h = build(&[("pan", ParameterValue::Float(-1.0))]);
        h.disconnect_input("pan");
        h.set_mono("in", 0.5);
        h.tick();
        let (l, r) = h.read_stereo("out");
        assert!((l - 0.5).abs() < 1e-6, "L={l}");
        assert!(r.abs() < 1e-6, "R={r}");
    }

    #[test]
    fn hard_right_routes_only_to_right() {
        let mut h = build(&[("pan", ParameterValue::Float(1.0))]);
        h.disconnect_input("pan");
        h.set_mono("in", 0.5);
        h.tick();
        let (l, r) = h.read_stereo("out");
        assert!(l.abs() < 1e-6, "L={l}");
        assert!((r - 0.5).abs() < 1e-6, "R={r}");
    }

    #[test]
    fn pan_cv_adds_to_param() {
        let mut h = build(&[("pan", ParameterValue::Float(-0.5))]);
        // CV of +0.5 cancels the param to land at centre.
        h.set_mono("in", 1.0);
        h.set_mono("pan", 0.5);
        h.tick();
        let (l, r) = h.read_stereo("out");
        let expected = std::f32::consts::FRAC_1_SQRT_2;
        assert!((l - expected).abs() < 1e-5);
        assert!((r - expected).abs() < 1e-5);
    }

    #[test]
    fn power_preserved_across_sweep() {
        // Sweep pan from -1 to 1; total power L²+R² should stay ≈ 1.
        for i in 0..=20 {
            let p = -1.0 + (i as f32) * 0.1;
            let mut h = build(&[("pan", ParameterValue::Float(p))]);
            h.disconnect_input("pan");
            h.set_mono("in", 1.0);
            h.tick();
            let (l, r) = h.read_stereo("out");
            let power = l * l + r * r;
            assert!((power - 1.0).abs() < 1e-5, "pan={p}: power={power}");
        }
    }
}
