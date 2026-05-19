//! Linear stereo balance (ADR 0076).
//!
//! Attenuates one side of a stereo signal while the other passes
//! unchanged. With `balance` in `[-1, 1]`:
//!
//! - `gain_L = clamp(1 - balance, 0, 1)`
//! - `gain_R = clamp(1 + balance, 0, 1)`
//!
//! At `balance = 0` the signal passes through unchanged. At
//! `balance = -1` the right side is silenced; at `balance = +1` the
//! left side is silenced. The linear ramp on each side matches the
//! mixer's pan-law ramp shape (the per-side gain is a linear function
//! of the control value, reaching unity at one extreme and zero at the
//! other). The `balance` CV input is added to the parameter before
//! clamping.
//!
//! # Inputs
//!
//! | Port      | Kind   | Description                                |
//! |-----------|--------|--------------------------------------------|
//! | `in`      | stereo | Source signal                              |
//! | `balance` | mono   | Additive CV (offsets the `balance` param)  |
//!
//! # Outputs
//!
//! | Port  | Kind   | Description       |
//! |-------|--------|-------------------|
//! | `out` | stereo | Balanced L/R pair |
//!
//! # Parameters
//!
//! | Name      | Type  | Range | Default | Description                  |
//! |-----------|-------|-------|---------|------------------------------|
//! | `balance` | float | -1..1 | `0`     | Base balance; -1 = L, +1 = R |

use patches_core::modules::{CountAxis, ModuleDescriptorTemplate, ParameterTemplate, PortTemplate};
use patches_core::param_frame::ParamView;
use patches_core::{
    module_params, AudioEnvironment, BuildError, CablePool, InputPort, InstanceId, Module,
    ModuleDescriptor, MonoInput, OutputPort, ParameterKind, StereoInput, StereoOutput,
    StructuralParams,
};

module_params! {
    Balance {
        balance: Float,
    }
}

pub struct Balance {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    balance_base: f32,
    in_stereo: StereoInput,
    in_balance_cv: MonoInput,
    out_stereo: StereoOutput,
}

#[inline]
pub(crate) fn balance_gains(balance: f32) -> (f32, f32) {
    let gl = (1.0 - balance).clamp(0.0, 1.0);
    let gr = (1.0 + balance).clamp(0.0, 1.0);
    (gl, gr)
}

impl Module for Balance {
    fn template() -> ModuleDescriptorTemplate {
        const T: ModuleDescriptorTemplate = ModuleDescriptorTemplate {
            name: "Balance",
            axes: &[CountAxis::CHANNELS],
            global_inputs: &[PortTemplate::stereo("in"), PortTemplate::mono("balance")],
            per_axis_inputs: &[],
            global_outputs: &[PortTemplate::stereo("out")],
            per_axis_outputs: &[],
            realtime_params: &[ParameterTemplate {
                name: params::balance.as_str(),
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
            balance_base: 0.0,
            in_stereo: StereoInput::default(),
            in_balance_cv: MonoInput::default(),
            out_stereo: StereoOutput::default(),
        })
    }

    fn update_validated_parameters(&mut self, p: &ParamView<'_>) {
        self.balance_base = p.get(params::balance);
    }

    fn descriptor(&self) -> &ModuleDescriptor {
        &self.descriptor
    }
    fn instance_id(&self) -> InstanceId {
        self.instance_id
    }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        self.in_stereo = StereoInput::from_ports(inputs, 0);
        self.in_balance_cv = MonoInput::from_ports(inputs, 1);
        self.out_stereo = StereoOutput::from_ports(outputs, 0);
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let (l, r) = pool.read_stereo(&self.in_stereo);
        let cv = pool.read_mono(&self.in_balance_cv);
        let (gl, gr) = balance_gains(self.balance_base + cv);
        pool.write_stereo(&self.out_stereo, l * gl, r * gr);
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
        ModuleHarness::build_full::<Balance>(params, ENV, patches_core::ModuleShape { channels: 0 })
    }

    #[test]
    fn descriptor_shape() {
        let h = build(&[]);
        let desc = h.descriptor();
        assert_eq!(desc.inputs.len(), 2);
        assert_eq!(desc.outputs.len(), 1);
        assert_eq!(desc.inputs[0].name, "in");
        assert_eq!(desc.inputs[1].name, "balance");
        assert_eq!(desc.outputs[0].name, "out");
    }

    #[test]
    fn centre_passes_through_unchanged() {
        let mut h = build(&[("balance", ParameterValue::Float(0.0))]);
        h.disconnect_input("balance");
        h.set_stereo("in", 0.3, -0.7);
        h.tick();
        let (l, r) = h.read_stereo("out");
        assert!((l - 0.3).abs() < 1e-6);
        assert!((r + 0.7).abs() < 1e-6);
    }

    #[test]
    fn hard_left_silences_right() {
        let mut h = build(&[("balance", ParameterValue::Float(-1.0))]);
        h.disconnect_input("balance");
        h.set_stereo("in", 0.5, 0.5);
        h.tick();
        let (l, r) = h.read_stereo("out");
        assert!((l - 0.5).abs() < 1e-6);
        assert!(r.abs() < 1e-6);
    }

    #[test]
    fn hard_right_silences_left() {
        let mut h = build(&[("balance", ParameterValue::Float(1.0))]);
        h.disconnect_input("balance");
        h.set_stereo("in", 0.5, 0.5);
        h.tick();
        let (l, r) = h.read_stereo("out");
        assert!(l.abs() < 1e-6);
        assert!((r - 0.5).abs() < 1e-6);
    }

    #[test]
    fn linear_ramp_matches_mixer_pan_law() {
        // Cross-reference the mixer's pan law: per-side gain is a linear
        // function of the control value, equal to 1 - b on one side and
        // 1 + b on the other (both clamped to [0, 1]). The mixer's mono
        // panner uses the same `1 ± p` ramp (halved to share a mono
        // input across two channels); the Balance applies it as a
        // per-side attenuator on a stereo input. At any control value
        // in [-1, 1] the two gains agree with the closed-form law.
        for i in 0..=20 {
            let b = -1.0 + (i as f32) * 0.1;
            let mut h = build(&[("balance", ParameterValue::Float(b))]);
            h.disconnect_input("balance");
            h.set_stereo("in", 1.0, 1.0);
            h.tick();
            let (l, r) = h.read_stereo("out");
            let (gl, gr) = balance_gains(b);
            assert!((l - gl).abs() < 1e-6, "balance={b}: L={l}, expected {gl}");
            assert!((r - gr).abs() < 1e-6, "balance={b}: R={r}, expected {gr}");
        }
    }

    #[test]
    fn cv_adds_to_param() {
        let mut h = build(&[("balance", ParameterValue::Float(-0.5))]);
        h.set_stereo("in", 1.0, 1.0);
        h.set_mono("balance", 0.5);
        h.tick();
        let (l, r) = h.read_stereo("out");
        assert!((l - 1.0).abs() < 1e-6, "L={l}");
        assert!((r - 1.0).abs() < 1e-6, "R={r}");
    }
}
