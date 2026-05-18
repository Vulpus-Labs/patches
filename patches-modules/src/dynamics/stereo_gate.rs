//! True-stereo threshold gate with linked detection (ADR 0076).
//!
//! Stereo variant of [`Gate`](super::gate::Gate). One detector is fed by
//! `max(|L|, |R|)`. A single gate state drives both channels, preserving
//! the stereo image — per-channel detection would cause audible image
//! shift on asymmetric transients.
//!
//! # Sidechain
//!
//! The `sidechain` port is stereo. When unconnected the detector self-keys
//! from `in`. A mono source patched into the stereo `sidechain` port is
//! broadcast to both channels automatically (ADR 0059).
//!
//! # Inputs
//!
//! | Port        | Kind   | Description                                                 |
//! |-------------|--------|-------------------------------------------------------------|
//! | `in`        | stereo | Audio input                                                 |
//! | `sidechain` | stereo | External detector key; self-keys from `in` when unconnected |
//!
//! # Outputs
//!
//! | Port  | Kind   | Description     |
//! |-------|--------|-----------------|
//! | `out` | stereo | Gated stereo    |
//!
//! # Parameters
//!
//! Identical to [`Gate`](super::gate::Gate).

use patches_core::modules::{CountAxis, ModuleDescriptorTemplate, ParameterTemplate, PortTemplate};
use patches_core::param_frame::ParamView;
use patches_core::{
    module_params, AudioEnvironment, BuildError, CablePool, InputPort, InstanceId, Module,
    ModuleDescriptor, OutputPort, ParameterKind, StereoInput, StereoOutput, StructuralParams,
};

use crate::common::sidechain::stereo_key;
use crate::dynamics::common::GateDetector;

module_params! {
    StereoGate {
        threshold:  Float,
        hysteresis: Float,
        attack:     Float,
        hold:       Float,
        release:    Float,
    }
}

pub struct StereoGate {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    detector: GateDetector,
    in_audio: StereoInput,
    in_sidechain: StereoInput,
    out_audio: StereoOutput,
}

impl Module for StereoGate {
    fn template() -> ModuleDescriptorTemplate {
        const T: ModuleDescriptorTemplate = ModuleDescriptorTemplate {
            name: "StereoGate",
            axes: &[CountAxis::CHANNELS],
            global_inputs: &[PortTemplate::stereo("in"), PortTemplate::stereo("sidechain")],
            per_axis_inputs: &[],
            global_outputs: &[PortTemplate::stereo("out")],
            per_axis_outputs: &[],
            realtime_params: &[
                ParameterTemplate {
                    name: params::threshold.as_str(),
                    kind: ParameterKind::Float { min: -80.0, max: 0.0, default: -40.0 },
                },
                ParameterTemplate {
                    name: params::hysteresis.as_str(),
                    kind: ParameterKind::Float { min: 0.0, max: 24.0, default: 3.0 },
                },
                ParameterTemplate {
                    name: params::attack.as_str(),
                    kind: ParameterKind::Float { min: 0.01, max: 1000.0, default: 1.0 },
                },
                ParameterTemplate {
                    name: params::hold.as_str(),
                    kind: ParameterKind::Float { min: 0.0, max: 5000.0, default: 10.0 },
                },
                ParameterTemplate {
                    name: params::release.as_str(),
                    kind: ParameterKind::Float { min: 1.0, max: 5000.0, default: 100.0 },
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
        Ok(Self {
            instance_id,
            descriptor,
            detector: GateDetector::new(env.sample_rate),
            in_audio: StereoInput::default(),
            in_sidechain: StereoInput::default(),
            out_audio: StereoOutput::default(),
        })
    }

    fn update_validated_parameters(&mut self, p: &ParamView<'_>) {
        self.detector.set_threshold_db(p.get(params::threshold));
        self.detector.set_hysteresis_db(p.get(params::hysteresis));
        self.detector.set_attack_ms(p.get(params::attack));
        self.detector.set_hold_ms(p.get(params::hold));
        self.detector.set_release_ms(p.get(params::release));
    }

    fn descriptor(&self) -> &ModuleDescriptor {
        &self.descriptor
    }
    fn instance_id(&self) -> InstanceId {
        self.instance_id
    }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        self.in_audio = StereoInput::from_ports(inputs, 0);
        self.in_sidechain = StereoInput::from_ports(inputs, 1);
        self.out_audio = StereoOutput::from_ports(outputs, 0);
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let (dry_l, dry_r) = pool.read_stereo(&self.in_audio);
        let sc = pool.read_stereo(&self.in_sidechain);
        let (k_l, k_r) = stereo_key((dry_l, dry_r), sc, self.in_sidechain.is_connected());
        let mag = k_l.abs().max(k_r.abs());
        let gain = self.detector.tick(mag);
        pool.write_stereo(&self.out_audio, dry_l * gain, dry_r * gain);
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
        ModuleHarness::build_full::<StereoGate>(
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
        assert_eq!(desc.inputs[1].name, "sidechain");
        assert_eq!(desc.outputs[0].name, "out");
    }

    #[test]
    fn asymmetric_lr_produces_same_gate_state_on_both_channels() {
        // L is loud and above threshold, R is below the rearm band. Linked
        // detector must open the gate (driven by L's magnitude) and apply
        // the same gain to both channels — measured as identical out/in
        // ratios on each.
        let mut h = build(&[
            ("threshold", ParameterValue::Float(-20.0)),
            ("hysteresis", ParameterValue::Float(3.0)),
            ("attack", ParameterValue::Float(0.1)),
            ("hold", ParameterValue::Float(5.0)),
            ("release", ParameterValue::Float(50.0)),
        ]);
        h.disconnect_input("sidechain");

        // L drives the detector; R is constant but well below rearm.
        let l = 1.0_f32; // 0 dBFS
        let r = 0.005_f32; // ~ -46 dBFS — well below rearm band
        for _ in 0..2_000 {
            h.set_stereo("in", l, r);
            h.tick();
        }
        let (out_l, out_r) = h.read_stereo("out");
        let g_l = out_l / l;
        let g_r = out_r / r;
        assert!(
            (g_l - g_r).abs() < 1e-4,
            "linked detector must apply identical gain on L/R: g_l={g_l}, g_r={g_r}"
        );
        // Sanity: gate is in fact open (gain ≈ 1).
        assert!(g_l > 0.99, "expected gate open on linked detector: g_l={g_l}");
    }

    #[test]
    fn closes_when_both_channels_quiet() {
        let mut h = build(&[
            ("threshold", ParameterValue::Float(-20.0)),
            ("hysteresis", ParameterValue::Float(3.0)),
            ("attack", ParameterValue::Float(0.1)),
            ("hold", ParameterValue::Float(0.0)),
            ("release", ParameterValue::Float(5.0)),
        ]);
        h.disconnect_input("sidechain");
        for _ in 0..2_000 {
            h.set_stereo("in", 0.005, 0.005);
            h.tick();
        }
        let (out_l, out_r) = h.read_stereo("out");
        assert!(out_l.abs() < 1e-4 && out_r.abs() < 1e-4, "expected closed: ({out_l}, {out_r})");
    }
}
