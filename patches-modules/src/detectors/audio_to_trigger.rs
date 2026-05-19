//! Audio-to-trigger edge detector (mono variant, ADR 0076).
//!
//! Converts an audio signal into ADR 0047 sub-sample sync events on a
//! `Trigger` cable. The fire condition is `armed && prev <= threshold &&
//! curr > threshold` for the rising direction (falling and bidirectional
//! follow symmetrically); the sub-sample fraction
//! `frac = (threshold - prev) / (curr - prev)` is the ADR 0047 event
//! fraction. The fire threshold is `threshold`, never `threshold - hysteresis`
//! — hysteresis controls eligibility (re-arm), never event location. See
//! [`crate::detectors::common::EdgeDetector`] for the kernel.
//!
//! # Inputs
//!
//! | Port | Kind | Description |
//! |------|------|-------------|
//! | `in` | mono | Audio source |
//!
//! # Outputs
//!
//! | Port  | Kind    | Description                                   |
//! |-------|---------|-----------------------------------------------|
//! | `out` | trigger | Sub-sample sync event (`(0, 1]`) on fire, `0` otherwise |
//!
//! # Parameters
//!
//! | Name         | Type  | Range                       | Default | Description                  |
//! |--------------|-------|-----------------------------|---------|------------------------------|
//! | `threshold`  | float | -60..0 dB                   | `-12`   | Fire threshold               |
//! | `hysteresis` | float | 0..24 dB                    | `3`     | Re-arm band around threshold |
//! | `direction`  | enum  | `rising`/`falling`/`both`   | `rising`| Edge polarity                |
//! | `cooldown`   | float | 0..1000 ms                  | `0`     | Debounce after fire          |

use patches_core::modules::{CountAxis, ModuleDescriptorTemplate, ParameterTemplate, PortTemplate};
use patches_core::param_frame::ParamView;
use patches_core::{
    module_params, params_enum, AudioEnvironment, BuildError, CablePool, InputPort, InstanceId,
    Module, ModuleDescriptor, MonoInput, MonoOutput, OutputPort, ParameterKind, StructuralParams,
};

use crate::detectors::common::{EdgeDetector, EdgeDirection};

params_enum! {
    pub enum EdgeDirectionParam {
        Rising => "rising",
        Falling => "falling",
        Both => "both",
    }
}

impl From<EdgeDirectionParam> for EdgeDirection {
    fn from(v: EdgeDirectionParam) -> Self {
        match v {
            EdgeDirectionParam::Rising => EdgeDirection::Rising,
            EdgeDirectionParam::Falling => EdgeDirection::Falling,
            EdgeDirectionParam::Both => EdgeDirection::Both,
        }
    }
}

module_params! {
    AudioToTrigger {
        threshold:  Float,
        hysteresis: Float,
        direction:  Enum<EdgeDirectionParam>,
        cooldown:   Float,
    }
}

pub struct AudioToTrigger {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    detector: EdgeDetector,
    in_audio: MonoInput,
    out_trigger: MonoOutput,
}

impl Module for AudioToTrigger {
    fn template() -> ModuleDescriptorTemplate {
        const T: ModuleDescriptorTemplate = ModuleDescriptorTemplate {
            name: "AudioToTrigger",
            axes: &[CountAxis::CHANNELS],
            global_inputs: &[PortTemplate::mono("in")],
            per_axis_inputs: &[],
            global_outputs: &[PortTemplate::trigger("out")],
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
                ParameterTemplate {
                    name: params::direction.as_str(),
                    kind: ParameterKind::Enum {
                        variants: EdgeDirectionParam::VARIANTS,
                        default: "rising",
                    },
                },
                ParameterTemplate {
                    name: params::cooldown.as_str(),
                    kind: ParameterKind::Float { min: 0.0, max: 1000.0, default: 0.0 },
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
            detector: EdgeDetector::new(env.sample_rate),
            in_audio: MonoInput::default(),
            out_trigger: MonoOutput::default(),
        })
    }

