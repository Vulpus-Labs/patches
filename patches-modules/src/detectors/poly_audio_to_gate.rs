//! Poly audio-to-gate Schmitt detector (ADR 0076).
//!
//! Polyphonic variant of [`AudioToGate`](super::AudioToGate). One
//! independent [`GateSchmitt`] runs per voice channel, so per-voice gate
//! states are emitted on the lanes of the output poly cable (gate
//! convention: `0.0` closed, `1.0` open). See
//! [`crate::detectors::common::GateSchmitt`] for the kernel.
//!
//! # Inputs
//!
//! | Port | Kind | Description |
//! |------|------|-------------|
//! | `in` | poly | Per-voice audio source |
//!
//! # Outputs
//!
//! | Port  | Kind | Description |
//! |-------|------|-------------|
//! | `out` | poly | Per-voice gate (`0.0` / `1.0`) |
//!
//! # Parameters
//!
//! Identical to [`AudioToGate`](super::AudioToGate); all parameters apply
//! identically across voices.

use patches_core::modules::{CountAxis, ModuleDescriptorTemplate, ParameterTemplate, PortTemplate};
use patches_core::param_frame::ParamView;
use patches_core::{
    module_params, AudioEnvironment, BuildError, CablePool, InputPort, InstanceId, Module,
    ModuleDescriptor, OutputPort, ParameterKind, PolyInput, PolyOutput, StructuralParams,
};

use crate::detectors::common::GateSchmitt;

module_params! {
    PolyAudioToGate {
        threshold:  Float,
        hysteresis: Float,
    }
}

pub struct PolyAudioToGate {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    detectors: [GateSchmitt; 16],
    voice_count: usize,
    in_audio: PolyInput,
    out_gate: PolyOutput,
}

impl Module for PolyAudioToGate {
    fn template() -> ModuleDescriptorTemplate {
        const T: ModuleDescriptorTemplate = ModuleDescriptorTemplate {
            name: "PolyAudioToGate",
            axes: &[CountAxis::CHANNELS],
            global_inputs: &[PortTemplate::poly("in")],
            per_axis_inputs: &[],
            global_outputs: &[PortTemplate::poly("out")],
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
        env: &AudioEnvironment,
        descriptor: ModuleDescriptor,
        instance_id: InstanceId,
        _structural: &StructuralParams,
    ) -> Result<Self, BuildError> {
        let detectors = std::array::from_fn(|_| GateSchmitt::new());
        Ok(Self {
            instance_id,
            descriptor,
            detectors,
            voice_count: env.poly_voices.min(16),
            in_audio: PolyInput::default(),
            out_gate: PolyOutput::default(),
        })
    }

    fn update_validated_parameters(&mut self, p: &ParamView<'_>) {
        let threshold = p.get(params::threshold);
        let hysteresis = p.get(params::hysteresis);
        for d in &mut self.detectors {
            d.set_threshold_db(threshold);
            d.set_hysteresis_db(hysteresis);
        }
    }

    fn descriptor(&self) -> &ModuleDescriptor {
        &self.descriptor
    }
    fn instance_id(&self) -> InstanceId {
        self.instance_id
    }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        self.in_audio = PolyInput::from_ports(inputs, 0);
        self.out_gate = PolyOutput::from_ports(outputs, 0);
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let voices = pool.read_poly(&self.in_audio);
        let mut gates = [0.0_f32; 16];
        for i in 0..self.voice_count {
            gates[i] = if self.detectors[i].tick(voices[i]) { 1.0 } else { 0.0 };
        }
        pool.write_poly(&self.out_gate, gates);
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
        ModuleHarness::build_full::<PolyAudioToGate>(
            params,
            ENV,
            patches_core::ModuleShape { channels: 0 },
        )
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
    fn per_voice_gate_states_are_independent() {
        // Voice 0 driven above threshold (gate open); voice 1 below
        // (gate closed). The lanes must report independent states on
        // the same tick.
        let mut h = build(&[
            ("threshold", ParameterValue::Float(-20.0)),
            ("hysteresis", ParameterValue::Float(6.0)),
        ]);
        // Prime: all low.
        h.set_poly("in", [0.0; 16]);
        h.tick();
        let _ = h.read_poly("out");

        let mut v = [0.0_f32; 16];
        v[0] = 0.8;
        // voice 1 stays at 0.0
        h.set_poly("in", v);
        h.tick();
        let out = h.read_poly("out");
        assert_eq!(out[0], 1.0, "voice 0 should be open, got {}", out[0]);
        assert_eq!(out[1], 0.0, "voice 1 should be closed, got {}", out[1]);
    }

    #[test]
    fn voice_hysteresis_states_are_independent() {
        // Voice 0 opens and stays in the schmitt band; voice 1 also opens
        // but drops below rearm_low. The two lanes must report different
        // states even though parameters are shared.
        let mut h = build(&[
            ("threshold", ParameterValue::Float(-20.0)),
            ("hysteresis", ParameterValue::Float(6.0)),
        ]);
        // Prime: all low.
        h.set_poly("in", [0.0; 16]);
        h.tick();
        let _ = h.read_poly("out");

        // Both open.
        let mut v = [0.0_f32; 16];
        v[0] = 0.8;
        v[1] = 0.8;
        h.set_poly("in", v);
        h.tick();
        let out = h.read_poly("out");
        assert_eq!(out[0], 1.0);
        assert_eq!(out[1], 1.0);

        // Voice 0 inside band (≈0.07, between threshold ≈0.1 and rearm_low ≈0.05);
        // voice 1 well below rearm_low.
        let mut v2 = [0.0_f32; 16];
        v2[0] = 0.07;
        v2[1] = 0.0;
        h.set_poly("in", v2);
        h.tick();
        let out = h.read_poly("out");
        assert_eq!(out[0], 1.0, "voice 0 in schmitt band must stay open");
        assert_eq!(out[1], 0.0, "voice 1 below rearm_low must close");
    }
}
