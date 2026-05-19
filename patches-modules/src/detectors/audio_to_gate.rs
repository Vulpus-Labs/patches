//! Audio-to-gate Schmitt detector (mono variant, ADR 0076).
//!
//! Converts an audio signal into a sustained gate on a mono cable
//! (gate convention: `0.0` closed, `1.0` open). Opens when
//! `signal > threshold`, closes when `signal < threshold - hysteresis`;
//! values inside the schmitt band hold the previous state.
//! Sample-accurate by ADR 0030 — no sub-sample reporting. See
//! [`crate::detectors::common::GateSchmitt`] for the kernel.
//!
//! # Inputs
//!
//! | Port | Kind | Description |
//! |------|------|-------------|
//! | `in` | mono | Audio source |
//!
//! # Outputs
//!
//! | Port  | Kind | Description |
//! |-------|------|-------------|
//! | `out` | mono | Gate (`0.0` / `1.0`) |
//!
//! # Parameters
//!
//! | Name         | Type  | Range     | Default | Description           |
//! |--------------|-------|-----------|---------|-----------------------|
//! | `threshold`  | float | -60..0 dB | `-12`   | Open threshold        |
//! | `hysteresis` | float | 0..24 dB  | `3`     | Close band below open |

use patches_core::modules::{CountAxis, ModuleDescriptorTemplate, ParameterTemplate, PortTemplate};
use patches_core::param_frame::ParamView;
use patches_core::{
    module_params, AudioEnvironment, BuildError, CablePool, InputPort, InstanceId, Module,
    ModuleDescriptor, MonoInput, MonoOutput, OutputPort, ParameterKind, StructuralParams,
};

use crate::detectors::common::GateSchmitt;

module_params! {
    AudioToGate {
        threshold:  Float,
        hysteresis: Float,
    }
}

pub struct AudioToGate {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    detector: GateSchmitt,
    in_audio: MonoInput,
    out_gate: MonoOutput,
}

impl Module for AudioToGate {
    fn template() -> ModuleDescriptorTemplate {
        const T: ModuleDescriptorTemplate = ModuleDescriptorTemplate {
            name: "AudioToGate",
            axes: &[CountAxis::CHANNELS],
            global_inputs: &[PortTemplate::mono("in")],
            per_axis_inputs: &[],
            global_outputs: &[PortTemplate::mono("out")],
            per_axis_outputs: &[],
            realtime_params: &[
                ParameterTemplate {
                    name: params::threshold.as_str(),
                    kind: ParameterKind::Float { min: -60.0, max: 0.0, default: -12.0 },
                },
                ParameterTemplate {
                    name: params::hysteresis.as_str(),
                    kind: ParameterKind::Float { min: 0.0, max: 24.0, default: 3.0 },
                },
            ],
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
            detector: GateSchmitt::new(),
            in_audio: MonoInput::default(),
            out_gate: MonoOutput::default(),
        })
    }

    fn update_validated_parameters(&mut self, p: &ParamView<'_>) {
        self.detector.set_threshold_db(p.get(params::threshold));
        self.detector.set_hysteresis_db(p.get(params::hysteresis));
    }

    fn descriptor(&self) -> &ModuleDescriptor {
        &self.descriptor
    }
    fn instance_id(&self) -> InstanceId {
        self.instance_id
    }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        self.in_audio = MonoInput::from_ports(inputs, 0);
        self.out_gate = MonoOutput::from_ports(outputs, 0);
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let x = pool.read_mono(&self.in_audio);
        let open = self.detector.tick(x);
        pool.write_mono(&self.out_gate, if open { 1.0 } else { 0.0 });
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

    const SR: f32 = 48_000.0;
    const ENV: AudioEnvironment = AudioEnvironment {
        sample_rate: SR,
        poly_voices: 16,
        periodic_update_interval: 32,
        hosted: false,
    };

    fn build(params: &[(&'static str, ParameterValue)]) -> ModuleHarness {
        ModuleHarness::build_full::<AudioToGate>(
            params,
            ENV,
            patches_core::ModuleShape { channels: 0 },
        )
    }

    fn gate_at_minus_20() -> ModuleHarness {
        build(&[
            ("threshold", ParameterValue::Float(-20.0)),
            ("hysteresis", ParameterValue::Float(6.0)),
        ])
    }

    #[test]
    fn descriptor_shape() {
        let h = build(&[]);
        let desc = h.descriptor();
        assert_eq!(desc.inputs.len(), 1);
        assert_eq!(desc.outputs.len(), 1);
        assert_eq!(desc.inputs[0].name, "in");
        assert_eq!(desc.outputs[0].name, "out");
    }

    #[test]
    fn rising_crossing_opens_gate() {
        let mut h = gate_at_minus_20();
        h.set_mono("in", 0.0);
        h.tick();
        assert_eq!(h.read_mono("out"), 0.0);
        h.set_mono("in", 0.5);
        h.tick();
        assert_eq!(h.read_mono("out"), 1.0);
    }

    #[test]
    fn output_is_strict_zero_or_one() {
        let mut h = gate_at_minus_20();
        for i in 0..1_000 {
            let x = ((i as f32) * 0.013).sin();
            h.set_mono("in", x);
            h.tick();
            let v = h.read_mono("out");
            assert!(v == 0.0 || v == 1.0, "gate output must be binary, got {v}");
        }
    }
}