    fn update_validated_parameters(&mut self, p: &ParamView<'_>) {
        self.detector.set_threshold_db(p.get(params::threshold));
        self.detector.set_hysteresis_db(p.get(params::hysteresis));
        self.detector.set_direction(p.get(params::direction).into());
        self.detector.set_cooldown_ms(p.get(params::cooldown));
    }

    fn descriptor(&self) -> &ModuleDescriptor {
        &self.descriptor
    }
    fn instance_id(&self) -> InstanceId {
        self.instance_id
    }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        self.in_audio = MonoInput::from_ports(inputs, 0);
        self.out_trigger = outputs[0].expect_trigger();
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let x = pool.read_mono(&self.in_audio);
        let event = self.detector.tick(x);
        pool.write_mono(&self.out_trigger, event);
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
        ModuleHarness::build_full::<AudioToTrigger>(
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

    /// Threshold = -20 dB ≈ 0.1 linear; rising direction.
    fn rising_at_minus_20() -> ModuleHarness {
        build(&[
            ("threshold", ParameterValue::Float(-20.0)),
            ("hysteresis", ParameterValue::Float(6.0)),
            ("direction", EdgeDirectionParam::Rising.into()),
            ("cooldown", ParameterValue::Float(0.0)),
        ])
    }

    #[test]
    fn direction_param_selects_rising_branch() {
        let mut h = rising_at_minus_20();
        // Drive a falling-only signal — must produce no events.
        h.set_mono("in", 0.5);
        h.tick();
        let _ = h.read_mono("out"); // discard priming
        for _ in 0..50 {
            h.set_mono("in", 0.5);
            h.tick();
            assert_eq!(h.read_mono("out"), 0.0);
        }
        for _ in 0..50 {
            h.set_mono("in", 0.0);
            h.tick();
            assert_eq!(h.read_mono("out"), 0.0, "rising-only must ignore falling edge");
        }
    }

    #[test]
    fn direction_param_selects_falling_branch() {
        let mut h = build(&[
            ("threshold", ParameterValue::Float(-20.0)),
            ("hysteresis", ParameterValue::Float(6.0)),
            ("direction", EdgeDirectionParam::Falling.into()),
            ("cooldown", ParameterValue::Float(0.0)),
        ]);
        // Prime well above threshold.
        h.set_mono("in", 0.8);
        h.tick();
        let _ = h.read_mono("out");
        // Falling crossing: 0.8 → 0.0 (crosses ~0.1 threshold).
        h.set_mono("in", 0.0);
        h.tick();
        let v = h.read_mono("out");
        assert!(v > 0.0, "falling direction should fire on downward crossing, got {v}");
    }

    #[test]
    fn direction_param_both_fires_on_either_polarity() {
        let mut h = build(&[
            ("threshold", ParameterValue::Float(-20.0)),
            ("hysteresis", ParameterValue::Float(6.0)),
            ("direction", EdgeDirectionParam::Both.into()),
            ("cooldown", ParameterValue::Float(0.0)),
        ]);
        // Start below.
        h.set_mono("in", 0.0);
        h.tick();
        let _ = h.read_mono("out");
        // Rising fire.
        h.set_mono("in", 0.8);
        h.tick();
        let rising = h.read_mono("out");
        assert!(rising > 0.0, "rising fire missing, got {rising}");
        // Falling fire.
        h.set_mono("in", 0.0);
        h.tick();
        let falling = h.read_mono("out");
        assert!(falling > 0.0, "falling fire missing, got {falling}");
    }

    #[test]
    fn output_is_zero_when_no_crossing() {
        let mut h = rising_at_minus_20();
        for _ in 0..200 {
            h.set_mono("in", 0.05); // below threshold, no crossing
            h.tick();
            assert_eq!(h.read_mono("out"), 0.0);
        }
    }
}
