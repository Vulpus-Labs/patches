//! Threshold gate with hysteresis and attack/hold/release (ADR 0076).
//!
//! Mono variant. The detector reads from `sidechain` when that port is
//! connected, otherwise self-keys from `in`. The DSP kernel lives in
//! [`crate::dynamics::common::GateDetector`] and is tested independently.
//!
//! Gating is binary in intent — there is no `mix` parameter. A patch
//! author wanting ducking-with-blend uses `Compressor` with high ratio
//! and `mix < 1`.
//!
//! # Inputs
//!
//! | Port        | Kind | Description                                                 |
//! |-------------|------|-------------------------------------------------------------|
//! | `in`        | mono | Audio input                                                 |
//! | `sidechain` | mono | External detector key; self-keys from `in` when unconnected |
//!
//! # Outputs
//!
//! | Port  | Kind | Description  |
//! |-------|------|--------------|
//! | `out` | mono | Gated audio  |
//!
//! # Parameters
//!
//! | Name         | Type  | Range          | Default | Description                       |
//! |--------------|-------|----------------|---------|-----------------------------------|
//! | `threshold`  | float | -80..0 dB      | `-40`   | Open threshold                    |
//! | `hysteresis` | float | 0..24 dB       | `3`     | Re-arm band below threshold       |
//! | `attack`     | float | 0.01..1000 ms  | `1`     | Open ramp                         |
//! | `hold`       | float | 0..5000 ms     | `10`    | Minimum open time once triggered  |
//! | `release`    | float | 1..5000 ms     | `100`   | Close ramp                        |

use patches_core::modules::{CountAxis, ModuleDescriptorTemplate, ParameterTemplate, PortTemplate};
use patches_core::param_frame::ParamView;
use patches_core::{
    module_params, AudioEnvironment, BuildError, CablePool, InputPort, InstanceId, Module,
    ModuleDescriptor, MonoInput, MonoOutput, OutputPort, ParameterKind, StructuralParams,
};

use crate::common::sidechain::mono_key;
use crate::dynamics::common::GateDetector;

module_params! {
    Gate {
        threshold:  Float,
        hysteresis: Float,
        attack:     Float,
        hold:       Float,
        release:    Float,
    }
}

pub struct Gate {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    detector: GateDetector,
    in_audio: MonoInput,
    in_sidechain: MonoInput,
    out_audio: MonoOutput,
}

impl Module for Gate {
    fn template() -> ModuleDescriptorTemplate {
        const T: ModuleDescriptorTemplate = ModuleDescriptorTemplate {
            name: "Gate",
            axes: &[CountAxis::CHANNELS],
            global_inputs: &[PortTemplate::mono("in"), PortTemplate::mono("sidechain")],
            per_axis_inputs: &[],
            global_outputs: &[PortTemplate::mono("out")],
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
            in_audio: MonoInput::default(),
            in_sidechain: MonoInput::default(),
            out_audio: MonoOutput::default(),
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
        self.in_audio = MonoInput::from_ports(inputs, 0);
        self.in_sidechain = MonoInput::from_ports(inputs, 1);
        self.out_audio = MonoOutput::from_ports(outputs, 0);
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let dry = pool.read_mono(&self.in_audio);
        let sc = pool.read_mono(&self.in_sidechain);
        let key = mono_key(dry, sc, self.in_sidechain.is_connected());
        let gain = self.detector.tick(key);
        pool.write_mono(&self.out_audio, dry * gain);
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
        ModuleHarness::build_full::<Gate>(
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
    fn sidechain_self_key_fallback_matches_explicit_self_wire() {
        // Two gates with identical params. One leaves sidechain unconnected
        // (self-keys from `in`); the other wires the same signal into both
        // `in` and `sidechain`. Outputs must agree.
        let mut self_keyed = build(&[
            ("threshold", ParameterValue::Float(-20.0)),
            ("hysteresis", ParameterValue::Float(6.0)),
            ("attack", ParameterValue::Float(0.5)),
            ("hold", ParameterValue::Float(5.0)),
            ("release", ParameterValue::Float(20.0)),
        ]);
        self_keyed.disconnect_input("sidechain");
        let mut sc_keyed = build(&[
            ("threshold", ParameterValue::Float(-20.0)),
            ("hysteresis", ParameterValue::Float(6.0)),
            ("attack", ParameterValue::Float(0.5)),
            ("hold", ParameterValue::Float(5.0)),
            ("release", ParameterValue::Float(20.0)),
        ]);

        for i in 0..4_000 {
            let x = 0.5 * (std::f32::consts::TAU * 200.0 * i as f32 / SR).sin();
            self_keyed.set_mono("in", x);
            self_keyed.tick();
            sc_keyed.set_mono("in", x);
            sc_keyed.set_mono("sidechain", x);
            sc_keyed.tick();
            let a = self_keyed.read_mono("out");
            let b = sc_keyed.read_mono("out");
            assert!((a - b).abs() < 1e-5, "self-key vs explicit-sidechain diverged at {i}: {a} vs {b}");
        }
    }

    #[test]
    fn external_sidechain_opens_quiet_input() {
        // Quiet audio that would otherwise stay gated, plus a loud key.
        // With sidechain wired, gate opens and the quiet signal passes.
        let mut h = build(&[
            ("threshold", ParameterValue::Float(-20.0)),
            ("hysteresis", ParameterValue::Float(3.0)),
            ("attack", ParameterValue::Float(0.1)),
            ("hold", ParameterValue::Float(5.0)),
            ("release", ParameterValue::Float(50.0)),
        ]);
        let quiet = 0.01_f32; // -40 dBFS — below threshold
        for _ in 0..2_000 {
            h.set_mono("in", quiet);
            h.set_mono("sidechain", 1.0);
            h.tick();
        }
        let out = h.read_mono("out");
        assert!(
            (out - quiet).abs() < 1e-4,
            "external key should open gate and pass quiet input: out={out}, expected≈{quiet}"
        );
    }
}
