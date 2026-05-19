//! Stereo width via internal M-S scaling (ADR 0076).
//!
//! Encodes the input to mid (`M = (L+R)/2`) and side (`S = (L-R)/2`),
//! scales `S` by `width`, leaves `M` unchanged, and decodes back:
//!
//! - `L_out = M + width * S`
//! - `R_out = M − width * S`
//!
//! At `width = 0` both outputs become the mono sum `M`. At `width = 1`
//! the M-S round-trip is identity; the implementation short-circuits in
//! that case so the round-trip is bit-exact (no intermediate (L+R)/2
//! rounding). At `width = 2` antiphase content is doubled in magnitude.
//! The `width` CV input is added to the parameter and clamped to
//! `[0, 2]` before scaling.
//!
//! # Inputs
//!
//! | Port    | Kind   | Description                              |
//! |---------|--------|------------------------------------------|
//! | `in`    | stereo | Source signal                            |
//! | `width` | mono   | Additive CV (offsets the `width` param)  |
//!
//! # Outputs
//!
//! | Port  | Kind   | Description           |
//! |-------|--------|-----------------------|
//! | `out` | stereo | Width-scaled L/R pair |
//!
//! # Parameters
//!
//! | Name    | Type  | Range | Default | Description                                  |
//! |---------|-------|-------|---------|----------------------------------------------|
//! | `width` | float | 0..2  | `1`     | 0 = mono sum, 1 = unchanged, 2 = double-wide |

use patches_core::modules::{CountAxis, ModuleDescriptorTemplate, ParameterTemplate, PortTemplate};
use patches_core::param_frame::ParamView;
use patches_core::{
    module_params, AudioEnvironment, BuildError, CablePool, InputPort, InstanceId, Module,
    ModuleDescriptor, MonoInput, OutputPort, ParameterKind, StereoInput, StereoOutput,
    StructuralParams,
};

module_params! {
    StereoWidth {
        width: Float,
    }
}

pub struct StereoWidth {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    width_base: f32,
    in_stereo: StereoInput,
    in_width_cv: MonoInput,
    out_stereo: StereoOutput,
}

impl Module for StereoWidth {
    fn template() -> ModuleDescriptorTemplate {
        const T: ModuleDescriptorTemplate = ModuleDescriptorTemplate {
            name: "StereoWidth",
            axes: &[CountAxis::CHANNELS],
            global_inputs: &[PortTemplate::stereo("in"), PortTemplate::mono("width")],
            per_axis_inputs: &[],
            global_outputs: &[PortTemplate::stereo("out")],
            per_axis_outputs: &[],
            realtime_params: &[ParameterTemplate {
                name: params::width.as_str(),
                kind: ParameterKind::Float { min: 0.0, max: 2.0, default: 1.0 },
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
            width_base: 1.0,
            in_stereo: StereoInput::default(),
            in_width_cv: MonoInput::default(),
            out_stereo: StereoOutput::default(),
        })
    }

    fn update_validated_parameters(&mut self, p: &ParamView<'_>) {
        self.width_base = p.get(params::width);
    }

    fn descriptor(&self) -> &ModuleDescriptor {
        &self.descriptor
    }
    fn instance_id(&self) -> InstanceId {
        self.instance_id
    }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        self.in_stereo = StereoInput::from_ports(inputs, 0);
        self.in_width_cv = MonoInput::from_ports(inputs, 1);
        self.out_stereo = StereoOutput::from_ports(outputs, 0);
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let (l, r) = pool.read_stereo(&self.in_stereo);
        let cv = pool.read_mono(&self.in_width_cv);
        let w = (self.width_base + cv).clamp(0.0, 2.0);
        // Bit-exact passthrough at unity width — skips intermediate
        // (L+R)/2, (L-R)/2 rounding the math would otherwise introduce.
        if w == 1.0 {
            pool.write_stereo(&self.out_stereo, l, r);
            return;
        }
        let m = (l + r) * 0.5;
        let s = (l - r) * 0.5 * w;
        pool.write_stereo(&self.out_stereo, m + s, m - s);
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
        ModuleHarness::build_full::<StereoWidth>(
            params,
            ENV,
            patches_core::ModuleShape { channels: 0 },
        )
    }

    #[test]
    fn descriptor_shape() {
        let h = build(&[]);
        let desc = h.descriptor();
        assert_eq!(desc.inputs.len(), 2);
        assert_eq!(desc.outputs.len(), 1);
        assert_eq!(desc.inputs[0].name, "in");
        assert_eq!(desc.inputs[1].name, "width");
        assert_eq!(desc.outputs[0].name, "out");
    }

    #[test]
    fn width_one_is_bit_exact_identity() {
        let mut h = build(&[("width", ParameterValue::Float(1.0))]);
        h.disconnect_input("width");
        for &(l_in, r_in) in &[
            (0.0_f32, 0.0_f32),
            (1.0, 1.0),
            (0.7, -0.3),
            (-0.5, 0.5),
            (1.0e-7, -1.0e-7),
            (123.4, -56.78),
        ] {
            h.set_stereo("in", l_in, r_in);
            h.tick();
            let (l, r) = h.read_stereo("out");
            assert_eq!(l.to_bits(), l_in.to_bits(), "L not bit-exact for ({l_in}, {r_in})");
            assert_eq!(r.to_bits(), r_in.to_bits(), "R not bit-exact for ({l_in}, {r_in})");
        }
    }

    #[test]
    fn width_zero_outputs_mono_sum_on_both_channels() {
        let mut h = build(&[("width", ParameterValue::Float(0.0))]);
        h.disconnect_input("width");
        h.set_stereo("in", 0.6, 0.2);
        h.tick();
        let (l, r) = h.read_stereo("out");
        let expected = (0.6_f32 + 0.2) * 0.5;
        assert!((l - expected).abs() < 1e-6, "L={l}, expected {expected}");
        assert!((r - expected).abs() < 1e-6, "R={r}, expected {expected}");
    }

    #[test]
    fn width_two_doubles_side_content() {
        // Pure side content: L = +1, R = -1 → M = 0, S = 1. At width=2
        // the side doubles to ±2 (no mid cancellation).
        let mut h = build(&[("width", ParameterValue::Float(2.0))]);
        h.disconnect_input("width");
        h.set_stereo("in", 1.0, -1.0);
        h.tick();
        let (l, r) = h.read_stereo("out");
        assert!((l - 2.0).abs() < 1e-6, "L={l}");
        assert!((r + 2.0).abs() < 1e-6, "R={r}");
    }

    #[test]
    fn width_cv_adds_to_param() {
        let mut h = build(&[("width", ParameterValue::Float(0.0))]);
        // CV adds 1.0 → effective width = 1.0 → identity.
        h.set_stereo("in", 0.4, -0.2);
        h.set_mono("width", 1.0);
        h.tick();
        let (l, r) = h.read_stereo("out");
        assert!((l - 0.4).abs() < 1e-6, "L={l}");
        assert!((r + 0.2).abs() < 1e-6, "R={r}");
    }
}
